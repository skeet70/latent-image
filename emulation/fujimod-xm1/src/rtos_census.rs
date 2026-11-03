//! Opt-in stock census of RTOS root-slot calls for #156. Instruction points stop both
//! cores at the 194 published slot targets and at each entered call's continuation.
//! A classified fetch watch over the 194 entry words is begun after every point
//! change, so each stopped entry owns one stopped and one resumed fetch row.

use flate2::{Compression, GzBuilder, write::GzEncoder};
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::machine::{Machine, RunOutcome};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

const USAGE: &str = "rtos-census --run RUN --update STOCK_DAT --boot-rom ROM --analysis ROOT --manifest MANIFEST --trajectory TRAJECTORY --output NEW_DIR";
const RUN_SHA256: &str = "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef";
const DAT_SHA256: &str = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";
const ROM_SHA256: &str = "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be";
const LOADER_A_SHA256: &str = "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882";
const LOADER_B_SHA256: &str = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec";
const SLOTS_PER_DOMAIN: usize = 97;
const ENTRY_LABEL: &str = "rtos-entry";
const RETURN_LABEL: &str = "rtos-return";

/// The pinned engine refuses more than 4,096 instruction points per core.
const SITE_LIMIT: usize = 4096;
const PENDING_LIMIT: usize = 4096;
const EVENT_LIMIT: u64 = 8_000_000;
const INSTRUCTION_LIMIT: u64 = 1_500_000_000;
const WATCH_LIMIT: usize = 1 << 20;
const SEGMENT_STOPS: usize = 32_768;
const SEGMENT_INSTRUCTIONS: u64 = 50_000_000;
const UNSTOPPED_SAMPLE: usize = 1024;
const CALLER_LIMIT: usize = 512;
const RESULT_LIMIT: usize = 64;

const INITIAL_MENU: u64 = 80_000_000;
const SETUP_SETTLE: u64 = 40_000_000;
const SHOOTING_SETTLE: u64 = 90_000_000;
const SETUP_HOLD: u64 = 8_000_000;
const SETUP_RELEASE: u64 = 8_000_000;
const RELEASE_POLL: u64 = 250_000;
const MENU_POLL: u64 = 250_000;
const STEP_STATE_CAP: u64 = 90_000_000;
const STEP_SETTLE: u64 = 40_000_000;

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}

fn hex<S: Serializer>(value: u32, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{value:#x}"))
}

fn parse_hex(text: &str) -> Result<u32, String> {
    let digits = text.strip_prefix("0x").ok_or_else(|| format!("{text}: hexadecimal prefix missing"))?;
    u32::from_str_radix(digits, 16).map_err(|_| format!("{text}: invalid hexadecimal word"))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Address(u32);
impl Serialize for Address {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { hex(self.0, serializer) }
}

/// Byte offset of a callable word within a domain root.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Offset(u32);
impl Serialize for Offset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { hex(self.0, serializer) }
}
impl Offset {
    fn callable(self) -> bool {
        self.0 % 4 == 0 && (self.0 <= 0x170 || [0x1618, 0x161c, 0x1620, 0x1624].contains(&self.0))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
enum Core { Primary, Secondary }

/// Kernel cells of the domain that a core's own kernel uses.
struct KernelCells { current_task: Address, task_base: u32, context: Address }

impl Core {
    fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
    fn from_index(index: usize) -> Result<Self, String> {
        match index { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("census expects two cores".into()) }
    }
    fn kernel(self) -> KernelCells {
        match self {
            Self::Primary => KernelCells { current_task: Address(0x4005_4760), task_base: 0x4005_ff0c, context: Address(0x4005_45a4) },
            Self::Secondary => KernelCells { current_task: Address(0x400e_4720), task_base: 0x400e_fecc, context: Address(0x400e_4564) },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum Domain { A, B }
impl Domain {
    fn root(self) -> Address { Address(match self { Self::A => 0x4004_dae0, Self::B => 0x400d_daa0 }) }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Recovery { Recovered, Partial, Unknown }

#[derive(Clone, Copy, Debug, Serialize)]
struct Slot { domain: Domain, offset: Offset, target: Address, recovery: Recovery }

#[derive(Deserialize)]
struct Manifest { firmware: String, domains: Vec<ManifestDomain> }
#[derive(Deserialize)]
struct ManifestDomain { domain: Domain, root: String, slots: Vec<ManifestSlot> }
#[derive(Deserialize)]
struct ManifestSlot { offset: String, target: String, recovery: Recovery }

/// Every slot keyed by target; two slots never share a target.
fn load_manifest(bytes: &[u8]) -> Result<BTreeMap<Address, Slot>, String> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|error| format!("manifest: {error}"))?;
    require(manifest.firmware == "X-M1 1.50" && manifest.domains.len() == 2, "manifest firmware or domain count differs")?;
    let mut domains = BTreeSet::new();
    let mut slots = BTreeMap::new();
    for domain in manifest.domains {
        require(domains.insert(domain.domain) && Address(parse_hex(&domain.root)?) == domain.domain.root()
            && domain.slots.len() == SLOTS_PER_DOMAIN, "manifest domain geometry differs")?;
        let mut offsets = BTreeSet::new();
        for slot in domain.slots {
            let offset = Offset(parse_hex(&slot.offset)?);
            let target = Address(parse_hex(&slot.target)?);
            require(offset.callable() && offsets.insert(offset) && target.0 % 4 == 0, "manifest slot offset or target is invalid")?;
            require(slots.insert(target, Slot { domain: domain.domain, offset, target, recovery: slot.recovery }).is_none(),
                "two manifest slots share a target")?;
        }
    }
    require(slots.len() == 2 * SLOTS_PER_DOMAIN, "manifest does not hold 194 slot targets")?;
    Ok(slots)
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "view", rename_all = "kebab-case")]
enum WordView {
    Committed { value: u32 },
    /// The executing core's TCM overlays the word.
    TcmOverlay,
    ViewsDiffer { guest: u32, backing: u32 },
}
impl WordView {
    fn committed(self) -> Option<u32> { if let Self::Committed { value } = self { Some(value) } else { None } }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct TaskView {
    /// The executing core's own kernel's current task node.
    current: WordView,
    /// One-based task identifier when `current` lies in that kernel's 120-node table.
    index: Option<u32>,
    kernel_context: WordView,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Context {
    core: Core,
    pc: Address,
    lr: Address,
    sp: Address,
    cpsr: u32,
    r0_r3: [u32; 4],
    instructions: u64,
    counts: [u64; 2],
    task: TaskView,
}
impl Context {
    fn mode(&self) -> u32 { self.cpsr & 0x1f }
}

struct Board<'a> { machine: &'a Machine, maps: &'a [MemoryMap] }
impl Board<'_> {
    fn register(&self, core: Core, register: arm::Register) -> Result<u32, String> {
        u32::try_from(self.machine.session.board.read_register(core.index(), Register::Arm32(register))?)
            .map_err(|_| "ARM register exceeds 32 bits".to_owned())
    }
    fn tcm(&self, core: Core) -> Result<[u32; 2], String> {
        let mut banks = [0; 2];
        for (bank, value) in banks.iter_mut().enumerate() {
            *value = u32::try_from(self.machine.session.board.cpu(core.index())?.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false,
            }, None)?).map_err(|_| "TCM region register exceeds 32 bits".to_owned())?;
        }
        Ok(banks)
    }
    fn word(&self, core: Core, tcm: [u32; 2], address: Address) -> Result<WordView, String> {
        let low = u64::from(address.0);
        let high = low + 4;
        for bank in tcm {
            if tcm_window(TcmRegister(bank))?.is_some_and(|(start, end)| start < high && low < end) {
                return Ok(WordView::TcmOverlay);
            }
        }
        let span = static_span(self.maps, MemoryCore(core.index()), MemoryAddress(low)..MemoryAddress(high))?;
        let (mut guest, mut backing) = ([0; 4], [0; 4]);
        self.machine.session.board.read_memory(core.index(), low, &mut guest)?;
        self.machine.session.board.read_physical(span.physical.0, &mut backing)?;
        let (guest, backing) = (u32::from_le_bytes(guest), u32::from_le_bytes(backing));
        Ok(if guest == backing { WordView::Committed { value: guest } } else { WordView::ViewsDiffer { guest, backing } })
    }
    fn context(&self, core: Core) -> Result<(Context, [u32; 2]), String> {
        let tcm = self.tcm(core)?;
        let cells = core.kernel();
        let current = self.word(core, tcm, cells.current_task)?;
        let index = current.committed()
            .and_then(|node| node.checked_sub(cells.task_base))
            .filter(|offset| offset % 56 == 0 && offset / 56 < 120)
            .map(|offset| offset / 56 + 1);
        let mut r0_r3 = [0; 4];
        for (index, value) in r0_r3.iter_mut().enumerate() {
            *value = self.register(core, arm::Register::R(index as u8))?;
        }
        let counts: [u64; 2] = self.machine.session.board.instruction_counts().try_into()
            .map_err(|_| "census expects two core clocks".to_owned())?;
        Ok((Context {
            core,
            pc: Address(self.register(core, arm::Register::R(15))?),
            lr: Address(self.register(core, arm::Register::R(14))?),
            sp: Address(self.register(core, arm::Register::R(13))?),
            cpsr: self.register(core, arm::Register::Cpsr)?,
            r0_r3,
            instructions: self.machine.instructions(),
            counts,
            task: TaskView { current, index, kernel_context: self.word(core, tcm, cells.context)? },
        }, tcm))
    }
}

/// A call completes when its core next stops at its continuation with the entry's
/// stack pointer and processor mode; usr, svc and sys calls also need the entry's
/// executing-core current task.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ReturnKey { core: Core, pc: Address, sp: Address, task: Option<u32>, mode: u32 }
impl ReturnKey {
    fn of(context: &Context, pc: Address) -> Self {
        let task = [0x10, 0x13, 0x1f].contains(&context.mode()).then(|| context.task.current.committed()).flatten();
        Self { core: context.core, pc, sp: context.sp, task, mode: context.mode() }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OpenCall { id: u64, target: Address, entered: u64 }

/// Calls sharing a key complete together; `chain` reports how many.
#[derive(Default)]
struct OpenCalls { pending: BTreeMap<ReturnKey, Vec<OpenCall>>, count: usize }
impl OpenCalls {
    fn open(&mut self, key: ReturnKey, call: OpenCall) -> bool {
        if self.count >= PENDING_LIMIT { return false; }
        self.pending.entry(key).or_default().push(call);
        self.count += 1;
        true
    }
    fn close(&mut self, key: &ReturnKey) -> Vec<OpenCall> {
        let closed = self.pending.remove(key).unwrap_or_default();
        self.count -= closed.len();
        closed
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FetchSource { CpuFetch }

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct FetchRow { source: FetchSource, address: u64, physical: Option<u64>, core: usize, instructions: Option<u64> }

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StopKey { core: usize, address: u64, instructions: u64 }

/// Reconciles each watch segment's entry-word fetch rows with that segment's entry stops.
#[derive(Default, Serialize)]
struct FetchLedger {
    #[serde(skip)]
    awaiting: [Option<u64>; 2],
    #[serde(skip)]
    stops: BTreeMap<StopKey, u64>,
    stopped: u64,
    resumed: u64,
    stops_without_fetch: u64,
    stops_without_fetch_sample: Vec<u64>,
    unstopped: u64,
    unstopped_sample: Vec<FetchRow>,
    segments: u64,
}
impl FetchLedger {
    fn stop(&mut self, core: Core, pc: Address, instructions: u64, call: u64) {
        self.stops.insert(StopKey { core: core.index(), address: u64::from(pc.0), instructions }, call);
    }
    /// A stop owns the row at its own clock; the core's next row at that address is its resumption.
    fn reconcile(&mut self, rows: impl IntoIterator<Item = FetchRow>) -> Result<(), String> {
        for row in rows {
            let awaiting = self.awaiting.get_mut(row.core).ok_or("fetch row names an unknown core")?;
            let stop = row.instructions.map(|instructions| StopKey { core: row.core, address: row.address, instructions });
            if *awaiting == Some(row.address) {
                *awaiting = None;
                self.resumed += 1;
            } else if stop.is_some_and(|stop| self.stops.remove(&stop).is_some()) {
                *awaiting = Some(row.address);
                self.stopped += 1;
            } else {
                self.unstopped += 1;
                if self.unstopped_sample.len() < UNSTOPPED_SAMPLE { self.unstopped_sample.push(row); }
            }
        }
        self.flush();
        Ok(())
    }
    /// Ends the segment; its unmatched stops have no fetch row.
    fn flush(&mut self) {
        for call in std::mem::take(&mut self.stops).into_values() {
            self.stops_without_fetch += 1;
            if self.stops_without_fetch_sample.len() < UNSTOPPED_SAMPLE { self.stops_without_fetch_sample.push(call); }
        }
        self.segments += 1;
    }
}

struct FetchWatch { watch: Box<MemoryWatch>, classes: Vec<WatchRange>, stops: usize, started: u64 }
impl FetchWatch {
    fn begin(machine: &Machine, maps: &[MemoryMap], classes: Vec<WatchRange>) -> Result<Self, String> {
        let watch = machine.session.board.memory_watch_begin_classified(classes.clone(), WATCH_LIMIT, maps.to_vec(), ViewPolicy::Observe)?;
        Ok(Self { watch, classes, stops: 0, started: machine.instructions() })
    }
    fn end(mut self, machine: &Machine, ledger: &mut FetchLedger) -> Result<(), String> {
        let state = self.watch.records.borrow().state();
        let ended = machine.session.board.memory_watch_end(&mut self.watch);
        if let Some(violation) = state.get("violation").and_then(serde_json::Value::as_str) {
            ledger.flush();
            return Err(format!("entry fetch watch: {violation}"));
        }
        let rows = state.get("records").and_then(serde_json::Value::as_array).ok_or("entry fetch watch has no records")?;
        let rows = rows.iter().map(|row| FetchRow::deserialize(row).map_err(|error| format!("entry fetch row: {error}")))
            .collect::<Result<Vec<_>, _>>()?;
        ledger.reconcile(rows)?;
        ended
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ReturnTracking { Pending, SiteBound, PendingBound, NoContinuation }

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
enum Event<'a> {
    Entry {
        call: u64,
        slot: &'a Slot,
        phase: &'a str,
        context: &'a Context,
        published: WordView,
        code: WordView,
        canonical: u32,
        tracking: ReturnTracking,
    },
    Return { call: u64, target: Address, context: &'a Context, elapsed: u64, chain: usize },
    Pending { call: u64, target: Address, entered: u64 },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct ContextClass { core: Core, mode: u32, irq_masked: bool, fiq_masked: bool }

#[derive(Serialize)]
struct Counted<K> { key: K, count: u64 }

fn counted<K: Clone>(map: &BTreeMap<K, u64>) -> Vec<Counted<K>> {
    let mut entries: Vec<_> = map.iter().map(|(key, &count)| Counted { key: key.clone(), count }).collect();
    entries.sort_by(|left, right| right.count.cmp(&left.count));
    entries
}

#[derive(Default)]
struct Reach {
    calls: u64,
    returns: u64,
    untracked: u64,
    first: Option<u64>,
    last: Option<u64>,
    elapsed_total: u64,
    elapsed_max: u64,
    phases: BTreeMap<String, u64>,
    contexts: BTreeMap<ContextClass, u64>,
    tasks: BTreeMap<Option<u32>, u64>,
    callers: BTreeMap<Address, u64>,
    callers_overflow: u64,
    results: BTreeMap<i32, u64>,
    results_overflow: u64,
    published_mismatch: u64,
    code_mismatch: u64,
}
impl Reach {
    fn enter(&mut self, phase: &str, context: &Context, tracked: bool) {
        self.calls += 1;
        self.untracked += u64::from(!tracked);
        self.first.get_or_insert(context.instructions);
        self.last = Some(context.instructions);
        *self.phases.entry(phase.to_owned()).or_default() += 1;
        let class = ContextClass { core: context.core, mode: context.mode(), irq_masked: context.cpsr & 0x80 != 0, fiq_masked: context.cpsr & 0x40 != 0 };
        *self.contexts.entry(class).or_default() += 1;
        *self.tasks.entry(context.task.index).or_default() += 1;
        if let Some(count) = self.callers.get_mut(&context.lr) {
            *count += 1;
        } else if self.callers.len() < CALLER_LIMIT {
            self.callers.insert(context.lr, 1);
        } else {
            self.callers_overflow += 1;
        }
    }
    fn complete(&mut self, context: &Context, elapsed: u64) {
        self.returns += 1;
        self.elapsed_total += elapsed;
        self.elapsed_max = self.elapsed_max.max(elapsed);
        let result = context.r0_r3[0] as i32;
        if let Some(count) = self.results.get_mut(&result) {
            *count += 1;
        } else if self.results.len() < RESULT_LIMIT {
            self.results.insert(result, 1);
        } else {
            self.results_overflow += 1;
        }
    }
}

#[derive(Serialize)]
struct SlotReach<'a> {
    slot: &'a Slot,
    calls: u64,
    returns: u64,
    pending: u64,
    untracked: u64,
    first: Option<u64>,
    last: Option<u64>,
    elapsed_total: u64,
    elapsed_max: u64,
    phases: Vec<Counted<String>>,
    contexts: Vec<Counted<ContextClass>>,
    tasks: Vec<Counted<Option<u32>>>,
    callers: Vec<Counted<Address>>,
    callers_overflow: u64,
    results: Vec<Counted<i32>>,
    results_overflow: u64,
    published_mismatch: u64,
    code_mismatch: u64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Key { Up, Down, Ok, Menu, Back }
impl Key {
    fn guest(self) -> RouteKey {
        match self { Self::Up => RouteKey::Up, Self::Down => RouteKey::Down, Self::Ok | Self::Menu => RouteKey::Ok, Self::Back => RouteKey::Back }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct MenuTriple { menu: u32, setting: u32, cursor: u32 }

#[derive(Deserialize)]
struct Trajectory { before_unload: Vec<TrajectoryPhase> }
#[derive(Deserialize)]
struct TrajectoryPhase { phase: String, key_pulse: Option<TrajectoryPulse>, steps: Vec<(Key, u32, u32, u32)> }
#[derive(Clone, Copy, Deserialize)]
struct TrajectoryPulse { hold_instructions: u64, release_instructions: u64 }

#[derive(Serialize)]
struct Pulse { key: Key, pressed: u64, released: u64, finished: u64, debounced: bool }

#[derive(Serialize)]
struct Milestone {
    phase: String,
    pulse: Option<Pulse>,
    expected: MenuTriple,
    reached: Option<u64>,
    observed: Option<MenuTriple>,
    finished: u64,
}

#[derive(Serialize)]
struct Binding { path: PathBuf, sha256: String }

#[derive(Serialize)]
struct Limits {
    sites: usize, pending: usize, events: u64, instructions: u64, watch_rows: usize, segment_stops: usize,
    segment_instructions: u64, unstopped_sample: usize, callers: usize, results: usize,
}

#[derive(Serialize)]
struct Report<'a> {
    schema: u32,
    inputs: Vec<Binding>,
    checkpoint_sha256: String,
    limits: Limits,
    instructions: u64,
    counts: Option<[u64; 2]>,
    events: u64,
    entries: u64,
    returns: u64,
    pending: usize,
    unrelated_return_stops: u64,
    misplaced_stops: u64,
    events_dropped: u64,
    return_sites: usize,
    fetches: &'a FetchLedger,
    milestones: &'a [Milestone],
    reach: Vec<SlotReach<'a>>,
    error: Option<String>,
    machine_failure: Option<String>,
}

struct Census {
    slots: BTreeMap<Address, Slot>,
    images: [(Domain, Address, Vec<u8>); 2],
    maps: Vec<MemoryMap>,
    return_sites: BTreeSet<Address>,
    open: OpenCalls,
    reach: BTreeMap<Address, Reach>,
    ledger: FetchLedger,
    watch: Option<FetchWatch>,
    events: Option<GzEncoder<BufWriter<File>>>,
    event_count: u64,
    events_dropped: u64,
    misplaced_stops: u64,
    entries: u64,
    returns: u64,
    unrelated: u64,
    phase: String,
    milestones: Vec<Milestone>,
}

impl Census {
    /// Past the event bound, lines are counted but not written.
    fn emit(&mut self, event: &Event) -> Result<(), String> {
        if self.event_count >= EVENT_LIMIT {
            self.events_dropped += 1;
            return Ok(());
        }
        self.event_count += 1;
        let events = self.events.as_mut().ok_or("census event stream is closed")?;
        serde_json::to_writer(&mut *events, event).map_err(|error| error.to_string())?;
        events.write_all(b"\n").map_err(|error| error.to_string())
    }

    fn canonical(&self, slot: &Slot) -> Result<u32, String> {
        let (_, base, bytes) = self.images.iter().find(|(domain, _, _)| *domain == slot.domain).ok_or("slot domain image absent")?;
        let offset = slot.target.0.checked_sub(base.0).ok_or("slot precedes its loader")? as usize;
        let word = bytes.get(offset..offset + 4).ok_or("slot exceeds its loader")?;
        Ok(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    }

    fn stop(&mut self, machine: &mut Machine, stops: &[(Core, Address)]) -> Result<(), String> {
        let mut restart = false;
        for &(core, point) in stops {
            let (context, tcm) = Board { machine, maps: &self.maps }.context(core)?;
            if context.pc != point {
                self.misplaced_stops += 1;
                continue;
            }
            if self.return_sites.contains(&context.pc) {
                let closed = self.open.close(&ReturnKey::of(&context, context.pc));
                if closed.is_empty() { self.unrelated += 1; }
                for call in &closed {
                    let elapsed = context.instructions - call.entered;
                    self.returns += 1;
                    self.reach.get_mut(&call.target).ok_or("closed call names no slot")?.complete(&context, elapsed);
                    self.emit(&Event::Return { call: call.id, target: call.target, context: &context, elapsed, chain: closed.len() })?;
                }
            }
            let Some(slot) = self.slots.get(&context.pc).copied() else { continue };
            let board = Board { machine, maps: &self.maps };
            let code = board.word(core, tcm, slot.target)?;
            let published = board.word(core, tcm, Address(slot.domain.root().0 + slot.offset.0))?;
            let canonical = self.canonical(&slot)?;
            let id = self.entries;
            self.entries += 1;
            self.ledger.stop(core, context.pc, context.instructions, id);
            if let Some(watch) = &mut self.watch { watch.stops += 1; }
            let continuation = Address(context.lr.0 & !1);
            let tracking = if continuation.0 == 0 || (context.lr.0 & 1 == 0 && continuation.0 % 4 != 0) {
                ReturnTracking::NoContinuation
            } else if !self.return_sites.contains(&continuation) && machine.traces.sites.len() >= SITE_LIMIT {
                ReturnTracking::SiteBound
            } else {
                if !self.return_sites.contains(&continuation) {
                    if !machine.traces.sites.contains_key(&continuation.0) {
                        machine.traces.add_sites(&machine.session, [(continuation.0, RETURN_LABEL.to_owned())].into_iter().collect())?;
                        restart = true;
                    }
                    self.return_sites.insert(continuation);
                }
                let call = OpenCall { id, target: slot.target, entered: context.instructions };
                if self.open.open(ReturnKey::of(&context, continuation), call) { ReturnTracking::Pending } else { ReturnTracking::PendingBound }
            };
            let reach = self.reach.get_mut(&slot.target).ok_or("entered slot has no reach record")?;
            reach.enter(&self.phase, &context, matches!(tracking, ReturnTracking::Pending));
            reach.published_mismatch += u64::from(published.committed() != Some(slot.target.0));
            reach.code_mismatch += u64::from(code.committed() != Some(canonical));
            let phase = self.phase.clone();
            self.emit(&Event::Entry { call: id, slot: &slot, phase: &phase, context: &context, published, code, canonical, tracking })?;
        }
        if restart || self.watch.as_ref().is_some_and(|watch| watch.stops >= SEGMENT_STOPS) {
            self.restart_watch(machine)?;
        }
        Ok(())
    }

    /// Ends the segment and begins another whose hooks follow every armed point.
    fn restart_watch(&mut self, machine: &Machine) -> Result<(), String> {
        let watch = self.watch.take().ok_or("entry fetch watch is not active")?;
        let classes = watch.classes.clone();
        watch.end(machine, &mut self.ledger)?;
        self.watch = Some(FetchWatch::begin(machine, &self.maps, classes)?);
        Ok(())
    }

    fn advance(&mut self, machine: &mut Machine, budget: u64) -> Result<(), String> {
        let end = machine.instructions().checked_add(budget).filter(|end| *end <= INSTRUCTION_LIMIT)
            .ok_or("census instruction bound exceeded")?;
        while machine.instructions() < end {
            machine.traces.hits.clear();
            let before = machine.instructions();
            let outcome = machine.run(end - before);
            machine.require_running()?;
            require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected census run outcome")?;
            let mut stops = Vec::with_capacity(2);
            for hit in &machine.traces.hits {
                require(hit.instructions == machine.instructions(), "census stop is no longer current")?;
                let core = Core::from_index(hit.core)?;
                require(!stops.iter().any(|&(other, _)| other == core), "one core stopped twice at one boundary")?;
                stops.push((core, Address(parse_hex(&hit.address)?)));
            }
            if stops.is_empty() {
                require(machine.instructions() >= end, "run ended early without a census stop")?;
            } else {
                self.stop(machine, &stops)?;
            }
            require(machine.instructions() > before || !stops.is_empty(), "census made no progress")?;
            if self.watch.as_ref().is_some_and(|watch| machine.instructions() - watch.started >= SEGMENT_INSTRUCTIONS) {
                self.restart_watch(machine)?;
            }
        }
        Ok(())
    }

    fn pulse(&mut self, machine: &mut Machine, key: Key, hold: u64, release: u64) -> Result<Pulse, String> {
        let pressed = machine.instructions();
        machine.press(key.guest())?;
        self.advance(machine, hold)?;
        machine.release_key(key.guest())?;
        let released = machine.instructions();
        let debounced = loop {
            if machine.document.firmware.released(&machine.session, key.guest())? { break true; }
            let spent = machine.instructions() - released;
            if spent >= release { break false; }
            self.advance(machine, (release - spent).min(RELEASE_POLL))?;
        };
        Ok(Pulse { key, pressed, released, finished: machine.instructions(), debounced })
    }

    fn menu(machine: &Machine) -> Result<Option<MenuTriple>, String> {
        Ok(machine.menu()?.map(|menu| MenuTriple { menu: menu.menu_id, setting: menu.setting_id, cursor: menu.cursor }))
    }

    /// Records a fixed-time endpoint.
    fn endpoint(&mut self, machine: &Machine, pulse: Option<Pulse>, expected: MenuTriple) -> Result<(), String> {
        let observed = Self::menu(machine)?;
        let reached = (observed == Some(expected)).then(|| machine.instructions());
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse, expected, reached, observed, finished: machine.instructions() });
        require(reached.is_some(), "menu endpoint differs")
    }

    /// Runs until the menu state is `expected`, then a fixed settle.
    fn state(&mut self, machine: &mut Machine, pulse: Pulse, expected: MenuTriple) -> Result<(), String> {
        let start = machine.instructions();
        let reached = loop {
            if Self::menu(machine)? == Some(expected) { break Some(machine.instructions()); }
            let spent = machine.instructions() - start;
            if spent >= STEP_STATE_CAP { break None; }
            self.advance(machine, (STEP_STATE_CAP - spent).min(MENU_POLL))?;
        };
        if reached.is_some() { self.advance(machine, STEP_SETTLE)?; }
        let observed = Self::menu(machine)?;
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse: Some(pulse), expected, reached, observed, finished: machine.instructions() });
        require(reached.is_some() && observed == Some(expected), "menu trajectory state differs")
    }

    fn scenario(&mut self, machine: &mut Machine, trajectory: &[TrajectoryPhase]) -> Result<(), String> {
        self.phase = "cold-start".into();
        self.advance(machine, INITIAL_MENU)?;
        self.endpoint(machine, None, MenuTriple { menu: 69, setting: 52, cursor: 12 })?;
        self.phase = "initial-setup".into();
        for key in std::iter::repeat_n(Key::Up, 12).chain([Key::Ok, Key::Ok]) {
            self.pulse(machine, key, SETUP_HOLD, SETUP_RELEASE)?;
        }
        self.advance(machine, SETUP_SETTLE)?;
        self.phase = "shooting-menu".into();
        let pulse = self.pulse(machine, Key::Menu, SETUP_HOLD, SETUP_RELEASE)?;
        self.advance(machine, SHOOTING_SETTLE)?;
        self.endpoint(machine, Some(pulse), MenuTriple { menu: 1, setting: 5, cursor: 2 })?;
        for phase in trajectory {
            self.phase = phase.phase.clone();
            let timing = phase.key_pulse.ok_or("trajectory phase has no key pulse")?;
            for &(key, menu, setting, cursor) in &phase.steps {
                let pulse = self.pulse(machine, key, timing.hold_instructions, timing.release_instructions)?;
                self.state(machine, pulse, MenuTriple { menu, setting, cursor })?;
            }
        }
        Ok(())
    }

    fn reach(&self) -> Vec<SlotReach<'_>> {
        let mut pending: BTreeMap<Address, u64> = BTreeMap::new();
        for call in self.open.pending.values().flatten() { *pending.entry(call.target).or_default() += 1; }
        let mut rows: Vec<_> = self.slots.values().map(|slot| {
            let reach = &self.reach[&slot.target];
            SlotReach {
                slot,
                calls: reach.calls,
                returns: reach.returns,
                pending: pending.get(&slot.target).copied().unwrap_or(0),
                untracked: reach.untracked,
                first: reach.first,
                last: reach.last,
                elapsed_total: reach.elapsed_total,
                elapsed_max: reach.elapsed_max,
                phases: counted(&reach.phases),
                contexts: counted(&reach.contexts),
                tasks: counted(&reach.tasks),
                callers: counted(&reach.callers),
                callers_overflow: reach.callers_overflow,
                results: counted(&reach.results),
                results_overflow: reach.results_overflow,
                published_mismatch: reach.published_mismatch,
                code_mismatch: reach.code_mismatch,
            }
        }).collect();
        rows.sort_by(|left, right| right.calls.cmp(&left.calls)
            .then(left.slot.domain.cmp(&right.slot.domain)).then(left.slot.offset.cmp(&right.slot.offset)));
        rows
    }
}

fn arguments() -> Result<BTreeMap<String, PathBuf>, String> {
    const OPTIONS: [&str; 7] = ["--run", "--update", "--boot-rom", "--analysis", "--manifest", "--trajectory", "--output"];
    let mut arguments = std::env::args_os().skip(1);
    let mut paths = BTreeMap::new();
    while let Some(flag) = arguments.next() {
        let name = flag.to_str().filter(|name| OPTIONS.contains(name)).ok_or(USAGE)?.to_owned();
        require(paths.insert(name, PathBuf::from(arguments.next().ok_or(USAGE)?)).is_none(), USAGE)?;
    }
    require(paths.len() == OPTIONS.len(), USAGE)?;
    Ok(paths)
}

fn bound(path: &Path, expected: Option<&str>) -> Result<(Vec<u8>, Binding), String> {
    let bytes = read(path)?;
    let digest = sha256(&bytes);
    if let Some(expected) = expected { require(digest == expected, &format!("{}: input differs from the admitted stock input", path.display()))?; }
    Ok((bytes, Binding { path: path.to_owned(), sha256: digest }))
}

fn drive() -> Result<(), String> {
    let paths = arguments()?;
    let output = paths["--output"].clone();
    let analysis = &paths["--analysis"];
    let (run_bytes, run_binding) = bound(&paths["--run"], Some(RUN_SHA256))?;
    let (dat, dat_binding) = bound(&paths["--update"], Some(DAT_SHA256))?;
    let (rom, rom_binding) = bound(&paths["--boot-rom"], Some(ROM_SHA256))?;
    let (loader_a, loader_a_binding) = bound(&analysis.join("decode/loader-40000.bin"), Some(LOADER_A_SHA256))?;
    let (loader_b, loader_b_binding) = bound(&analysis.join("decode/loader-d0000.bin"), Some(LOADER_B_SHA256))?;
    let (manifest_bytes, manifest_binding) = bound(&paths["--manifest"], None)?;
    let (trajectory_bytes, trajectory_binding) = bound(&paths["--trajectory"], None)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let (_, executable_binding) = bound(&executable, None)?;
    let slots = load_manifest(&manifest_bytes)?;
    let trajectory: Trajectory = serde_json::from_slice(&trajectory_bytes).map_err(|error| format!("trajectory: {error}"))?;
    let phases: Vec<&str> = trajectory.before_unload.iter().map(|phase| phase.phase.as_str()).collect();
    require(phases == ["package-menu-prefix", "modal75-shooting", "modal75-film", "modal75-returned", "modal75-film-repeated",
        "modal75-returned-repeated", "modal75-handback"], "trajectory phases differ from the #128 FILM SIMULATION route")?;
    require(trajectory.before_unload[1].steps.iter().map(|&(_, menu, setting, cursor)| [menu, setting, cursor]).eq([[1, 5, 2]]),
        "trajectory shooting-menu entry differs")?;
    require(trajectory.before_unload[2..].iter().all(|phase| phase.key_pulse.is_some()), "trajectory step phase has no key pulse")?;
    std::fs::create_dir(&output).map_err(|error| format!("{}: {error}", output.display()))?;
    let events_file = File::create_new(output.join("calls.jsonl.gz")).map_err(|error| error.to_string())?;
    let events = GzBuilder::new().mtime(0).operating_system(255).write(BufWriter::new(events_file), Compression::default());

    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(dat), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    write(&output.join("reset.fujimod"), &checkpoint)?;
    let restored = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "reset checkpoint is not preexecution")?;

    let classes = slots.keys().map(|target| WatchRange { start: u64::from(target.0), end: u64::from(target.0) + 4, kinds: WatchKinds::CPU_FETCH }).collect();
    let mut census = Census {
        reach: slots.keys().map(|&target| (target, Reach::default())).collect(),
        slots,
        images: [(Domain::A, Address(0x40000), loader_a), (Domain::B, Address(0xd0000), loader_b)],
        maps,
        return_sites: BTreeSet::new(),
        open: OpenCalls::default(),
        ledger: FetchLedger::default(),
        watch: None,
        events: Some(events),
        event_count: 0,
        events_dropped: 0,
        misplaced_stops: 0,
        entries: 0,
        returns: 0,
        unrelated: 0,
        phase: String::new(),
        milestones: Vec::new(),
    };
    machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
    machine.traces.stop_labels = [ENTRY_LABEL.to_owned(), RETURN_LABEL.to_owned()].into_iter().collect();
    let observed = (|| -> Result<(), String> {
        machine.traces.arm(&machine.session, census.slots.keys().map(|target| (target.0, ENTRY_LABEL.to_owned())).collect())?;
        census.watch = Some(FetchWatch::begin(&machine, &census.maps, classes)?);
        census.scenario(&mut machine, &trajectory.before_unload[2..])
    })();
    let ended = match census.watch.take() {
        Some(watch) => watch.end(&machine, &mut census.ledger),
        None => Ok(()),
    };
    let pending: Vec<OpenCall> = census.open.pending.values().flatten().copied().collect();
    let mut written = pending.iter().try_for_each(|call| census.emit(&Event::Pending { call: call.id, target: call.target, entered: call.entered }));
    if let Some(events) = census.events.take() {
        written = written.and(events.finish().and_then(|mut inner| inner.flush()).map_err(|error| error.to_string()));
    }
    let error = observed.err().or(ended.err()).or(written.err());
    let report = Report {
        schema: 1,
        inputs: vec![run_binding, dat_binding, rom_binding, loader_a_binding, loader_b_binding, manifest_binding, trajectory_binding, executable_binding],
        checkpoint_sha256: sha256(&checkpoint),
        limits: Limits {
            sites: SITE_LIMIT, pending: PENDING_LIMIT, events: EVENT_LIMIT, instructions: INSTRUCTION_LIMIT, watch_rows: WATCH_LIMIT,
            segment_stops: SEGMENT_STOPS, segment_instructions: SEGMENT_INSTRUCTIONS, unstopped_sample: UNSTOPPED_SAMPLE, callers: CALLER_LIMIT, results: RESULT_LIMIT,
        },
        instructions: machine.instructions(),
        counts: machine.session.board.instruction_counts().try_into().ok(),
        events: census.event_count,
        entries: census.entries,
        returns: census.returns,
        pending: census.open.count,
        unrelated_return_stops: census.unrelated,
        misplaced_stops: census.misplaced_stops,
        events_dropped: census.events_dropped,
        return_sites: census.return_sites.len(),
        fetches: &census.ledger,
        milestones: &census.milestones,
        reach: census.reach(),
        error: error.clone(),
        machine_failure: machine.failure.clone(),
    };
    write(&output.join("census.json"), &serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?)?;
    match error { Some(error) => Err(error), None => Ok(()) }
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rtos-census: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(core: usize, address: u64, instructions: u64) -> FetchRow {
        FetchRow { source: FetchSource::CpuFetch, address, physical: Some(address), core, instructions: Some(instructions) }
    }

    fn task(current: u32) -> TaskView {
        TaskView { current: WordView::Committed { value: current }, index: None, kernel_context: WordView::Committed { value: 0 } }
    }

    fn context(core: Core, sp: u32, current: u32, cpsr: u32) -> Context {
        Context { core, pc: Address(0), lr: Address(0), sp: Address(sp), cpsr, r0_r3: [0; 4], instructions: 0, counts: [0; 2], task: task(current) }
    }

    #[test]
    fn stopped_and_resumed_entry_fetches_reconcile_across_segments() {
        let mut ledger = FetchLedger::default();
        ledger.stop(Core::Primary, Address(0x4d094), 100, 0);
        ledger.reconcile([row(0, 0x4d094, 100)]).unwrap();
        ledger.reconcile([row(0, 0x4d094, 101)]).unwrap();
        assert_eq!((ledger.stopped, ledger.resumed, ledger.unstopped, ledger.stops_without_fetch), (1, 1, 0, 0));
    }

    #[test]
    fn entry_fetch_without_a_stop_is_unstopped() {
        let mut ledger = FetchLedger::default();
        ledger.reconcile([row(1, 0x4d094, 7)]).unwrap();
        assert_eq!((ledger.unstopped, ledger.unstopped_sample.len()), (1, 1));
    }

    #[test]
    fn stop_without_its_fetch_row_is_counted_and_later_fetch_is_unstopped() {
        let mut ledger = FetchLedger::default();
        ledger.stop(Core::Secondary, Address(0xdd054), 9, 4);
        ledger.reconcile([row(1, 0xdd054, 10)]).unwrap();
        assert_eq!((ledger.stops_without_fetch, ledger.stops_without_fetch_sample.clone(), ledger.resumed, ledger.unstopped), (1, vec![4], 0, 1));
    }

    #[test]
    fn peer_core_rows_do_not_resume_the_stopped_core() {
        let mut ledger = FetchLedger::default();
        ledger.stop(Core::Primary, Address(0x4d094), 5, 0);
        ledger.reconcile([row(0, 0x4d094, 5), row(1, 0x4d094, 6), row(0, 0x4d094, 7)]).unwrap();
        assert_eq!((ledger.stopped, ledger.resumed, ledger.unstopped), (1, 1, 1));
    }

    #[test]
    fn tail_transfer_chain_completes_at_one_continuation() {
        let mut calls = OpenCalls::default();
        let entry = context(Core::Primary, 0x1000, 0x4005_ff0c, 0x13);
        let key = ReturnKey::of(&entry, Address(0x725e00));
        assert!(calls.open(key, OpenCall { id: 0, target: Address(0x4d094), entered: 1 }));
        assert!(calls.open(key, OpenCall { id: 1, target: Address(0x4d1a4), entered: 2 }));
        assert_eq!(calls.close(&key).len(), 2);
        assert_eq!(calls.count, 0);
    }

    #[test]
    fn continuation_with_another_stack_task_or_mode_does_not_complete() {
        let mut calls = OpenCalls::default();
        let entry = context(Core::Primary, 0x1000, 0x4005_ff0c, 0x13);
        calls.open(ReturnKey::of(&entry, Address(0x725e00)), OpenCall { id: 0, target: Address(0x4d094), entered: 1 });
        for other in [context(Core::Primary, 0x0ff8, 0x4005_ff0c, 0x13), context(Core::Primary, 0x1000, 0x4005_ff44, 0x13),
            context(Core::Primary, 0x1000, 0x4005_ff0c, 0x92), context(Core::Secondary, 0x1000, 0x4005_ff0c, 0x13)] {
            assert!(calls.close(&ReturnKey::of(&other, Address(0x725e00))).is_empty());
        }
        assert_eq!(calls.count, 1);
    }

    #[test]
    fn interrupt_mode_call_completes_across_a_current_task_change() {
        let mut calls = OpenCalls::default();
        let entry = context(Core::Secondary, 0x2000, 0x400e_fecc, 0xd2);
        calls.open(ReturnKey::of(&entry, Address(0x40364)), OpenCall { id: 0, target: Address(0xdd054), entered: 1 });
        let returned = context(Core::Secondary, 0x2000, 0x400e_ff04, 0xd2);
        assert_eq!(calls.close(&ReturnKey::of(&returned, Address(0x40364))).len(), 1);
    }

    #[test]
    fn manifest_requires_194_distinct_callable_targets() {
        let domain = |name: &str, root: &str, base: u32| {
            let offsets = (0..93u32).map(|index| index * 4).chain([0x1618, 0x161c, 0x1620, 0x1624]);
            let slots: Vec<_> = offsets.enumerate().map(|(index, offset)| serde_json::json!({
                "offset": format!("{offset:#x}"), "target": format!("{:#x}", base + 4 * index as u32), "recovery": "unknown"})).collect();
            serde_json::json!({"domain": name, "root": root, "slots": slots})
        };
        let manifest = |b_base: u32| serde_json::to_vec(&serde_json::json!({"firmware": "X-M1 1.50",
            "domains": [domain("A", "0x4004dae0", 0x40000), domain("B", "0x400ddaa0", b_base)]})).unwrap();
        assert_eq!(load_manifest(&manifest(0xd0000)).unwrap().len(), 194);
        assert!(load_manifest(&manifest(0x40000)).is_err());
    }
}
