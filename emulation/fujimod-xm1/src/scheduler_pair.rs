
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Access, Hook, HookId, HookRange, Register, arm};
use fujimod_xm1::bridge::{read, sha256};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::machine::{Hold, KeyPulse, Machine, RunOutcome, key_pulse};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::cell::RefCell;
use std::ffi::c_void;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

const USAGE: &str = "scheduler-pair --plan PLAN --run RUN --update DAT --boot-rom ROM --trajectory TRAJECTORY --output NEW_DIR";
const SCHEMA: u32 = 3;
const PLAN_SCHEMA: u32 = 3;
const ALIAS: u32 = 0x4000_0000;
const ENTRY_LABEL: &str = "scheduler-entry";
const CONTINUATION_LABEL: &str = "scheduler-continuation";
const FILL_LABEL: &str = "scheduler-fill";
const ARM_LABEL: &str = "scheduler-arm";

/// The pinned engine refuses more than 4,096 instruction points per core.
const SITE_LIMIT: usize = 4096;
const PENDING_LIMIT: usize = 4096;
const CLASS_LIMIT: usize = 1 << 20;
const SAMPLE_LIMIT: usize = 64;
const INSTRUCTION_LIMIT: u64 = 1_500_000_000;
const WATCH_LIMIT: usize = 4096;

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

fn parse_hex(text: &str) -> Result<u32, String> {
    let digits = text.strip_prefix("0x").ok_or_else(|| format!("{text}: hexadecimal prefix missing"))?;
    u32::from_str_radix(digits, 16).map_err(|_| format!("{text}: invalid hexadecimal word"))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(try_from = "String")]
struct Hex(u32);
impl TryFrom<String> for Hex {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> { parse_hex(&text).map(Self) }
}
impl Serialize for Hex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{:#x}", self.0))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
enum Domain { A, B }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Helper { Acquire, Release, Find, Insert, Remove, Reset, Prepare, Notify, Requeue, Rotate }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DispatchPoint { Save, Resume, NoSave, Select, FirstEntry, IrqReturn, IrqResume, IrqTail }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Service {
    DisableDispatch, EnableDispatch, ExitTask, RotateReady, InterruptRotateReady, CoreRotateReady, InterruptCoreRotateReady,
    InterruptActivateTask, ActivateTask, CancelActivation, WakeTask, InterruptWakeTask, CancelWakeup,
    GetTaskPriority, SleepTask, SuspendTask, ResumeTask, ForceResumeTask, RaiseTaskException, InterruptRaiseTaskException,
    DisableTaskException, EnableTaskException, DeliverTaskException, DeliverTaskExceptionEntry,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum IrqKind { Doorbell }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Semantic { Helper { name: Helper }, Dispatch { name: DispatchPoint }, Service { name: Service }, Irq { name: IrqKind } }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProbeKind { SelectedRead, DiscardedRead, EnabledRead, GlobalLockRead, Retry, Callback, CallbackReturn, SelectComplete, FirstEntryExit, IrqTail }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
struct ProbeRegister(u8);
impl TryFrom<String> for ProbeRegister {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        text.strip_prefix('r').and_then(|digits| digits.parse::<u8>().ok()).filter(|&index| index<16)
            .map(Self).ok_or_else(|| "probe register must be r0..r15".into())
    }
}
impl From<ProbeRegister> for String {
    fn from(register: ProbeRegister) -> Self { format!("r{}", register.0) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanProbe { domain: Domain, address: Hex, kind: ProbeKind, register: ProbeRegister }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Stock, Owned, ControlRestored, ControlSilentNotify, ControlWrongPredicate, ControlWakeNoInsert }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanDomain { domain: Domain, loader_entry: Hex }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanEntry { domain: Domain, semantic: Semantic, entry: Hex }


#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanRange { domain: Domain, start: Hex, end: Hex, sha256: String }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u32,
    role: Role,
    dat_sha256: String,
    domains: Vec<PlanDomain>,
    entries: Vec<PlanEntry>,
    probes: Vec<PlanProbe>,
    guarded: Vec<PlanRange>,
    fills: Vec<Hex>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct SlotId { domain: Domain, entry: Hex, semantic: Semantic }

struct Slot { id: SlotId }

/// Every planned site in both spellings.
struct Sites {
    entries: BTreeMap<u32, usize>,
    probes: BTreeMap<u32, Vec<usize>>,
    fills: BTreeSet<u32>,
    arms: BTreeMap<u32, Domain>,
}

impl Sites {
    fn labels(&self) -> BTreeMap<u32, String> {
        self.entries.keys().map(|&address| (address, ENTRY_LABEL))
            .chain(self.probes.keys().map(|&address| (address, "scheduler-probe")))
            .chain(self.fills.iter().map(|&address| (address, FILL_LABEL)))
            .chain(self.arms.keys().map(|&address| (address, ARM_LABEL)))
            .map(|(address, label)| (address, label.to_owned())).collect()
    }
}

fn spellings(address: u32) -> [u32; 2] { [address, address | ALIAS] }

struct Validated {
    plan: Plan,
    slots: Vec<Slot>,
    sites: Sites,
}

fn validate(plan: Plan) -> Result<Validated, String> {
    require(plan.schema == PLAN_SCHEMA, "plan schema differs")?;
    let mut domains = BTreeSet::new();
    let mut arms = BTreeMap::new();
    for domain in &plan.domains {
        require(domains.insert(domain.domain), "plan repeats a domain")?;
        for address in spellings(domain.loader_entry.0) { arms.insert(address, domain.domain); }
    }
    require(domains.len()==2, "plan does not name both loaders")?;
    let mut slots = Vec::new();
    let mut entries = BTreeMap::new();
    for entry in &plan.entries {
        require(entry.entry.0 % 4 == 0 && entry.entry.0 < ALIAS, "plan entry is not an aligned low address")?;
        for address in spellings(entry.entry.0) {
            require(entries.insert(address, slots.len()).is_none(), "two plan entries share an address")?;
        }
        slots.push(Slot { id: SlotId { domain: entry.domain, entry: entry.entry, semantic: entry.semantic } });
    }
    let mut probes = BTreeMap::<u32, Vec<usize>>::new();
    for (index, probe) in plan.probes.iter().enumerate() {
        require(probe.address.0 % 4==0 && probe.address.0<ALIAS, "unaligned probe")?;
        for address in spellings(probe.address.0) { probes.entry(address).or_default().push(index); }
    }
    let fills: BTreeSet<u32> = plan.fills.iter().flat_map(|fill| spellings(fill.0)).collect();
    require(fills.len() == 2 * plan.fills.len() && plan.fills.iter().all(|fill| fill.0 % 4 == 0 && fill.0 < ALIAS),
        "plan fill words are repeated or unaligned")?;
    require(!plan.guarded.is_empty() && plan.guarded.len() <= 256 && plan.guarded.iter().all(|range| {
        range.start.0 % 4 == 0 && range.end.0 % 4 == 0 && range.start.0 < range.end.0 && range.end.0 <= ALIAS
    }), "plan guarded ranges are malformed")?;
    let sites = Sites { entries, probes, fills, arms };
    require(sites.fills.iter().all(|address| !sites.entries.contains_key(address) && !sites.probes.contains_key(address)), "fill shares an entry or probe")?;
    Ok(Validated { plan, slots, sites })
}

/// A word through the executing core's view.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "view", rename_all = "kebab-case")]
enum Read {
    Committed { value: u32 },
    /// The executing core's TCM overlays the word.
    TcmOverlay,
    ViewsDiffer { guest: u32, backing: u32 },
    /// No static map translates the word.
    Unresolved,
    Unaligned,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
enum Core { Primary, Secondary }
impl Core {
    fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
    fn from_index(index: usize) -> Result<Self, String> {
        match index { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("the pair expects two cores".into()) }
    }
    fn own_kernel_current_task(self) -> u32 { match self { Self::Primary => 0x4005_4760, Self::Secondary => 0x400e_4720 } }
}

#[derive(Clone, Copy, Debug)]
struct Context { core: Core, pc: u32, lr: u32, sp: u32, cpsr: u32, r0: u32, arguments: [u32; 4], registers: [u32; 16], instructions: u64, current: Read }
impl Context {
    fn mode(&self) -> u32 { self.cpsr & 0x1f }
}

struct Board<'a> { machine: &'a Machine, maps: &'a [MemoryMap] }
impl Board<'_> {
    fn register(&self, core: Core, register: arm::Register) -> Result<u32, String> {
        u32::try_from(self.machine.session.board.read_register(core.index(), Register::Arm32(register))?)
            .map_err(|_| "ARM register exceeds 32 bits".to_owned())
    }
    fn registers(&self, core: Core) -> Result<[u32; 16], String> {
        let mut registers = [0; 16];
        for (index, value) in registers.iter_mut().enumerate() { *value = self.register(core, arm::Register::R(index as u8))?; }
        Ok(registers)
    }
    fn cp15(&self, core: Core, crn: u32, crm: u32, opc2: u32) -> Result<u32, String> {
        u32::try_from(self.machine.session.board.cpu(core.index())?.coprocessor(arm::Coprocessor {
            cp: 15, opc1: 0, crn, crm, opc2, secure: 0, wide: false,
        }, None)?).map_err(|_| "CP15 register exceeds 32 bits".into())
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
    fn word(&self, core: Core, tcm: [u32; 2], address: u32) -> Result<Read, String> {
        if address % 4 != 0 { return Ok(Read::Unaligned); }
        let low = u64::from(address);
        let high = low + 4;
        for bank in tcm {
            if tcm_window(TcmRegister(bank))?.is_some_and(|(start, end)| start < high && low < end) {
                return Ok(Read::TcmOverlay);
            }
        }
        let Ok(span) = static_span(self.maps, MemoryCore(core.index()), MemoryAddress(low)..MemoryAddress(high)) else {
            return Ok(Read::Unresolved);
        };
        let (mut guest, mut backing) = ([0; 4], [0; 4]);
        self.machine.session.board.read_memory(core.index(), low, &mut guest)?;
        self.machine.session.board.read_physical(span.physical.0, &mut backing)?;
        let (guest, backing) = (u32::from_le_bytes(guest), u32::from_le_bytes(backing));
        Ok(if guest == backing { Read::Committed { value: guest } } else { Read::ViewsDiffer { guest, backing } })
    }
    fn context(&self, core: Core) -> Result<(Context, [u32; 2]), String> {
        let tcm = self.tcm(core)?;
        Ok((Context {
            core,
            pc: self.register(core, arm::Register::R(15))?,
            lr: self.register(core, arm::Register::R(14))?,
            sp: self.register(core, arm::Register::R(13))?,
            cpsr: self.register(core, arm::Register::Cpsr)?,
            r0: self.register(core, arm::Register::R(0))?,
            arguments: [self.register(core, arm::Register::R(0))?, self.register(core, arm::Register::R(1))?,
                self.register(core, arm::Register::R(2))?, self.register(core, arm::Register::R(3))?],
            instructions: self.machine.instructions(),
            registers: self.registers(core)?,
            current: self.word(core, tcm, core.own_kernel_current_task())?,
        }, tcm))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct EntryState {
    slot: SlotId,
    core: Core,
    cpsr: u32,
    arguments: [u32; 4],
    memory: Vec<WordValue>,
    r0: u32,
    lr: Hex,
    task: Read,
    sp: Hex,
    registers: [u32; 16],
    cpacr: u32,
    fpexc: u32,
    tpidrurw: u32,
    origin: Option<DispatchPoint>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct ReturnState {
    r0: u32,
    cpsr: u32,
    memory: Vec<WordValue>,
    writes: Vec<Written>,
    bank: Vec<Written>,
    completion: Completion,
    task: Read,
    sp: Hex,
    registers: [u32; 16],
    cpacr: u32,
    fpexc: u32,
    tpidrurw: u32,
    history: Vec<Boundary>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct CallClass { entry: EntryState, result: ReturnState }

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Completion { Return, Discard, Dispatched }

#[derive(Clone, Debug, Serialize)]
struct Boundary {
    kind: BoundaryKind, address: Hex, instructions: u64, core: Core, task: Read,
    sp: Hex, cpsr: u32, registers: [u32; 16], memory: Vec<WordValue>,
    writes: Vec<Written>, bank: Vec<Written>, value: Option<u32>, cpacr: u32, fpexc: u32, tpidrurw: u32,
}

impl Ord for Boundary {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.kind,self.address,self.core,self.task,self.sp,self.cpsr,&self.registers,&self.memory,&self.writes,&self.bank,self.value,(self.cpacr,self.fpexc,self.tpidrurw))
            .cmp(&(other.kind,other.address,other.core,other.task,other.sp,other.cpsr,&other.registers,&other.memory,&other.writes,&other.bank,other.value,(other.cpacr,other.fpexc,other.tpidrurw)))
    }
}
impl PartialOrd for Boundary {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl PartialEq for Boundary {
    fn eq(&self, other: &Self) -> bool { self.cmp(other).is_eq() }
}
impl Eq for Boundary {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum BoundaryKind { Save, Resume, NoSave, Retry, Callback, CallbackReturn, SelectedRead, DiscardedRead, EnabledRead, GlobalLockRead, SelectComplete, FirstEntryExit, IrqTail, Return, Acquired }

#[derive(Serialize)]
struct ClassCount { count: u64, first: u64, last: u64 }

#[derive(Serialize)]
struct ClassRow<'a> { #[serde(flatten)] class: &'a CallClass, #[serde(flatten)] count: &'a ClassCount }

/// A call completes when its core next stops at its continuation with the entry's stack pointer
/// and processor mode; usr, svc and sys calls also need the entry's executing-core current task.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ReturnKey { core: Core, pc: u32, sp: u32, task: Option<u32>, mode: u32 }
impl ReturnKey {
    fn of(context: &Context, pc: u32) -> Self {
        let task = match context.current {
            Read::Committed { value } if [0x10, 0x13, 0x1f].contains(&context.mode()) => Some(value),
            _ => None,
        };
        Self { core: context.core, pc, sp: context.sp, task, mode: context.mode() }
    }
}

#[derive(Clone, Debug, Serialize)]
struct OpenCall { entry: EntryState, entered: u64, writes: Vec<Written>, bank: Vec<Written>, history: Vec<Boundary>, write_cursor: usize, bank_cursor: usize, suspended: bool }

impl OpenCall {
    fn record(&mut self, mut boundary: Boundary) -> Result<(), String> {
        require(self.history.len() < WATCH_LIMIT, "call boundary bound exceeded")?;
        boundary.writes = self.writes[self.write_cursor..].to_vec();
        boundary.bank = self.bank[self.bank_cursor..].to_vec();
        self.write_cursor = self.writes.len();
        self.bank_cursor = self.bank.len();
        if matches!(self.entry.slot.semantic, Semantic::Service { .. }) {
            if boundary.kind == BoundaryKind::Save { self.suspended = true; }
            if boundary.kind == BoundaryKind::Resume { self.suspended = false; }
        }
        self.history.push(boundary);
        Ok(())
    }
}

#[derive(Default)]
struct OpenCalls { pending: BTreeMap<ReturnKey, Vec<OpenCall>>, count: usize }
impl OpenCalls {
    fn open(&mut self, key: ReturnKey, call: OpenCall) -> Result<(), String> {
        require(self.count < PENDING_LIMIT, "open call bound exceeded")?;
        self.pending.entry(key).or_default().push(call);
        self.count += 1;
        Ok(())
    }
    fn close(&mut self, key: &ReturnKey) -> Vec<OpenCall> {
        let closed = self.pending.remove(key).unwrap_or_default();
        self.count -= closed.len();
        closed
    }
    fn take_where(&mut self, mut predicate: impl FnMut(&ReturnKey, &OpenCall) -> bool) -> Vec<OpenCall> {
        let mut taken = Vec::new();
        self.pending.retain(|key, calls| {
            let mut kept = Vec::new();
            for call in calls.drain(..) {
                if predicate(key, &call) { taken.push(call); } else { kept.push(call); }
            }
            *calls = kept;
            !calls.is_empty()
        });
        self.count -= taken.len();
        taken
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct StopRecord { core: Core, address: Hex, sp: Hex, lr: Hex, instructions: u64 }
impl StopRecord {
    fn of(context: &Context) -> Self {
        Self { core: context.core, address: Hex(context.pc), sp: Hex(context.sp), lr: Hex(context.lr), instructions: context.instructions }
    }
}

#[derive(Serialize)]
struct ViewHash {
    core: usize,
    address: Hex,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// One guarded extent, keyed by its physical range.
#[derive(Serialize)]
struct RangeReadback { domain: Domain, start: Hex, end: Hex, physical_sha256: String, views: Vec<ViewHash> }
impl RangeReadback {
    fn matches(&self, expected: &str) -> bool {
        self.physical_sha256 == expected && self.views.iter().all(|view| view.sha256.as_deref() == Some(expected))
    }
}

#[derive(Serialize)]
struct Readback { instructions: u64, ranges: Vec<RangeReadback> }

#[derive(Serialize)]
struct Arming { domain: Domain, stop: StopRecord, readback: Readback }

#[derive(Serialize)]
struct Segment { domains: Vec<Domain>, started: u64, ended: u64, state: serde_json::Value }

struct GuardWatch { watch: Box<MemoryWatch>, domains: Vec<Domain>, started: u64 }

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
/// A `key_pulse` is refused; every key takes the bridge's default pulse.
#[derive(Deserialize)]
struct TrajectoryPhase { phase: String, key_pulse: Option<serde::de::IgnoredAny>, steps: Vec<(Key, u32, u32, u32)> }

#[derive(Serialize)]
struct Pulse { key: Key, pressed: u64, released: u64, finished: u64, debounced: bool }

#[derive(Serialize)]
struct Milestone { phase: String, pulse: Option<Pulse>, expected: MenuTriple, reached: Option<u64>, observed: Option<MenuTriple>, finished: u64 }

#[derive(Serialize)]
struct Binding { path: PathBuf, sha256: String }

#[derive(Serialize)]
struct Limits { sites: usize, pending: usize, classes: usize, samples: usize, instructions: u64, watch_rows: usize }

#[derive(Serialize)]
struct Reached { slot: SlotId, calls: u64, returns: u64, pending: u64, first: Option<u64>, last: Option<u64> }

#[derive(Serialize)]
struct Sample<T> { count: u64, sample: Vec<T> }
impl<T> Default for Sample<T> {
    fn default() -> Self { Self { count: 0, sample: Vec::new() } }
}
impl<T> Sample<T> {
    fn push(&mut self, item: T) {
        self.count += 1;
        if self.sample.len() < SAMPLE_LIMIT { self.sample.push(item); }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct WordValue { address: Hex, value: Read }

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Written { address: Hex, size: u32, value: u32 }

struct DataCapture { active: BTreeSet<u32>, rows: Vec<(u32, u32, u32)>, overflow: bool }

unsafe fn data_write(_access: Access, address: u64, size: i32, value: i64, data: *mut c_void) {
    // The boxed context outlives its installed CPU hook.
    let context = unsafe { &*data.cast::<RefCell<DataCapture>>() };
    let mut capture = context.borrow_mut();
    let Ok(address) = u32::try_from(address) else { capture.overflow = true; return; };
    if !capture.active.contains(&(address & !ALIAS & !3)) { return; }
    if capture.rows.len() == WATCH_LIMIT || ![1, 2, 4].contains(&size) {
        capture.overflow = true;
    } else {
        let mask = match size { 1 => 0xff, 2 => 0xffff, _ => u32::MAX };
        capture.rows.push((address, size as u32, value as u32 & mask));
    }
}

#[derive(Serialize)]
struct PcWatch { core: Core, address: Hex, count: u64, first: u64 }

#[derive(Serialize)]
struct ProbeHit { domain: Domain, core: Core, address: Hex, kind: ProbeKind, register: ProbeRegister,
    value: u32, cpsr: u32, sp: Hex, lr: Hex, instructions: u64 }

fn root_snapshot(board: &Board, core: Core, tcm: [u32; 2], domain: Domain) -> Result<Vec<WordValue>, String> {
    let (context, lock, reschedule, heads, bitmap, running, dispatch, selected) = match domain {
        Domain::A => (0x400545a4, 0x400545c0, 0x400546d8, 0x400546dc, 0x4005475c, 0x40054760, 0x40054764, 0x40054768),
        Domain::B => (0x400e4564, 0x400e4580, 0x400e4698, 0x400e469c, 0x400e471c, 0x400e4720, 0x400e4724, 0x400e4728),
    };
    let mut addresses: BTreeSet<_> = [context, lock, reschedule, bitmap, running, dispatch, selected, 0x400c0804].into_iter().collect();
    addresses.extend((0..128).step_by(4).map(|offset| heads+offset));
    for cell in [running, selected] {
        let task = committed(board.word(core, tcm, cell)?)?;
        if task != 0 { addresses.extend((0..0x38).step_by(4).map(|offset| task+offset)); }
        if task != 0 {
            let config = committed(board.word(core, tcm, task + 8)?)?;
            if config != 0 { addresses.extend((0..0x28).step_by(4).map(|offset| config+offset)); }
            let saved_sp = committed(board.word(core, tcm, task + 0x18)?)?;
            if saved_sp != 0 && saved_sp % 4 == 0 { addresses.extend((0..68).step_by(4).map(|offset| saved_sp+offset)); }
        }
    }
    addresses.extend((0..0x10c).step_by(4).map(|offset| lock+8+offset));
    addresses.into_iter().map(|address| Ok(WordValue { address: Hex(address), value: board.word(core, tcm, address)? })).collect()
}

fn service_snapshot(board: &Board, core: Core, tcm: [u32; 2], domain: Domain, service: Service, arguments: [u32; 4]) -> Result<Vec<WordValue>, String> {
    let mut words = root_snapshot(board, core, tcm, domain)?;
    let contexts = match domain { Domain::A => 0x400545cc, Domain::B => 0x400e458c };
    let mut tasks = BTreeSet::new();
    let running = committed(board.word(core,tcm,core.own_kernel_current_task())?)?;
    if running != 0 { tasks.insert(running); }
    for record in [contexts, contexts+0x84] {
        words.extend(snapshot(board, core, tcm, Helper::Find, [record, 0, 0, 0])?);
        if matches!(service, Service::WakeTask | Service::InterruptWakeTask) {
            for offset in [0x38,0x3c] {
                let cell = committed(board.word(core,tcm,record+offset)?)?;
                words.push(WordValue { address: Hex(cell), value: board.word(core,tcm,cell)? });
            }
            let count_cell = committed(board.word(core,tcm,record+0x38)?)?;
            let count = committed(board.word(core,tcm,count_cell)?)?;
            require(count <= 4096, "timer snapshot bound exceeded")?;
            let heap = committed(board.word(core,tcm,record+0x34)?)?;
            for index in 0..count {
                for offset in [0,4] {
                    let address = heap+index*8+offset;
                    let value = board.word(core,tcm,address)?;
                    words.push(WordValue { address: Hex(address), value });
                    if offset==4 {
                        let pointer = committed(value)?;
                        words.push(WordValue { address: Hex(pointer), value: board.word(core,tcm,pointer)? });
                    }
                }
            }
        }
    }
    let class = (arguments[0] as i32) >> 16;
    let number = arguments[0] as i16;
    if (0..=2).contains(&class) && (number > 0 || (number == 0 && service == Service::InterruptRaiseTaskException)) {
        let record = if class == 0 { committed(board.word(core, tcm, contexts-4)?)? } else { contexts+(class as u32-1)*0x84 };
        let count = committed(board.word(core, tcm, record+0x48)?)?;
        if number as u32 <= count {
            let base = committed(board.word(core, tcm, record+0x44)?)?;
            let task = if number==0 { base.wrapping_sub(0x38) } else { base+(number as u32-1)*0x38 };
            tasks.insert(task);
            for address in (task..task+0x38).step_by(4) { words.push(WordValue { address: Hex(address), value: board.word(core,tcm,address)? }); }
            let config = committed(board.word(core,tcm,task+8)?)?;
            for address in (config..config+0x28).step_by(4) { words.push(WordValue { address: Hex(address), value: board.word(core,tcm,address)? }); }
        }
    }
    for task in tasks {
        let state = committed(board.word(core,tcm,task+12)?)? & 0xff;
        if state & 2 != 0 {
            let wait = committed(board.word(core,tcm,task+0x14)?)?;
            if wait != 0 { words.push(WordValue { address: Hex(wait), value: board.word(core,tcm,wait)? }); }
        }
        if service == Service::SleepTask {
            let config = committed(board.word(core,tcm,task+8)?)?;
            let slot = committed(board.word(core,tcm,config+0x20)?)?;
            require(slot <= 120, "sleep result slot bound exceeded")?;
            let address = match domain { Domain::A => 0x4005477c, Domain::B => 0x400e473c } + 4*slot;
            words.push(WordValue { address: Hex(address), value: board.word(core,tcm,address)? });
        }
    }
    if service == Service::GetTaskPriority && arguments[1] != 0 {
        words.push(WordValue { address: Hex(arguments[1]), value: board.word(core,tcm,arguments[1])? });
    }
    words.sort_by_key(|word| word.address);
    words.dedup_by_key(|word| word.address);
    Ok(words)
}

const PC_WATCHES: [u32; 5] = [0x436ec, 0xd36ec, 0xec8014, 0x40b00, 0xd0b00];
const DISPATCH_LOADS: [u32; 2] = [0x436e4, 0xd36e4];

fn retained_dispatch_point(plan: &Plan, address: u32) -> bool {
    let domain = match address { 0x436e4 | 0x436ec => Domain::A, 0xd36e4 | 0xd36ec => Domain::B, _ => return true };
    !plan.entries.iter().any(|entry| entry.domain==domain && matches!(entry.semantic, Semantic::Dispatch { .. }))
}

fn committed(value: Read) -> Result<u32, String> {
    match value { Read::Committed { value } => Ok(value), _ => Err("scheduler state is not committed".into()) }
}

fn snapshot(board: &Board, core: Core, tcm: [u32; 2], helper: Helper, args: [u32; 4]) -> Result<Vec<WordValue>, String> {
    let mut addresses = BTreeSet::new();
    let mut add = |base: u32, bytes: u32| -> Result<(), String> {
        require(base != 0 && base % 4 == 0 && base.checked_add(bytes).is_some(), "invalid scheduler pointer")?;
        addresses.extend((0..bytes).step_by(4).map(|offset| base + offset));
        Ok(())
    };
    if matches!(helper, Helper::Acquire | Helper::Release) {
        add(args[0], 4)?;
        add(committed(board.word(core, tcm, args[0])?)?, 4)?;
    } else if helper != Helper::Notify {
        if helper == Helper::Reset {
            add(args[0], 0x38)?;
            add(committed(board.word(core, tcm, args[0] + 8)?)?, 0x18)?;
        } else {
            add(args[0], 0x80)?;
            let pointer = |offset| committed(board.word(core, tcm, args[0] + offset)?);
            add(pointer(0x20)?, 128)?;
            for offset in [8, 0x14, 0x24, 0x28, 0x2c, 0x30] { add(pointer(offset)?, 4)?; }
            let mut tasks = BTreeSet::new();
            if matches!(helper, Helper::Insert | Helper::Remove | Helper::Prepare | Helper::Requeue) { tasks.insert(args[1]); }
            let selected = committed(board.word(core, tcm, pointer(0x2c)?)?)?;
            if selected != 0 { tasks.insert(selected); }
            for priority in 0..16 {
                let head = pointer(0x20)? + priority * 8;
                for offset in [0, 4] {
                    let task = committed(board.word(core, tcm, head + offset)?)?;
                    if task != head { tasks.insert(task); }
                }
            }
            let initial: Vec<_> = tasks.iter().copied().collect();
            for task in initial {
                if task == args[1] && matches!(helper, Helper::Insert | Helper::Prepare) { continue; }
                for offset in [0, 4] {
                    let neighbor = committed(board.word(core, tcm, task + offset)?)?;
                    if neighbor != 0 { add(neighbor, 8)?; }
                }
            }
            for task in tasks {
                add(task, 0x38)?;
                if helper == Helper::Prepare && task == args[1] { add(committed(board.word(core, tcm, task + 8)?)?, 0x18)?; }
            }
        }
    }
    addresses.into_iter().map(|address| Ok(WordValue { address: Hex(address), value: board.word(core, tcm, address)? })).collect()
}
struct Observer<'p> {
    validated: &'p Validated,
    maps: Vec<MemoryMap>,
    return_sites: BTreeSet<u32>,
    open: OpenCalls,
    classes: BTreeMap<CallClass, ClassCount>,
    entries: u64,
    returns: u64,
    unrelated_continuations: u64,
    misplaced_stops: u64,
    untracked: Sample<OpenCall>,
    unarmed_entries: Sample<StopRecord>,
    fill_fetch: Option<StopRecord>,
    armings: Vec<Arming>,
    watch: Option<GuardWatch>,
    segments: Vec<Segment>,
    phase: String,
    milestones: Vec<Milestone>,
    data_hooks: Vec<(usize, HookId, Box<RefCell<DataCapture>>)>,
    bank_counts: [u64; 2],
    pc_watches: BTreeMap<(Core, u32), PcWatch>,
    dispatched: [bool; 2],
    probe_hits: Vec<ProbeHit>,
}

impl Observer<'_> {
    fn slot(&self, index: usize) -> &Slot { &self.validated.slots[index] }


    fn readback(&self, machine: &Machine, domains: &[Domain]) -> Result<Readback, String> {
        let mut ranges = Vec::new();
        for range in self.validated.plan.guarded.iter().filter(|range| domains.contains(&range.domain)) {
            let count = (range.end.0 - range.start.0) as usize;
            let physical = machine.read(u64::from(range.start.0), count)?;
            let mut views = Vec::new();
            for core in 0..2 {
                for address in spellings(range.start.0) {
                    let mut bytes = vec![0; count];
                    let read = machine.session.board.read_memory(core, u64::from(address), &mut bytes);
                    views.push(match read {
                        Ok(()) => ViewHash { core, address: Hex(address), sha256: Some(sha256(&bytes)), error: None },
                        Err(error) => ViewHash { core, address: Hex(address), sha256: None, error: Some(error) },
                    });
                }
            }
            ranges.push(RangeReadback { domain: range.domain, start: range.start, end: range.end, physical_sha256: sha256(&physical), views });
        }
        Ok(Readback { instructions: machine.instructions(), ranges })
    }

    fn mismatch(&self, readback: &Readback) -> Option<Hex> {
        readback.ranges.iter().find(|observed| !self.validated.plan.guarded.iter()
            .any(|range| range.start == observed.start && range.end == observed.end && observed.matches(&range.sha256)))
            .map(|observed| observed.start)
    }

    fn end_watch(&mut self, machine: &Machine) -> Result<(), String> {
        let Some(mut guard) = self.watch.take() else { return Ok(()) };
        let state = guard.watch.records.borrow().state();
        let ended = machine.session.board.memory_watch_end(&mut guard.watch);
        self.segments.push(Segment { domains: guard.domains, started: guard.started, ended: machine.instructions(), state });
        ended
    }

    fn arm(&mut self, machine: &Machine, context: &Context, domain: Domain) -> Result<(), String> {
        if self.armings.iter().any(|arming| arming.domain == domain) { return Ok(()); }
        let readback = self.readback(machine, &[domain])?;
        let differs = self.mismatch(&readback);
        self.armings.push(Arming { domain, stop: StopRecord::of(context), readback });
        if let Some(start) = differs {
            return Err(format!("arming readback of the extent at {:#x} differs from the plan", start.0));
        }
        self.end_watch(machine)?;
        let domains: Vec<Domain> = self.armings.iter().map(|arming| arming.domain).collect();
        let classes = self.validated.plan.guarded.iter().filter(|range| domains.contains(&range.domain))
            .filter(|range| !self.validated.plan.guarded.iter().any(|outer| outer.domain == range.domain
                && outer.start <= range.start && range.end <= outer.end && (outer.start != range.start || outer.end != range.end)))
            .map(|range| WatchRange { start: u64::from(range.start.0), end: u64::from(range.end.0), kinds: WatchKinds::WRITES }).collect();
        let watch = machine.session.board.memory_watch_begin_classified(classes, WATCH_LIMIT, self.maps.clone(), ViewPolicy::Observe)?;
        self.watch = Some(GuardWatch { watch, domains, started: machine.instructions() });
        Ok(())
    }

    fn check_watch(&self) -> Result<(), String> {
        let Some(guard) = &self.watch else { return Ok(()) };
        let records = guard.watch.records.borrow();
        require(records.violation().is_none() && records.row_count() == 0, "an immutable-byte watch recorded a write or a violation")
    }
    fn begin_data(&mut self, machine: &Machine) -> Result<(), String> {
        if self.data_hooks.is_empty() {
            for core in 0..2 {
                let context = Box::new(RefCell::new(DataCapture { active: BTreeSet::new(), rows: Vec::new(), overflow: false }));
                let data = (&*context as *const RefCell<DataCapture>).cast_mut().cast();
                // The context is retired only after remove_hook.
                let hook = unsafe { machine.session.board.cpu(core)?.install_hook(Hook::Memory {
                    callback: data_write, data, read: false, write: true, range: HookRange::All,
                })? };
                self.data_hooks.push((core, hook, context));
            }
        }
        for (core, _, capture) in &self.data_hooks {
            capture.borrow_mut().active = self.open.pending.iter().filter(|(key,_)| key.core.index()==*core)
                .flat_map(|(_,calls)| calls.iter().filter(|call| !call.suspended).flat_map(|call| call.entry.memory.iter().map(|word| word.address.0 & !ALIAS & !3))).collect();
        }
        machine.session.board.transaction_watch_begin(vec![(0xfffb30cc, 0xfffb30d0), (0xfffb40cc, 0xfffb40d0),
            (0xfffb30ec, 0xfffb30f0), (0xfffb40ec, 0xfffb40f0)], WATCH_LIMIT)
    }

    fn drain_data(&mut self, machine: &Machine) -> Result<(), String> {
        for (index, _, capture) in &self.data_hooks {
            let mut capture = capture.borrow_mut();
            require(!capture.overflow, "scheduler write hook overflowed or saw an unsupported width")?;
            let core = Core::from_index(*index)?;
            for (address, size, value) in capture.rows.drain(..) {
                for (key, calls) in &mut self.open.pending {
                    if key.core != core { continue; }
                    for call in calls {
                        if call.suspended { continue; }
                        if call.entry.memory.iter().any(|word| word.address.0 & !ALIAS & !3 == address & !ALIAS & !3) {
                            call.writes.push(Written { address: Hex(address), size, value });
                        }
                    }
                }
            }
        }
        let batch = machine.session.board.transaction_watch_end()?;
        require(batch.omitted == 0, "scheduler bank watch overflowed")?;
        for transaction in batch.transactions {
            if transaction.operation.as_ref() != "write" { continue; }
            require(transaction.accepted && transaction.size == 4, "scheduler bank write was refused or not a word")?;
            let core = Core::from_index(transaction.cpu)?;
            self.bank_counts[core.index()] += 1;
            let write = Written { address: Hex(u32::try_from(transaction.address).map_err(|_| "bank address overflow")?),
                size: 4, value: u32::try_from(transaction.value).map_err(|_| "bank value overflow")? };
            for (_, calls) in self.open.pending.iter_mut().filter(|(key,_)| key.core == core) {
                for call in calls.iter_mut().filter(|call| !call.suspended) { call.bank.push(write.clone()); }
            }
        }
        Ok(())
    }
    fn end_data(&mut self, machine: &Machine) -> Result<(), String> {
        let mut failure = None;
        for (core, hook, context) in self.data_hooks.drain(..) {
            if let Err(error) = machine.session.board.cpu(core).and_then(|cpu| cpu.remove_hook(hook)) {
                std::mem::forget(context);
                failure.get_or_insert(error);
            }
        }
        match failure { Some(error) => Err(error), None => Ok(()) }
    }

    fn enter(&mut self, machine: &mut Machine, context: &Context, tcm: [u32; 2], index: usize, origin: Option<(DispatchPoint,Vec<Boundary>)>) -> Result<(), String> {
        let board = Board { machine, maps: &self.maps };
        let slot = self.slot(index);
        let domain = slot.id.domain;
        require(context.core.index() == match domain { Domain::A => 0, Domain::B => 1 }, "scheduler entry executed on the other core")?;
        let mut entry = EntryState {
            slot: slot.id, core: context.core, cpsr: context.cpsr, r0: context.r0, lr: Hex(context.lr),
            arguments: context.arguments,
            task: context.current, sp: Hex(context.sp), registers: context.registers,
            cpacr: board.register(context.core, arm::Register::Cpacr)?,
            fpexc: board.register(context.core, arm::Register::Fpexc)?,
            tpidrurw: board.cp15(context.core, 13, 0, 2)?,
            origin: origin.as_ref().map(|(point,_)| *point),
            memory: match slot.id.semantic {
                Semantic::Helper { name } => snapshot(&board, context.core, tcm, name, context.arguments)?,
                Semantic::Service { name } => service_snapshot(&board, context.core, tcm, domain, name, context.arguments)?,
                _ => root_snapshot(&board, context.core, tcm, domain)?,
            },
        };
        if let Semantic::Dispatch { name } = entry.slot.semantic {
            let (start, bytes) = match name {
                DispatchPoint::Save => (context.sp.saturating_sub(36), 36),
                DispatchPoint::IrqReturn => (context.sp.saturating_sub(32), 64),
                DispatchPoint::Resume => (context.sp, 36),
                DispatchPoint::IrqResume | DispatchPoint::IrqTail => (context.sp, 68),
                _ => (context.sp, 0),
            };
            for address in (start..start+bytes).step_by(4) {
                entry.memory.push(WordValue { address: Hex(address), value: board.word(context.core,tcm,address)? });
            }
            entry.memory.sort_by_key(|word| word.address);
            entry.memory.dedup_by_key(|word| word.address);
        }
        self.entries += 1;
        if !self.armings.iter().any(|arming| arming.domain == domain) { self.unarmed_entries.push(StopRecord::of(context)); }
        let semantic = entry.slot.semantic;
        let call = OpenCall { entry, entered: context.instructions, writes: Vec::new(), bank: Vec::new(),
            history: origin.map(|(_,history)| history).unwrap_or_default(), write_cursor: 0, bank_cursor: 0, suspended: false };
        let continuation = match semantic {
            Semantic::Dispatch { name: DispatchPoint::Select } => {
                let selected_cell = match domain { Domain::A => 0x40054768, Domain::B => 0x400e4728 };
                let task = committed(board.word(context.core, tcm, selected_cell)?)?;
                if task == 0 { 0 } else { committed(board.word(context.core, tcm, task+0x1c)?)? & !1 }
            }
            Semantic::Dispatch { name: DispatchPoint::FirstEntry } => {
                let task = committed(context.current)?;
                let config = committed(board.word(context.core, tcm, task+8)?)?;
                committed(board.word(context.core, tcm, config+8)?)? & !1
            }
            Semantic::Dispatch { name: DispatchPoint::Resume } => committed(board.word(context.core,tcm,context.sp+32)?)? & !1,
            Semantic::Dispatch { name: DispatchPoint::IrqResume } => match domain { Domain::A => 0x437a8, Domain::B => 0xd37a8 },
            Semantic::Dispatch { name: DispatchPoint::IrqTail } => committed(board.word(context.core,tcm,context.sp+28)?)? & !1,
            _ => context.lr & !1,
        };
        if !matches!(semantic, Semantic::Dispatch { .. }) && (continuation == 0 || (context.lr & 1 == 0 && continuation % 4 != 0)) {
            self.untracked.push(call);
            return Ok(());
        }
        if continuation != 0 && !self.return_sites.contains(&continuation) {
            if !machine.traces.sites.contains_key(&continuation) {
                require(machine.traces.sites.len() < SITE_LIMIT, "continuation site bound exceeded")?;
                machine.traces.add_sites(&machine.session, [(continuation, CONTINUATION_LABEL.to_owned())].into_iter().collect())?;
            }
            self.return_sites.insert(continuation);
        }
        if matches!(semantic, Semantic::Dispatch { .. }) {
            let mut key = ReturnKey::of(context, continuation);
            key.task = None;
            key.sp = 0;
            key.mode = 0;
            return self.open.open(key, call);
        }
        self.open.open(ReturnKey::of(context, continuation), call)
    }

    fn finish(&mut self, machine: &Machine, context: &Context, tcm: [u32; 2], closed: Vec<OpenCall>, completion: Completion) -> Result<(), String> {
        let board = Board { machine, maps: &self.maps };
        for mut call in closed {
            let memory = call.entry.memory.iter().map(|word| Ok(WordValue {
                address: word.address, value: board.word(context.core, tcm, word.address.0)?,
            })).collect::<Result<Vec<_>, String>>()?;
            if matches!(call.entry.slot.semantic, Semantic::Service { .. }) && completion == Completion::Return {
                call.record(Boundary { kind: BoundaryKind::Return, address: Hex(context.pc), instructions: context.instructions,
                    core: context.core, task: context.current, sp: Hex(context.sp), cpsr: context.cpsr,
                    registers: context.registers, memory: memory.clone(), writes: Vec::new(),
                    bank: Vec::new(), value: Some(context.r0),
                    cpacr: board.register(context.core, arm::Register::Cpacr)?,
                    fpexc: board.register(context.core, arm::Register::Fpexc)?,
                    tpidrurw: board.cp15(context.core, 13, 0, 2)? })?;
            }
            let class = CallClass { entry: call.entry, result: ReturnState { r0: context.r0, cpsr: context.cpsr,
                memory, writes: call.writes, bank: call.bank, completion, task: context.current,
                sp: Hex(context.sp), registers: context.registers, history: call.history,
                cpacr: board.register(context.core, arm::Register::Cpacr)?,
                fpexc: board.register(context.core, arm::Register::Fpexc)?,
                tpidrurw: board.cp15(context.core, 13, 0, 2)? } };
            self.returns += 1;
            if let Some(count) = self.classes.get_mut(&class) {
                count.count += 1;
                count.last = call.entered;
            } else {
                require(self.classes.len() < CLASS_LIMIT, "call class bound exceeded")?;
                self.classes.insert(class, ClassCount { count: 1, first: call.entered, last: call.entered });
            }
        }
        Ok(())
    }

    fn complete(&mut self, machine: &Machine, context: &Context, tcm: [u32; 2]) -> Result<(), String> {
        let key = ReturnKey::of(context, context.pc);
        let closed = self.open.close(&key);
        if closed.is_empty() { self.unrelated_continuations += 1; }
        if context.r0 == 0 && closed.iter().any(|call| call.entry.slot.semantic == (Semantic::Helper { name: Helper::Acquire })) {
            self.boundary(machine,context,tcm,BoundaryKind::Acquired,Some(0))?;
        }
        self.finish(machine, context, tcm, closed, Completion::Return)
    }

    fn boundary(&mut self, machine: &Machine, context: &Context, tcm: [u32; 2], kind: BoundaryKind, value: Option<u32>) -> Result<(), String> {
        let board = Board { machine, maps: &self.maps };
        for (key, calls) in &mut self.open.pending {
            if key.core != context.core { continue; }
            for call in calls {
                if key.task.is_some() && call.entry.task != context.current { continue; }
                if call.suspended && kind != BoundaryKind::Resume
                    && !(kind==BoundaryKind::DiscardedRead && matches!(context.pc & !ALIAS,0x43650 | 0xd3650)) { continue; }
                let memory = call.entry.memory.iter().map(|word| Ok(WordValue {
                    address: word.address, value: board.word(context.core, tcm, word.address.0)?,
                })).collect::<Result<Vec<_>, String>>()?;
                call.record(Boundary { kind, address: Hex(context.pc), instructions: context.instructions,
                    core: context.core, task: context.current, sp: Hex(context.sp), cpsr: context.cpsr,
                    registers: context.registers, memory, writes: Vec::new(),
                    bank: Vec::new(), value,
                    cpacr: board.register(context.core, arm::Register::Cpacr)?,
                    fpexc: board.register(context.core, arm::Register::Fpexc)?,
                    tpidrurw: board.cp15(context.core, 13, 0, 2)? })?;
            }
        }
        Ok(())
    }

    fn dispatch_boundary(&mut self, machine: &Machine, context: &Context, tcm: [u32; 2], semantic: Option<Semantic>) -> Result<Option<(DispatchPoint,Vec<Boundary>)>, String> {
        let stage_entry = matches!(semantic, Some(Semantic::Dispatch { .. }));
        let delivery = matches!(semantic, Some(Semantic::Service { name: Service::DeliverTaskExceptionEntry }));
        let closed = self.open.take_where(|key, call| key.core == context.core && match call.entry.slot.semantic {
            Semantic::Dispatch { name } => stage_entry || key.pc == context.pc || (delivery && name == DispatchPoint::Resume),
            _ => false,
        });
        let origin = if semantic == Some(Semantic::Dispatch { name: DispatchPoint::Select }) {
            closed.last().and_then(|call| match call.entry.slot.semantic {
                Semantic::Dispatch { name } => Some((call.entry.origin.unwrap_or(name), call.history.iter()
                    .filter(|event| matches!(event.kind,BoundaryKind::SelectedRead | BoundaryKind::DiscardedRead | BoundaryKind::EnabledRead | BoundaryKind::GlobalLockRead))
                    .map(|event| { let mut inherited=event.clone(); inherited.writes.clear(); inherited.bank.clear(); inherited }).collect())),
                _ => None,
            })
        } else { None };
        if !closed.is_empty() { self.finish(machine, context, tcm, closed, Completion::Dispatched)?; }
        let kind = match semantic {
            Some(Semantic::Dispatch { name: DispatchPoint::Save }) => Some(BoundaryKind::Save),
            Some(Semantic::Dispatch { name: DispatchPoint::Resume }) => Some(BoundaryKind::Resume),
            Some(Semantic::Dispatch { name: DispatchPoint::NoSave }) => Some(BoundaryKind::NoSave),
            _ => None,
        };
        if let Some(kind) = kind { self.boundary(machine, context, tcm, kind, Some(context.r0))?; }
        if kind == Some(BoundaryKind::NoSave) {
            let discarded = self.open.take_where(|key, call| key.core == context.core && call.entry.task == context.current && matches!(call.entry.slot.semantic, Semantic::Service { .. }));
            self.finish(machine, context, tcm, discarded, Completion::Discard)?;
        }
        Ok(origin)
    }

    fn stop(&mut self, machine: &mut Machine, stops: &[(Core, u32)]) -> Result<(), String> {
        self.drain_data(machine)?;
        for &(core, point) in stops {
            let (context, tcm) = Board { machine, maps: &self.maps }.context(core)?;
            if context.pc != point {
                self.misplaced_stops += 1;
                continue;
            }
            if PC_WATCHES.contains(&(context.pc & !ALIAS)) && retained_dispatch_point(&self.validated.plan, context.pc & !ALIAS) {
                let address = context.pc & !ALIAS;
                self.pc_watches.entry((core, address)).and_modify(|watch| watch.count += 1)
                    .or_insert(PcWatch { core, address: Hex(address), count: 1, first: context.instructions });
            }
            let semantic = self.validated.sites.entries.get(&context.pc).map(|&index| self.slot(index).id.semantic);
            let origin = self.dispatch_boundary(machine, &context, tcm, semantic)?;
            let low = context.pc & !ALIAS;
            if DISPATCH_LOADS.contains(&low) && retained_dispatch_point(&self.validated.plan, low) { self.dispatched[core.index()] = true; }
            if [0x436ec, 0xd36ec].contains(&low) && retained_dispatch_point(&self.validated.plan, low) {
                require(!self.dispatched[core.index()], "null selected cell after the first dispatch")?;
            }
            if let Some(indices) = self.validated.sites.probes.get(&context.pc).cloned() {
                for index in indices {
                    require(self.probe_hits.len()<CLASS_LIMIT, "probe record bound exceeded")?;
                    let probe = &self.validated.plan.probes[index];
                    let board = Board { machine, maps: &self.maps };
                    let value = if probe.kind==ProbeKind::GlobalLockRead && self.validated.plan.role==Role::Stock {
                        committed(board.word(core,tcm,0x400c0804)?)?
                    } else { board.register(core, arm::Register::R(probe.register.0))? };
                    self.probe_hits.push(ProbeHit { domain: probe.domain, core, address: probe.address, kind: probe.kind,
                        register: probe.register, value, cpsr: context.cpsr, sp: Hex(context.sp), lr: Hex(context.lr), instructions: context.instructions });
                    let kind = match probe.kind {
                        ProbeKind::SelectedRead => BoundaryKind::SelectedRead,
                        ProbeKind::DiscardedRead => BoundaryKind::DiscardedRead,
                        ProbeKind::EnabledRead => BoundaryKind::EnabledRead,
                        ProbeKind::GlobalLockRead => BoundaryKind::GlobalLockRead,
                        ProbeKind::Retry => BoundaryKind::Retry,
                        ProbeKind::Callback => BoundaryKind::Callback,
                        ProbeKind::CallbackReturn => BoundaryKind::CallbackReturn,
                        ProbeKind::SelectComplete => BoundaryKind::SelectComplete,
                        ProbeKind::FirstEntryExit => BoundaryKind::FirstEntryExit,
                        ProbeKind::IrqTail => BoundaryKind::IrqTail,
                    };
                    self.boundary(machine, &context, tcm, kind, Some(value))?;
                    if kind == BoundaryKind::DiscardedRead && value == 1 && self.validated.plan.probes[index].register.0 == 3 {
                        let discarded = self.open.take_where(|key,call| key.core==core && call.entry.task==context.current && matches!(call.entry.slot.semantic,Semantic::Service { .. }));
                        self.finish(machine,&context,tcm,discarded,Completion::Discard)?;
                    }
                    if kind == BoundaryKind::SelectedRead && self.validated.plan.probes[index].register.0 == 1 {
                        require(value != 0 || !self.dispatched[core.index()], "null selected task after first dispatch")?;
                        if value != 0 {
                            let destination = committed(Board { machine, maps: &self.maps }.word(core,tcm,value+0x1c)?)? & !1;
                            if !machine.traces.sites.contains_key(&destination) {
                                require(machine.traces.sites.len()<SITE_LIMIT, "dispatch destination site bound exceeded")?;
                                machine.traces.add_sites(&machine.session, [(destination,CONTINUATION_LABEL.to_owned())].into_iter().collect())?;
                            }
                            self.return_sites.insert(destination);
                            let selected = self.open.take_where(|key,call| key.core==core && matches!(call.entry.slot.semantic,Semantic::Dispatch { name: DispatchPoint::Select }));
                            for call in selected {
                                self.open.open(ReturnKey { core,pc:destination,sp:0,task:None,mode:0 },call)?;
                            }
                            self.dispatched[core.index()] = true;
                        }
                    }
                }
            }
            if self.return_sites.contains(&context.pc) { self.complete(machine, &context, tcm)?; }
            let sites = &self.validated.sites;
            if sites.fills.contains(&context.pc) {
                self.fill_fetch = Some(StopRecord::of(&context));
                return Err(format!("fill word at {:#x} fetched", context.pc));
            }
            if let Some(&domain) = sites.arms.get(&context.pc) { self.arm(machine, &context, domain)?; }
            if let Some(&index) = sites.entries.get(&context.pc) { self.enter(machine, &context, tcm, index, origin)?; }
        }
        self.begin_data(machine)?;
        Ok(())
    }

    fn advance(&mut self, machine: &mut Machine, budget: u64) -> Result<(), String> {
        self.advance_until(machine, budget, None)
    }

    /// Runs `budget` instructions, or until board tick `until` when given.
    fn advance_until(&mut self, machine: &mut Machine, budget: u64, until: Option<u64>) -> Result<(), String> {
        let end = machine.instructions().checked_add(budget).filter(|end| *end <= INSTRUCTION_LIMIT)
            .ok_or("pair instruction bound exceeded")?;
        let reached = |machine: &Machine| machine.instructions() >= end || until.is_some_and(|tick| machine.session.board.now() >= tick);
        while !reached(machine) {
            machine.traces.hits.clear();
            let before = machine.instructions();
            let outcome = machine.run_until(end - before, until);
            machine.require_running()?;
            require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected pair run outcome")?;
            let mut stops = Vec::with_capacity(2);
            for hit in &machine.traces.hits {
                require(hit.instructions == machine.instructions(), "pair stop is no longer current")?;
                let core = Core::from_index(hit.core)?;
                require(!stops.iter().any(|&(other, _)| other == core), "one core stopped twice at one boundary")?;
                stops.push((core, parse_hex(&hit.address)?));
            }
            if stops.is_empty() {
                require(reached(machine), "run ended early without a pair stop")?;
            } else {
                self.stop(machine, &stops)?;
            }
            self.check_watch()?;
            require(machine.instructions() > before || !stops.is_empty(), "pair made no progress")?;
        }
        Ok(())
    }

    fn pulse(&mut self, machine: &mut Machine, key: Key, pulse: KeyPulse) -> Result<Pulse, String> {
        let pressed = machine.instructions();
        machine.press(key.guest())?;
        match pulse.hold {
            Hold::Instructions(hold) => self.advance(machine, hold)?,
            Hold::BoardTicks(ticks) => {
                let until = machine.session.board.now().saturating_add(ticks);
                self.advance_until(machine, machine.hold_bound(ticks), Some(until))?;
            }
        }
        machine.release_key(key.guest())?;
        let released = machine.instructions();
        let debounced = loop {
            if machine.document.firmware.released(&machine.session, key.guest())? { break true; }
            let spent = machine.instructions() - released;
            if spent >= pulse.release { break false; }
            self.advance(machine, (pulse.release - spent).min(RELEASE_POLL))?;
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
            self.pulse(machine, key, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        }
        self.advance(machine, SETUP_SETTLE)?;
        self.phase = "shooting-menu".into();
        let pulse = self.pulse(machine, Key::Menu, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        self.advance(machine, SHOOTING_SETTLE)?;
        self.endpoint(machine, Some(pulse), MenuTriple { menu: 1, setting: 5, cursor: 2 })?;
        for phase in trajectory {
            self.phase = phase.phase.clone();
            for &(key, menu, setting, cursor) in &phase.steps {
                let pulse = self.pulse(machine, key, key_pulse(key.guest()))?;
                self.state(machine, pulse, MenuTriple { menu, setting, cursor })?;
            }
        }
        Ok(())
    }

    fn reached(&self) -> Vec<Reached> {
        self.validated.slots.iter().map(|slot| {
            let rows: Vec<_> = self.classes.iter().filter(|(class, _)| class.entry.slot == slot.id).map(|(_, count)| count).collect();
            let pending = self.open.pending.values().flatten().filter(|call| call.entry.slot == slot.id).count() as u64;
            let untracked = self.untracked.sample.iter().filter(|call| call.entry.slot == slot.id).count() as u64;
            let returns: u64 = rows.iter().map(|count| count.count).sum();
            Reached {
                slot: slot.id, calls: returns + pending + untracked, returns, pending,
                first: rows.iter().map(|count| count.first).min(), last: rows.iter().map(|count| count.last).max(),
            }
        }).collect()
    }
}

#[derive(Serialize)]
struct Report<'a> {
    schema: u32,
    role: Role,
    inputs: Vec<Binding>,
    reset_sha256: String,
    limits: Limits,
    instructions: u64,
    counts: Option<[u64; 2]>,
    entries: u64,
    returns: u64,
    unrelated_continuations: u64,
    misplaced_stops: u64,
    continuation_sites: usize,
    untracked: &'a Sample<OpenCall>,
    unarmed_entries: &'a Sample<StopRecord>,
    fill_fetch: Option<StopRecord>,
    armings: &'a [Arming],
    segments: &'a [Segment],
    final_readback: Option<Readback>,
    milestones: &'a [Milestone],
    reached: Vec<Reached>,
    classes: Vec<ClassRow<'a>>,
    pending: Vec<&'a OpenCall>,
    error: Option<String>,
    machine_failure: Option<String>,
    bank_counts: [u64; 2],
    pc_watches: Vec<&'a PcWatch>,
    probes: &'a [ProbeHit],
}

fn arguments() -> Result<BTreeMap<String, PathBuf>, String> {
    const OPTIONS: [&str; 6] = ["--plan", "--run", "--update", "--boot-rom", "--trajectory", "--output"];
    let mut arguments = std::env::args_os().skip(1);
    let mut paths = BTreeMap::new();
    while let Some(flag) = arguments.next() {
        let name = flag.to_str().filter(|name| OPTIONS.contains(name)).ok_or(USAGE)?.to_owned();
        require(paths.insert(name, PathBuf::from(arguments.next().ok_or(USAGE)?)).is_none(), USAGE)?;
    }
    require(paths.len() == OPTIONS.len(), USAGE)?;
    Ok(paths)
}

fn bound(path: &Path) -> Result<(Vec<u8>, Binding), String> {
    let bytes = read(path)?;
    let digest = sha256(&bytes);
    Ok((bytes, Binding { path: path.to_owned(), sha256: digest }))
}

fn drive() -> Result<(), String> {
    let paths = arguments()?;
    let output = paths["--output"].clone();
    let (plan_bytes, plan_binding) = bound(&paths["--plan"])?;
    let (run_bytes, run_binding) = bound(&paths["--run"])?;
    let (dat, dat_binding) = bound(&paths["--update"])?;
    let (rom, rom_binding) = bound(&paths["--boot-rom"])?;
    let (trajectory_bytes, trajectory_binding) = bound(&paths["--trajectory"])?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let (_, executable_binding) = bound(&executable)?;
    let plan: Plan = serde_json::from_slice(&plan_bytes).map_err(|error| format!("plan: {error}"))?;
    let validated = validate(plan)?;
    require(dat_binding.sha256 == validated.plan.dat_sha256, "DAT differs from the plan's DAT")?;
    let trajectory: Trajectory = serde_json::from_slice(&trajectory_bytes).map_err(|error| format!("trajectory: {error}"))?;
    let phases: Vec<&str> = trajectory.before_unload.iter().map(|phase| phase.phase.as_str()).collect();
    require(phases == ["package-menu-prefix", "modal75-shooting", "modal75-film", "modal75-returned", "modal75-film-repeated",
        "modal75-returned-repeated", "modal75-handback"], "trajectory phases differ from the #128 FILM SIMULATION route")?;
    require(trajectory.before_unload[1].steps.iter().map(|&(_, menu, setting, cursor)| [menu, setting, cursor]).eq([[1, 5, 2]]),
        "trajectory shooting-menu entry differs")?;
    require(trajectory.before_unload.iter().all(|phase| phase.key_pulse.is_none()), "trajectory pins a key pulse; pair keys take the default pulse")?;
    std::fs::create_dir(&output).map_err(|error| format!("{}: {error}", output.display()))?;

    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(dat), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    let restored = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "reset checkpoint is not preexecution")?;

    let mut observer = Observer {
        validated: &validated,
        maps,
        return_sites: BTreeSet::new(),
        open: OpenCalls::default(),
        classes: BTreeMap::new(),
        entries: 0,
        returns: 0,
        unrelated_continuations: 0,
        misplaced_stops: 0,
        untracked: Sample::default(),
        unarmed_entries: Sample::default(),
        fill_fetch: None,
        armings: Vec::new(),
        watch: None,
        segments: Vec::new(),
        phase: String::new(),
        milestones: Vec::new(),
        data_hooks: Vec::new(),
        bank_counts: [0; 2], pc_watches: BTreeMap::new(),
        dispatched: [false; 2],
        probe_hits: Vec::new(),
    };
    machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
    machine.traces.stop_labels = [ENTRY_LABEL, CONTINUATION_LABEL, FILL_LABEL, ARM_LABEL, "scheduler-pc-watch", "scheduler-probe"]
        .into_iter().map(str::to_owned).collect();
    let observed = (|| -> Result<(), String> {
        let mut labels = validated.sites.labels();
        for address in PC_WATCHES.into_iter().chain(DISPATCH_LOADS).filter(|&address| retained_dispatch_point(&validated.plan,address)).flat_map(spellings) {
            labels.insert(address, "scheduler-pc-watch".into());
        }
        machine.traces.arm(&machine.session, labels)?;
        observer.begin_data(&machine)?;
        observer.scenario(&mut machine, &trajectory.before_unload[2..])
    })();
    let domains: Vec<Domain> = observer.armings.iter().map(|arming| arming.domain).collect();
    let final_readback = observer.readback(&machine, &domains);
    let ended = observer.end_watch(&machine);
    let (final_readback, read_error) = match final_readback {
        Ok(readback) => (Some(readback), None),
        Err(error) => (None, Some(error)),
    };
    let drained = observer.drain_data(&machine);
    let unhooked = observer.end_data(&machine);
    let error = observed.err().or(ended.err()).or(read_error).or(drained.err()).or(unhooked.err());
    let report = Report {
        schema: SCHEMA,
        role: validated.plan.role,
        inputs: vec![plan_binding, run_binding, dat_binding, rom_binding, trajectory_binding, executable_binding],
        reset_sha256: sha256(&checkpoint),
        limits: Limits { sites: SITE_LIMIT, pending: PENDING_LIMIT, classes: CLASS_LIMIT, samples: SAMPLE_LIMIT, instructions: INSTRUCTION_LIMIT, watch_rows: WATCH_LIMIT },
        instructions: machine.instructions(),
        counts: machine.session.board.instruction_counts().try_into().ok(),
        entries: observer.entries,
        returns: observer.returns,
        unrelated_continuations: observer.unrelated_continuations,
        misplaced_stops: observer.misplaced_stops,
        continuation_sites: observer.return_sites.len(),
        untracked: &observer.untracked,
        unarmed_entries: &observer.unarmed_entries,
        fill_fetch: observer.fill_fetch,
        armings: &observer.armings,
        segments: &observer.segments,
        final_readback,
        milestones: &observer.milestones,
        reached: observer.reached(),
        classes: observer.classes.iter().map(|(class, count)| ClassRow { class, count }).collect(),
        pending: observer.open.pending.values().flatten().collect(),
        error: error.clone(),
        machine_failure: machine.failure.clone(),
        bank_counts: observer.bank_counts,
        pc_watches: observer.pc_watches.values().collect(),
        probes: &observer.probe_hits,
    };
    let file = File::create_new(output.join("observation.json")).map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &report).map_err(|error| error.to_string())?;
    writer.write_all(b"\n").and_then(|()| writer.flush()).map_err(|error| error.to_string())?;
    match error { Some(error) => Err(error), None => Ok(()) }
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("scheduler-pair: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> OpenCall {
        OpenCall { entry: EntryState { slot: SlotId { domain: Domain::A, entry: Hex(0x45cd4),
            semantic: Semantic::Service { name: Service::SleepTask } }, core: Core::Primary, cpsr: 0x53,
            arguments: [0;4], memory: vec![], r0: 0, lr: Hex(0x8000), task: Read::Committed { value: 0x9000 },
            sp: Hex(0xa000), registers: [0;16], cpacr: 0, fpexc: 0, tpidrurw: 7, origin: None },
            entered: 1, writes: vec![], bank: vec![], history: vec![], write_cursor: 0, bank_cursor: 0, suspended: false }
    }

    fn boundary(kind: BoundaryKind) -> Boundary {
        let mut registers = [0;16];
        registers[0] = 0xffff_ffef;
        registers[13] = 0x9fd0;
        registers[15] = 0x43640;
        Boundary { kind, address: Hex(0x43640), instructions: 20, core: Core::Primary,
            task: Read::Committed { value: 0x9000 }, sp: Hex(0x9fd0), cpsr: 0xd3, registers,
            memory: vec![WordValue { address: Hex(0x9018), value: Read::Committed { value: 0x9fd0 } }],
            writes: vec![], bank: vec![], value: Some(0xffff_ffef), cpacr: 0xf00000, fpexc: 0, tpidrurw: 7 }
    }

    #[test]
    fn every_boundary_exports_loaded_values_and_completion_state() {
        for (kind, name) in [(BoundaryKind::Save,"save"),(BoundaryKind::Resume,"resume"),
            (BoundaryKind::NoSave,"no-save"),(BoundaryKind::Retry,"retry"),(BoundaryKind::Callback,"callback"),
            (BoundaryKind::CallbackReturn,"callback-return"),(BoundaryKind::SelectedRead,"selected-read"),
            (BoundaryKind::DiscardedRead,"discarded-read"),(BoundaryKind::EnabledRead,"enabled-read"),
            (BoundaryKind::GlobalLockRead,"global-lock-read"),
            (BoundaryKind::SelectComplete,"select-complete"),(BoundaryKind::FirstEntryExit,"first-entry-exit"),
            (BoundaryKind::IrqTail,"irq-tail"),(BoundaryKind::Return,"return"),(BoundaryKind::Acquired,"acquired")] {
            let mut call = call();
            call.writes.push(Written { address: Hex(0x9018), size: 4, value: 0x9fd0 });
            call.bank.push(Written { address: Hex(0xfffb30ec), size: 4, value: 0x1000000 });
            call.record(boundary(kind)).unwrap();
            let wire = serde_json::to_value(&call).unwrap();
            let event = &wire["history"][0];
            assert_eq!(event["kind"], name);
            assert_eq!(event["registers"][0], 0xffff_ffefu32);
            assert_eq!(event["value"], 0xffff_ffefu32);
            assert_eq!(event["sp"], "0x9fd0");
            assert_eq!(event["cpsr"], 0xd3);
            assert_eq!(event["memory"][0]["value"]["value"], 0x9fd0);
            assert_eq!(event["writes"][0]["address"], "0x9018");
            assert_eq!(event["bank"][0]["address"], "0xfffb30ec");
        }
    }

    #[test]
    fn boundary_classification_ignores_sample_time_but_keeps_loaded_state() {
        let first = boundary(BoundaryKind::SelectedRead);
        let mut later = first.clone();
        later.instructions = 900;
        assert_eq!(first,later);
        later.value = Some(0x9100);
        assert_ne!(first,later);
    }

    #[test]
    fn scheduler_completion_keeps_running_saved_frame_trampoline_and_doorbell_effects() {
        let mut call = call();
        for (address,value) in [(0x40054760,0x9100),(0x9018,0x9fd0),(0x901c,0x43668),(0x400546d8,1)] {
            call.writes.push(Written { address: Hex(address),size:4,value });
        }
        call.bank.push(Written { address: Hex(0xfffb30ec),size:4,value:0x1000000 });
        let mut event = boundary(BoundaryKind::FirstEntryExit);
        event.cpsr = 0x53;
        event.registers[15] = 0x8100;
        call.record(event).unwrap();
        let wire = serde_json::to_value(call).unwrap();
        assert_eq!(wire["history"][0]["cpsr"],0x53);
        assert_eq!(wire["history"][0]["registers"][15],0x8100);
        let writes=wire["history"][0]["writes"].as_array().unwrap();
        assert_eq!(writes[0]["address"],"0x40054760");
        assert_eq!(writes[0]["value"],0x9100);
        assert_eq!(writes[1]["value"],0x9fd0);
        assert_eq!(writes[2]["value"],0x43668);
        assert_eq!(writes[3]["address"],"0x400546d8");
        assert_eq!(writes[3]["value"],1);
        assert_eq!(wire["history"][0]["bank"][0]["address"],"0xfffb30ec");
    }

    #[test]
    fn history_segments_effects_and_retains_save_resume_retry_callback_order() {
        let mut call = call();
        call.writes.push(Written { address: Hex(0x900c), size: 1, value: 10 });
        call.record(boundary(BoundaryKind::Save)).unwrap();
        assert!(call.suspended);
        call.record(boundary(BoundaryKind::Resume)).unwrap();
        assert!(!call.suspended);
        call.record(boundary(BoundaryKind::Retry)).unwrap();
        call.bank.push(Written { address: Hex(0xfffb40cc), size: 4, value: 0x1000000 });
        call.record(boundary(BoundaryKind::Callback)).unwrap();
        call.record(boundary(BoundaryKind::CallbackReturn)).unwrap();
        call.record(boundary(BoundaryKind::Return)).unwrap();
        let wire = serde_json::to_value(call).unwrap();
        let history = wire["history"].as_array().unwrap();
        assert_eq!(history.iter().map(|event| event["kind"].as_str().unwrap()).collect::<Vec<_>>(),
            ["save","resume","retry","callback","callback-return","return"]);
        assert_eq!(history[0]["writes"].as_array().unwrap().len(), 1);
        assert_eq!(history[1]["writes"].as_array().unwrap().len(), 0);
        assert_eq!(history[3]["bank"].as_array().unwrap().len(), 1);
        assert_eq!(history[4]["bank"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn returns_match_task_core_stack_and_continuation() {
        let key = ReturnKey { core: Core::Primary, pc: 0x8000, sp: 0xa000, task: Some(0x9000), mode: 0x13 };
        let mut calls = OpenCalls::default();
        calls.open(key,call()).unwrap();
        for other in [ReturnKey { core: Core::Secondary,..key },ReturnKey { task: Some(0x9100),..key },
            ReturnKey { sp: 0xa004,..key },ReturnKey { pc: 0x8004,..key }] {
            assert!(calls.close(&other).is_empty());
        }
        assert_eq!(calls.close(&key).len(), 1);
    }

    #[test]
    fn no_save_discards_only_services_owned_by_the_current_task_and_core() {
        let key = ReturnKey { core: Core::Primary, pc: 0x8000, sp: 0xa000, task: Some(0x9000), mode: 0x13 };
        let mut calls = OpenCalls::default();
        calls.open(key,call()).unwrap();
        let other = ReturnKey { core: Core::Secondary,..key };
        calls.open(other,call()).unwrap();
        let closed = calls.take_where(|key,call| key.core==Core::Primary && call.entry.task==(Read::Committed { value: 0x9000 }));
        assert_eq!(closed.len(), 1);
        assert_eq!(calls.close(&other).len(), 1);
        assert_eq!(serde_json::to_value(Completion::Discard).unwrap(), "discard");
        assert_eq!(serde_json::to_value(Completion::Dispatched).unwrap(), "dispatched");
    }
}

