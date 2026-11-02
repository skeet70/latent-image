use crate::{
    loader_code::immutable_loader_spans,
    modal_lifetime::{CodeSiteRole, CurrentState, LifetimeConfig, LifetimeMemory, LifetimeMonitor,
        LifetimeReceipt, LifetimeRequest, Word},
    modal_memory::FixedMemory,
    modal_permit::{CoreId, Cpsr, Layout},
    modal_task::{PhysicalExtent, source_layouts},
    modal_trace::SourceImage,
    provenance::Guest,
};
use fujimod_emulator::{board::{factory::MemoryMap, observation::{ByteOrder, CoreIndex,
    CpuDataAttribution, FetchAttribution, InstructionCores, InstructionExecutionState, InstructionPointEvent,
    MemoryWatch, PhysicalInstructionGuard, WatchKinds, WatchRange}}, session::Session};
use fujimod_emulator_cpu::{Register, arm};
use serde::{Serialize, Serializer, ser::{SerializeMap, SerializeSeq}};

const WRITE_LIMIT: usize = 1 << 20;

pub(crate) struct GuardConfig {
    contexts: Vec<SourceImage>,
    lifetime: LifetimeConfig,
}

fn image_extent(image: &SourceImage) -> Result<PhysicalExtent, String> {
    PhysicalExtent::new(image.start(), u32::try_from(image.bytes().len())
        .map_err(|_| "modal source length exceeds32bits")?).map_err(|error| format!("{error:?}"))
}

fn image_word(image: &SourceImage, pc: Guest) -> Option<Word> {
    let offset = pc.0.checked_sub(image.start().0)? as usize;
    let bytes: [u8; 4] = image.bytes().get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(Word(u32::from_le_bytes(bytes)))
}

impl GuardConfig {
    pub(crate) fn new(contexts: Vec<SourceImage>, request: LifetimeRequest,
        images: &[SourceImage], entry_branch: Option<Word>) -> Result<Self, String> {
        let expected = immutable_loader_spans(0x40000, 0x4dae0).into_iter()
            .chain(immutable_loader_spans(0xd0000, 0xddaa0));
        if contexts.len() != 16 || contexts.iter().zip(expected).any(|(image, (start, end))| {
            u64::from(image.start().0) != start || image.bytes().len() as u64 != end - start
        }) {
            return Err("modal task context requires all16 immutable loader source spans".into());
        }
        let sources = images.iter().chain(&contexts).map(image_extent)
            .collect::<Result<Vec<_>, _>>()?;
        let lifetime = request.into_config(sources).map_err(|failure| failure.to_string())?;
        for image in images.iter().skip(1) {
            let extent = image_extent(image)?;
            if extent.start() < lifetime.runtime().start() || extent.end() > lifetime.runtime().end() {
                return Err("owned modal source leaves the uploaded runtime allocation".into());
            }
        }
        let entry = lifetime.publications().iter().find(|row| row.address == crate::modal_permit::STOCK_ENTRY);
        match (entry_branch, entry) {
            (None, None) => {}
            (Some(branch), Some(row)) if row.old == Word(0xe92d_4ff0) && row.new == branch => {}
            _ => return Err("modal publication does not bind the selected shared-entry instruction".into()),
        }
        for row in lifetime.publications() {
            if row.address == crate::modal_permit::STOCK_ENTRY { continue; }
            let start = u64::from(row.address.0);
            let end = start + 4;
            let immutable = images.iter().chain(&contexts).any(|image| {
                u64::from(image.start().0) < end && start < u64::from(image.start().0) + image.bytes().len() as u64
            }) || lifetime.readonly_images().any(|(extent, _)| {
                u64::from(extent.start().0) < end && start < u64::from(extent.end().0)
            });
            if immutable { return Err("publication modifies an immutable modal or context source".into()); }
        }
        for site in lifetime.code_sites() {
            let expected = images.iter().chain(&contexts).find_map(|image| image_word(image, site.pc))
                .or_else(|| lifetime.readonly_images().find_map(|(extent, bytes)| {
                    let offset = site.pc.0.checked_sub(extent.start().0)? as usize;
                    let word: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
                    Some(Word(u32::from_le_bytes(word)))
                }));
            if expected != Some(site.word) {
                return Err("modal lifecycle site differs from its complete bound source image".into());
            }
        }
        Ok(Self { contexts, lifetime })
    }

    pub(crate) fn contains(&self, pc: Guest) -> bool {
        self.lifetime.source_site(pc).is_some()
    }
}

#[derive(Serialize)]
pub(crate) struct GuardReport<'a> {
    hook_entries: u64,
    protected_entries: u64,
    accepted_entries: u64,
    refused_entries: u64,
    failure: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub(crate) struct BoundFetch {
    guest: Guest,
    physical: fujimod_emulator::board::observation::PhysicalAddress,
    core: CoreId,
    progress: crate::trace::InstructionCount,
    word: Word,
}
impl Serialize for BoundFetch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_map(Some(6))?;
        state.serialize_entry("guest", &self.guest.0)?;
        state.serialize_entry("physical", &self.physical.0)?;
        state.serialize_entry("bytes", &self.word.0.to_le_bytes())?;
        state.serialize_entry("core", &self.core.0)?;
        state.serialize_entry("entry_progress", &self.progress.0)?;
        state.serialize_entry("byte_order", "little")?;
        state.end()
    }
}

pub(crate) struct InstalledGuard {
    contexts: Vec<SourceImage>,
    lifetime: LifetimeMonitor,
    watch: Box<MemoryWatch>,
    write_ranges: Vec<WatchRange>,
    fetch_ranges: Vec<WatchRange>,
    protected_sites: Vec<Guest>,
    row_cursor: usize,
    physical_failure: Option<String>,
}

fn merged_ranges(sites: &[Guest]) -> Vec<WatchRange> {
    let mut result: Vec<WatchRange> = Vec::new();
    for site in sites {
        if let Some(previous) = result.last_mut() {
            if previous.end == u64::from(site.0) {
                previous.end += 4;
                continue;
            }
        }
        result.push(WatchRange { start: u64::from(site.0), end: u64::from(site.0) + 4,
            kinds: WatchKinds::CPU_FETCH });
    }
    result
}

impl InstalledGuard {
    pub(crate) fn new(config: GuardConfig, session: &Session, maps: &[MemoryMap],
        static_sites: impl Iterator<Item = Guest>, layout: Layout) -> Result<Self, String> {
        let memory = FixedMemory { session, maps };
        let mut observed = [0; 256];
        for image in &config.contexts {
            for (index, expected) in image.bytes().chunks(observed.len()).enumerate() {
                let address = Guest(image.start().0 + (index * observed.len()) as u32);
                let bytes = &mut observed[..expected.len()];
                LifetimeMemory::read(&memory, address, bytes).map_err(|failure| failure.to_string())?;
                if bytes != expected { return Err("modal task context differs from its bound source".into()); }
            }
        }
        let lifetime = LifetimeMonitor::new(config.lifetime, &memory)
            .map_err(|failure| failure.to_string())?;
        let mut protected_sites: Vec<_> = static_sites.chain(lifetime.config().code_sites().iter().map(|site| site.pc)).collect();
        protected_sites.sort_unstable();
        protected_sites.dedup();
        let fetch_ranges = merged_ranges(&protected_sites);
        let execution = |pc: Guest| {
            if layout.owned().is_some_and(|range| range.start() <= pc && pc < range.end())
                || layout.helpers().any(|range| range.start() <= pc && pc < range.end()) {
                InstructionExecutionState::UnmaskedSvcA32Little
            } else { InstructionExecutionState::A32Little }
        };
        let mut sites = protected_sites.iter().copied().peekable();
        let mut guards = Vec::new();
        while let Some(start) = sites.next() {
            let state = execution(start);
            let mut end = u64::from(start.0) + 4;
            while sites.peek().is_some_and(|next| u64::from(next.0) == end && execution(*next) == state) {
                sites.next();
                end += 4;
            }
            let range = WatchRange { start: u64::from(start.0), end, kinds: WatchKinds::CPU_FETCH };
            guards.push(PhysicalInstructionGuard::arm32_little(range,
                fujimod_emulator::board::observation::GuestAddress(range.start), InstructionCores::Both)?
                .with_execution_state(state));
        }
        let attribution = FetchAttribution::new(CpuDataAttribution::None, fetch_ranges.clone())?
            .with_instruction_guards(guards)?;
        let write_ranges: Vec<_> = lifetime.config().watched_extents().iter().map(|extent| WatchRange {
            start: u64::from(extent.start().0), end: u64::from(extent.end().0), kinds: WatchKinds::WRITES,
        }).collect();
        let watch = session.board.memory_watch_begin_with_fetch_attribution(write_ranges.clone(),
            attribution, WRITE_LIMIT, maps.to_vec(), fujimod_emulator::board::observation::ViewPolicy::Fixed)?;
        Ok(Self { contexts: config.contexts, lifetime, watch, write_ranges, fetch_ranges,
            protected_sites, row_cursor: 0, physical_failure: None })
    }

    pub(crate) fn sites(&self) -> impl Iterator<Item = Guest> + '_ {
        self.lifetime.config().code_sites().iter().map(|site| site.pc)
    }

    pub(crate) fn contains(&self, pc: Guest) -> bool {
        self.lifetime.config().source_site(pc).is_some()
    }

    pub(crate) fn source_word(&self, pc: Guest) -> Option<Word> {
        self.contexts.iter().find_map(|image| image_word(image, pc))
            .or_else(|| self.lifetime.config().readonly_images().find_map(|(extent, bytes)| {
                let offset = pc.0.checked_sub(extent.start().0)? as usize;
                let word: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
                Some(Word(u32::from_le_bytes(word)))
            }))
    }

    pub(crate) fn check(&mut self) -> Result<(), String> {
        if let Some(status) = self.watch.instruction_guard_status() {
            if let Some(failure) = status.first_failure {
                let message = self.physical_failure.get_or_insert_with(|| format!("{failure:?}"));
                return Err(message.clone());
            }
        } else {
            return Err("modal physical guard status is missing".into());
        }
        let records = self.watch.records.borrow();
        if let Some(failure) = records.violation() { return Err(failure.to_owned()); }
        for row in records.rows_since(self.row_cursor)? {
            self.lifetime.observe_row(row).map_err(|failure| failure.to_string())?;
        }
        self.row_cursor = records.row_count();
        Ok(())
    }

    pub(crate) fn bind_point(&mut self, core: CoreId, pc: Guest,
        point: InstructionPointEvent, cpsr: Cpsr, word: Word) -> Result<Option<BoundFetch>, String> {
        if point.core.0 != core.0 || point.entry.address != u64::from(pc.0)
            || point.entry.bytes != 4 || point.entry.pstate != cpsr.0
            || point.entry.pstate & (1 << 5 | 1 << 9 | 1 << 24) != 0 {
            return Err("modal point does not bind its actual A32 instruction entry".into());
        }
        if self.protected_sites.binary_search(&pc).is_ok() {
            let fetch = self.watch.fetch_event(CoreIndex(core.0), point.entry.id)
                .ok_or("modal point lacks its exact physical fetch entry")?;
            if fetch.entry != point.entry || fetch.core.0 != core.0
                || fetch.address.0 != u64::from(pc.0) || fetch.physical.0 != u64::from(pc.0)
                || fetch.size.0 != 4 || fetch.byte_order != ByteOrder::Little {
                return Err("modal point fetch differs from its canonical physical source".into());
            }
            Ok(Some(BoundFetch { guest: pc, physical: fetch.physical, core,
                progress: crate::trace::InstructionCount(fetch.entry.progress), word }))
        } else {
            let physical = u64::from(pc.0 & !0x4000_0000);
            if point.entry.physical != Some(physical) || point.entry.tcm.iter().flatten().any(|window| {
                window.address < u64::from(pc.0) + 4 && u64::from(pc.0) < window.address + window.bytes as u64
            }) {
                return Err("modal return entry lacks its fixed physical backing".into());
            }
            Ok(None)
        }
    }

    pub(crate) fn observe_source(&mut self, session: &Session, maps: &[MemoryMap],
        core: CoreId, pc: Guest, word: Word, cpsr: Cpsr) -> Result<(), String> {
        let site = self.lifetime.config().source_site(pc).ok_or("modal lifecycle source site is missing")?;
        if site.instruction.word != word { return Err("modal lifecycle instruction differs from source".into()); }
        if site.role == CodeSiteRole::Entry {
            return self.lifetime.observe_entry(core, pc, word).map_err(|failure| failure.to_string());
        }
        let base = ((word.0 >> 16) & 15) as u8;
        let value = ((word.0 >> 12) & 15) as u8;
        let mut registers = [0; 13];
        for register in [base, value] {
            let index = usize::from(register);
            if index >= registers.len() { return Err("modal lifecycle store uses a non-general register".into()); }
            registers[index] = u32::try_from(session.board.read_register(core.0,
                Register::Arm32(arm::Register::R(register)))?).map_err(|_| "modal store register exceeds32bits")?;
        }
        let target = Guest(registers[usize::from(base)]);
        let layout = source_layouts().get(core.0).copied().ok_or("modal lifecycle store core is invalid")?;
        let expected = match site.role {
            CodeSiteRole::PublicationStore => {
                if !self.lifetime.config().publications().iter().any(|row| row.address == target) {
                    return Err("modal publication store target is not declared".into());
                }
                target
            }
            CodeSiteRole::ContextStore(expected_core) | CodeSiteRole::CurrentStore(expected_core) => {
                if core != expected_core { return Err("modal context writer runs on the wrong core".into()); }
                let extent = if matches!(site.role, CodeSiteRole::CurrentStore(_)) { layout.current } else { layout.context };
                if target.0 & !0x4000_0000 != extent.start().0 {
                    return Err("modal context store target differs from its source cell".into());
                }
                extent.start()
            }
            CodeSiteRole::Entry => return Err("modal entry cannot be a store".into()),
        };
        let memory = FixedMemory { session, maps };
        let read_word = |address| {
            let mut bytes = [0; 4];
            LifetimeMemory::read(&memory, address, &mut bytes).map_err(|failure| failure.to_string())?;
            Ok::<_, String>(Word(u32::from_le_bytes(bytes)))
        };
        let before = read_word(expected)?;
        if matches!(site.role, CodeSiteRole::CurrentStore(_)) {
            let selected = read_word(Guest(layout.current.start().0 + 8))?;
            let context_depth = read_word(layout.context.start())?;
            self.lifetime.prepare_current_store(core, pc, word, &registers, before, cpsr,
                CurrentState { selected, context_depth }).map_err(|failure| failure.to_string())
        } else {
            self.lifetime.prepare_store(core, pc, word, &registers, before, cpsr)
                .map_err(|failure| failure.to_string())
        }
    }

    pub(crate) fn prepare_finish(&mut self, session: &Session, maps: &[MemoryMap],
        active_frames: usize) -> Result<(), String> {
        self.check()?;
        self.lifetime.verify_rollback(&FixedMemory { session, maps }, active_frames)
            .map_err(|failure| failure.to_string())
    }

    pub(crate) fn close(&mut self, session: &Session) -> Result<(), String> {
        session.board.memory_watch_end(&mut self.watch)?;
        self.lifetime.end().map_err(|failure| failure.to_string())?;
        Ok(())
    }

    pub(crate) fn receipt(&self) -> LifetimeReceipt { self.lifetime.receipt() }
    pub(crate) fn context_count(&self) -> usize { self.contexts.len() }
    pub(crate) fn healthy(&self) -> bool {
        self.physical_failure.is_none() && self.lifetime.receipt().failure.is_none()
            && self.watch.records.borrow().violation().is_none()
            && self.watch.instruction_guard_status().is_some_and(|status| status.first_failure.is_none())
    }
    pub(crate) fn report(&self) -> Option<GuardReport<'_>> {
        self.watch.instruction_guard_status().map(|status| GuardReport {
            hook_entries: status.hook_entries, protected_entries: status.protected_entries,
            accepted_entries: status.accepted_entries, refused_entries: status.refused_entries,
            failure: self.physical_failure.as_deref(),
        })
    }
    pub(crate) fn watch_evidence(&self) -> WatchEvidence<'_> { WatchEvidence(self) }
}

pub(crate) struct WatchEvidence<'a>(&'a InstalledGuard);
struct RangeEvidence<'a>(&'a [WatchRange]);
impl Serialize for RangeEvidence<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for range in self.0 { sequence.serialize_element(&[range.start, range.end])?; }
        sequence.end()
    }
}
struct ClassEvidence<'a>(&'a [WatchRange]);
impl Serialize for ClassEvidence<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Class { range: [u64; 2], kinds: [&'static str; 3] }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for range in self.0 {
            sequence.serialize_element(&Class { range: [range.start, range.end],
                kinds: ["cpu-write", "host-write", "model-bus-write"] })?;
        }
        sequence.end()
    }
}
impl Serialize for WatchEvidence<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Attribution<'a> { sources: [&'static str; 0], ranges: RangeEvidence<'a> }
        let guard = self.0;
        let records = guard.watch.records.borrow();
        let mut state = serializer.serialize_map(Some(7))?;
        state.serialize_entry("enabled", &true)?;
        state.serialize_entry("ranges", &RangeEvidence(&guard.write_ranges))?;
        state.serialize_entry("records", records.rows_since(0).map_err(serde::ser::Error::custom)?)?;
        state.serialize_entry("violation", &records.violation())?;
        state.serialize_entry("classes", &ClassEvidence(&guard.write_ranges))?;
        state.serialize_entry("views", "fixed")?;
        state.serialize_entry("fetch_attribution", &Attribution { sources: [], ranges: RangeEvidence(&guard.fetch_ranges) })?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_fetch_wire_bytes_round_trip_as_the_bound_little_endian_instruction() {
        let fetch = BoundFetch { guest: Guest(0x00bf_b028),
            physical: fujimod_emulator::board::observation::PhysicalAddress(0x00bf_b028),
            core: CoreId(0), progress: crate::trace::InstructionCount(91), word: Word(0xea10_0abc) };
        let encoded = serde_json::to_vec(&fetch).unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, serde_json::json!({"guest":0x00bf_b028,"physical":0x00bf_b028,
            "bytes":[0xbc,0x0a,0x10,0xea],"core":0,"entry_progress":91,"byte_order":"little"}));
    }
}
