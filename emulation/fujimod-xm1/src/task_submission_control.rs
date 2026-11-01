use super::{Address, Context, Core, Object, TaskId, View, Word, context, register, require, view, word};
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::machine::Machine;
use serde::{Serialize, Serializer};
use std::str::FromStr;

const RETURN_PC: Address = Address(0x69d164);
const SUBMISSION_ENTRY: Address = Address(0x725dd8);
const DELETION_ENTRY: Address = Address(0x72612c);
const RELEASE_ENTRY: Address = Address(0x726098);
const RELEASE_RETURN: Address = Address(0x726240);
const CALLBACK_ENTRY: Address = Address(0x69cbf0);
const CALLBACK_ENTERED: Address = Address(0x69cbf4);
const LOCK: Address = Address(0x409c280c);
const CALLBACK_WORD: Address = Address(0x4089ea8c);
const IRQ_MASK: u32 = 0x80;

// The controlled descriptor occupies root104 backing bytes 32..60.
pub(super) fn descriptor_address(caller_sp: Address) -> Result<Address, String> {
    caller_sp.0.checked_sub(976).map(Address).ok_or_else(|| "controlled descriptor address wraps".into())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ProbeCase {
    Natural,
    FreshEnabled,
    FreshMasked,
    ReusedRecord,
    ReservedRefusal,
    PartialPair,
    SentinelFresh,
    ReleaseUnlocked,
    ReleasePeerLock,
    ReleaseUnlockRefusal,
    CallbackUnreturned,
}

impl FromStr for ProbeCase {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "natural" => Ok(Self::Natural),
            "fresh-enabled" => Ok(Self::FreshEnabled),
            "fresh-masked" => Ok(Self::FreshMasked),
            "reused-record" => Ok(Self::ReusedRecord),
            "reserved-refusal" => Ok(Self::ReservedRefusal),
            "partial-pair" => Ok(Self::PartialPair),
            "sentinel-fresh" => Ok(Self::SentinelFresh),
            "release-unlocked" => Ok(Self::ReleaseUnlocked),
            "release-peer-lock" => Ok(Self::ReleasePeerLock),
            "release-unlock-refusal" => Ok(Self::ReleaseUnlockRefusal),
            "callback-unreturned" => Ok(Self::CallbackUnreturned),
            _ => Err(format!("unknown task-submission probe case: {value}")),
        }
    }
}

impl ProbeCase {
    pub(super) fn target(self) -> ProbeTarget {
        match self {
            Self::ReleaseUnlocked | Self::ReleasePeerLock | Self::ReleaseUnlockRefusal => ProbeTarget::Deletion,
            _ => ProbeTarget::Submission,
        }
    }

    pub(super) fn task(self) -> TaskId {
        match self {
            Self::FreshEnabled | Self::FreshMasked | Self::SentinelFresh => TaskId(1),
            _ => TaskId(102),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ProbeTarget {
    Submission,
    Deletion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProbeStop {
    SubmissionEntry,
    DeletionEntry,
    ReleaseEntry,
    ReleaseReturn,
    UnlockCall,
    CallbackEntry,
    CallbackEntered,
    OuterReturn,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProbeProgress {
    Continue,
    Complete,
}

#[derive(Clone, Copy)]
enum ConfigRegister {
    Argument,
    Link,
    ProgramCounter,
    Status,
}

impl ConfigRegister {
    fn id(self) -> u8 {
        match self {
            Self::Argument => 0,
            Self::Link => 14,
            Self::ProgramCounter => 15,
            Self::Status => 16,
        }
    }

    fn arm(self) -> arm::Register {
        match self {
            Self::Status => arm::Register::Cpsr,
            _ => arm::Register::R(self.id()),
        }
    }
}

impl Serialize for ConfigRegister {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.id())
    }
}

#[derive(Serialize)]
struct RegisterChange {
    register: ConfigRegister,
    before: Word,
    after: Word,
}

#[derive(Serialize)]
struct ControlPatch {
    after_hit: usize,
    watch_cursor: usize,
    address: Address,
    before: View,
    after: View,
}

#[derive(Serialize)]
pub(super) struct ProbeRecord {
    pub(super) case: ProbeCase,
    pub(super) target: ProbeTarget,
    pub(super) task: TaskId,
    pub(super) descriptor: Address,
    pub(super) return_pc: Address,
    initial_hit: usize,
    before: Context,
    configured: Context,
    entry_hit: Option<usize>,
    delete_entry_hit: Option<usize>,
    return_hit: Option<usize>,
    release_entry_hit: Option<usize>,
    release_return_hit: Option<usize>,
    callback_entry_hit: Option<usize>,
    callback_entered_hit: Option<usize>,
    register_changes: Vec<RegisterChange>,
    patches: Vec<ControlPatch>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase {
    AwaitEntry,
    Submission,
    Deletion,
    Release,
    Released,
    CallbackPrologue,
    Complete,
}

pub(super) struct PreparedProbe {
    record: ProbeRecord,
    phase: Phase,
    last_hit: usize,
    unlock_seen: bool,
    callback_before: Option<Context>,
}

fn wrapper(task: TaskId) -> Address {
    Address(0x40a5b204 + 8 * (u32::from(task.0) - 1))
}

fn storage(task: TaskId) -> Address {
    Address(0x40a0fc44 + 12 * (u32::from(task.0) - 1))
}

fn read_word(machine: &Machine, maps: &[MemoryMap], state: &Context, address: Address) -> Result<Word, String> {
    let observed = view(machine, maps, state, Object::ControlPatch, address.0, 4)?;
    Ok(Word(word(&observed.bytes, 0)?))
}

fn same_architecture(left: &Context, right: &Context) -> bool {
    left.core == right.core
        && left.pc == right.pc
        && left.registers == right.registers
        && left.sp == right.sp
        && left.lr == right.lr
        && left.cpsr == right.cpsr
        && left.identities == right.identities
        && left.tcm == right.tcm
}

fn same_context(left: &Context, right: &Context) -> bool {
    same_architecture(left, right) && left.counts == right.counts
}

fn configured_entry(actual: &Context, configured: &Context) -> bool {
    same_architecture(actual, configured)
        && actual.counts[0] == configured.counts[0]
        && actual.counts[1] >= configured.counts[1]
}

fn patch_word(
    machine: &mut Machine,
    maps: &[MemoryMap],
    state: &Context,
    after_hit: usize,
    address: Address,
    value: Word,
    cursor: &impl Fn() -> Result<usize, String>,
    patches: &mut Vec<ControlPatch>,
) -> Result<(), String> {
    let before = view(machine, maps, state, Object::ControlPatch, address.0, 4)?;
    if word(&before.bytes, 0)? == value.0 {
        return Ok(());
    }
    let watch_cursor = cursor()?;
    machine.session.board.write_memory(Core::Primary.index(), u64::from(address.0), &value.0.to_le_bytes())?;
    let after = view(machine, maps, state, Object::ControlPatch, address.0, 4)?;
    require(word(&after.bytes, 0)? == value.0, "control patch did not commit")?;
    patches.push(ControlPatch { after_hit, watch_cursor, address, before, after });
    Ok(())
}

pub(super) fn prepare(
    machine: &mut Machine,
    maps: &[MemoryMap],
    case: ProbeCase,
    initial: Context,
    initial_hit: usize,
    cursor: impl Fn() -> Result<usize, String>,
) -> Result<PreparedProbe, String> {
    require(case != ProbeCase::Natural, "natural observation has no controlled preparation")?;
    require(initial.core == Core::Primary && initial.pc == RETURN_PC && initial.cpsr == Word(0x53), "control probe requires the natural root104 prologue endpoint")?;
    require(same_context(&initial, &context(machine, Core::Primary)?), "control preparation context differs from the natural snapshot")?;
    let root = view(machine, maps, &initial, Object::Wrapper, wrapper(TaskId(104)).0, 8)?;
    let root_base = word(&root.bytes, 0)?;
    let root_size = word(&root.bytes, 4)?;
    require(root_base != 0 && root_size == 1024
        && root_base.checked_add(root_size).and_then(|top| top.checked_sub(16)).map(|sp| sp & !0x40000000) == Some(initial.sp.0 & !0x40000000),
        "control probe root104 stack differs from its natural allocation")?;
    let descriptor = descriptor_address(initial.sp)?;
    require(root_base.checked_add(32).map(|address| address & !0x40000000) == Some(descriptor.0 & !0x40000000),
        "controlled descriptor leaves its root104 backing placement")?;
    let idle = view(machine, maps, &initial, Object::Wrapper, wrapper(TaskId(102)).0, 8)?;
    let idle_state = view(machine, maps, &initial, Object::Storage, storage(TaskId(102)).0, 12)?;
    let idle_base = word(&idle.bytes, 0)?;
    require(idle_base != 0 && word(&idle.bytes, 4)? == 1024
        && word(&idle_state.bytes, 0)? == idle_base && word(&idle_state.bytes, 4)? == 1024
        && word(&idle_state.bytes, 8)? == 1,
        "control probe requires the genuine retained idle102 reservation")?;
    require(read_word(machine, maps, &initial, LOCK)? == Word(0), "natural control lock is not free")?;
    if case.task() == TaskId(1) {
        let fresh = view(machine, maps, &initial, Object::Wrapper, wrapper(TaskId(1)).0, 8)?;
        let fresh_state = view(machine, maps, &initial, Object::Storage, storage(TaskId(1)).0, 12)?;
        require(fresh.bytes.iter().chain(fresh_state.bytes.iter()).all(|byte| *byte == 0), "fresh control task already owns storage")?;
        require(read_word(machine, maps, &initial, Address(0x7cad9c))? == Word(1), "fresh control task does not select class1")?;
    }
    let mut patches = Vec::new();
    if case.target() == ProbeTarget::Submission {
        let tail = if case == ProbeCase::SentinelFresh { 0xa5f0a5f0 } else { 1 };
        for (offset, value) in [u32::from(case.task().0), 0x781968, 15, 1024, CALLBACK_ENTRY.0, 0, tail].into_iter().enumerate() {
            patch_word(machine, maps, &initial, initial_hit, Address(descriptor.0 + 4 * offset as u32), Word(value), &cursor, &mut patches)?;
        }
    }
    match case {
        ProbeCase::ReservedRefusal | ProbeCase::CallbackUnreturned => {
            patch_word(machine, maps, &initial, initial_hit, wrapper(TaskId(102)), Word(0), &cursor, &mut patches)?;
            patch_word(machine, maps, &initial, initial_hit, Address(wrapper(TaskId(102)).0 + 4), Word(0), &cursor, &mut patches)?;
        }
        ProbeCase::PartialPair => {
            patch_word(machine, maps, &initial, initial_hit, Address(wrapper(TaskId(102)).0 + 4), Word(0), &cursor, &mut patches)?;
        }
        _ => {}
    }
    if case == ProbeCase::CallbackUnreturned {
        patch_word(machine, maps, &initial, initial_hit, CALLBACK_WORD, Word(CALLBACK_ENTRY.0), &cursor, &mut patches)?;
    }
    let target = case.target();
    let argument = match target {
        ProbeTarget::Submission => descriptor.0,
        ProbeTarget::Deletion => u32::from(case.task().0),
    };
    let entry = match target {
        ProbeTarget::Submission => SUBMISSION_ENTRY,
        ProbeTarget::Deletion => DELETION_ENTRY,
    };
    let cpsr = if case == ProbeCase::FreshMasked { initial.cpsr.0 | IRQ_MASK } else { initial.cpsr.0 & !IRQ_MASK };
    let mut register_changes = Vec::with_capacity(4);
    for (selected, value) in [
        (ConfigRegister::Argument, argument),
        (ConfigRegister::Link, RETURN_PC.0),
        (ConfigRegister::ProgramCounter, entry.0),
        (ConfigRegister::Status, cpsr),
    ] {
        let before = Word(register(machine, Core::Primary, selected.arm())?);
        let after = Word(value);
        if before != after {
            machine.session.board.write_register(Core::Primary.index(), Register::Arm32(selected.arm()), u64::from(value))?;
            register_changes.push(RegisterChange { register: selected, before, after });
        }
    }
    let configured = context(machine, Core::Primary)?;
    let mut expected = initial.clone();
    expected.registers[0] = Word(argument);
    expected.lr = RETURN_PC;
    expected.pc = entry;
    expected.cpsr = Word(cpsr);
    require(same_context(&configured, &expected), "control configuration changed an undeclared register or context field")?;
    Ok(PreparedProbe {
        record: ProbeRecord {
            case, target, task: case.task(), descriptor, return_pc: RETURN_PC,
            initial_hit, before: initial, configured, entry_hit: None, delete_entry_hit: None,
            return_hit: None, release_entry_hit: None, release_return_hit: None,
            callback_entry_hit: None, callback_entered_hit: None, register_changes, patches,
        },
        phase: Phase::AwaitEntry,
        last_hit: initial_hit,
        unlock_seen: false,
        callback_before: None,
    })
}

impl PreparedProbe {
    pub(super) fn record(&self) -> &ProbeRecord {
        &self.record
    }

    pub(super) fn on_stop(
        &mut self,
        machine: &mut Machine,
        maps: &[MemoryMap],
        stop: ProbeStop,
        hit: usize,
        state: &Context,
        cursor: impl Fn() -> Result<usize, String>,
    ) -> Result<ProbeProgress, String> {
        require(self.phase != Phase::Complete, "control probe already completed")?;
        require(hit > self.last_hit, "control probe snapshot indices are not increasing")?;
        self.last_hit = hit;
        if state.core != Core::Primary {
            return Ok(ProbeProgress::Continue);
        }
        match stop {
            ProbeStop::SubmissionEntry => {
                require(self.phase == Phase::AwaitEntry && self.record.target == ProbeTarget::Submission
                    && state.pc == SUBMISSION_ENTRY && configured_entry(state, &self.record.configured),
                    "controlled submission entry differs from configured context")?;
                self.record.entry_hit = Some(hit);
                self.phase = Phase::Submission;
            }
            ProbeStop::DeletionEntry => {
                require(self.phase == Phase::AwaitEntry && self.record.target == ProbeTarget::Deletion
                    && state.pc == DELETION_ENTRY && configured_entry(state, &self.record.configured),
                    "controlled deletion entry differs from configured context")?;
                self.record.delete_entry_hit = Some(hit);
                self.phase = Phase::Deletion;
            }
            ProbeStop::ReleaseEntry => {
                require(self.phase == Phase::Deletion && state.pc == RELEASE_ENTRY
                    && state.lr == RELEASE_RETURN && state.registers[0] == Word(101),
                    "release did not enter through the retained nonself deletion caller")?;
                self.record.release_entry_hit = Some(hit);
                if self.record.case == ProbeCase::ReleasePeerLock {
                    require(read_word(machine, maps, state, LOCK)? == Word(0), "peer-lock injection requires a free release-entry lock")?;
                    patch_word(machine, maps, state, hit, LOCK, Word(2), &cursor, &mut self.record.patches)?;
                }
                self.phase = Phase::Release;
            }
            ProbeStop::UnlockCall if self.phase == Phase::Release => {
                require(!self.unlock_seen && state.registers[0] == Word(LOCK.0), "release unlock call has an unexpected argument or is repeated")?;
                self.unlock_seen = true;
                if self.record.case == ProbeCase::ReleaseUnlockRefusal {
                    require(read_word(machine, maps, state, LOCK)? == Word(1), "unlock-refusal injection requires the actual acquired lock")?;
                    patch_word(machine, maps, state, hit, LOCK, Word(0), &cursor, &mut self.record.patches)?;
                }
            }
            ProbeStop::ReleaseReturn => {
                require(self.phase == Phase::Release && self.unlock_seen && state.pc == RELEASE_RETURN,
                    "release return lacks its actual entry or unlock call")?;
                self.record.release_return_hit = Some(hit);
                self.phase = Phase::Released;
            }
            ProbeStop::CallbackEntry if self.record.case == ProbeCase::CallbackUnreturned => {
                require(self.phase == Phase::Submission && state.pc == CALLBACK_ENTRY,
                    "bounded callback did not enter from the controlled submission")?;
                self.record.callback_entry_hit = Some(hit);
                self.callback_before = Some(state.clone());
                self.phase = Phase::CallbackPrologue;
            }
            ProbeStop::CallbackEntered if self.record.case == ProbeCase::CallbackUnreturned => {
                let before = self.callback_before.as_ref().ok_or("bounded callback entry snapshot is absent")?;
                require(self.phase == Phase::CallbackPrologue && state.pc == CALLBACK_ENTERED
                    && before.counts[0].checked_add(1) == Some(state.counts[0])
                    && before.sp.0.checked_sub(4) == Some(state.sp.0)
                    && state.registers == before.registers && state.lr == before.lr && state.cpsr == before.cpsr
                    && state.identities == before.identities && state.tcm == before.tcm,
                    "bounded callback endpoint is not its actual single-instruction prologue")?;
                require(read_word(machine, maps, state, state.sp)? == before.registers[11], "bounded callback prologue save did not commit")?;
                self.record.callback_entered_hit = Some(hit);
                self.phase = Phase::Complete;
                return Ok(ProbeProgress::Complete);
            }
            ProbeStop::OuterReturn => {
                require(self.record.case != ProbeCase::CallbackUnreturned, "bounded callback probe returned before its declared endpoint")?;
                let returned = match self.record.target {
                    ProbeTarget::Submission => self.phase == Phase::Submission,
                    ProbeTarget::Deletion => self.phase == Phase::Released,
                };
                require(returned && state.pc == self.record.return_pc && state.sp == self.record.before.sp,
                    "control endpoint is not the actual restored outer return")?;
                self.record.return_hit = Some(hit);
                self.phase = Phase::Complete;
                return Ok(ProbeProgress::Complete);
            }
            _ => {}
        }
        Ok(ProbeProgress::Continue)
    }
}
