
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Access, Cpu, Hook, HookId, HookRange, InstructionEntryId, Register, arm};
use fujimod_xm1::bridge::{read, sha256};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::machine::{Hold, KeyPulse, Machine, RunOutcome, key_pulse};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::cell::RefCell;
use std::cell::Cell;
use std::rc::Rc;
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

/// The family bodies and their shared helpers, in the catalog's kebab-case spelling.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FamilyService {
    FamilyRelease, HelperRelease, HelperBlock, HelperBlockTimed, HelperCheck,
    FamilyFlagCreate, FamilyFlagDelete, FamilyFlagSet, FamilyFlagSetHandler, FamilyFlagClear,
    FamilyFlagWait, FamilyFlagPoll, FamilyFlagTimedWait,
    FamilyMailboxCreate, FamilyMailboxDelete, FamilyMailboxSend, FamilyMailboxReceive,
    FamilyMailboxPoll, FamilyMailboxTimedReceive,
}

/// The object table a family service names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Family { Flag, Mailbox }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum IrqKind { Doorbell }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Semantic { Helper { name: Helper }, Dispatch { name: DispatchPoint }, Service { name: Service }, Irq { name: IrqKind }, Family { name: FamilyService } }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProbeKind { SelectedRead, DiscardedRead, EnabledRead, GlobalLockRead, Retry, Callback, CallbackReturn, SelectComplete, FirstEntryExit, IrqTail, TimerInsert, TimerRemove, TimerConsume }

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
    fn own_kernel_context(self) -> u32 { match self { Self::Primary => 0x4005_45a4, Self::Secondary => 0x400e_4564 } }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Context { core: Core, pc: u32, lr: u32, sp: u32, cpsr: u32, r0: u32, arguments: [u32; 4], registers: [u32; 16], instructions: u64, current: Read }
impl Context {
    fn mode(&self) -> u32 { self.cpsr & 0x1f }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct TrampolineLoad { context: Context, target: u32, core_instructions: u64 }

#[derive(Serialize)]
struct DemandService { context: Context, context_word: Read, root: WordValue }

#[derive(Serialize)]
struct DemandPaging {
    load: TrampolineLoad, abort: Context, core_instructions: u64,
    spsr: u32, ifar: u32, ifsr: u32, service: Option<DemandService>,
}

impl DemandPaging {
    fn transfer(&self) -> Result<Context, String> {
        let before = self.load.context;
        let after = self.abort;
        require(before.core == after.core && before.current == after.current, "prefetch abort changed trampoline task")?;
        require(matches!(before.pc & !ALIAS, 0x43758 | 0xd3758) && after.pc == 0xc && after.mode() == 0x17,
            "prefetch abort is not a trampoline transfer")?;
        require(self.core_instructions == self.load.core_instructions + 1, "prefetch abort did not immediately follow trampoline load")?;
        require(self.spsr == before.cpsr && self.spsr & 0xff == 0x53, "prefetch abort has the wrong trampoline status")?;
        require(self.ifsr & 0x140f == 13, "trampoline prefetch abort is not a permission fault")?;
        require(self.ifar == self.load.target && after.lr.checked_sub(4) == Some(self.load.target),
            "prefetch abort is not at the trampoline target")?;
        require(before.registers[..13] == after.registers[..13], "prefetch abort changed trampoline registers")?;
        let mut transfer = before;
        transfer.pc = self.ifar;
        transfer.registers[15] = self.ifar;
        transfer.instructions = after.instructions;
        Ok(transfer)
    }
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
    kernel_context: u32,
    task_stack: Option<TaskStack>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct ReturnState {
    r0: u32,
    cpsr: u32,
    memory: Vec<WordValue>,
    writes: Vec<CapturedWrite>,
    bank: Vec<CapturedWrite>,
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
    writes: Vec<CapturedWrite>, bank: Vec<CapturedWrite>, value: Option<u32>, cpacr: u32, fpexc: u32, tpidrurw: u32,
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
enum BoundaryKind { Save, Resume, NoSave, Retry, Callback, CallbackReturn, SelectedRead, DiscardedRead, EnabledRead, GlobalLockRead, SelectComplete, FirstEntryExit, IrqTail, Return, Acquired, PrefetchAbort, TimerInsert, TimerRemove, TimerConsume }

impl ProbeKind {
    fn boundary(self) -> BoundaryKind {
        match self {
            Self::SelectedRead => BoundaryKind::SelectedRead,
            Self::DiscardedRead => BoundaryKind::DiscardedRead,
            Self::EnabledRead => BoundaryKind::EnabledRead,
            Self::GlobalLockRead => BoundaryKind::GlobalLockRead,
            Self::Retry => BoundaryKind::Retry,
            Self::Callback => BoundaryKind::Callback,
            Self::CallbackReturn => BoundaryKind::CallbackReturn,
            Self::SelectComplete => BoundaryKind::SelectComplete,
            Self::FirstEntryExit => BoundaryKind::FirstEntryExit,
            Self::IrqTail => BoundaryKind::IrqTail,
            Self::TimerInsert => BoundaryKind::TimerInsert,
            Self::TimerRemove => BoundaryKind::TimerRemove,
            Self::TimerConsume => BoundaryKind::TimerConsume,
        }
    }

    /// A timer probe's boundary value is the entry argument.
    fn timer(self) -> bool { matches!(self, Self::TimerInsert | Self::TimerRemove | Self::TimerConsume) }

    /// Insert and remove carry the entry in `r1`; the expiry callback carries it in `r0`.
    fn timer_value_register(self) -> Option<u8> {
        match self { Self::TimerInsert | Self::TimerRemove => Some(1), Self::TimerConsume => Some(0), _ => None }
    }
}

#[derive(Serialize)]
struct ClassCount { count: u64, first: u64, last: u64 }

#[derive(Serialize)]
struct ClassRow<'a> { #[serde(flatten)] class: &'a CallClass, #[serde(flatten)] count: &'a ClassCount }

/// Counts one completed call; `entered` bounds the class's first and last entry.
fn classify(classes: &mut BTreeMap<CallClass, ClassCount>, class: CallClass, entered: u64) -> Result<(), String> {
    if let Some(count) = classes.get_mut(&class) {
        count.count += 1;
        count.last = entered;
        return Ok(());
    }
    require(classes.len() < CLASS_LIMIT, "call class bound exceeded")?;
    classes.insert(class, ClassCount { count: 1, first: entered, last: entered });
    Ok(())
}

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
        Self { core: context.core, pc: pc & !ALIAS, sp: context.sp, task, mode: context.mode() }
    }
}

#[derive(Clone, Debug, Serialize)]
struct OpenCall { entry: EntryState, entered: u64, writes: Vec<CapturedWrite>, bank: Vec<CapturedWrite>, history: Vec<Boundary>, write_cursor: usize, bank_cursor: usize, suspended: bool, terminal_memory: Option<Vec<WordValue>> }

impl OpenCall {
    fn record(&mut self, mut boundary: Boundary) -> Result<(), String> {
        require(self.history.len() < WATCH_LIMIT, "call boundary bound exceeded")?;
        boundary.writes = self.writes[self.write_cursor..].to_vec();
        boundary.bank = self.bank[self.bank_cursor..].to_vec();
        self.write_cursor = self.writes.len();
        self.bank_cursor = self.bank.len();
        if matches!(self.entry.slot.semantic, Semantic::Service { .. } | Semantic::Family { .. }) {
            if boundary.kind == BoundaryKind::Save { self.suspended = true; }
            if boundary.kind == BoundaryKind::Resume { self.suspended = false; }
        }
        self.history.push(boundary);
        Ok(())
    }

    fn dispatch_complete(&self, key: &ReturnKey, context: &Context, semantic: Option<Semantic>) -> bool {
        if key.core != context.core { return false; }
        let Semantic::Dispatch { name } = self.entry.slot.semantic else { return false; };
        let delivery = semantic == Some(Semantic::Service { name: Service::DeliverTaskExceptionEntry });
        let frame = match name {
            DispatchPoint::FirstEntry => Some(0),
            DispatchPoint::Resume => Some(36),
            DispatchPoint::IrqTail => Some(32),
            _ => None,
        };
        if let Some(bytes) = frame {
            self.entry.task == context.current && self.entry.sp.0.checked_add(bytes) == Some(context.sp)
                && (key.pc & !ALIAS == context.pc & !ALIAS || (delivery && name == DispatchPoint::Resume))
        } else {
            matches!(semantic, Some(Semantic::Dispatch { .. })) || key.pc & !ALIAS == context.pc & !ALIAS
        }
    }

    /// A family call is tied to the task that entered it; another task's stop is not its boundary.
    fn owns_boundary(&self, key: &ReturnKey, context: &Context) -> bool {
        match self.entry.slot.semantic {
            Semantic::Family { .. } => self.entry.task == context.current,
            _ => key.task.is_none() || self.entry.task == context.current,
        }
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Written { address: Hex, size: u32, value: u32 }

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct TaskStack { start: Hex, end: Hex }

#[derive(Clone, Copy, Debug, Serialize)]
struct WriteOrigin { core: Core, pc: Hex, sp: Hex, cpsr: u32, task: Hex, kernel_context: u32, instructions: u64 }
impl PartialEq for WriteOrigin { fn eq(&self, other: &Self) -> bool { self.cmp(other).is_eq() } }
impl Eq for WriteOrigin {}
impl PartialOrd for WriteOrigin { fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) } }
impl Ord for WriteOrigin {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.core,self.pc,self.sp,self.cpsr,self.task,self.kernel_context)
            .cmp(&(other.core,other.pc,other.sp,other.cpsr,other.task,other.kernel_context))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct CapturedWrite { #[serde(flatten)] write: Written, origin: WriteOrigin }

/// # Safety
/// The CPU must outlive every callback that reads the returned pointer.
unsafe fn callback_cpu(cpu: &dyn Cpu) -> *const dyn Cpu {
    unsafe { std::mem::transmute::<*const (dyn Cpu + '_), *const dyn Cpu>(cpu as *const dyn Cpu) }
}

struct DataCapture {
    cpu: *const dyn Cpu, core: Core, sequence: Rc<Cell<u64>>,
    active: BTreeSet<u32>, rows: Vec<(u64,CapturedWrite)>, overflow: bool,
    failure: Option<String>, cached: Option<(InstructionEntryId,WriteOrigin)>,
}
impl DataCapture {
    fn origin(&mut self) -> Result<WriteOrigin, String> {
        let cpu = unsafe { &*self.cpu };
        let entry = cpu.instruction_entry().ok_or("scheduler writer has no instruction entry")?;
        if let Some((id,origin)) = self.cached {
            if id == entry.id { return Ok(origin); }
        }
        let register = |register| -> Result<u32,String> {
            let mut bytes = [0;4];
            require(cpu.read_register(Register::Arm32(register), &mut bytes)? == 4, "scheduler writer register width differs")?;
            Ok(u32::from_le_bytes(bytes))
        };
        let word = |address: u32| -> Result<u32,String> {
            let mut bytes = [0;4];
            cpu.read_memory(u64::from(address), &mut bytes)?;
            Ok(u32::from_le_bytes(bytes))
        };
        let origin = WriteOrigin { core: self.core, pc: Hex(u32::try_from(entry.address).map_err(|_| "scheduler writer PC exceeds 32 bits")?),
            sp: Hex(register(arm::Register::R(13))?), cpsr: entry.pstate,
            task: Hex(word(self.core.own_kernel_current_task())?), kernel_context: word(self.core.own_kernel_context())?,
            instructions: entry.progress };
        self.cached = Some((entry.id,origin));
        Ok(origin)
    }
}

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
        match capture.origin() {
            Ok(origin) => {
                let sequence = capture.sequence.get();
                capture.sequence.set(sequence+1);
                capture.rows.push((sequence,CapturedWrite { write: Written { address: Hex(address), size: size as u32,
                    value: value as u32 & mask }, origin }));
            }
            Err(error) => { capture.failure = Some(error); }
        }
    }
}

#[derive(Serialize)]
struct PcWatch { core: Core, address: Hex, count: u64, first: u64 }

#[derive(Serialize)]
struct ProbeHit { domain: Domain, core: Core, address: Hex, kind: ProbeKind, register: ProbeRegister,
    value: u32, cpsr: u32, sp: Hex, lr: Hex, instructions: u64 }

const LOCK_LIMIT: usize = 256;
#[derive(Clone, Copy, Debug, Serialize)]
struct LockEvent { task: u32, core: Core, call: u64, at: u64, address: Hex, value: u32 }

fn root_snapshot(board: &Board, core: Core, tcm: [u32; 2], domain: Domain) -> Result<Vec<WordValue>, String> {
    let (context, lock, reschedule, heads, bitmap, running, dispatch, selected) = match domain {
        Domain::A => (0x400545a4, 0x400545c0, 0x400546d8, 0x400546dc, 0x4005475c, 0x40054760, 0x40054764, 0x40054768),
        Domain::B => (0x400e4564, 0x400e4580, 0x400e4698, 0x400e469c, 0x400e471c, 0x400e4720, 0x400e4724, 0x400e4728),
    };
    let (object_lock, timer_base, timer_count) = match domain {
        Domain::A => (0x400545c4, 0x4005476c, 0x40054778),
        Domain::B => (0x400e4584, 0x400e472c, 0x400e4738),
    };
    let mut addresses: BTreeSet<_> = [context, lock, object_lock, timer_base, timer_count, reschedule, bitmap, running, dispatch, selected, 0x400c0804].into_iter().collect();
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
            require((1..=120).contains(&slot), "sleep result slot bound exceeded")?;
            let address = match domain { Domain::A => 0x4005477c, Domain::B => 0x400e473c } + 4*(slot-1);
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

/// Family record sizes and table extents of one loader (`replacement/rtos_layout`).
const TASK_RECORD_BYTES: u32 = 0x38;
const TASK_RECORD_COUNT: u32 = 120;
const FLAG_RECORD_COUNT: u32 = 120;
const MAILBOX_RECORD_COUNT: u32 = 1;
const TIMER_ENTRY_BYTES: u32 = 0x0c;
const MESSAGE_BYTES: u32 = 0x08;

/// A family wait queue or message chain longer than this is truncated.
const FAMILY_QUEUE_LIMIT: usize = 256;
const FAMILY_MESSAGE_LIMIT: usize = 256;

/// One fixed family table: base, record size and record count.
#[derive(Clone, Copy, Debug)]
struct Table { base: u32, size: u32, count: u32 }
impl Table {
    /// The record's address for a one-based slot inside the table.
    fn record(self, slot: u32) -> Option<u32> {
        (1..=self.count).contains(&slot).then(|| self.base + (slot - 1) * self.size)
    }
    /// The record's one-based slot when `address` starts one.
    fn slot(self, address: u32) -> Option<u32> {
        let offset = address.checked_sub(self.base)?;
        (offset % self.size == 0).then(|| offset / self.size + 1).filter(|slot| *slot <= self.count)
    }
}

/// The family tables of one loader (`replacement/rtos_layout::Domain::words`).
#[derive(Clone, Copy, Debug)]
struct FamilyTables {
    tasks: Table,
    flag_configs: Table,
    flag_controls: Table,
    flag_waits: Table,
    flag_timers: Table,
    mailbox_configs: Table,
    mailbox_controls: Table,
    mailbox_waits: Table,
    mailbox_timers: Table,
}

impl FamilyTables {
    fn of(domain: Domain) -> Self {
        let table = |base, size, count| Table { base, size, count };
        match domain {
            Domain::A => Self {
                tasks: table(0x4005_ff0c, TASK_RECORD_BYTES, TASK_RECORD_COUNT),
                flag_configs: table(0x4005_d178, 0x0c, FLAG_RECORD_COUNT),
                flag_controls: table(0x4006_24b4, 0x10, FLAG_RECORD_COUNT),
                flag_waits: table(0x4005_5fdc, 0x18, TASK_RECORD_COUNT),
                flag_timers: table(0x4005_5a3c, TIMER_ENTRY_BYTES, TASK_RECORD_COUNT),
                mailbox_configs: table(0x4005_d16c, 0x0c, MAILBOX_RECORD_COUNT),
                mailbox_controls: table(0x4006_2c34, 0x14, MAILBOX_RECORD_COUNT),
                mailbox_waits: table(0x4005_783c, 0x10, TASK_RECORD_COUNT),
                mailbox_timers: table(0x4005_7fbc, TIMER_ENTRY_BYTES, TASK_RECORD_COUNT),
            },
            Domain::B => Self {
                tasks: table(0x400e_fecc, TASK_RECORD_BYTES, TASK_RECORD_COUNT),
                flag_configs: table(0x400e_d138, 0x0c, FLAG_RECORD_COUNT),
                flag_controls: table(0x400f_2474, 0x10, FLAG_RECORD_COUNT),
                flag_waits: table(0x400e_5f9c, 0x18, TASK_RECORD_COUNT),
                flag_timers: table(0x400e_59fc, TIMER_ENTRY_BYTES, TASK_RECORD_COUNT),
                mailbox_configs: table(0x400e_d12c, 0x0c, MAILBOX_RECORD_COUNT),
                mailbox_controls: table(0x400f_2bf4, 0x14, MAILBOX_RECORD_COUNT),
                mailbox_waits: table(0x400e_77fc, 0x10, TASK_RECORD_COUNT),
                mailbox_timers: table(0x400e_7f7c, TIMER_ENTRY_BYTES, TASK_RECORD_COUNT),
            },
        }
    }

    fn configs(self, family: Family) -> Table {
        match family { Family::Flag => self.flag_configs, Family::Mailbox => self.mailbox_configs }
    }
    fn controls(self, family: Family) -> Table {
        match family { Family::Flag => self.flag_controls, Family::Mailbox => self.mailbox_controls }
    }
    fn waits(self, family: Family) -> Table {
        match family { Family::Flag => self.flag_waits, Family::Mailbox => self.mailbox_waits }
    }
    fn timers(self, family: Family) -> Table {
        match family { Family::Flag => self.flag_timers, Family::Mailbox => self.mailbox_timers }
    }
}

impl FamilyService {
    /// The object table the service names; the release and block helpers take raw records.
    fn family(self) -> Option<Family> {
        match self {
            Self::FamilyFlagCreate | Self::FamilyFlagDelete | Self::FamilyFlagSet | Self::FamilyFlagSetHandler
            | Self::FamilyFlagClear | Self::FamilyFlagWait | Self::FamilyFlagPoll | Self::FamilyFlagTimedWait => Some(Family::Flag),
            Self::FamilyMailboxCreate | Self::FamilyMailboxDelete | Self::FamilyMailboxSend
            | Self::FamilyMailboxReceive | Self::FamilyMailboxPoll | Self::FamilyMailboxTimedReceive => Some(Family::Mailbox),
            Self::FamilyRelease | Self::HelperRelease | Self::HelperBlock | Self::HelperBlockTimed | Self::HelperCheck => None,
        }
    }

    fn object_argument(self) -> Option<usize> { self.family().map(|_| 0) }

    /// The flag waits and polls take the output word fourth, the mailbox receives and polls second.
    fn output_argument(self) -> Option<usize> {
        match self {
            Self::FamilyFlagWait | Self::FamilyFlagPoll | Self::FamilyFlagTimedWait => Some(3),
            Self::FamilyMailboxReceive | Self::FamilyMailboxPoll | Self::FamilyMailboxTimedReceive => Some(1),
            _ => None,
        }
    }

    fn message_argument(self) -> Option<usize> { (self == Self::FamilyMailboxSend).then_some(1) }

    /// The flag timed wait passes its timeout as the fifth argument, on the caller's stack.
    fn stack_timeout(self) -> bool { self == Self::FamilyFlagTimedWait }

    /// A wait or receive arms the executing task's own timer entry.
    fn blocks(self) -> bool {
        matches!(self, Self::FamilyFlagWait | Self::FamilyFlagTimedWait | Self::FamilyMailboxReceive | Self::FamilyMailboxTimedReceive)
    }

    /// Create and delete index the entry domain's own table whatever the class half says.
    fn owns_its_table(self) -> bool {
        matches!(self, Self::FamilyFlagCreate | Self::FamilyFlagDelete | Self::FamilyMailboxCreate | Self::FamilyMailboxDelete)
    }

    /// Every mailbox service reads or writes the message chain.
    fn messages(self) -> bool { self.family() == Some(Family::Mailbox) }

    /// Class 0 is the entry domain, 1 is A and 2 is B.
    fn resolve(self, entry: Domain, id: u32) -> Option<(Domain, u32)> {
        let count = match self.family()? { Family::Flag => FLAG_RECORD_COUNT, Family::Mailbox => MAILBOX_RECORD_COUNT };
        let slot = id & 0xffff;
        if !(1..=count).contains(&slot) { return None; }
        if self.owns_its_table() { return Some((entry, slot)); }
        match (id >> 16) & 0xffff {
            0 => Some((entry, slot)),
            1 => Some((Domain::A, slot)),
            2 => Some((Domain::B, slot)),
            _ => None,
        }
    }
}

/// The family snapshot's word set, read through the executing core's view.
struct FamilyWalk<'a> {
    addresses: BTreeSet<u32>,
    read: &'a mut dyn FnMut(u32) -> Result<Read, String>,
}

impl FamilyWalk<'_> {
    fn pointer(&mut self, address: u32) -> Result<Option<u32>, String> {
        if address % 4 != 0 { return Ok(None); }
        match (self.read)(address)? { Read::Committed { value } => Ok(Some(value)), _ => Ok(None) }
    }

    fn capture_word(&mut self, address: u32) {
        if address != 0 && address % 4 == 0 { self.addresses.insert(address); }
    }

    fn record(&mut self, address: u32, bytes: u32) {
        if address == 0 || address % 4 != 0 { return; }
        let Some(end) = address.checked_add(bytes) else { return };
        for word in (address..end).step_by(4) { self.addresses.insert(word); }
    }
}

/// The task's domain and one-based slot when `task` starts a record in either table.
fn family_task(task: u32) -> Option<(Domain, u32)> {
    [Domain::A, Domain::B].into_iter().find_map(|domain| FamilyTables::of(domain).tasks.slot(task).map(|slot| (domain, slot)))
}

/// Walks an object's wait queue from the control block's self-linked head.
fn family_queue(walk: &mut FamilyWalk, family: Family, control: u32) -> Result<(), String> {
    let mut node = walk.pointer(control)?;
    for _ in 0..FAMILY_QUEUE_LIMIT {
        let Some(task) = node.filter(|&task| task != control) else { return Ok(()) };
        for offset in [0x0c, 0x14, 0x18, 0x1c, 0x20] { walk.capture_word(task + offset); }
        if let Some(saved) = walk.pointer(task + 0x18)? { if saved % 4 == 0 { walk.record(saved, 36); } }
        if let Some((waiter, slot)) = family_task(task) {
            let tables = FamilyTables::of(waiter);
            if let Some(wait) = tables.waits(family).record(slot) {
                walk.record(wait, tables.waits(family).size);
                if let Some(entry) = walk.pointer(wait)? {
                    if tables.timers(family).slot(entry).is_some() { walk.record(entry, tables.timers(family).size); }
                }
            }
        }
        node = walk.pointer(task)?;
    }
    Ok(())
}

/// Walks a mailbox's message chain from the control block's head.
fn family_messages(walk: &mut FamilyWalk, control: u32) -> Result<(), String> {
    let mut node = walk.pointer(control + 0x0c)?;
    for _ in 0..FAMILY_MESSAGE_LIMIT {
        let Some(message) = node.filter(|&message| message != 0) else { return Ok(()) };
        walk.record(message, MESSAGE_BYTES);
        node = walk.pointer(message)?;
    }
    Ok(())
}

/// The family words one call involves: the caller's output and stack argument, the object's
/// records, its wait queue and its message chain, bounded by the family limits.
fn family_words(read: &mut dyn FnMut(u32) -> Result<Read, String>, domain: Domain, name: FamilyService,
    arguments: [u32; 4], caller_sp: u32, running: Option<u32>) -> Result<Vec<u32>, String> {
    let entry_tables = FamilyTables::of(domain);
    let mut walk = FamilyWalk { addresses: BTreeSet::new(), read };
    if let Some(index) = name.output_argument() { walk.capture_word(arguments[index]); }
    if name.stack_timeout() { walk.capture_word(caller_sp); }
    if let Some(index) = name.message_argument() { walk.record(arguments[index], MESSAGE_BYTES); }
    if name.family().is_none() {
        helper_words(&mut walk, name, arguments, running)?;
        return Ok(walk.addresses.into_iter().collect());
    }
    let family = name.family().unwrap();
    if name.blocks() {
        if let Some(entry) = running.and_then(|task| entry_tables.tasks.slot(task)).and_then(|slot| entry_tables.timers(family).record(slot)) {
            walk.record(entry, entry_tables.timers(family).size);
        }
    }
    if let Some((object, slot)) = name.object_argument().and_then(|index| name.resolve(domain, arguments[index])) {
        let tables = FamilyTables::of(object);
        for domain in [domain, object] {
            walk.capture_word(if domain==Domain::A { 0x4005_45c0 } else { 0x400e_4580 });
            walk.capture_word(if domain==Domain::A { 0x4005_45c4 } else { 0x400e_4584 });
        }
        if let Some(config) = tables.configs(family).record(slot) { walk.record(config, tables.configs(family).size); }
        if let Some(control) = tables.controls(family).record(slot) {
            walk.record(control, tables.controls(family).size);
            family_queue(&mut walk, family, control)?;
            if name.messages() { family_messages(&mut walk, control)?; }
        }
    }
    Ok(walk.addresses.into_iter().collect())
}

fn helper_words(walk: &mut FamilyWalk, name: FamilyService, arguments: [u32; 4], running: Option<u32>) -> Result<(), String> {
    match name {
        FamilyService::FamilyRelease | FamilyService::HelperRelease => {
            walk.capture_word(arguments[1] + 0x14);
            if let Some(wait) = walk.pointer(arguments[1] + 0x14)? {
                walk.record(wait, 0x18);
                if let Some(timer) = walk.pointer(wait)? { walk.capture_word(timer + 8); }
            }
        }
        FamilyService::HelperBlock => {
            walk.capture_word(arguments[1]);
            walk.record(arguments[2], 0x18);
            if let Some(task) = running { walk.capture_word(task + 0x14); }
        }
        FamilyService::HelperBlockTimed => {
            walk.capture_word(arguments[1]);
            walk.record(arguments[2], 0x18);
            walk.capture_word(arguments[3] + 8);
            if let Some(task) = running { walk.capture_word(task + 0x14); }
        }
        FamilyService::HelperCheck => {
            walk.capture_word(arguments[0] + 0x0c);
            walk.capture_word(arguments[0] + 8);
            walk.capture_word(arguments[3]);
            if let Some(config) = walk.pointer(arguments[0] + 8)? { walk.capture_word(config); }
        }
        _ => {}
    }
    Ok(())
}

fn family_snapshot(board: &Board, core: Core, tcm: [u32; 2], domain: Domain, name: FamilyService,
    arguments: [u32; 4], caller_sp: u32) -> Result<Vec<WordValue>, String> {
    let running_cell = match domain { Domain::A => 0x4005_4760, Domain::B => 0x400e_4720 };
    let running = match board.word(core, tcm, running_cell)? {
        Read::Committed { value } if value != 0 => Some(value),
        _ => None,
    };
    let mut read = |address: u32| board.word(core, tcm, address);
    let mut words = root_snapshot(board, core, tcm, domain)?;
    for address in family_words(&mut read, domain, name, arguments, caller_sp, running)? {
        words.push(WordValue { address: Hex(address), value: board.word(core, tcm, address)? });
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
    trampoline_loads: [Option<TrampolineLoad>; 2],
    demand_paging: Vec<DemandPaging>,
    demand_services: BTreeMap<(Core, u32), u32>,
    write_batch: Vec<(u64,CapturedWrite)>,
    write_sequence: Rc<Cell<u64>>,
    lock_events: Vec<LockEvent>,
    last_instructions: u64,
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
                let context = Box::new(RefCell::new(DataCapture { cpu: unsafe { callback_cpu(machine.session.board.cpu(core)?) },
                    core: Core::from_index(core)?, sequence: self.write_sequence.clone(),
                    active: BTreeSet::new(), rows: Vec::new(), overflow: false, failure: None, cached: None }));
                let data = (&*context as *const RefCell<DataCapture>).cast_mut().cast();
                // The context is retired only after remove_hook.
                let hook = unsafe { machine.session.board.cpu(core)?.install_hook(Hook::Memory {
                    callback: data_write, data, read: false, write: true, range: HookRange::All,
                })? };
                self.data_hooks.push((core, hook, context));
            }
        }
        for (_, _, capture) in &self.data_hooks {
            let mut capture = capture.borrow_mut();
            capture.active = self.open.pending.values()
                .flat_map(|calls| calls.iter()
                    .flat_map(|call| call.entry.memory.iter().map(|word| word.address.0 & !ALIAS & !3))).collect();
            capture.active.extend([0xfffb30cc,0xfffb40cc,0xfffb30ec,0xfffb40ec].map(|address| address & !ALIAS));
        }
        self.last_instructions = machine.instructions();
        machine.session.board.transaction_watch_begin(vec![(0xfffb30cc,0xfffb30d0), (0xfffb40cc,0xfffb40d0),
            (0xfffb30ec, 0xfffb30f0), (0xfffb40ec, 0xfffb40f0)], WATCH_LIMIT)
    }

    fn drain_data(&mut self, machine: &Machine) -> Result<(), String> {
        self.write_batch.clear();
        for (_, _, capture) in &self.data_hooks {
            let mut capture = capture.borrow_mut();
            require(!capture.overflow, "scheduler write hook overflowed or saw an unsupported width")?;
            if let Some(error) = &capture.failure { return Err(error.clone()); }
            self.write_batch.extend(capture.rows.drain(..));
        }
        self.write_batch.sort_unstable_by_key(|(sequence,_)| *sequence);
        for (_,write) in &mut self.write_batch { write.origin.instructions += self.last_instructions; }
        for (_,write) in &self.write_batch {
            if [0xfffb30cc,0xfffb40cc,0xfffb30ec,0xfffb40ec].contains(&write.write.address.0) { continue; }
            for calls in self.open.pending.values_mut() {
                for call in calls.iter_mut() {
                    if call.entry.memory.iter().any(|word| word.address.0 & !ALIAS & !3 == write.write.address.0 & !ALIAS & !3) {
                        call.writes.push(*write);
                    }
                }
            }
        }
        let batch = machine.session.board.transaction_watch_end()?;
        require(batch.omitted == 0, "scheduler bank watch overflowed")?;
        let mut bank_writes = self.write_batch.iter().map(|(_,write)| write)
            .filter(|write| [0xfffb30cc,0xfffb40cc,0xfffb30ec,0xfffb40ec].contains(&write.write.address.0));
        for transaction in batch.transactions {
            if transaction.operation.as_ref() != "write" { continue; }
            require(transaction.accepted && transaction.size == 4, "scheduler bank write was refused or not a word")?;
            let core = Core::from_index(transaction.cpu)?;
            self.bank_counts[core.index()] += 1;
            let write = bank_writes.next().ok_or("scheduler bank transaction has no writer envelope")?;
            require(write.origin.core == core && u64::from(write.origin.pc.0) == transaction.pc
                && u64::from(write.write.address.0) == transaction.address && write.write.size == 4
                && u64::from(write.write.value) == transaction.value, "scheduler bank writer envelope differs from transaction")?;
            for calls in self.open.pending.values_mut() {
                for call in calls.iter_mut() { call.bank.push(*write); }
            }
        }
        require(bank_writes.next().is_none(), "scheduler bank writer has no transaction")?;
        self.last_instructions = machine.instructions();
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
        if !matches!(slot.id.semantic, Semantic::Family { .. }) {
            require(context.core.index() == match domain { Domain::A => 0, Domain::B => 1 }, "scheduler entry executed on the other core")?;
        }
        let mut entry = EntryState {
            slot: slot.id, core: context.core, cpsr: context.cpsr, r0: context.r0, lr: Hex(context.lr),
            arguments: context.arguments,
            task: context.current, sp: Hex(context.sp), registers: context.registers,
            cpacr: board.register(context.core, arm::Register::Cpacr)?,
            fpexc: board.register(context.core, arm::Register::Fpexc)?,
            tpidrurw: board.cp15(context.core, 13, 0, 2)?,
            origin: origin.as_ref().map(|(point,_)| *point),
            kernel_context: committed(board.word(context.core,tcm,context.core.own_kernel_context())?)?,
            task_stack: None,
            memory: match slot.id.semantic {
                Semantic::Helper { name } => snapshot(&board, context.core, tcm, name, context.arguments)?,
                Semantic::Service { name } => service_snapshot(&board, context.core, tcm, domain, name, context.arguments)?,
                Semantic::Family { name } => family_snapshot(&board, context.core, tcm, domain, name, context.arguments, context.sp)?,
                _ => root_snapshot(&board, context.core, tcm, domain)?,
            },
        };
        if entry.kernel_context == 0 && matches!(context.mode(),0x10 | 0x13 | 0x1f) {
            if let Read::Committed { value: task } = context.current {
                if task != 0 {
                    let config = committed(board.word(context.core,tcm,task+8)?)?;
                    let start = committed(board.word(context.core,tcm,config+0x14)?)?;
                    let bytes = committed(board.word(context.core,tcm,config+0x10)?)?;
                    let end = start.checked_add(bytes).ok_or("scheduler task stack wraps")?;
                    entry.task_stack = Some(TaskStack { start: Hex(start), end: Hex(end) });
                }
            }
        }
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
            history: origin.map(|(_,history)| history).unwrap_or_default(), write_cursor: 0, bank_cursor: 0, suspended: false, terminal_memory: None };
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
        if continuation != 0 {
            for address in spellings(continuation & !ALIAS) {
                if self.return_sites.contains(&address) { continue; }
                if !machine.traces.sites.contains_key(&address) {
                    require(machine.traces.sites.len() < SITE_LIMIT, "continuation site bound exceeded")?;
                    machine.traces.add_sites(&machine.session, [(address, CONTINUATION_LABEL.to_owned())].into_iter().collect())?;
                }
                self.return_sites.insert(address);
            }
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
            if matches!(call.entry.slot.semantic, Semantic::Service { .. } | Semantic::Family { .. }) && completion == Completion::Return {
                call.record(Boundary { kind: BoundaryKind::Return, address: Hex(context.pc), instructions: context.instructions,
                    core: context.core, task: context.current, sp: Hex(context.sp), cpsr: context.cpsr,
                    registers: context.registers, memory: memory.clone(), writes: Vec::new(),
                    bank: Vec::new(), value: Some(context.r0),
                    cpacr: board.register(context.core, arm::Register::Cpacr)?,
                    fpexc: board.register(context.core, arm::Register::Fpexc)?,
                    tpidrurw: board.cp15(context.core, 13, 0, 2)? })?;
            }
            if matches!(call.entry.slot.semantic, Semantic::Helper { name: Helper::Release } | Semantic::Helper { name: Helper::Acquire }) {
                for write in &call.writes {
                    if write.origin.core == call.entry.core && call.entry.task == (Read::Committed { value: write.origin.task.0 }) {
                        require(self.lock_events.len() < LOCK_LIMIT, "lock event bound exceeded")?;
                        self.lock_events.push(LockEvent { task: write.origin.task.0, core: write.origin.core,
                            call: call.entered, at: write.origin.instructions, address: write.write.address, value: write.write.value });
                    }
                }
            }
            let class = CallClass { entry: call.entry, result: ReturnState { r0: context.r0, cpsr: context.cpsr,
                memory, writes: call.writes, bank: call.bank, completion, task: context.current,
                sp: Hex(context.sp), registers: context.registers, history: call.history,
                cpacr: board.register(context.core, arm::Register::Cpacr)?,
                fpexc: board.register(context.core, arm::Register::Fpexc)?,
                tpidrurw: board.cp15(context.core, 13, 0, 2)? } };
            self.returns += 1;
            classify(&mut self.classes, class, call.entered)?;
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

    /// A timer expiry belongs to the parked owner, not to the callback's current call.
    fn consume_timer(&mut self, machine: &Machine, context: &Context, tcm: [u32; 2], entry: u32, task: u32) -> Result<(), String> {
        let board = Board { machine, maps: &self.maps };
        let keys: Vec<ReturnKey> = self.open.pending.iter()
            .filter(|(_, calls)| calls.iter().any(|call| matches!(call.entry.slot.semantic, Semantic::Family { .. }) && call.entry.task == Read::Committed { value: task }))
            .map(|(key, _)| *key).collect();
        for key in keys {
            if let Some(calls) = self.open.pending.get_mut(&key) {
                for call in calls.iter_mut().filter(|call| matches!(call.entry.slot.semantic, Semantic::Family { .. }) && call.entry.task == Read::Committed { value: task }) {
                    let memory = call.entry.memory.iter().map(|word| Ok(WordValue { address: word.address, value: board.word(context.core, tcm, word.address.0)? })).collect::<Result<Vec<_>, String>>()?;
                    call.record(Boundary { kind: BoundaryKind::TimerConsume, address: Hex(context.pc), instructions: context.instructions,
                        core: context.core, task: Read::Committed { value: task }, sp: Hex(context.sp), cpsr: context.cpsr,
                        registers: context.registers, memory, writes: Vec::new(), bank: Vec::new(), value: Some(entry),
                        cpacr: board.register(context.core, arm::Register::Cpacr)?,
                        fpexc: board.register(context.core, arm::Register::Fpexc)?,
                        tpidrurw: board.cp15(context.core, 13, 0, 2)? })?;
                }
            }
        }
        Ok(())
    }

    fn boundary(&mut self, machine: &Machine, context: &Context, tcm: [u32; 2], kind: BoundaryKind, value: Option<u32>) -> Result<(), String> {
        let board = Board { machine, maps: &self.maps };
        for (key, calls) in &mut self.open.pending {
            if key.core != context.core { continue; }
            for call in calls {
                if !call.owns_boundary(key, context) { continue; }
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
        let closed = self.open.take_where(|key, call| call.dispatch_complete(key, context, semantic));
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
            let discarded = self.open.take_where(|key, call| key.core == context.core && call.entry.task == context.current && matches!(call.entry.slot.semantic, Semantic::Service { .. } | Semantic::Family { .. }));
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
            let low = context.pc & !ALIAS;
            if low == 0xc {
                if let Some(load) = self.trampoline_loads[core.index()].take() {
                    let board = Board { machine, maps: &self.maps };
                    let demand = DemandPaging { load, abort: context,
                        core_instructions: machine.session.board.instruction_counts()[core.index()],
                        spsr: board.register(core, arm::Register::Spsr)?,
                        ifar: board.cp15(core, 6, 0, 2)?, ifsr: board.cp15(core, 5, 0, 1)?, service: None };
                    let transfer = demand.transfer()?;
                    let closed = self.open.take_where(|key, call| call.entry.slot.semantic == (Semantic::Dispatch { name: DispatchPoint::FirstEntry })
                        && call.dispatch_complete(key, &transfer, None));
                    require(closed.len() == 1, "prefetch abort has no unique trampoline call")?;
                    let boundary = Boundary { kind: BoundaryKind::PrefetchAbort, address: Hex(context.pc),
                        instructions: context.instructions, core, task: context.current, sp: Hex(context.sp),
                        cpsr: context.cpsr, registers: context.registers, memory: Vec::new(), writes: Vec::new(),
                        bank: Vec::new(), value: Some(transfer.pc),
                        cpacr: board.register(core, arm::Register::Cpacr)?,
                        fpexc: board.register(core, arm::Register::Fpexc)?, tpidrurw: board.cp15(core, 13, 0, 2)? };
                    self.demand_paging.push(demand);
                    for mut call in closed {
                        call.record(boundary.clone())?;
                        self.finish(machine, &transfer, tcm, vec![call], Completion::Dispatched)?;
                    }
                    let root = match core { Core::Primary => 0x4004dae0, Core::Secondary => 0x400ddaa0 };
                    for offset in (0..=0x124).step_by(4) {
                        let address = committed(Board { machine, maps: &self.maps }.word(core,tcm,root+offset)?)? & !1;
                        for view in spellings(address & !ALIAS) {
                            self.demand_services.insert((core,view),root+offset);
                            if !machine.traces.sites.contains_key(&view) {
                                require(machine.traces.sites.len() < SITE_LIMIT, "demand service site bound exceeded")?;
                                machine.traces.add_sites(&machine.session, [(view,"scheduler-demand-service".into())].into_iter().collect())?;
                            }
                        }
                    }
                }
            } else if matches!(low, 0x43758 | 0xd3758) {
                let target = committed(Board { machine, maps: &self.maps }.word(core,tcm,context.registers[2]+8)?)?;
                self.trampoline_loads[core.index()] = Some(TrampolineLoad { context, target,
                    core_instructions: machine.session.board.instruction_counts()[core.index()] });
            } else if self.trampoline_loads[core.index()].is_some_and(|load| {
                context.pc & !ALIAS == load.target & !ALIAS && context.current == load.context.current
            }) {
                self.trampoline_loads[core.index()] = None;
            }
            if let Some(&root) = self.demand_services.get(&(core,context.pc)) {
                let board = Board { machine, maps: &self.maps };
                let context_word = board.word(core,tcm,match core { Core::Primary => 0x400545a4, Core::Secondary => 0x400e4564 })?;
                if context_word == (Read::Committed { value: 0 }) && context.cpsr & 0x9f == 0x13 {
                    for demand in self.demand_paging.iter_mut().filter(|demand| demand.service.is_none()
                        && demand.load.context.core == core && demand.load.context.current == context.current) {
                        demand.service = Some(DemandService { context, context_word,
                            root: WordValue { address: Hex(root), value: board.word(core,tcm,root)? } });
                    }
                }
            }
            if PC_WATCHES.contains(&(context.pc & !ALIAS)) {
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
                    let kind = probe.kind.boundary();
                    let callback_task = board.register(core, arm::Register::R(0))?;
                    let boundary_value = if probe.kind == ProbeKind::TimerConsume {
                        let wait = committed(board.word(core, tcm, callback_task + 0x14)?)?;
                        let entry = committed(board.word(core, tcm, wait)?)?;
                        require(committed(board.word(core, tcm, entry + 8)?)? == callback_task, "timer callback task differs from entry owner")?;
                        entry
                    } else if let Some(register) = probe.kind.timer_value_register() { board.register(core, arm::Register::R(register))? } else { value };
                    if kind == BoundaryKind::TimerConsume {
                        self.consume_timer(machine, &context, tcm, boundary_value, callback_task)?;
                    } else {
                        self.boundary(machine, &context, tcm, kind, Some(boundary_value))?;
                    }
                    if kind == BoundaryKind::DiscardedRead && value == 1 && self.validated.plan.probes[index].register.0 == 3 {
                        let discarded = self.open.take_where(|key,call| key.core==core && call.entry.task==context.current && matches!(call.entry.slot.semantic,Semantic::Service { .. } | Semantic::Family { .. }));
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
    lock_events: &'a [LockEvent],
    pending: Vec<&'a OpenCall>,
    error: Option<String>,
    machine_failure: Option<String>,
    bank_counts: [u64; 2],
    pc_watches: Vec<&'a PcWatch>,
    probes: &'a [ProbeHit],
    demand_paging: &'a [DemandPaging],
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
        trampoline_loads: [None; 2], demand_paging: Vec::new(), demand_services: BTreeMap::new(),
        write_batch: Vec::with_capacity(WATCH_LIMIT*2), write_sequence: Rc::new(Cell::new(0)),
        lock_events: Vec::new(), last_instructions: 0,
    };
    machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
    machine.traces.stop_labels = [ENTRY_LABEL, CONTINUATION_LABEL, FILL_LABEL, ARM_LABEL, "scheduler-pc-watch", "scheduler-probe"]
        .into_iter().map(str::to_owned).collect();
    let observed = (|| -> Result<(), String> {
        let mut labels = validated.sites.labels();
        for address in PC_WATCHES.into_iter().chain(DISPATCH_LOADS.into_iter().filter(|&address| retained_dispatch_point(&validated.plan,address))).flat_map(spellings) {
            labels.insert(address, "scheduler-pc-watch".into());
            for point in [0xc,0x43758,0xd3758].into_iter().flat_map(spellings) {
                labels.insert(point, "scheduler-pc-watch".into());
            }
        }
        machine.traces.arm_mutable(&machine.session, labels)?;
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
    let mut error = observed.err().or(ended.err()).or(read_error).or(drained.err()).or(unhooked.err());
    let maps = observer.maps.clone();
    {
        let board = Board { machine: &machine, maps: &maps };
        for call in observer.open.pending.values_mut().flatten() {
            let result = (|| -> Result<Vec<WordValue>, String> {
                let tcm = board.tcm(call.entry.core)?;
                let mut addresses: BTreeSet<u32> = call.entry.memory.iter().map(|word| word.address.0).collect();
                if let Semantic::Family { name } = call.entry.slot.semantic {
                    if name.blocks() {
                        if let Some(family) = name.family() {
                            if let Read::Committed { value: task } = &call.entry.task {
                                if *task != 0 {
                                    for offset in [0x14, 0x18, 0x1c, 0x20, 0x24, 8] { addresses.insert(*task + offset); }
                                    if let Read::Committed { value: definition } = board.word(call.entry.core, tcm, *task + 8)? {
                                        if definition != 0 { addresses.insert(definition + 0x10); addresses.insert(definition + 0x14); }
                                    }
                                    if let Some((waiter, slot)) = family_task(*task) {
                                        let tables = FamilyTables::of(waiter);
                                        if let Some(wait) = tables.waits(family).record(slot) {
                                            for offset in (0..tables.waits(family).size).step_by(4) { addresses.insert(wait + offset); }
                                            if let Read::Committed { value: entry } = board.word(call.entry.core, tcm, wait)? {
                                                if entry != 0 && tables.timers(family).slot(entry).is_some() {
                                                    for offset in (0..tables.timers(family).size).step_by(4) { addresses.insert(entry + offset); }
                                                }
                                            }
                                        }
                                    }
                                    if let Read::Committed { value: saved } = board.word(call.entry.core, tcm, *task + 0x18)? {
                                        if saved % 4 == 0 { for offset in (0..36).step_by(4) { addresses.insert(saved + offset); } }
                                    }
                                }
                            }
                        }
                    }
                }
                addresses.into_iter().map(|address| Ok(WordValue { address: Hex(address), value: board.word(call.entry.core, tcm, address)? })).collect()
            })();
            match result {
                Ok(memory) => call.terminal_memory = Some(memory),
                Err(failure) => if error.is_none() { error = Some(failure); },
            }
        }
    }
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
        lock_events: &observer.lock_events,
        pending: observer.open.pending.values().flatten().collect(),
        error: error.clone(),
        machine_failure: machine.failure.clone(),
        bank_counts: observer.bank_counts,
        pc_watches: observer.pc_watches.values().collect(),
        probes: &observer.probe_hits,
        demand_paging: &observer.demand_paging,
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
            sp: Hex(0xa000), registers: [0;16], cpacr: 0, fpexc: 0, tpidrurw: 7, origin: None, kernel_context: 0, task_stack: None },
            entered: 1, writes: vec![], bank: vec![], history: vec![], write_cursor: 0, bank_cursor: 0, suspended: false, terminal_memory: None }
    }

    fn captured(write: Written) -> CapturedWrite {
        CapturedWrite { write, origin: WriteOrigin { core: Core::Primary, pc: Hex(0x45cd4), sp: Hex(0x9ff0),
            cpsr: 0x53, task: Hex(0x9000), kernel_context: 0, instructions: 1 } }
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

    fn family_call(name: FamilyService) -> OpenCall {
        let mut call = call();
        call.entry.slot.semantic = Semantic::Family { name };
        call
    }

    fn family_read(words: &BTreeMap<u32, u32>) -> impl FnMut(u32) -> Result<Read, String> + '_ {
        move |address| Ok(words.get(&address).map_or(Read::Unresolved, |&value| Read::Committed { value }))
    }

    fn context(task: u32) -> Context {
        Context { core: Core::Primary, pc: 0, lr: 0, sp: 0, cpsr: 0x13, r0: 0, arguments: [0; 4],
            registers: [0; 16], instructions: 0, current: Read::Committed { value: task } }
    }

    #[test]
    fn family_semantics_serialize_with_the_catalog_names() {
        let services = [FamilyService::FamilyRelease, FamilyService::HelperRelease, FamilyService::HelperBlock,
            FamilyService::HelperBlockTimed, FamilyService::HelperCheck, FamilyService::FamilyFlagCreate,
            FamilyService::FamilyFlagDelete, FamilyService::FamilyFlagSet, FamilyService::FamilyFlagSetHandler,
            FamilyService::FamilyFlagClear, FamilyService::FamilyFlagWait, FamilyService::FamilyFlagPoll,
            FamilyService::FamilyFlagTimedWait, FamilyService::FamilyMailboxCreate, FamilyService::FamilyMailboxDelete,
            FamilyService::FamilyMailboxSend, FamilyService::FamilyMailboxReceive, FamilyService::FamilyMailboxPoll,
            FamilyService::FamilyMailboxTimedReceive];
        let names = ["family-release", "helper-release", "helper-block", "helper-block-timed", "helper-check",
            "family-flag-create", "family-flag-delete", "family-flag-set", "family-flag-set-handler", "family-flag-clear",
            "family-flag-wait", "family-flag-poll", "family-flag-timed-wait", "family-mailbox-create",
            "family-mailbox-delete", "family-mailbox-send", "family-mailbox-receive", "family-mailbox-poll",
            "family-mailbox-timed-receive"];
        for (service, name) in services.into_iter().zip(names) {
            let wire = serde_json::json!({ "kind": "family", "name": name });
            assert_eq!(serde_json::to_value(Semantic::Family { name: service }).unwrap(), wire);
            assert_eq!(serde_json::from_value::<Semantic>(wire).unwrap(), Semantic::Family { name: service });
        }
        let entry: PlanEntry = serde_json::from_value(serde_json::json!({ "domain": "a", "entry": "0x44568",
            "semantic": { "kind": "family", "name": "family-release" } })).unwrap();
        assert_eq!(entry.semantic, Semantic::Family { name: FamilyService::FamilyRelease });
    }

    #[test]
    fn helper_snapshots_capture_argument_records() {
        let task = 0x9000u32;
        let wait = 0x9100u32;
        let timer = 0x9200u32;
        let control = 0x9300u32;
        let config = 0x9400u32;
        let mut words = BTreeMap::new();
        words.insert(task + 0x14, wait);
        words.insert(wait, timer);
        words.insert(control + 0x0c, 0x11);
        words.insert(control + 8, config);
        words.insert(config, 0x4);
        let mut read = family_read(&words);
        let release = family_words(&mut read, Domain::A, FamilyService::FamilyRelease, [0, task, 0, 0], 0, None).unwrap();
        assert!(release.contains(&(task + 0x14)) && release.contains(&wait) && release.contains(&(wait + 4)) && release.contains(&(timer + 8)));
        let mut read = family_read(&words);
        let check = family_words(&mut read, Domain::A, FamilyService::HelperCheck, [control, 0, 0, 0x9500], 0, None).unwrap();
        assert!(check.contains(&(control + 0x0c)) && check.contains(&(control + 8)) && check.contains(&config) && check.contains(&0x9500));
    }

    #[test]
    fn family_snapshot_is_bounded_and_keeps_the_involved_words() {
        let tables = FamilyTables::of(Domain::A);
        let config = tables.flag_configs.record(3).unwrap();
        let control = tables.flag_controls.record(3).unwrap();
        let tasks: Vec<u32> = [1, 2].into_iter().map(|slot| tables.tasks.record(slot).unwrap()).collect();
        let waits: Vec<u32> = [1, 2].into_iter().map(|slot| tables.flag_waits.record(slot).unwrap()).collect();
        let synthetic: Vec<u32> = (0..300).map(|index| 0x5000_0000 + index * TASK_RECORD_BYTES).collect();
        let mut nodes = tasks.clone();
        nodes.extend(&synthetic);
        let mut words = BTreeMap::new();
        words.insert(control, nodes[0]);
        words.insert(control + 4, *nodes.last().unwrap());
        words.insert(control + 8, config);
        words.insert(control + 0x0c, 0x11);
        for (index, &node) in nodes.iter().enumerate() {
            words.insert(node, nodes.get(index + 1).copied().unwrap_or(control));
        }
        for (index, (&task, &wait)) in tasks.iter().zip(&waits).enumerate() {
            words.insert(task + 0x0c, 0x32);
            words.insert(task + 0x14, wait);
            words.insert(wait, tables.flag_timers.record(index as u32 + 1).unwrap());
            words.insert(wait + 0x0c, 0x10);
            words.insert(wait + 0x10, 1);
            words.insert(wait + 0x14, 0x11);
        }
        let out = 0x6000_0000;
        let caller_sp = 0x6000_1000;
        let mut read = family_read(&words);
        let addresses = family_words(&mut read, Domain::A, FamilyService::FamilyFlagTimedWait,
            [0x0000_0003, 0x10, 1, out], caller_sp, Some(tables.tasks.record(5).unwrap())).unwrap();
        assert!(addresses.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(addresses.contains(&config) && addresses.contains(&control) && addresses.contains(&(control + 0x0c)));
        assert!(addresses.contains(&out) && addresses.contains(&caller_sp));
        assert!(addresses.contains(&waits[0]) && addresses.contains(&(waits[0] + 0x14)));
        assert!(addresses.contains(&tables.flag_timers.record(1).unwrap()));
        assert!(addresses.contains(&tables.flag_timers.record(5).unwrap()));
        for offset in [0x0c, 0x14, 0x18, 0x1c, 0x20] { assert!(addresses.contains(&(tasks[0] + offset))); }
        let last = FAMILY_QUEUE_LIMIT - tasks.len() - 1;
        for offset in [0x0c, 0x14, 0x18, 0x1c, 0x20] {
            assert!(addresses.contains(&(synthetic[last] + offset)));
            assert!(!addresses.contains(&(synthetic[last + 1] + offset)));
        }
        assert!(!addresses.contains(&tables.flag_controls.record(4).unwrap()));
        assert!(addresses.len() <= FAMILY_QUEUE_LIMIT * 5 + 64);
    }

    #[test]
    fn family_snapshot_follows_the_message_chain_in_the_resolved_domain() {
        let tables = FamilyTables::of(Domain::B);
        let config = tables.mailbox_configs.record(1).unwrap();
        let control = tables.mailbox_controls.record(1).unwrap();
        let messages = [0x7000_0000u32, 0x7000_0010, 0x7000_0020];
        let mut words = BTreeMap::new();
        for (index, &message) in messages.iter().enumerate() {
            words.insert(message, messages.get(index + 1).copied().unwrap_or(0));
            words.insert(message + 4, index as u32);
        }
        words.insert(control + 0x0c, messages[0]);
        words.insert(control + 0x10, messages[2]);
        let caller = 0x7000_0100;
        words.insert(caller, 0);
        words.insert(caller + 4, 7);
        let mut read = family_read(&words);
        let addresses = family_words(&mut read, Domain::A, FamilyService::FamilyMailboxSend,
            [0x0002_0001, caller, 0, 0], 0, None).unwrap();
        assert!(addresses.contains(&config) && addresses.contains(&control) && addresses.contains(&(control + 0x10)));
        for message in messages { assert!(addresses.contains(&message) && addresses.contains(&(message + 4))); }
        assert!(addresses.contains(&caller) && addresses.contains(&(caller + 4)));
        assert!(!addresses.contains(&FamilyTables::of(Domain::A).mailbox_controls.record(1).unwrap()));
    }

    #[test]
    fn timer_probes_record_the_entry_as_a_family_boundary() {
        assert_eq!(serde_json::to_value(ProbeKind::TimerInsert).unwrap(), "timer-insert");
        assert_eq!(serde_json::to_value(ProbeKind::TimerRemove).unwrap(), "timer-remove");
        assert_eq!(serde_json::to_value(ProbeKind::TimerConsume).unwrap(), "timer-consume");
        assert_eq!(serde_json::to_value(BoundaryKind::TimerInsert).unwrap(), "timer-insert");
        assert_eq!(serde_json::to_value(BoundaryKind::TimerRemove).unwrap(), "timer-remove");
        assert_eq!(serde_json::to_value(BoundaryKind::TimerConsume).unwrap(), "timer-consume");
        assert_eq!(ProbeKind::TimerInsert.boundary(), BoundaryKind::TimerInsert);
        assert_eq!(ProbeKind::TimerRemove.boundary(), BoundaryKind::TimerRemove);
        assert_eq!(ProbeKind::TimerConsume.boundary(), BoundaryKind::TimerConsume);
        assert_eq!(ProbeKind::Retry.boundary(), BoundaryKind::Retry);
        assert!(ProbeKind::TimerInsert.timer() && ProbeKind::TimerRemove.timer() && ProbeKind::TimerConsume.timer() && !ProbeKind::Callback.timer());
        assert_eq!(ProbeKind::TimerConsume.timer_value_register(), Some(0));
        assert_eq!(ProbeKind::TimerRemove.timer_value_register(), Some(1));
        let entry = FamilyTables::of(Domain::A).flag_timers.record(1).unwrap();
        let mut call = family_call(FamilyService::FamilyFlagTimedWait);
        for kind in [BoundaryKind::TimerInsert, BoundaryKind::TimerRemove, BoundaryKind::TimerInsert] {
            let mut event = boundary(kind);
            event.registers[1] = entry;
            event.value = Some(entry);
            call.record(event).unwrap();
        }
        assert!(!call.suspended);
        let wire = serde_json::to_value(&call).unwrap();
        let history = wire["history"].as_array().unwrap();
        assert_eq!(history.iter().map(|event| event["kind"].as_str().unwrap()).collect::<Vec<_>>(),
            ["timer-insert", "timer-remove", "timer-insert"]);
        assert_eq!(history[0]["value"], entry);
        assert_eq!(history[0]["registers"][1], entry);
    }

    #[test]
    fn a_family_call_stays_open_until_its_own_task_returns() {
        let key = ReturnKey { core: Core::Primary, pc: 0x8000, sp: 0xa000, task: Some(0x9000), mode: 0x13 };
        let mut calls = OpenCalls::default();
        calls.open(key, family_call(FamilyService::FamilyFlagWait)).unwrap();
        assert!(calls.close(&ReturnKey { task: Some(0x9100), ..key }).is_empty());
        assert_eq!((calls.count, calls.pending.values().flatten().count()), (1, 1));
        let pending = calls.pending.get(&key).unwrap().first().unwrap();
        assert!(pending.owns_boundary(&key, &context(0x9000)));
        assert!(!pending.owns_boundary(&key, &context(0x9100)));
        let mut family = family_call(FamilyService::FamilyFlagWait);
        family.record(boundary(BoundaryKind::Save)).unwrap();
        assert!(family.suspended);
        let service = call();
        assert!(service.owns_boundary(&ReturnKey { task: None, ..key }, &context(0x9100)));
        assert!(!service.owns_boundary(&key, &context(0x9100)));
        assert_eq!(calls.close(&key).len(), 1);
        assert_eq!(calls.count, 0);
    }

    #[test]
    fn repeated_family_invocations_keep_one_call_class() {
        let entry = family_call(FamilyService::FamilyFlagSet).entry;
        let class = CallClass { entry: entry.clone(), result: ReturnState { r0: 0, cpsr: 0x53,
            memory: entry.memory.clone(), writes: Vec::new(), bank: Vec::new(), completion: Completion::Return,
            task: entry.task, sp: entry.sp, registers: [0; 16], cpacr: 0, fpexc: 0, tpidrurw: 0, history: Vec::new() } };
        let mut classes = BTreeMap::new();
        classify(&mut classes, class.clone(), 10).unwrap();
        classify(&mut classes, class.clone(), 20).unwrap();
        assert_eq!(classes.len(), 1);
        let count = classes.values().next().unwrap();
        assert_eq!((count.count, count.first, count.last), (2, 10, 20));
        let mut other = class;
        other.entry.memory.push(WordValue { address: Hex(0x4006_24b4), value: Read::Committed { value: 1 } });
        classify(&mut classes, other, 30).unwrap();
        assert_eq!(classes.len(), 2);
    }

    #[test]
    fn terminal_dispatch_stages_survive_preemption_until_their_own_continuation() {
        for (point, bytes) in [(DispatchPoint::FirstEntry,0),(DispatchPoint::Resume,36),(DispatchPoint::IrqTail,32)] {
            let mut call = call();
            call.entry.slot.semantic = Semantic::Dispatch { name: point };
            let key = ReturnKey { core: Core::Primary, pc: 0x8000, sp: 0, task: None, mode: 0 };
            let context = Context { core: Core::Primary, pc: 0x43640, lr: 0, sp: 0xa000+bytes,
                cpsr: 0x53, r0: 0, arguments: [0;4], registers: [0;16], instructions: 10,
                current: Read::Committed { value: 0x9000 } };
            assert!(!call.dispatch_complete(&key, &context, Some(Semantic::Dispatch { name: DispatchPoint::Save })));
            let target = Context { pc: 0x8000, ..context };
            assert!(!call.dispatch_complete(&key, &Context { current: Read::Committed { value: 0x9100 }, ..target }, None));
            assert!(!call.dispatch_complete(&key, &Context { sp: target.sp-4, ..target }, None));
            assert!(call.dispatch_complete(&key, &target, None));
            let alias_target = Context { pc: 0x40008000, ..target };
            assert!(call.dispatch_complete(&key, &alias_target, None));
        }
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
            call.writes.push(captured(Written { address: Hex(0x9018), size: 4, value: 0x9fd0 }));
            call.bank.push(captured(Written { address: Hex(0xfffb30ec), size: 4, value: 0x1000000 }));
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
            call.writes.push(captured(Written { address: Hex(address),size:4,value }));
        }
        call.bank.push(captured(Written { address: Hex(0xfffb30ec),size:4,value:0x1000000 }));
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
        call.writes.push(captured(Written { address: Hex(0x900c), size: 1, value: 10 }));
        call.record(boundary(BoundaryKind::Save)).unwrap();
        assert!(call.suspended);
        call.record(boundary(BoundaryKind::Resume)).unwrap();
        assert!(!call.suspended);
        call.record(boundary(BoundaryKind::Retry)).unwrap();
        call.bank.push(captured(Written { address: Hex(0xfffb40cc), size: 4, value: 0x1000000 }));
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

    fn demand_paging() -> DemandPaging {
        let mut registers = std::array::from_fn(|index| index as u32);
        registers[13] = 0xa42200;
        registers[14] = 0x453f0;
        registers[15] = 0x43758;
        let context = Context { core: Core::Primary, pc: 0x43758, lr: registers[14], sp: registers[13],
            cpsr: 0x53, r0: registers[0], arguments: [0,1,2,3], registers, instructions: 10,
            current: Read::Committed { value: 0x4005ffb4 } };
        let target = 0x1215480;
        let mut abort = Context { pc: 0xc, lr: target+4, sp: 0x70000c00, cpsr: 0x1d7, instructions: 11, ..context };
        abort.registers[13] = abort.sp;
        abort.registers[14] = abort.lr;
        abort.registers[15] = abort.pc;
        DemandPaging { load: TrampolineLoad { context, target, core_instructions: 7 }, abort,
            core_instructions: 8, spsr: context.cpsr, ifar: target, ifsr: 13, service: None }
    }

    #[test]
    fn trampoline_permission_fault_preserves_the_attempted_task_entry_envelope() {
        let demand = demand_paging();
        let transfer = demand.transfer().unwrap();
        assert_eq!(transfer.pc, demand.load.target);
        assert_eq!(transfer.registers[..15], demand.load.context.registers[..15]);
        assert_eq!(transfer.registers[15], demand.load.target);
        assert_eq!(transfer.cpsr, 0x53);
        assert_eq!(transfer.instructions, demand.abort.instructions);
    }

    #[test]
    fn trampoline_fault_refuses_every_unproven_exception_transition() {
        let mutations: [fn(&mut DemandPaging); 10] = [
            |d| d.abort.core = Core::Secondary,
            |d| d.abort.current = Read::Committed { value: 0x4005ffec },
            |d| d.load.context.pc += 4,
            |d| d.abort.pc = 0x10,
            |d| d.abort.cpsr = 0x1d3,
            |d| d.core_instructions += 1,
            |d| d.spsr ^= 0x80,
            |d| d.ifsr = 5,
            |d| d.ifar += 4,
            |d| d.abort.lr += 4,
        ];
        for mutate in mutations {
            let mut demand = demand_paging();
            mutate(&mut demand);
            assert!(demand.transfer().is_err());
        }
        for index in 0..13 {
            let mut demand = demand_paging();
            demand.abort.registers[index] ^= 1;
            assert!(demand.transfer().is_err());
        }
    }

    #[test]
    fn write_hooks_retain_task_irq_and_foreign_writer_envelopes() {
        use fujimod_emulator::board::{bus::{Bus,Mode},factory::{self,Graph,Processor,Ram},memory::BusMapping};
        use fujimod_emulator_cpu::{ExecutionPolicy,Model,timeline::Ratio};
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let graph = Graph {
            processors: vec![Processor { model: Model::CortexR5F, rprofile: Some(Default::default()), mprofile: None,
                released: true, registers: vec![(Register::Pc,0x1000),(Register::Pstate,0x13)] }],
            ram: vec![Ram { base: 0, size: 0x60000 }], peripherals: vec![], decoders: vec![],
            dma_mappings: vec![BusMapping::ram(0,0x60000)],
            mappings: vec![MemoryMap { core: 0, address: 0, physical: 0, size: 0x60000, permissions: 7 },
                MemoryMap { core: 0, address: 0x40000000, physical: 0, size: 0x60000, permissions: 7 },
                MemoryMap { core: 0, address: 0xfffb3000, physical: 0x3000, size: 0x1000, permissions: 3 }],
            instructions_per_tick: Ratio::new(24,1).unwrap(),
        };
        let mut board = graph.build(factory::Options { engine: factory::Engine::Unicorn, translation_cache_bytes: 1<<20 },
            Bus::new(Mode::Strict),None).unwrap();
        board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
        board.write_physical(0x1000,&[0xe5810000u32,0xeafffffe].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>()).unwrap();
        let mut points = board.instruction_points_begin_mutable(0,Vec::new()).unwrap();
        let capture = Box::new(RefCell::new(DataCapture { cpu: unsafe { callback_cpu(board.cpu(0).unwrap()) },
            core: Core::Primary, sequence: Rc::new(Cell::new(0)), active: BTreeSet::from([0x3000,0xfffb30cc & !ALIAS]),
            rows: Vec::new(), overflow: false, failure: None, cached: None }));
        let data = (&*capture as *const RefCell<DataCapture>).cast_mut().cast();
        let hook = unsafe { board.cpu(0).unwrap().install_hook(Hook::Memory {
            callback: data_write, data, read: false, write: true, range: HookRange::All,
        }).unwrap() };
        for (index,(task,context,sp,address)) in [(0x9000u32,0u32,0x5000u32,0x3000u32),
            (0x9000,1,0x7000,0x3000),(0xa000,0,0x6000,0xfffb30cc)].into_iter().enumerate() {
            board.write_physical(0x54760,&task.to_le_bytes()).unwrap();
            board.write_physical(0x545a4,&context.to_le_bytes()).unwrap();
            for (register,value) in [(arm::Register::R(0),index as u64),(arm::Register::R(1),u64::from(address)),
                (arm::Register::R(13),u64::from(sp)),(arm::Register::R(15),0x1000)] {
                board.write_register(0,Register::Arm32(register),value).unwrap();
            }
            board.run_window(index as u64+1,1,None,None).unwrap();
        }
        board.cpu(0).unwrap().remove_hook(hook).unwrap();
        board.instruction_points_end(0,&mut points).unwrap();
        let captured = capture.borrow();
        assert!(!captured.overflow && captured.failure.is_none(), "{:?}", captured.failure);
        assert_eq!(captured.rows.len(),3);
        let owners: Vec<_> = captured.rows.iter().map(|(_,row)| (row.origin.task.0,row.origin.kernel_context,row.origin.sp.0)).collect();
        assert_eq!(owners,[(0x9000,0,0x5000),(0x9000,1,0x7000),(0xa000,0,0x6000)]);
        assert_eq!(captured.rows.iter().map(|(_,row)| row.write.value).collect::<Vec<_>>(),[0,1,2]);
        assert_eq!(captured.rows.iter().map(|(_,row)| row.write.address.0).collect::<Vec<_>>(),[0x3000,0x3000,0xfffb30cc]);
    }
}

