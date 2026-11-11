use crate::{
    loader_code::immutable_loader_spans,
    modal_lifetime::{CodeSiteRole, CurrentState, LifetimeConfig, LifetimeMemory, LifetimeMonitor,
        LifetimeReceipt, LifetimeRequest, RowProjection, VfpOwnerState, Word},
    modal_memory::FixedMemory,
    modal_permit::{CoreId, Cpsr, Layout},
    modal_task::{PhysicalExtent, VfpOwner, current_task_id, source_layouts, vfp_owner},
    modal_trace::SourceImage,
    provenance::Guest,
};
use fujimod_emulator::{board::{factory::MemoryMap, observation::{ByteOrder, CoreIndex,
    CpuDataAttribution, FetchAttribution, InstructionCores, InstructionExecutionState, InstructionPointEvent,
    MemoryWatch, PassiveFetchAttribution, PassiveRetention, PassiveRetentionStatus, PassiveStopReason, PhysicalInstructionGuard,
    WatchKinds, WatchRange}}, session::Session};
use fujimod_emulator_cpu::{Register, arm};
use serde::{Serialize, Serializer, ser::{SerializeMap, SerializeSeq}};
use sha2::{Digest, Sha256};
use std::cell::RefCell;

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
        if source_layouts().iter().any(|layout| contexts.iter()
            .find_map(|image| image_word(image, layout.vfp_owner.start())) != Some(Word(VfpOwner::UNOWNED))) {
            return Err("modal task context does not bind the unowned lazy-VFP word".into());
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

#[derive(Clone, Copy, Serialize)]
#[serde(transparent)]
struct EntryProgress(u64);

#[derive(Clone, Copy)]
pub(crate) struct BoundFetch {
    guest: Guest,
    physical: fujimod_emulator::board::observation::PhysicalAddress,
    core: CoreId,
    progress: EntryProgress,
    word: Word,
}
impl Serialize for BoundFetch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_map(Some(6))?;
        state.serialize_entry("guest", &self.guest.0)?;
        state.serialize_entry("physical", &self.physical.0)?;
        state.serialize_entry("bytes", &self.word.0.to_le_bytes())?;
        state.serialize_entry("core", &self.core.0)?;
        state.serialize_entry("entry_progress", &self.progress)?;
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
    /// Retired extents, then each reload's call and return words.
    fetch_classes: Vec<WatchRange>,
    protected_sites: Vec<Guest>,
    row_cursor: usize,
    physical_failure: Option<String>,
    row_digest: RefCell<RowDigest>,
    af_ranges: Vec<WatchRange>,
    lifetime_rows: usize,
}

/// SHA-256 over each retained row's compact JSON encoding followed by LF, in row order.
#[derive(Default)]
struct RowDigest { rows: usize, hasher: Sha256 }
impl RowDigest {
    fn advance<T: Serialize>(&mut self, rows: &[T]) -> Result<(), String> {
        for row in rows {
            self.hasher.update(serde_json::to_vec(row).map_err(|failure| failure.to_string())?);
            self.hasher.update(b"\n");
        }
        self.rows += rows.len();
        Ok(())
    }
    fn hex(&self) -> String {
        self.hasher.clone().finalize().iter().map(|byte| format!("{byte:02x}")).collect()
    }
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
        static_sites: impl Iterator<Item = Guest>, layout: Layout,
        af: Option<&crate::af_ownership::Projection>) -> Result<Self, String> {
        let memory = FixedMemory { session, maps };
        let mut observed = [0; 256];
        for image in &config.contexts {
            for (index, expected) in image.bytes().chunks(observed.len()).enumerate() {
                let address = Guest(image.start().0 + (index * observed.len()) as u32);
                let bytes = &mut observed[..expected.len()];
                LifetimeMemory::read(&memory, address, bytes).map_err(|failure| failure.to_string())?;
                let owner_byte = |offset: usize| {
                    let byte = address.0 + offset as u32;
                    source_layouts().iter().any(|layout| layout.vfp_owner.start().0 <= byte && byte < layout.vfp_owner.end().0)
                };
                if bytes.iter().zip(expected).enumerate().any(|(offset, (actual, source))| actual != source && !owner_byte(offset)) {
                    return Err("modal task context differs from its bound source".into());
                }
            }
        }
        for layout in source_layouts() {
            let mut bytes = [0; 4];
            LifetimeMemory::read(&memory, layout.vfp_owner.start(), &mut bytes).map_err(|failure| failure.to_string())?;
            vfp_owner(&memory, layout.kernel, u32::from_le_bytes(bytes))
                .map_err(|failure| format!("modal lazy-VFP owner is neither unowned nor a created task: {failure:?}"))?;
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
        let af_sites: Vec<_> = af.into_iter().flat_map(|projection| projection.sites().map(Guest)).collect();
        if af_sites.iter().any(|site| protected_sites.binary_search(site).is_ok()) {
            return Err("AF site overlaps an installed lifecycle instruction".into());
        }
        let af_ranges = if af.is_some() { crate::af_ownership::ranges() } else { Vec::new() };
        let attribution = FetchAttribution::new(CpuDataAttribution::None, fetch_ranges.clone())?
            .with_instruction_guards(guards)?;
        let write_ranges: Vec<_> = lifetime.config().watched_extents().iter().map(|extent| WatchRange {
            start: u64::from(extent.start().0), end: u64::from(extent.end().0), kinds: WatchKinds::WRITES,
        }).collect();
        let word_class = |site: Guest| WatchRange { start: u64::from(site.0), end: u64::from(site.0) + 4, kinds: WatchKinds::CPU_FETCH };
        let fetch_classes: Vec<_> = lifetime.config().retired_extents().iter().map(|extent| WatchRange {
            start: u64::from(extent.start().0), end: u64::from(extent.end().0), kinds: WatchKinds::CPU_FETCH,
        }).chain(lifetime.config().settings_reloads().iter().flat_map(|reload| [word_class(reload.call), word_class(reload.resume)])).collect();
        if fetch_classes.iter().any(|class| fetch_ranges.iter().any(|site| site.start < class.end && class.start < site.end)) {
            return Err("modal retired extent or settings reload site overlaps a guarded instruction site".into());
        }
        let protected_classes = write_ranges.iter().chain(&fetch_classes).cloned().collect();
        let watch = if af.is_some() {
            session.board.memory_watch_begin_with_passive_fetch_attribution(
                protected_classes, attribution, WRITE_LIMIT, maps.to_vec(),
                fujimod_emulator::board::observation::ViewPolicy::Fixed,
                PassiveRetention::new(af_ranges.clone(), crate::af_ownership::ROW_LIMIT)?
                    .with_fetch_attribution(PassiveFetchAttribution::new(CpuDataAttribution::Writes,
                        crate::af_ownership::attribution_ranges(maps)?)?))?
        } else {
            session.board.memory_watch_begin_with_fetch_attribution(
                protected_classes, attribution, WRITE_LIMIT, maps.to_vec(),
                fujimod_emulator::board::observation::ViewPolicy::Fixed)?
        };
        Ok(Self { contexts: config.contexts, lifetime, watch, write_ranges, fetch_ranges, fetch_classes,
            protected_sites, row_cursor: 0, physical_failure: None, row_digest: RefCell::default(), af_ranges, lifetime_rows: 0 })
    }

    pub(crate) fn installed(&self) -> bool {
        self.lifetime.receipt().phase == crate::modal_lifetime::LifetimePhase::Installed
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

    pub(crate) fn arm_af_retention(&mut self) -> Result<(), String> {
        self.watch.records.borrow_mut().arm_passive_retention()
    }

    pub(crate) fn stop_af_retention(&mut self, reason: PassiveStopReason) -> Result<(), String> {
        let mut records = self.watch.records.borrow_mut();
        if matches!(records.passive_retention_status(), PassiveRetentionStatus::Unarmed | PassiveRetentionStatus::Armed) {
            records.stop_passive_retention(reason)?;
        }
        Ok(())
    }

    pub(crate) fn sync_af_retention(&self, af: &mut crate::af_ownership::Projection) {
        af.sync_retention(self.watch.records.borrow().passive_retention_status());
    }

    fn protects_row(&self, row: &serde_json::Value) -> Result<bool, String> {
        let start = row.get("physical").and_then(serde_json::Value::as_u64).ok_or("modal row lacks physical address")?;
        let width = row.get("size").and_then(serde_json::Value::as_u64).filter(|width| *width > 0).ok_or("modal row lacks width")?;
        let end = start.checked_add(width).ok_or("modal row wraps")?;
        let classes = match row.get("source").and_then(serde_json::Value::as_str) {
            Some("cpu-write" | "host-write" | "model-bus-write") => &self.write_ranges,
            Some("cpu-fetch") => &self.fetch_classes,
            _ => return Err("modal row has an unknown source".into()),
        };
        Ok(classes.iter().any(|range| range.start < end && start < range.end))
    }

    pub(crate) fn check(&mut self, maps: &[MemoryMap], mut af: Option<&mut crate::af_ownership::Projection>) -> Result<(), String> {
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
            let is_af = if self.af_ranges.is_empty() { false } else {
                match crate::af_ownership::selects(row) {
                    Ok(selected) => selected,
                    Err(failure) => {
                        if let Some(projection) = af.as_deref_mut() { projection.fail(failure); }
                        false
                    }
                }
            };
            if is_af {
                af.as_deref_mut().ok_or("AF projection missing from its installed watch")?.row(row);
            }
            let protected_overlap = self.af_ranges.is_empty() || self.protects_row(row)?;
            if protected_overlap {
                if self.lifetime_rows == WRITE_LIMIT {return Err("modal lifetime write-row capacity exhausted".into());}
                self.lifetime_rows += 1;
                let result = if self.af_ranges.is_empty() { self.lifetime.observe_row(row) }
                    else { self.lifetime.observe_attributed_row(row,maps) };
                result.map_err(|failure| failure.to_string())?;
            }
        }
        self.row_cursor = records.row_count();
        drop(records);
        if let Some(projection) = af {
            self.sync_af_retention(projection);
            if !projection.active() && matches!(self.watch.records.borrow().passive_retention_status(), PassiveRetentionStatus::Armed) {
                self.stop_af_retention(PassiveStopReason::Refused)?;
                self.sync_af_retention(projection);
            }
        }
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
                progress: EntryProgress(fetch.entry.progress), word }))
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
        let target = Guest(match site.role {
            CodeSiteRole::SelectedRelativeCurrentStore(_) => registers[usize::from(base)]
                .checked_sub(8).ok_or("modal selected-relative store target underflows")?,
            _ => registers[usize::from(base)],
        });
        let layout = source_layouts().get(core.0).copied().ok_or("modal lifecycle store core is invalid")?;
        let expected = match site.role {
            CodeSiteRole::PublicationStore => {
                if !self.lifetime.config().publications().iter().any(|row| row.address == target) {
                    return Err("modal publication store target is not declared".into());
                }
                target
            }
            CodeSiteRole::ContextStore(expected_core) | CodeSiteRole::CurrentStore(expected_core)
            | CodeSiteRole::SelectedRelativeCurrentStore(expected_core) | CodeSiteRole::VfpOwnerStore(expected_core) => {
                if core != expected_core { return Err("modal context writer runs on the wrong core".into()); }
                let extent = match site.role {
                    CodeSiteRole::CurrentStore(_) | CodeSiteRole::SelectedRelativeCurrentStore(_) => layout.current,
                    CodeSiteRole::VfpOwnerStore(_) => layout.vfp_owner,
                    _ => layout.context,
                };
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
        match site.role {
            CodeSiteRole::CurrentStore(_) | CodeSiteRole::SelectedRelativeCurrentStore(_) => {
                let selected = read_word(layout.selected.start())?;
                let context_depth = read_word(layout.context.start())?;
                self.lifetime.prepare_current_store(core, pc, word, &registers, before, cpsr,
                    CurrentState { selected, context_depth }).map_err(|failure| failure.to_string())
            }
            CodeSiteRole::VfpOwnerStore(_) => {
                let previous = vfp_owner(&memory, layout.kernel, before.0)
                    .map_err(|failure| format!("modal lazy-VFP owner is neither unowned nor a created task: {failure:?}"))?;
                let current = current_task_id(&memory, core)
                    .map_err(|failure| format!("modal lazy-VFP trap has no valid current task: {failure:?}"))?;
                self.lifetime.prepare_vfp_owner_store(core, pc, word, &registers, before, cpsr,
                    VfpOwnerState { before: previous, current }).map_err(|failure| failure.to_string())
            }
            _ => self.lifetime.prepare_store(core, pc, word, &registers, before, cpsr)
                .map_err(|failure| failure.to_string()),
        }
    }

    pub(crate) fn prepare_finish(&mut self, session: &Session, maps: &[MemoryMap],
        active_frames: usize, af: Option<&mut crate::af_ownership::Projection>) -> Result<(), String> {
        self.check(maps,af)?;
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
    pub(crate) fn watch_evidence<'a>(&'a self, maps: &'a [MemoryMap]) -> WatchEvidence<'a> { WatchEvidence { guard: self, records: WatchRecords::Rows, maps } }
    pub(crate) fn watch_summary<'a>(&'a self, maps: &'a [MemoryMap]) -> WatchEvidence<'a> { WatchEvidence { guard: self, records: WatchRecords::Count, maps } }
}

/// Watch-state carries the rows; modal reports carry only their count and digest.
#[derive(Clone, Copy)]
enum WatchRecords { Rows, Count }
pub(crate) struct WatchEvidence<'a> { guard: &'a InstalledGuard, records: WatchRecords, maps: &'a [MemoryMap] }
struct RangeEvidence<'a>(&'a [WatchRange]);
impl Serialize for RangeEvidence<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for range in self.0 { sequence.serialize_element(&[range.start, range.end])?; }
        sequence.end()
    }
}
/// Union of the watched ranges with adjacent ranges merged.
fn union_ranges(writes: &[WatchRange], fetches: &[WatchRange]) -> Vec<[u64; 2]> {
    let mut bounds: Vec<[u64; 2]> = writes.iter().chain(fetches).map(|range| [range.start, range.end]).collect();
    bounds.sort_unstable();
    let mut merged: Vec<[u64; 2]> = Vec::with_capacity(bounds.len());
    for [start, end] in bounds {
        match merged.last_mut() {
            Some(previous) if start <= previous[1] => previous[1] = previous[1].max(end),
            _ => merged.push([start, end]),
        }
    }
    merged
}
/// Write classes for the lifetime extents, then the fetch classes.
struct ClassEvidence<'a> { writes: &'a [WatchRange], fetches: &'a [WatchRange] }
impl Serialize for ClassEvidence<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Class { range: [u64; 2], kinds: &'static [&'static str] }
        let mut sequence = serializer.serialize_seq(Some(self.writes.len() + self.fetches.len()))?;
        for range in self.writes {
            sequence.serialize_element(&Class { range: [range.start, range.end],
                kinds: &["cpu-write", "host-write", "model-bus-write"] })?;
        }
        for range in self.fetches {
            sequence.serialize_element(&Class { range: [range.start, range.end], kinds: &["cpu-fetch"] })?;
        }
        sequence.end()
    }
}
struct LifetimeRow<'a>(&'a serde_json::Value, bool, &'a [MemoryMap]);
impl Serialize for LifetimeRow<'_> {
    fn serialize<S: Serializer>(&self, serializer:S)->Result<S::Ok,S::Error> {
        if !self.1 || !RowProjection::AfAttribution(self.2).validates(self.0) {return self.0.serialize(serializer);}
        let fields=self.0.as_object().ok_or_else(||serde::ser::Error::custom("lifetime projection row is not an object"))?;
        let mut map=serializer.serialize_map(None)?;
        for (key,value) in fields {
            if RowProjection::AfAttribution(self.2).retains(key) {map.serialize_entry(key,value)?;}
        }
        map.end()
    }
}

impl Serialize for WatchEvidence<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Attribution<'a> { sources: [&'static str; 0], ranges: RangeEvidence<'a> }
        let guard = self.guard;
        let records = guard.watch.records.borrow();
        let mut digest = guard.row_digest.borrow_mut();
        let all = records.rows_since(0).map_err(serde::ser::Error::custom)?;
        let old_rows=digest.rows;
        let mut index=0;
        for row in all {
            if !guard.af_ranges.is_empty() && !guard.protects_row(row).map_err(serde::ser::Error::custom)? {continue;}
            if index>=old_rows {
                digest.advance(std::slice::from_ref(&LifetimeRow(row,!guard.af_ranges.is_empty(),self.maps))).map_err(serde::ser::Error::custom)?;
            }
            index+=1;
        }
        let mut state = serializer.serialize_map(Some(if matches!(self.records, WatchRecords::Rows) { 9 } else { 8 }))?;
        state.serialize_entry("enabled", &true)?;
        state.serialize_entry("ranges", &union_ranges(&guard.write_ranges, &guard.fetch_classes))?;
        if matches!(self.records, WatchRecords::Rows) {
            struct Rows<'a> { rows: &'a [serde_json::Value], guard: &'a InstalledGuard, maps: &'a [MemoryMap] }
            impl Serialize for Rows<'_> {
                fn serialize<T: Serializer>(&self, serializer:T) -> Result<T::Ok,T::Error> {
                    let mut sequence=serializer.serialize_seq(None)?;
                    for row in self.rows {
                        if self.guard.af_ranges.is_empty() || self.guard.protects_row(row).map_err(serde::ser::Error::custom)? {
                            sequence.serialize_element(&LifetimeRow(row,!self.guard.af_ranges.is_empty(),self.maps))?;
                        }
                    }
                    sequence.end()
                }
            }
            state.serialize_entry("records", &Rows { rows: all, guard, maps:self.maps })?;
        }
        state.serialize_entry("record_count", &digest.rows)?;
        state.serialize_entry("records_sha256", &digest.hex())?;
        state.serialize_entry("violation", &records.violation())?;
        state.serialize_entry("classes", &ClassEvidence { writes: &guard.write_ranges, fetches: &guard.fetch_classes })?;
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
            core: CoreId(0), progress: EntryProgress(91), word: Word(0xea10_0abc) };
        let encoded = serde_json::to_vec(&fetch).unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, serde_json::json!({"guest":0x00bf_b028,"physical":0x00bf_b028,
            "bytes":[0xbc,0x0a,0x10,0xea],"core":0,"entry_progress":91,"byte_order":"little"}));
    }

    #[test]
    fn row_digest_is_incremental_and_binds_row_content_and_order() {
        let rows = [serde_json::json!({"pc":1,"value":2}), serde_json::json!({"pc":3,"value":4})];
        let mut whole = RowDigest::default();
        whole.advance(&rows).unwrap();
        let mut stepped = RowDigest::default();
        stepped.advance(&rows[..1]).unwrap();
        stepped.advance(&rows[1..]).unwrap();
        assert_eq!((stepped.rows, stepped.hex()), (whole.rows, whole.hex()));
        let mut reordered = RowDigest::default();
        reordered.advance(&[rows[1].clone(), rows[0].clone()]).unwrap();
        let mut changed = RowDigest::default();
        changed.advance(&[rows[0].clone(), serde_json::json!({"pc":3,"value":5})]).unwrap();
        assert_ne!(reordered.hex(), whole.hex());
        assert_ne!(changed.hex(), whole.hex());
        assert_eq!(RowDigest::default().hex(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn adjacent_retired_extents_merge_in_ranges_and_stay_separate_fetch_classes() {
        let writes = [WatchRange { start: 0x0146_7588, end: 0x0146_7840, kinds: WatchKinds::WRITES }];
        let retired = [
            WatchRange { start: 0x00c7_aef4, end: 0x00c7_af6c, kinds: WatchKinds::CPU_FETCH },
            WatchRange { start: 0x00c7_af6c, end: 0x00c7_afc8, kinds: WatchKinds::CPU_FETCH },
        ];
        assert_eq!(union_ranges(&writes, &retired), vec![[0x00c7_aef4, 0x00c7_afc8], [0x0146_7588, 0x0146_7840]]);
        let classes = serde_json::to_value(ClassEvidence { writes: &writes, fetches: &retired }).unwrap();
        assert_eq!(classes, serde_json::json!([
            {"range": [0x0146_7588, 0x0146_7840], "kinds": ["cpu-write", "host-write", "model-bus-write"]},
            {"range": [0x00c7_aef4, 0x00c7_af6c], "kinds": ["cpu-fetch"]},
            {"range": [0x00c7_af6c, 0x00c7_afc8], "kinds": ["cpu-fetch"]},
        ]));
    }
}
