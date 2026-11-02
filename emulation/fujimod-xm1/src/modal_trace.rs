use crate::{
    modal_guard::{BoundFetch, GuardConfig, GuardReport, InstalledGuard, WatchEvidence},
    modal_lifetime::{LifetimeMemory, LifetimeReceipt, LifetimeRequest, Word},
    modal_memory::FixedMemory,
    modal_permit::{CodeRange, ControllerId, CoreId, Cpsr, EntryInstruction, Layout, Monitor,
        Observation, ObservationLimit, Policy, Route, Snapshot, StockFrame, STOCK_END,
        STOCK_ENTRY, STOCK_RESUME},
    modal_task::{self, KernelId, TaskError, TaskExecution},
    provenance::Guest,
    trace::{InstructionCount, read_identity_bytes, read_mapped_bytes, sample_bound_word},
};
use fujimod_emulator::{board::{factory::MemoryMap, observation::InstructionPointEvent}, session::Session};
use fujimod_emulator_cpu::{Register, arm};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use std::{collections::BTreeMap, num::NonZeroU64};

const MAX_OBSERVATIONS: u64 = 65_536;
const MAX_LINKED_BYTES: usize = 1024 * 1024;
const MAX_RETURN_SITES: usize = 4096;
const DEPTH: usize = 32;
const STOCK_PREHOOK: Guest = Guest(0x00bf_b0a0);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedSource {
    start: u32,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginRequest {
    stock: Vec<u8>,
    linked: Option<LinkedSource>,
    seam: Option<[u32; 2]>,
    owned: Option<[u32; 2]>,
    owned_prehook: Option<u32>,
    #[serde(default)]
    helpers: Vec<[u32; 2]>,
    limit: u64,
    #[serde(default)]
    context_sources: Vec<LinkedSource>,
    lifetime: Option<LifetimeRequest>,
}

impl BeginRequest {
    pub fn into_config(self, maps: Vec<MemoryMap>) -> Result<Config, String> {
        let range = |[start, end]: [u32; 2]| CodeRange::new(Guest(start), Guest(end))
            .map_err(|failure| failure.to_string());
        let layout = match (self.seam, self.owned) {
            (None, None) => Layout::stock_baseline(),
            (Some(seam), Some(owned)) => Layout::new(range(seam)?, range(owned)?)
                .map_err(|failure| failure.to_string())?,
            _ => return Err("modal seam and owned ranges must be supplied together".into()),
        };
        let layout = match self.helpers.as_slice() {
            [] => layout,
            [first, second] => layout.with_helpers([range(*first)?, range(*second)?])
                .map_err(|failure| failure.to_string())?,
            _ => return Err("modal helpers must be absent or the complete two-function pair".into()),
        };
        let linked = self.linked.map(|source| SourceImage::new(Guest(source.start), source.bytes)).transpose()?;
        let limit = ObservationLimit(NonZeroU64::new(self.limit)
            .ok_or("modal observation limit must be positive")?);
        let config = Config::new(layout, self.stock, linked, maps, limit)?
            .with_owned_prehook(self.owned_prehook.map(Guest))?;
        match self.lifetime {
            Some(lifetime) => {
                let contexts = self.context_sources.into_iter()
                    .map(|source| SourceImage::new(Guest(source.start), source.bytes))
                    .collect::<Result<Vec<_>, _>>()?;
                config.with_lifetime(contexts, lifetime)
            }
            None if self.context_sources.is_empty() => Ok(config),
            None => Err("task context sources require an installed-lifetime observation".into()),
        }
    }
}

pub struct SourceImage {
    start: Guest,
    bytes: Vec<u8>,
}

impl SourceImage {
    pub fn new(start: Guest, bytes: Vec<u8>) -> Result<Self, String> {
        if start.0 % 4 != 0 || bytes.is_empty() || bytes.len() % 4 != 0
            || bytes.len() > MAX_LINKED_BYTES
            || u64::from(start.0) + bytes.len() as u64 > 1 << 32 {
            return Err("modal source image is not bounded A32 text".into());
        }
        Ok(Self { start, bytes })
    }

    pub(crate) fn start(&self) -> Guest { self.start }
    pub(crate) fn bytes(&self) -> &[u8] { &self.bytes }

    fn contains(&self, range: CodeRange) -> bool {
        self.start <= range.start()
            && u64::from(range.end().0) <= u64::from(self.start.0) + self.bytes.len() as u64
    }

    fn includes(&self, pc: Guest) -> bool {
        self.start <= pc && u64::from(pc.0) < u64::from(self.start.0) + self.bytes.len() as u64
    }
}

pub struct Config {
    layout: Layout,
    limit: ObservationLimit,
    images: Vec<SourceImage>,
    maps: Vec<MemoryMap>,
    owned_prehook: Option<Guest>,
    entry_branch: Option<Word>,
    guard: Option<GuardConfig>,
}

impl Config {
    pub fn new(layout: Layout, mut stock: Vec<u8>, linked: Option<SourceImage>,
        maps: Vec<MemoryMap>, limit: ObservationLimit) -> Result<Self, String> {
        if stock.len() != (STOCK_END.0 - STOCK_ENTRY.0) as usize
            || stock.get(..4) != Some(0xe92d_4ff0u32.to_le_bytes().as_slice()) {
            return Err("modal stock source is not the complete pinned dispatcher".into());
        }
        if limit.0.get() > MAX_OBSERVATIONS {
            return Err("modal observation limit exceeds its bound".into());
        }
        let mut entry_branch = None;
        match (layout.policy(), linked.as_ref()) {
            (Policy::StockBaseline, None) => {}
            (Policy::ScopedOwned75, Some(image)) => {
                let seam = layout.seam().ok_or("modal seam is missing")?;
                let owned = layout.owned().ok_or("owned modal range is missing")?;
                if !image.contains(seam) || !image.contains(owned)
                    || layout.helpers().any(|range| !image.contains(range))
                    || image.includes(STOCK_ENTRY) || image.includes(Guest(STOCK_END.0 - 4)) {
                    return Err("linked modal ranges leave their bound source image".into());
                }
                let delta = i64::from(seam.start().0) - i64::from(STOCK_ENTRY.0) - 8;
                if !(-(1 << 25)..(1 << 25)).contains(&delta) {
                    return Err("modal entry branch leaves the A32 branch domain".into());
                }
                let branch = 0xea00_0000 | (((delta as u32) >> 2) & 0x00ff_ffff);
                stock[..4].copy_from_slice(&branch.to_le_bytes());
                entry_branch = Some(Word(branch));
            }
            _ => return Err("modal source images disagree with the selected policy".into()),
        }
        let mut images = Vec::with_capacity(2);
        images.push(SourceImage::new(STOCK_ENTRY, stock)?);
        images.extend(linked);
        Ok(Self { layout, limit, images, maps, owned_prehook: None, entry_branch, guard: None })
    }

    pub fn with_owned_prehook(mut self, pc: Option<Guest>) -> Result<Self, String> {
        if let Some(pc) = pc {
            if pc.0 % 4 != 0 || self.layout.policy() != Policy::ScopedOwned75
                || !self.images.iter().skip(1).any(|image| image.includes(pc)) {
                return Err("owned prehook leaves the linked modal source image".into());
            }
        }
        self.owned_prehook = pc;
        Ok(self)
    }

    fn with_lifetime(mut self, contexts: Vec<SourceImage>, request: LifetimeRequest) -> Result<Self, String> {
        if self.layout.policy() == Policy::ScopedOwned75 {
            let mut helpers = self.layout.helpers();
            let prehook = helpers.next().ok_or("scoped lifetime requires both complete owned modal helpers")?;
            if helpers.count() != 1 || self.owned_prehook != Some(prehook.start()) {
                return Err("scoped lifetime prehook does not bind its complete helper pair".into());
            }
        }
        self.images[0].bytes[..4].copy_from_slice(&0xe92d_4ff0u32.to_le_bytes());
        self.guard = Some(GuardConfig::new(contexts, request, &self.images, self.entry_branch)?);
        Ok(self)
    }

    fn entry_instruction(&self, pc: Guest, word: Word) -> Result<EntryInstruction, String> {
        if pc != STOCK_ENTRY { return Ok(EntryInstruction::NotEntry); }
        if word == Word(0xe92d_4ff0) { return Ok(EntryInstruction::OriginalPush); }
        if Some(word) == self.entry_branch { return Ok(EntryInstruction::ScopedBranch); }
        Err("modal shared entry differs from its original or published source instruction".into())
    }

    pub fn owns_memory_watch(&self) -> bool { self.guard.is_some() }

    pub(crate) fn refuses_prior_stop(&self, pc: Guest) -> bool {
        self.guard.as_ref().is_some_and(|guard| guard.contains(pc)
            || self.instruction_sites().any(|site| site == pc))
    }

    pub fn instruction_sites(&self) -> impl Iterator<Item = Guest> + '_ {
        let stock = (STOCK_ENTRY.0..STOCK_END.0).step_by(4).map(Guest);
        let seam = self.layout.seam().into_iter()
            .flat_map(|range| (range.start().0..range.end().0).step_by(4).map(Guest));
        let owned = self.layout.owned().into_iter()
            .flat_map(|range| (range.start().0..range.end().0).step_by(4).map(Guest));
        let helpers = self.layout.helpers()
            .flat_map(|range| (range.start().0..range.end().0).step_by(4).map(Guest));
        stock.chain(seam).chain(owned).chain(helpers).chain(self.owned_prehook)
    }
}

#[derive(Clone, Copy, Serialize)]
pub struct StockFetch {
    #[serde(serialize_with = "serialize_core")]
    pub core: CoreId,
    #[serde(serialize_with = "serialize_guest")]
    pub pc: Guest,
    pub instructions: InstructionCount,
}

#[derive(Clone, Copy, Serialize)]
pub struct CapturedDomain {
    #[serde(serialize_with = "serialize_core")]
    pub core: CoreId,
    #[serde(serialize_with = "serialize_guest")]
    pub pc: Guest,
    pub instructions: InstructionCount,
    pub controller: u32,
    pub state: u32,
    pub event: u32,
}

#[derive(Clone, Copy)]
struct TaskIdentityEvidence(TaskExecution);
impl Serialize for TaskIdentityEvidence {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let identity = self.0.identity();
        let stack = self.0.stack().physical();
        let mut state = serializer.serialize_map(Some(5))?;
        state.serialize_entry("kernel", match identity.kernel {
            KernelId::Primary => "primary", KernelId::Secondary => "secondary",
        })?;
        state.serialize_entry("id", &identity.id.get())?;
        state.serialize_entry("node", &identity.node.address().0)?;
        state.serialize_entry("creation", &identity.creation.address().0)?;
        state.serialize_entry("stack", &[stack.start().0, stack.end().0])?;
        state.end()
    }
}

#[derive(Clone, Copy, Serialize)]
struct TaskEntry {
    #[serde(serialize_with = "serialize_core")]
    core: CoreId,
    #[serde(serialize_with = "serialize_guest")]
    pc: Guest,
    instructions: InstructionCount,
    #[serde(serialize_with = "serialize_controller")]
    controller: ControllerId,
    #[serde(serialize_with = "serialize_guest")]
    sp: Guest,
    #[serde(serialize_with = "serialize_guest")]
    lr: Guest,
    #[serde(serialize_with = "serialize_cpsr")]
    cpsr: Cpsr,
    identity: TaskIdentityEvidence,
    fetch: BoundFetch,
}

#[derive(Serialize)]
pub struct Counts {
    pub observations: u64,
    pub entries: u64,
    pub owned_handoffs: u64,
    pub fallback_handoffs: u64,
    pub returns: u64,
    pub active_frames: usize,
    pub peak_core_depth: usize,
}

#[derive(Serialize)]
pub struct Report<'a> {
    pub enabled: bool,
    #[serde(serialize_with = "serialize_policy")]
    pub policy: Policy,
    pub counts: Counts,
    pub stock_fetches: &'a [StockFetch],
    pub captured_domains: &'a [CapturedDomain],
    pub failure: Option<&'a str>,
    pub physical_fetch_coverage: bool,
    pub task_identity_verified: bool,
    physical_guard: Option<GuardReport<'a>>,
    context_source_spans: usize,
    task_entries: &'a [TaskEntry],
    lifetime: Option<LifetimeReceipt>,
    watch: Option<WatchEvidence<'a>>,
}

impl Report<'_> {
    pub(crate) fn inactive() -> Self {
        Self {
            enabled: false, policy: Policy::StockBaseline,
            counts: Counts {
                observations: 0, entries: 0, owned_handoffs: 0, fallback_handoffs: 0,
                returns: 0, active_frames: 0, peak_core_depth: 0,
            },
            stock_fetches: &[], captured_domains: &[], failure: None,
            physical_fetch_coverage: false, task_identity_verified: false,
            physical_guard: None, context_source_spans: 0, task_entries: &[],
            lifetime: None, watch: None,
        }
    }
}

fn serialize_core<S: serde::Serializer>(core: &CoreId, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(core.0 as u64)
}

fn serialize_guest<S: serde::Serializer>(guest: &Guest, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u32(guest.0)
}

fn serialize_controller<S: Serializer>(controller: &ControllerId, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u32(controller.0)
}

fn serialize_cpsr<S: Serializer>(cpsr: &Cpsr, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u32(cpsr.0)
}

fn serialize_policy<S: serde::Serializer>(policy: &Policy, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match policy {
        Policy::StockBaseline => "stock-baseline",
        Policy::ScopedOwned75 => "scoped-owned75",
    })
}

pub(crate) struct ModalTrace {
    config: Config,
    monitor: Monitor<DEPTH>,
    returns: BTreeMap<Guest, [u8; 4]>,
    stock_fetches: Vec<StockFetch>,
    captured_domains: Vec<CapturedDomain>,
    task_entries: Vec<TaskEntry>,
    guard: Option<InstalledGuard>,
    failure: Option<String>,
    enabled: bool,
}

impl ModalTrace {
    pub(crate) fn new(mut config: Config, session: &Session) -> Result<Self, String> {
        if session.board.instruction_counts().len() != 2 {
            return Err("modal observation requires the two X-M1 cores".into());
        }
        if session.board.mappings() != config.maps.as_slice() {
            return Err("modal source mappings differ from the current board".into());
        }
        for core in 0..2 {
            for image in &config.images {
                for (offset, expected) in image.bytes.chunks_exact(4).enumerate() {
                    let mut observed = [0; 4];
                    read_identity_bytes(session, core, image.start.0 + offset as u32 * 4,
                        &mut observed, &config.maps)?;
                    if observed != expected {
                        return Err("modal source differs from guest code".into());
                    }
                }
            }
        }
        let capacity = config.limit.0.get() as usize;
        let monitor = Monitor::new(config.layout, config.limit);
        let guard = config.guard.take().map(|guard| InstalledGuard::new(guard, session,
            &config.maps, config.instruction_sites(), config.layout)).transpose()?;
        let task_capacity = if guard.is_some() && config.layout.policy() == Policy::ScopedOwned75 { capacity } else { 0 };
        Ok(Self { config, monitor, returns: BTreeMap::new(),
            stock_fetches: Vec::with_capacity(capacity),
            captured_domains: Vec::with_capacity(capacity), task_entries: Vec::with_capacity(task_capacity),
            guard, failure: None, enabled: true })
    }

    pub(crate) fn sites(&self) -> impl Iterator<Item = Guest> + '_ {
        self.config.instruction_sites().chain(self.guard.iter().flat_map(InstalledGuard::sites))
            .chain(self.returns.keys().copied())
    }

    pub(crate) fn active(&self) -> bool { self.enabled }

    pub(crate) fn contains(&self, pc: Guest) -> bool {
        (STOCK_ENTRY <= pc && pc < STOCK_END)
            || [self.config.layout.seam(), self.config.layout.owned()].into_iter().flatten()
                .any(|range| range.start() <= pc && pc < range.end())
            || self.config.owned_prehook == Some(pc) || self.returns.contains_key(&pc)
            || self.guard.as_ref().is_some_and(|guard| guard.contains(pc))
    }

    pub(crate) fn fail(&mut self, failure: String) {
        self.failure.get_or_insert(failure);
    }

    pub(crate) fn guarded(&self) -> bool { self.guard.is_some() }

    pub(crate) fn check_interval(&mut self) -> Result<(), String> {
        if let Some(failure) = &self.failure { return Err(failure.clone()); }
        if let Some(guard) = &mut self.guard {
            if let Err(failure) = guard.check() {
                self.failure = Some(failure.clone());
                return Err(failure);
            }
        }
        Ok(())
    }

    pub(crate) fn observe(&mut self, session: &Session, core: usize, pc: Guest,
        instructions: InstructionCount, point: Option<InstructionPointEvent>) -> Result<Option<Guest>, String> {
        if let Some(failure) = &self.failure { return Err(failure.clone()); }
        let observed = self.observe_inner(session, core, pc, instructions, point);
        if let Err(failure) = &observed { self.failure = Some(failure.clone()); }
        observed
    }

    fn observe_inner(&mut self, session: &Session, core: usize, pc: Guest,
        instructions: InstructionCount, point: Option<InstructionPointEvent>) -> Result<Option<Guest>, String> {
        if !self.contains(pc) { return Err("instruction point outside modal observation".into()); }
        let cpsr = Cpsr(read_register(session, core, arm::Register::Cpsr)?);
        let bound_fetch = if let Some(guard) = &mut self.guard {
            let point = point.ok_or("modal point lacks its stable instruction-entry identity")?;
            if session.board.read_register(core, Register::Pc)? != u64::from(pc.0) {
                return Err("modal point does not name the current instruction".into());
            }
            guard.bind_point(CoreId(core), pc, point, cpsr)?
        } else { None };
        let word = if self.guard.is_some() && pc == STOCK_ENTRY {
            let mut bytes = [0; 4];
            LifetimeMemory::read(&FixedMemory { session, maps: &self.config.maps }, pc, &mut bytes)
                .map_err(|failure| failure.to_string())?;
            Word(u32::from_le_bytes(bytes))
        } else if let Some(image) = self.config.images.iter().find(|image| image.includes(pc)) {
            if self.guard.is_none() {
                sample_bound_word(session, core, pc.0, image.start.0, &image.bytes, &self.config.maps)?
                    .ok_or("monitored instruction leaves its bound source image")?;
            }
            let offset = (pc.0 - image.start.0) as usize;
            Word(u32::from_le_bytes(image.bytes[offset..offset + 4].try_into()
                .map_err(|_| "modal source word is truncated")?))
        } else if let Some(word) = self.guard.as_ref().and_then(|guard| guard.source_word(pc)) {
            word
        } else if let Some(expected) = self.returns.get(&pc) {
            sample_bound_word(session, core, pc.0, pc.0, expected, &self.config.maps)?
                .ok_or("modal return instruction leaves its source word")?;
            Word(u32::from_le_bytes(*expected))
        } else {
            return Err("modal instruction has no bound source word".into());
        };
        if let Some(guard) = self.guard.as_mut().filter(|guard| guard.contains(pc)) {
            guard.observe_source(session, &self.config.maps, CoreId(core), pc, word, cpsr)?;
            return Ok(None);
        }
        let entry_instruction = self.config.entry_instruction(pc, word)?;
        if self.stock_fetches.len() == self.config.limit.0.get() as usize
            || self.captured_domains.len() == self.config.limit.0.get() as usize {
            return Err("modal evidence limit reached".into());
        }
        let mut registers = [0; 13];
        for (index, value) in registers.iter_mut().enumerate() {
            *value = read_register(session, core, arm::Register::R(index as u8))?;
        }
        let sp = Guest(read_register(session, core, arm::Register::R(13))?);
        let lr = Guest(read_register(session, core, arm::Register::R(14))?);
        if STOCK_RESUME <= pc && pc < STOCK_END {
            self.stock_fetches.push(StockFetch { core: CoreId(core), pc, instructions });
        }
        if pc == STOCK_PREHOOK || self.config.owned_prehook == Some(pc) {
            self.captured_domains.push(CapturedDomain {
                core: CoreId(core), pc, instructions,
                controller: registers[0], state: registers[1], event: registers[2],
            });
            let in_owned = self.config.layout.owned()
                .is_some_and(|range| range.start() <= pc && pc < range.end())
                || self.config.layout.helpers().any(|range| range.start() <= pc && pc < range.end());
            if pc != STOCK_PREHOOK && !in_owned && !self.returns.contains_key(&pc) {
                if cpsr.0 & (1 << 5 | 1 << 24) != 0 || sp.0 % 4 != 0 {
                    return Err("captured modal prehook is not in aligned A32 state".into());
                }
                return Ok(None);
            }
        }
        let saved_stock_frame = if pc == STOCK_RESUME {
            let mut bytes = [0; 36];
            read_mapped_bytes(session, core, sp.0, &mut bytes, &self.config.maps)?;
            let words = std::array::from_fn(|index| {
                let offset = index * 4;
                u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
            });
            Some(StockFrame(words))
        } else { None };
        let needs_task = self.guard.is_some() && (self.monitor.has_owned_frame(CoreId(core))
            || (entry_instruction == EntryInstruction::ScopedBranch && registers[0] == 75));
        let task = if needs_task {
            modal_task::sample(&FixedMemory { session, maps: &self.config.maps }, CoreId(core), sp, cpsr)
        } else { Err(TaskError::Unavailable) };
        let observation = self.monitor.observe(Snapshot {
            core: CoreId(core), pc, registers, sp, lr, cpsr, saved_stock_frame, task, entry_instruction,
        }).map_err(|failure| failure.to_string())?;
        if let Observation::Entry { route: Route::Owned75, .. } = observation {
            if self.task_entries.len() == self.config.limit.0.get() as usize {
                return Err("modal task-entry evidence limit reached".into());
            }
            self.task_entries.push(TaskEntry {
                core: CoreId(core), pc, instructions, controller: ControllerId(registers[0]), sp, lr, cpsr,
                identity: TaskIdentityEvidence(task.map_err(|failure| format!("{failure:?}"))?),
                fetch: bound_fetch.ok_or("owned entry lacks its physical fetch binding")?,
            });
        }
        match observation {
            Observation::Entry { arm_return, .. } if !self.returns.contains_key(&arm_return) => {
                if self.returns.len() == MAX_RETURN_SITES {
                    return Err("modal return-site limit reached".into());
                }
                let mut word = [0; 4];
                read_identity_bytes(session, core, arm_return.0, &mut word, &self.config.maps)?;
                self.returns.insert(arm_return, word);
                Ok(Some(arm_return))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn report(&self) -> Report<'_> {
        let counts = self.monitor.receipt();
        let healthy_guard = self.failure.is_none() && self.guard.as_ref().is_some_and(InstalledGuard::healthy);
        let context_source_spans = self.guard.as_ref().map_or(0, InstalledGuard::context_count);
        Report {
            enabled: self.enabled, policy: self.config.layout.policy(),
            counts: Counts {
                observations: counts.observations.0, entries: counts.entries.0,
                owned_handoffs: counts.owned_handoffs.0, fallback_handoffs: counts.fallback_handoffs.0,
                returns: counts.returns.0, active_frames: counts.active_frames.0,
                peak_core_depth: counts.peak_core_depth.0,
            },
            stock_fetches: &self.stock_fetches, captured_domains: &self.captured_domains,
            failure: self.failure.as_deref(), physical_fetch_coverage: healthy_guard,
            task_identity_verified: healthy_guard && context_source_spans == 16 && !self.task_entries.is_empty(),
            physical_guard: self.guard.as_ref().and_then(InstalledGuard::report),
            context_source_spans, task_entries: &self.task_entries,
            lifetime: self.guard.as_ref().map(InstalledGuard::receipt),
            watch: self.guard.as_ref().map(InstalledGuard::watch_evidence),
        }
    }

    pub(crate) fn check_finish(&mut self, session: &Session) -> Result<(), String> {
        if !self.enabled { return Err("modal observation is not active".into()); }
        self.check_interval()?;
        if let Err(failure) = self.monitor.finish() {
            let failure = failure.to_string();
            self.failure = Some(failure.clone());
            return Err(failure);
        }
        if let Some(guard) = &mut self.guard {
            if let Err(failure) = guard.prepare_finish(session, &self.config.maps, self.monitor.receipt().active_frames.0) {
                self.failure = Some(failure.clone());
                return Err(failure);
            }
        }
        Ok(())
    }

    pub(crate) fn finish_guard(&mut self, session: &Session) -> Result<(), String> {
        if let Some(guard) = &mut self.guard { guard.close(session)?; }
        Ok(())
    }

    pub(crate) fn disable(&mut self) { self.enabled = false; }
}

fn read_register(session: &Session, core: usize, register: arm::Register) -> Result<u32, String> {
    u32::try_from(session.board.read_register(core, Register::Arm32(register))?)
        .map_err(|_| "modal register exceeds32bits".to_owned())
}
