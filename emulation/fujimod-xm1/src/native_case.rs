//! Bounded native cases on the pinned FujiMod Cortex-R5F core with the
//! Cortex-R4F exclusions of `r4f::classify_a32`. Input is the
//! `FUJI_NATIVE_V1` wire written by `Emulation.NativeCase`, or a
//! `FUJI_NATIVE_BATCH_V1` wire of such cases; output is the runner
//! observation read by `Emulation.NativeObservation`, one per case.
//!
//! Engine pages carry the union of the permissions of the regions they touch.
//! The memory hook admits each access against exact logical regions and
//! alignment, and refuses any other through `Cpu::refuse_access` before the
//! access happens. At the first access of an LDM/STM or a
//! LDRD/STRD it admits the instruction's whole transfer. Unmapped accesses fault
//! and are refused.
//!
//! The case's call contract is tracked from the decoded control-flow words
//! (B, BL, BX, BLX, single-register POP with PC, LDM with PC and the two LDR-to-PC
//! dispatch forms). All other instruction semantics belong to the backend, which executes the
//! Cortex-R5F instruction set. The user-ARM case profile refuses coprocessor,
//! supervisor-call, unconditional-space, non-APSR status writes, hint and
//! synchronization-primitive words, and ARMv7 UNPREDICTABLE transfer and
//! multiply forms.
//!
//! A privileged case enters in SVC or SYS with its own I and F bits and must
//! stay in that mode with T clear. Beyond the user profile it admits
//! `cpsie`/`cpsid` naming I, F or both with the A bit and mode field clear, and
//! register-form `msr CPSR_c`/`msr CPSR_fc` whose source keeps the entry mode
//! and T clear, and refuses SPSR and banked status access, user-bank LDM/STM
//! and ERET. With a case CP14 identity it serves
//! `mrc p14, 0, Rt, c2, c0, 0` (Rt not PC) in the instruction hook by the law of
//! FujiMod's `rprofile::core::Core` (crates/emulator-cpu-unicorn/src/rprofile/core.rs:783-790):
//! a passing condition writes the identity to Rt, and the PC advances either way.
//!
//! A privileged case may also be a kernel case. Beyond the privileged profile it
//! admits `nop` (the only hint), word `ldrex`/`strex`/`clrex` against a single-core
//! local monitor, DMB/DSB with a defined option, ISB SY and CP15 c7 c10 5/4,
//! TPIDRURW, CPACR and FPEXC accesses when the case declares that register (FPEXC
//! also needs CPACR cp10 and cp11 granting equal access), `mov pc, lr` as a tracked
//! return, a stop address, and device regions that are single interrupt-controller
//! bank words taking word writes only. Barriers and TPIDRURW/FPEXC writes are rows
//! of the provenance log in execution order. The code hook performs the exclusives,
//! barriers and register accesses and advances the PC itself.
//!
//! A kernel case may declare up to sixteen private stacks; its `ldr pc, [Rn, #imm]`
//! then also resumes the context holding SP, which may be the active one.

use crate::r4f;
use fujimod_emulator_cpu::{
    Access, Cpu, ExecutionError, Exit, Fault, Hook, HookRange, Model, Register, Run, arm,
};
use fujimod_emulator_cpu_unicorn::Library;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use std::cell::UnsafeCell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

/// Wire bound read by the binary; a complete 4 MiB case is about 8.4 MB.
pub const MAX_WIRE_BYTES: usize = 16 * 1024 * 1024;
/// Batch wire bound: 256 MiB of distinct artifacts plus case bodies, each
/// shorter than its case JSON, of which a batch carries at most 256 MiB.
pub const MAX_BATCH_WIRE_BYTES: usize = 768 * 1024 * 1024;
const MAX_BATCH_CASES: u32 = 4096;
const MAX_BATCH_ARTIFACTS: u32 = MAX_BATCH_CASES * 20;
const MAX_BATCH_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_REGIONS: u32 = 64;
const MAX_REGION_BYTES: u32 = 2 * 1024 * 1024;
const MAX_MAPPED_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FUNCTIONS: u32 = 256;
const MAX_STUBS: u32 = 32;
const MAX_INSTRUCTIONS: u32 = 4_000_000;
const MAX_WALL_MS: u32 = 10_000;
/// Engine limit on recorded stub events, separate from the 128 expected events.
const MAX_RECORDED_EVENTS: usize = 512;
const MAX_DEPTH: usize = 32;
const MAX_STACK_CONTEXTS: usize = 16;
const LOG_SIZE: usize = 64;
const PROVENANCE_SIZE: usize = 4096;
const PAGE: u64 = 4096;
const TRANSLATION_CACHE_BYTES: u32 = 4 * 1024 * 1024;
/// The PMSAv7 default map is execute-never from 0x80000000.
const EXECUTABLE_END: u64 = 0x7fff_fffc;
const USER_MODE: u32 = 0x10;
const SUPERVISOR_MODE: u32 = 0x13;
const SYSTEM_MODE: u32 = 0x1f;
const CPSR_MODE: u32 = 0x1f;
const CPSR_THUMB: u32 = 1 << 5;
/// NZCVQ, GE, I, F and the mode: the bits a privileged entry CPSR may set.
const PRIVILEGED_ENTRY_BITS: u32 = 0xf80f_00df;
/// The imod field and the I and F mask bits of `cpsie`/`cpsid`.
const CPS_IMOD: u32 = 3 << 18;
const CPS_FLAGS: u32 = 1 << 7 | 1 << 6;
/// `cpsie`/`cpsid` without imod, A, I, F, M or the mode field.
const CPS_BASE: u32 = 0xf100_0000;
/// `msr CPSR_c, Rn` and `msr CPSR_fc, Rn` without cond and Rn.
const MSR_CPSR_C: u32 = 0x0121_f000;
const MSR_CPSR_FC: u32 = 0x0129_f000;
/// `mov pc, lr` without cond.
const MOV_PC_LR: u32 = 0x01a0_f00e;
/// `mrc p14, 0, Rt, c2, c0, 0` without cond and Rt.
const MRC_P14_IDENTITY: u32 = 0x0e12_0e10;
const NOP: u32 = 0xe320_f000;
const CLREX: u32 = 0xf57f_f01f;
/// `dmb` and `dsb` without the option field.
const DMB: u32 = 0xf57f_f050;
const DSB: u32 = 0xf57f_f040;
const ISB_SY: u32 = 0xf57f_f06f;
/// SY, ST, ISH, ISHST, NSH, NSHST, OSH and OSHST.
const BARRIER_OPTIONS: [u32; 8] = [0xf, 0xe, 0xb, 0xa, 0x7, 0x6, 0x3, 0x2];
/// `mcr p15, 0, Rt, c7, c10, 5` and `mcr p15, 0, Rt, c7, c10, 4` without cond and Rt.
const CP15_DMB: u32 = 0x0e07_0fba;
const CP15_DSB: u32 = 0x0e07_0f9a;
/// `mrc`/`mcr p15, 0, Rt, c13, c0, 2`, `mrc p15, 0, Rt, c1, c0, 2`, `vmrs Rt, fpexc`
/// and `vmsr fpexc, Rt` without cond and Rt.
const TPIDRURW_READ: u32 = 0x0e1d_0f50;
const TPIDRURW_WRITE: u32 = 0x0e0d_0f50;
const CPACR_READ: u32 = 0x0e11_0f50;
const FPEXC_READ: u32 = 0x0ef8_0a10;
const FPEXC_WRITE: u32 = 0x0ee8_0a10;
const FPEXC_EN: u32 = 1 << 30;
/// `ldrex Rt, [Rn]` and `strex Rd, Rt, [Rn]` without cond and registers.
const LDREX: u32 = 0x0190_0f9f;
const STREX: u32 = 0x0180_0f90;
/// Set-pending and clear-pending words of the two interrupt-controller banks.
const BANK_WORDS: [u32; 4] = [0xfffb_30cc, 0xfffb_40cc, 0xfffb_30ec, 0xfffb_40ec];

/// Page permission bits as taken by `Cpu::map_zeroed`.
const PAGE_READ: u32 = 1;
const PAGE_WRITE: u32 = 2;
const PAGE_EXECUTE: u32 = 4;

pub struct Outcome {
    pub returned: bool,
    pub json: Vec<u8>,
}

/// Validates one wire, executes it and renders its observation.
pub fn run(wire: &[u8]) -> Outcome {
    let case = parse(wire);
    render(&match &case {
        Ok(case) => execute(case),
        Err(error) => Observation::invalid(*error),
    })
}

#[derive(Clone, Copy)]
pub enum CaptureAddress { Fixed(u32), Register(u8) }
#[derive(Clone, Copy)]
pub struct Capture { pub service: u32, pub address: CaptureAddress, pub bytes: u32 }
#[derive(Debug, Serialize)]
pub struct CapturedMemory { pub address: u32, pub bytes: Vec<u8> }
#[derive(Debug, Serialize)]
pub struct StubSnapshot { pub service: u32, pub registers: [u32; 15], pub memory: Vec<CapturedMemory> }

pub fn run_captured(wire: &[u8], captures: &[Capture]) -> (Outcome, Vec<StubSnapshot>) {
    let mut snapshots = Vec::new();
    let observation = match parse(wire) {
        Ok(case) => return (render(&execute_captured(&case, captures, &mut snapshots)), snapshots),
        Err(error) => Observation::invalid(error),
    };
    (render(&observation), snapshots)
}

fn render(observation: &Observation<'_>) -> Outcome {
    match serde_json::to_vec(observation) {
        Ok(mut json) => {
            json.push(b'\n');
            Outcome { returned: observation.status == Status::Returned, json }
        }
        Err(error) => Outcome {
            returned: false,
            json: format!(
                "{{\"wire_version\":1,\"status\":\"engine_error\",\"detail\":{:?}}}\n",
                format!("observation encoding failed: {error}")
            )
            .into_bytes(),
        },
    }
}

/// Checks the batch framing, then runs each case as `run` would and writes
/// `FUJI_NATIVE_BATCH_CASE <index> <exit>` and its observation line, flushed per
/// case, then `FUJI_NATIVE_BATCH_END <count>`. Each case parses its own regions
/// from the shared artifacts and executes on a newly opened engine. Returns
/// false for a refused batch framing, which runs no case.
pub fn run_batch(wire: &[u8], out: &mut impl std::io::Write) -> std::io::Result<bool> {
    let batch = match frame(wire) {
        Ok(batch) => batch,
        Err(error) => {
            writeln!(out, "FUJI_NATIVE_BATCH_INVALID {}", error.0)?;
            out.flush()?;
            return Ok(false);
        }
    };
    for (index, body) in batch.cases.iter().enumerate() {
        let case = parse_case(body, |tokens, bytes| slice(tokens, &batch.artifacts, bytes));
        let outcome = render(&match &case {
            Ok(case) => execute(case),
            Err(error) => Observation::invalid(*error),
        });
        writeln!(out, "FUJI_NATIVE_BATCH_CASE {index} {}", if outcome.returned { 0 } else { 1 })?;
        out.write_all(&outcome.json)?;
        out.flush()?;
    }
    writeln!(out, "FUJI_NATIVE_BATCH_END {}", batch.cases.len())?;
    out.flush()?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Case

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Address(u32);

/// A nonempty byte range inside the 32-bit address space.
#[derive(Clone, Copy, Debug)]
struct Span {
    base: u32,
    bytes: u32,
}

impl Span {
    fn new(base: u32, bytes: u32) -> Result<Self, WireError> {
        if bytes == 0 || u64::from(base) + u64::from(bytes) > 1 << 32 {
            return Err(WireError("span is empty or leaves the 32-bit address space"));
        }
        Ok(Self { base, bytes })
    }

    fn end(self) -> u64 {
        u64::from(self.base) + u64::from(self.bytes)
    }

    fn contains(self, address: u64, size: u64) -> bool {
        size != 0
            && address >= u64::from(self.base)
            && address
                .checked_add(size)
                .is_some_and(|end| end <= self.end())
    }

    fn overlaps(self, other: Span) -> bool {
        u64::from(self.base) < other.end() && u64::from(other.base) < self.end()
    }
}

/// A declared private stack, downward growing from `high` to `low`.
#[derive(Clone, Copy, Debug)]
struct StackBounds {
    low: u32,
    high: u32,
}

impl StackBounds {
    /// `low < sp <= high`.
    fn holds(self, sp: u32) -> bool {
        self.low < sp && sp <= self.high
    }

    fn overlaps(self, other: Self) -> bool {
        self.low < other.high && other.low < self.high
    }
}

/// A validated index of one declared stack context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContextId(usize);

impl ContextId {
    fn index(self) -> usize {
        self.0
    }
}

/// The declared private stacks of an opted-in kernel case, in wire order.
#[derive(Debug)]
struct StackContexts {
    bounds: Box<[StackBounds]>,
    initial: ContextId,
}

impl StackContexts {
    fn len(&self) -> usize {
        self.bounds.len()
    }
}

/// `COUNT LOW HIGH ...`, one pair per declared stack, pending admission.
fn parse_bounds(tokens: &mut Tokens<'_>) -> Result<Box<[StackBounds]>, WireError> {
    let count = tokens.count(MAX_STACK_CONTEXTS as u32)?;
    if count == 0 {
        return Err(WireError("a stack-context row needs at least one context"));
    }
    let mut bounds = Vec::with_capacity(count as usize);
    for _ in 0..count {
        bounds.push(StackBounds { low: tokens.word()?, high: tokens.word()? });
    }
    Ok(bounds.into())
}

/// `None` unless exactly one declared stack holds `sp`; adjacent declarations
/// can share a boundary.
fn context_of(bounds: &[StackBounds], sp: u32) -> Option<ContextId> {
    let mut held = bounds.iter().enumerate().filter(|(_, stack)| stack.holds(sp)).map(|(index, _)| ContextId(index));
    let id = held.next()?;
    held.next().is_none().then_some(id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Permission {
    Read,
    ReadWrite,
    ReadExecute,
    /// One bank word of a kernel case: aligned word writes only.
    Device,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Need {
    Read,
    Write,
    Execute,
}

impl Permission {
    fn grants(self, need: Need) -> bool {
        match need {
            Need::Read => self != Self::Device,
            Need::Write => matches!(self, Self::ReadWrite | Self::Device),
            Need::Execute => self == Self::ReadExecute,
        }
    }

    fn page_bits(self) -> u32 {
        match self {
            Self::Read => PAGE_READ,
            Self::ReadWrite | Self::Device => PAGE_READ | PAGE_WRITE,
            Self::ReadExecute => PAGE_READ | PAGE_EXECUTE,
        }
    }
}

struct Region {
    span: Span,
    permission: Permission,
    initial: Box<[u8]>,
}

/// One of r0-r12, the registers a stub contract may bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StubRegister(u8);

impl StubRegister {
    fn register(self) -> Register {
        arm::CORE[usize::from(self.0)]
    }

    fn label(self) -> &'static str {
        CORE_LABELS[usize::from(self.0)]
    }
}

#[derive(Clone, Copy, Debug)]
struct Binding {
    register: StubRegister,
    value: u32,
}

struct Stub {
    address: Address,
    inputs: Box<[Binding]>,
    outputs: Box<[Binding]>,
}

/// Entry CPSR: user mode with NZCV only, or a `PrivilegedMode` with
/// `PRIVILEGED_ENTRY_BITS` only.
#[derive(Clone, Copy, Debug)]
struct Cpsr(u32);

/// r0-r14 at entry; r13 is SP and r14 is LR.
#[derive(Clone, Copy, Debug)]
struct EntryRegisters {
    core: [u32; 15],
    cpsr: Cpsr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivilegedMode {
    Supervisor,
    System,
}

impl PrivilegedMode {
    fn of(cpsr: u32) -> Option<Self> {
        if cpsr & !PRIVILEGED_ENTRY_BITS != 0 {
            return None;
        }
        match cpsr & CPSR_MODE {
            SUPERVISOR_MODE => Some(Self::Supervisor),
            SYSTEM_MODE => Some(Self::System),
            _ => None,
        }
    }

    fn bits(self) -> u32 {
        match self {
            Self::Supervisor => SUPERVISOR_MODE,
            Self::System => SYSTEM_MODE,
        }
    }
}

/// The value `mrc p14, 0, Rt, c2, c0, 0` reads.
#[derive(Clone, Copy, Debug)]
struct Cp14Identity(u32);

#[derive(Clone, Copy, Debug)]
enum Profile {
    User,
    Privileged { mode: PrivilegedMode, identity: Option<Cp14Identity>, kernel: Option<Kernel> },
}

#[derive(Clone, Copy, Debug)]
struct Tpidrurw(u32);

#[derive(Clone, Copy, Debug)]
struct Cpacr(u32);

impl Cpacr {
    /// cp10 (bits 21:20) and cp11 (bits 23:22) equal and 0b01 or 0b11.
    fn grants_vfp(self) -> bool {
        let (cp10, cp11) = (self.0 >> 20 & 3, self.0 >> 22 & 3);
        cp10 == cp11 && cp10 & 1 == 1
    }
}

/// FPEXC with no bit other than EN.
#[derive(Clone, Copy, Debug)]
struct Fpexc(u32);

impl Fpexc {
    fn new(value: u32) -> Option<Self> {
        (value & !FPEXC_EN == 0).then_some(Self(value))
    }
}

/// A kernel case's stop address and the system register values it serves.
#[derive(Clone, Copy, Debug)]
struct Kernel {
    stop: Option<Address>,
    tpidrurw: Option<Tpidrurw>,
    cpacr: Option<Cpacr>,
    fpexc: Option<Fpexc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Barrier {
    Dmb,
    Dsb,
    Isb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SystemRegister {
    Tpidrurw,
    Cpacr,
    Fpexc,
}

/// A kernel-case word the code hook performs; register fields are core indices.
#[derive(Clone, Copy, Debug)]
enum KernelWord {
    Barrier(Barrier),
    LoadExclusive { target: usize, base: usize },
    StoreExclusive { status: usize, source: usize, base: usize },
    ClearExclusive,
    Read { target: usize, register: SystemRegister },
    Write { source: usize, register: SystemRegister },
}

impl Kernel {
    fn classify(self, word: u32) -> Option<ProfileWord> {
        let kernel = |op| Some(ProfileWord::Kernel(op));
        match word {
            NOP => return Some(ProfileWord::Ordinary),
            CLREX => return kernel(KernelWord::ClearExclusive),
            ISB_SY => return kernel(KernelWord::Barrier(Barrier::Isb)),
            _ => {}
        }
        if BARRIER_OPTIONS.contains(&(word & 15)) {
            match word & !15 {
                DMB => return kernel(KernelWord::Barrier(Barrier::Dmb)),
                DSB => return kernel(KernelWord::Barrier(Barrier::Dsb)),
                _ => {}
            }
        }
        let (rn, rt, rm) = ((word >> 16 & 15) as usize, (word >> 12 & 15) as usize, (word & 15) as usize);
        if word >> 28 == 15 || rt == 15 {
            return None;
        }
        let read = |register| kernel(KernelWord::Read { target: rt, register });
        let write = |register| kernel(KernelWord::Write { source: rt, register });
        match word & 0x0fff_0fff {
            CP15_DMB => return kernel(KernelWord::Barrier(Barrier::Dmb)),
            CP15_DSB => return kernel(KernelWord::Barrier(Barrier::Dsb)),
            TPIDRURW_READ if self.tpidrurw.is_some() => return read(SystemRegister::Tpidrurw),
            TPIDRURW_WRITE if self.tpidrurw.is_some() => return write(SystemRegister::Tpidrurw),
            CPACR_READ if self.cpacr.is_some() => return read(SystemRegister::Cpacr),
            FPEXC_READ if self.vfp() => return read(SystemRegister::Fpexc),
            FPEXC_WRITE if self.vfp() => return write(SystemRegister::Fpexc),
            _ => {}
        }
        if rn == 15 {
            return None;
        }
        if word & 0x0ff0_0fff == LDREX {
            return kernel(KernelWord::LoadExclusive { target: rt, base: rn });
        }
        if word & 0x0ff0_0ff0 == STREX && rm != 15 && rt != rn && rt != rm {
            return kernel(KernelWord::StoreExclusive { status: rt, source: rm, base: rn });
        }
        None
    }
}

/// How a profile treats one A32 word.
#[derive(Clone, Copy, Debug)]
enum ProfileWord {
    Excluded,
    Ordinary,
    /// Register-form `msr CPSR_c`/`msr CPSR_fc` from this core register.
    ControlWrite(u32),
    /// The CP14 identity read into this core register.
    IdentityRead(usize, Cp14Identity),
    Kernel(KernelWord),
}

/// A `cpsie`/`cpsid` word that changes only the I and F mask bits.
#[derive(Clone, Copy, Debug)]
enum CpsMask {
    EnableI,
    EnableF,
    EnableIf,
    DisableI,
    DisableF,
    DisableIf,
}

impl CpsMask {
    fn of(word: u32) -> Option<Self> {
        let form = match (word >> 18 & 3, word >> 6 & 3) {
            (0b10, 0b10) => Self::EnableI,
            (0b10, 0b01) => Self::EnableF,
            (0b10, 0b11) => Self::EnableIf,
            (0b11, 0b10) => Self::DisableI,
            (0b11, 0b01) => Self::DisableF,
            (0b11, 0b11) => Self::DisableIf,
            _ => return None,
        };
        (word & !(CPS_IMOD | CPS_FLAGS) == CPS_BASE).then_some(form)
    }
}

impl Profile {
    fn mode(self) -> u32 {
        match self {
            Self::User => USER_MODE,
            Self::Privileged { mode, .. } => mode.bits(),
        }
    }

    fn kernel(self) -> Option<Kernel> {
        match self {
            Self::User => None,
            Self::Privileged { kernel, .. } => kernel,
        }
    }

    fn classify(self, word: u32) -> ProfileWord {
        let Self::Privileged { identity, kernel, .. } = self else {
            return if excluded(word) { ProfileWord::Excluded } else { ProfileWord::Ordinary };
        };
        if CpsMask::of(word).is_some() {
            return ProfileWord::Ordinary;
        }
        let conditional = word >> 28 != 15;
        if conditional && matches!(word & 0x0fff_fff0, MSR_CPSR_C | MSR_CPSR_FC) && word & 15 != 15 {
            return ProfileWord::ControlWrite(word & 15);
        }
        let target = (word >> 12 & 15) as usize;
        if let Some(identity) = identity
            && conditional
            && word & 0x0fff_0fff == MRC_P14_IDENTITY
            && target != 15
        {
            return ProfileWord::IdentityRead(target, identity);
        }
        if let Some(class) = kernel.and_then(|kernel| kernel.classify(word)) {
            return class;
        }
        if privileged_refused(word) || excluded(word) { ProfileWord::Excluded } else { ProfileWord::Ordinary }
    }
}

#[derive(Clone, Copy, Debug)]
struct InstructionBudget(u32);

#[derive(Clone, Copy, Debug)]
struct WallBudget(Duration);

struct Case {
    entry: Address,
    exit: Address,
    registers: EntryRegisters,
    profile: Profile,
    budget: InstructionBudget,
    wall: WallBudget,
    regions: Box<[Region]>,
    functions: Box<[Span]>,
    stubs: Box<[Stub]>,
    stack_contexts: Option<StackContexts>,
}

impl Case {
    /// Containment of the whole range in one region granting `need`; a device
    /// region takes whole-word accesses only.
    fn covers(&self, address: u64, bytes: u64, need: Need) -> bool {
        self.regions.iter().any(|region| {
            region.permission.grants(need)
                && region.span.contains(address, bytes)
                && (region.permission != Permission::Device || bytes == 4)
        })
    }

    /// One guest access: covered and, wider than a byte, naturally aligned.
    fn admits(&self, address: u64, size: u64, need: Need) -> bool {
        (size <= 1 || address % size == 0) && self.covers(address, size, need)
    }

    /// An aligned word inside one non-device region granting `need`.
    fn admits_exclusive(&self, address: u32, need: Need) -> bool {
        address % 4 == 0
            && self.regions.iter().any(|region| {
                region.permission != Permission::Device
                    && region.permission.grants(need)
                    && region.span.contains(u64::from(address), 4)
            })
    }

    fn stops_at(&self, address: u32) -> bool {
        self.profile.kernel().and_then(|kernel| kernel.stop).is_some_and(|stop| stop.0 == address)
    }

    fn function_at(&self, address: u32) -> Option<usize> {
        self.functions
            .iter()
            .position(|function| function.contains(u64::from(address), 4))
    }

    fn stub_at(&self, address: u32) -> Option<usize> {
        self.stubs.iter().position(|stub| stub.address.0 == address)
    }
}

// ---------------------------------------------------------------------------
// Wire

#[derive(Clone, Copy, Debug)]
struct WireError(&'static str);

struct Tokens<'a> {
    rest: &'a [u8],
}

fn is_separator(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\r' | b'\t')
}

impl<'a> Tokens<'a> {
    fn next(&mut self) -> Result<&'a [u8], WireError> {
        let start = self
            .rest
            .iter()
            .position(|&byte| !is_separator(byte))
            .ok_or(WireError("wire ends early"))?;
        let rest = &self.rest[start..];
        let end = rest.iter().position(|&byte| is_separator(byte)).unwrap_or(rest.len());
        self.rest = &rest[end..];
        Ok(&rest[..end])
    }

    fn literal(&mut self, expected: &[u8], error: &'static str) -> Result<(), WireError> {
        if self.next()? == expected { Ok(()) } else { Err(WireError(error)) }
    }

    /// One to eight lowercase hexadecimal digits.
    fn word(&mut self) -> Result<u32, WireError> {
        hex_word(self.next()?)
    }

    /// `n`, or a word.
    fn optional_word(&mut self) -> Result<Option<u32>, WireError> {
        match self.next()? {
            b"n" => Ok(None),
            token => hex_word(token).map(Some),
        }
    }

    fn peek(&self) -> Option<&'a [u8]> {
        Tokens { rest: self.rest }.next().ok()
    }

    fn count(&mut self, max: u32) -> Result<u32, WireError> {
        let token = self.next()?;
        if token.is_empty() || token.len() > 10 || !token.iter().all(u8::is_ascii_digit) {
            return Err(WireError("count must be decimal digits"));
        }
        let value = token
            .iter()
            .fold(0u64, |value, &digit| value * 10 + u64::from(digit - b'0'));
        u32::try_from(value)
            .ok()
            .filter(|&value| value <= max)
            .ok_or(WireError("count exceeds its limit"))
    }

    fn bytes(&mut self, count: u32) -> Result<Box<[u8]>, WireError> {
        let token = self.next()?;
        if token.len() != 2 * count as usize {
            return Err(WireError("region bytes must be two hex digits per byte"));
        }
        token
            .chunks_exact(2)
            .map(|pair| match (nibble(pair[0]), nibble(pair[1])) {
                (Some(high), Some(low)) => Ok(high << 4 | low),
                _ => Err(WireError("region bytes must be lowercase hex")),
            })
            .collect()
    }

    fn finish(self) -> Result<(), WireError> {
        if self.rest.iter().all(|&byte| is_separator(byte)) {
            Ok(())
        } else {
            Err(WireError("wire has trailing tokens"))
        }
    }

    /// The `count` bytes after the single newline that ends the previous token.
    fn raw(&mut self, count: u32) -> Result<&'a [u8], WireError> {
        let rest = self.rest.strip_prefix(b"\n").ok_or(WireError("raw bytes must follow one newline"))?;
        if rest.len() < count as usize {
            return Err(WireError("raw bytes end early"));
        }
        let (bytes, rest) = rest.split_at(count as usize);
        self.rest = rest;
        Ok(bytes)
    }
}

fn nibble(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

fn hex_word(token: &[u8]) -> Result<u32, WireError> {
    if token.is_empty() || token.len() > 8 {
        return Err(WireError("hexadecimal word must have one to eight digits"));
    }
    token.iter().try_fold(0u32, |value, &digit| {
        nibble(digit)
            .map(|digit| value << 4 | u32::from(digit))
            .ok_or(WireError("hexadecimal word must be lowercase hex"))
    })
}

impl Kernel {
    /// `STOP TPIDRURW CPACR FPEXC`, each `n` or a word.
    fn parse(tokens: &mut Tokens<'_>) -> Result<Self, WireError> {
        let stop = tokens.optional_word()?.map(Address);
        let tpidrurw = tokens.optional_word()?.map(Tpidrurw);
        let cpacr = tokens.optional_word()?.map(Cpacr);
        let fpexc = match tokens.optional_word()? {
            None => None,
            Some(value) => Some(Fpexc::new(value).ok_or(WireError("a kernel case's FPEXC may set EN only"))?),
        };
        if fpexc.is_some() && cpacr.is_none() {
            return Err(WireError("a kernel case's FPEXC needs a CPACR"));
        }
        Ok(Self { stop, tpidrurw, cpacr, fpexc })
    }

    fn vfp(self) -> bool {
        self.fpexc.is_some() && self.cpacr.is_some_and(Cpacr::grants_vfp)
    }
}

fn parse(wire: &[u8]) -> Result<Case, WireError> {
    if wire.len() > MAX_WIRE_BYTES {
        return Err(WireError("wire exceeds its byte limit"));
    }
    parse_case(wire, |tokens, bytes| tokens.bytes(bytes))
}

/// One case wire whose region rows end in the form `initial` reads.
fn parse_case(
    wire: &[u8],
    mut initial: impl FnMut(&mut Tokens<'_>, u32) -> Result<Box<[u8]>, WireError>,
) -> Result<Case, WireError> {
    let mut tokens = Tokens { rest: wire };
    tokens.literal(b"FUJI_NATIVE_V1", "wire version must be FUJI_NATIVE_V1")?;
    tokens.literal(b"cortex-r5f-arm-le", "CPU profile must be cortex-r5f-arm-le")?;
    let entry = Address(tokens.word()?);
    let exit = Address(tokens.word()?);
    let budget = tokens.count(MAX_INSTRUCTIONS)?;
    let wall_ms = tokens.count(MAX_WALL_MS)?;
    if budget == 0 || wall_ms == 0 {
        return Err(WireError("instruction and wall budgets must be nonzero"));
    }
    if entry.0 % 4 != 0 || exit.0 % 4 != 0 || entry == exit {
        return Err(WireError("entry and exit must be distinct aligned addresses"));
    }
    let mut core = [0u32; 15];
    for value in &mut core {
        *value = tokens.word()?;
    }
    let cpsr = tokens.word()?;
    let (profile, stack_contexts) = if cpsr & 0x0fff_ffff == USER_MODE {
        (Profile::User, None)
    } else {
        let mode = PrivilegedMode::of(cpsr).ok_or(WireError(
            "CPSR must be user-mode ARM with flags only, or SVC or SYS ARM with flags, Q, GE, I and F only",
        ))?;
        let identity = match tokens.next()? {
            b"n" => None,
            b"i" => Some(Cp14Identity(tokens.word()?)),
            _ => return Err(WireError("a privileged case's CP14 identity must be n or i <word>")),
        };
        let kernel = if tokens.peek() == Some(b"k") {
            tokens.next()?;
            Some(Kernel::parse(&mut tokens)?)
        } else {
            None
        };
        let stack_contexts = if kernel.is_some() && tokens.peek() == Some(b"c") {
            tokens.next()?;
            Some(parse_bounds(&mut tokens)?)
        } else {
            None
        };
        (Profile::Privileged { mode, identity, kernel }, stack_contexts)
    };
    if core[13] % 8 != 0 || core[14] != exit.0 {
        return Err(WireError("SP must be eight-byte aligned and LR must be the exit"));
    }
    let region_count = tokens.count(MAX_REGIONS)?;
    let function_count = tokens.count(MAX_FUNCTIONS)?;
    let stub_count = tokens.count(MAX_STUBS)?;
    if region_count == 0 || function_count == 0 {
        return Err(WireError("a case needs a region and a function"));
    }

    let mut regions: Vec<Region> = Vec::with_capacity(region_count as usize);
    let mut mapped = 0u64;
    for _ in 0..region_count {
        let base = tokens.word()?;
        let bytes = tokens.word()?;
        if bytes > MAX_REGION_BYTES {
            return Err(WireError("region exceeds 2 MiB"));
        }
        let span = Span::new(base, bytes)?;
        let permission = match tokens.count(15)? {
            1 => Permission::Read,
            3 => Permission::ReadWrite,
            5 => Permission::ReadExecute,
            10 if profile.kernel().is_some() => Permission::Device,
            _ => return Err(WireError("region permission must be 1, 3 or 5, or 10 in a kernel case")),
        };
        if permission == Permission::Device && (bytes != 4 || !BANK_WORDS.contains(&base)) {
            return Err(WireError("a device region is one interrupt-controller bank word"));
        }
        if permission == Permission::ReadExecute && span.end() > EXECUTABLE_END {
            return Err(WireError("executable region must end by 0x7ffffffc"));
        }
        mapped += u64::from(bytes);
        if mapped > MAX_MAPPED_BYTES {
            return Err(WireError("regions exceed 4 MiB in total"));
        }
        if regions.iter().any(|region| region.span.overlaps(span)) {
            return Err(WireError("regions overlap"));
        }
        let initial = initial(&mut tokens, bytes)?;
        regions.push(Region { span, permission, initial });
    }

    let mut functions: Vec<Span> = Vec::with_capacity(function_count as usize);
    for _ in 0..function_count {
        let base = tokens.word()?;
        let bytes = tokens.word()?;
        if bytes > MAX_REGION_BYTES || base % 4 != 0 || bytes % 4 != 0 {
            return Err(WireError("function must be aligned, word-sized and at most 2 MiB"));
        }
        let span = Span::new(base, bytes)?;
        if functions.iter().any(|function| function.overlaps(span)) {
            return Err(WireError("functions overlap"));
        }
        functions.push(span);
    }

    let mut stubs: Vec<Stub> = Vec::with_capacity(stub_count as usize);
    for _ in 0..stub_count {
        let address = Address(tokens.word()?);
        let input_count = tokens.count(13)?;
        let output_count = tokens.count(13)?;
        if output_count == 0 {
            return Err(WireError("stub must declare an output"));
        }
        let inputs = bindings(&mut tokens, input_count)?;
        let outputs = bindings(&mut tokens, output_count)?;
        stubs.push(Stub { address, inputs, outputs });
    }
    tokens.finish()?;

    let mut case = Case {
        entry,
        exit,
        registers: EntryRegisters { core, cpsr: Cpsr(cpsr) },
        profile,
        budget: InstructionBudget(budget),
        wall: WallBudget(Duration::from_millis(u64::from(wall_ms))),
        regions: regions.into(),
        functions: functions.into(),
        stubs: stubs.into(),
        stack_contexts: None,
    };
    validate(&case)?;
    if let Some(bounds) = stack_contexts {
        case.stack_contexts = Some(validate_stack_contexts(&case, bounds)?);
    }
    Ok(case)
}

fn bindings(tokens: &mut Tokens<'_>, count: u32) -> Result<Box<[Binding]>, WireError> {
    let mut bindings: Vec<Binding> = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let register = StubRegister(tokens.count(12)? as u8);
        let value = tokens.word()?;
        if bindings.iter().any(|binding| binding.register == register) {
            return Err(WireError("stub binds a register twice"));
        }
        bindings.push(Binding { register, value });
    }
    Ok(bindings.into())
}

fn validate(case: &Case) -> Result<(), WireError> {
    let executable = |address: Address| case.covers(u64::from(address.0), 4, Need::Execute);
    if !executable(case.entry) || !executable(case.exit) {
        return Err(WireError("entry and exit need executable bytes"));
    }
    let sp = u64::from(case.registers.core[13]);
    if sp < 8 || !case.covers(sp - 8, 8, Need::Write) {
        return Err(WireError("stack top lacks eight writable bytes"));
    }
    for function in case.functions.iter() {
        if !case.covers(u64::from(function.base), u64::from(function.bytes), Need::Execute) {
            return Err(WireError("function lies outside one executable region"));
        }
        if function.contains(u64::from(case.exit.0), 4) {
            return Err(WireError("exit lies inside a function"));
        }
    }
    if !case.functions.iter().any(|function| function.base == case.entry.0) {
        return Err(WireError("entry must start a declared function"));
    }
    for (index, stub) in case.stubs.iter().enumerate() {
        if stub.address.0 % 4 != 0 || stub.address == case.exit {
            return Err(WireError("stub address must be aligned and differ from the exit"));
        }
        if case.function_at(stub.address.0).is_some() {
            return Err(WireError("stub address lies inside a function"));
        }
        if case.stubs[..index].iter().any(|prior| prior.address == stub.address) {
            return Err(WireError("stub address is declared twice"));
        }
    }
    if let Some(stop) = case.profile.kernel().and_then(|kernel| kernel.stop)
        && (stop.0 % 4 != 0 || stop == case.entry || stop == case.exit || !executable(stop) || case.stub_at(stop.0).is_some())
    {
        return Err(WireError("stop address must be aligned, executable and differ from the entry, the exit and every stub"));
    }
    Ok(())
}

/// Each declared stack is aligned, nonempty, disjoint and wholly writable case
/// memory; the initial SP's context is admitted with them.
fn validate_stack_contexts(case: &Case, bounds: Box<[StackBounds]>) -> Result<StackContexts, WireError> {
    for (index, stack) in bounds.iter().enumerate() {
        if stack.low % 8 != 0 || stack.high % 8 != 0 || stack.low >= stack.high {
            return Err(WireError("stack context bounds must be eight-byte aligned with low below high"));
        }
        if !case.regions.iter().any(|region| {
            region.permission == Permission::ReadWrite
                && region.span.contains(u64::from(stack.low), u64::from(stack.high - stack.low))
        }) {
            return Err(WireError("stack context is not wholly backed by writable case memory"));
        }
        if bounds[..index].iter().any(|prior| prior.overlaps(*stack)) {
            return Err(WireError("stack contexts overlap"));
        }
    }
    let initial = context_of(&bounds, case.registers.core[13])
        .ok_or(WireError("the initial SP must belong to exactly one declared stack context"))?;
    Ok(StackContexts { bounds, initial })
}

struct Batch<'a> {
    artifacts: Vec<&'a [u8]>,
    cases: Vec<&'a [u8]>,
}

/// `FUJI_NATIVE_BATCH_V1`, artifact and case counts, then each artifact and each
/// case body as a decimal length, one newline and that many raw bytes.
fn frame(wire: &[u8]) -> Result<Batch<'_>, WireError> {
    if wire.len() > MAX_BATCH_WIRE_BYTES {
        return Err(WireError("batch wire exceeds its byte limit"));
    }
    let mut tokens = Tokens { rest: wire };
    tokens.literal(b"FUJI_NATIVE_BATCH_V1", "batch version must be FUJI_NATIVE_BATCH_V1")?;
    let artifact_count = tokens.count(MAX_BATCH_ARTIFACTS)?;
    let case_count = tokens.count(MAX_BATCH_CASES)?;
    if case_count == 0 {
        return Err(WireError("a batch needs a case"));
    }
    let mut artifacts = Vec::with_capacity(artifact_count as usize);
    let mut total = 0u64;
    for _ in 0..artifact_count {
        let length = tokens.count(MAX_REGION_BYTES)?;
        total += u64::from(length);
        if length == 0 || total > MAX_BATCH_ARTIFACT_BYTES {
            return Err(WireError("artifacts must be nonempty and at most 256 MiB in total"));
        }
        artifacts.push(tokens.raw(length)?);
    }
    let mut cases = Vec::with_capacity(case_count as usize);
    for _ in 0..case_count {
        let length = tokens.count(MAX_WIRE_BYTES as u32)?;
        cases.push(tokens.raw(length)?);
    }
    tokens.finish()?;
    Ok(Batch { artifacts, cases })
}

/// A batch region's initial bytes: `z` for zeros, or `s <artifact> <offset> <length>`
/// for an artifact slice followed by zeros.
fn slice(tokens: &mut Tokens<'_>, artifacts: &[&[u8]], bytes: u32) -> Result<Box<[u8]>, WireError> {
    let mut initial = vec![0u8; bytes as usize];
    match tokens.next()? {
        b"z" => {}
        b"s" => {
            let artifact = tokens.count(MAX_BATCH_ARTIFACTS)?;
            let offset = tokens.count(MAX_REGION_BYTES)? as usize;
            let length = tokens.count(bytes)? as usize;
            let source = artifacts
                .get(artifact as usize)
                .and_then(|source| source.get(offset..offset + length))
                .filter(|_| length != 0)
                .ok_or(WireError("region slice lies outside its artifact"))?;
            initial[..length].copy_from_slice(source);
        }
        _ => return Err(WireError("region initial must be z or s")),
    }
    Ok(initial.into())
}

// ---------------------------------------------------------------------------
// Observation

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Returned,
    InstructionBudget,
    WallBudget,
    MemoryGuard,
    FetchGuard,
    UnknownCall,
    UnsupportedInstruction,
    StubContract,
    EngineError,
    /// A kernel case reached its stop address.
    StopAddress,
    InvalidCase,
}

/// `0x`-prefixed eight-digit hexadecimal.
#[derive(Clone, Copy, Default)]
struct Word(u32);

impl Serialize for Word {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("0x{:08x}", self.0))
    }
}

/// `0x`-prefixed sixteen-digit hexadecimal.
#[derive(Clone, Copy, Default)]
struct Wide(u64);

impl Serialize for Wide {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("0x{:016x}", self.0))
    }
}

/// A count rendered as a decimal string.
#[derive(Clone, Copy)]
struct Count(u64);

impl Serialize for Count {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

struct HexBytes(Vec<u8>);

impl fmt::Display for HexBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl Serialize for HexBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

const CORE_LABELS: [&str; 13] = [
    "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12",
];

#[derive(Serialize)]
struct Registers {
    r0: Word,
    r1: Word,
    r2: Word,
    r3: Word,
    r4: Word,
    r5: Word,
    r6: Word,
    r7: Word,
    r8: Word,
    r9: Word,
    r10: Word,
    r11: Word,
    r12: Word,
    sp: Word,
    lr: Word,
    cpsr: Word,
    pc: Word,
}

/// r0-r14, CPSR and PC.
#[derive(Clone, Copy)]
struct FinalRegisters {
    core: [u32; 15],
    cpsr: u32,
    pc: u32,
}

impl From<FinalRegisters> for Registers {
    fn from(values: FinalRegisters) -> Self {
        let r = |index: usize| Word(values.core[index]);
        Self {
            r0: r(0),
            r1: r(1),
            r2: r(2),
            r3: r(3),
            r4: r(4),
            r5: r(5),
            r6: r(6),
            r7: r(7),
            r8: r(8),
            r9: r(9),
            r10: r(10),
            r11: r(11),
            r12: r(12),
            sp: r(13),
            lr: r(14),
            cpsr: Word(values.cpsr),
            pc: Word(values.pc),
        }
    }
}

struct Bindings<'a>(&'a [Binding]);

impl Serialize for Bindings<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for binding in self.0 {
            map.serialize_entry(binding.register.label(), &Word(binding.value))?;
        }
        map.end()
    }
}

#[derive(Serialize)]
struct FinalRegion {
    address: Word,
    bytes: Option<HexBytes>,
}

#[derive(Serialize)]
struct Event<'a> {
    address: Word,
    inputs: Bindings<'a>,
    outputs: Bindings<'a>,
}

#[derive(Clone, Copy, Default, Serialize)]
struct TraceEntry {
    pc: Word,
    instruction: Word,
}

/// The `type` of an access record: Unicorn's published `uc_mem_type` numbering.
#[derive(Clone, Copy, Default)]
enum AccessType {
    #[default]
    Read,
    Write,
    ReadUnmapped,
    WriteUnmapped,
    FetchUnmapped,
    WriteProtected,
    ReadProtected,
    FetchProtected,
}

impl Serialize for AccessType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Self::Read => 16,
            Self::Write => 17,
            Self::ReadUnmapped => 19,
            Self::WriteUnmapped => 20,
            Self::FetchUnmapped => 21,
            Self::WriteProtected => 22,
            Self::ReadProtected => 23,
            Self::FetchProtected => 24,
        })
    }
}

impl From<Fault> for AccessType {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::ReadUnmapped => Self::ReadUnmapped,
            Fault::WriteUnmapped => Self::WriteUnmapped,
            Fault::FetchUnmapped => Self::FetchUnmapped,
            Fault::ReadProtected => Self::ReadProtected,
            Fault::WriteProtected => Self::WriteProtected,
            Fault::FetchProtected => Self::FetchProtected,
        }
    }
}

#[derive(Clone, Copy, Default, Serialize)]
struct AccessEntry {
    #[serde(rename = "type")]
    kind: AccessType,
    address: Word,
    size: i32,
    value: Wide,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProvenanceKind {
    Read,
    Write,
}

#[derive(Clone, Copy, Serialize)]
struct ProvenanceEntry {
    pc: Word,
    instruction: Word,
    kind: ProvenanceKind,
    address: Word,
    size: i32,
    value: Wide,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum BarrierKind {
    Barrier,
}

#[derive(Clone, Copy, Serialize)]
struct BarrierEntry {
    pc: Word,
    instruction: Word,
    kind: BarrierKind,
    barrier: Barrier,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SystemKind {
    System,
}

#[derive(Clone, Copy, Serialize)]
struct SystemEntry {
    pc: Word,
    instruction: Word,
    kind: SystemKind,
    register: SystemRegister,
    value: Word,
}

#[derive(Clone, Copy, Serialize)]
#[serde(untagged)]
enum LogEntry {
    Access(ProvenanceEntry),
    Barrier(BarrierEntry),
    System(SystemEntry),
}

/// A kernel case's final TPIDRURW and FPEXC, each when the case declares it, and
/// the word its last LDREX reserved, which STREX and CLREX leave reported.
#[derive(Serialize)]
struct KernelRegisters {
    #[serde(skip_serializing_if = "Option::is_none")]
    tpidrurw: Option<Word>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fpexc: Option<Word>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reserved: Option<Word>,
}

#[derive(Serialize)]
struct Observation<'a> {
    wire_version: u8,
    status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    unicorn_error: u32,
    instructions: Count,
    elapsed_ns: Count,
    registers: Registers,
    memory: Vec<FinalRegion>,
    events: Vec<Event<'a>>,
    trace_total: Count,
    trace: Vec<TraceEntry>,
    access_total: Count,
    accesses: Vec<AccessEntry>,
    memory_provenance_total: Count,
    memory_provenance: Vec<LogEntry>,
    /// Present exactly for an executed kernel case.
    #[serde(skip_serializing_if = "Option::is_none")]
    kernel: Option<KernelRegisters>,
}

impl<'a> Observation<'a> {
    fn invalid(error: WireError) -> Self {
        Self::empty(Status::InvalidCase, error.0.to_owned(), FinalRegisters { core: [0; 15], cpsr: 0, pc: 0 }, Vec::new())
    }

    /// An engine refusal before execution: entry registers and unread regions.
    fn refused(case: &Case, registers: FinalRegisters, detail: String) -> Self {
        let memory = case
            .regions
            .iter()
            .map(|region| FinalRegion { address: Word(region.span.base), bytes: None })
            .collect();
        Self::empty(Status::EngineError, detail, registers, memory)
    }

    fn empty(status: Status, detail: String, registers: FinalRegisters, memory: Vec<FinalRegion>) -> Self {
        Self {
            wire_version: 1,
            status,
            detail: Some(detail),
            unicorn_error: 0,
            instructions: Count(0),
            elapsed_ns: Count(0),
            registers: registers.into(),
            memory,
            events: Vec::new(),
            trace_total: Count(0),
            trace: Vec::new(),
            access_total: Count(0),
            accesses: Vec::new(),
            memory_provenance_total: Count(0),
            memory_provenance: Vec::new(),
            kernel: None,
        }
    }
}

/// The Unicorn `uc_err` value the pinned core decoded into this error.
fn unicorn_error(error: &ExecutionError) -> u32 {
    match error {
        ExecutionError::InvalidInstruction(_) => 10,
        ExecutionError::Memory { fault, .. } => match fault {
            Fault::ReadUnmapped => 6,
            Fault::WriteUnmapped => 7,
            Fault::FetchUnmapped => 8,
            Fault::WriteProtected => 12,
            Fault::ReadProtected => 13,
            Fault::FetchProtected => 14,
        },
        ExecutionError::AccessRefused { .. } => 23,
        ExecutionError::Backend(_) => u32::MAX,
    }
}

// ---------------------------------------------------------------------------
// Execution

fn execute(case: &Case) -> Observation<'_> {
    execute_captured(case, &[], &mut Vec::new())
}

fn execute_captured<'a>(case: &'a Case, captures: &[Capture], snapshots: &mut Vec<StubSnapshot>) -> Observation<'a> {
    let entry_registers = FinalRegisters {
        core: case.registers.core,
        cpsr: case.registers.cpsr.0,
        pc: case.entry.0,
    };
    let refused = |detail: String| Observation::refused(case, entry_registers, detail);
    let cpu = match Library::linked().and_then(|library| library.open(Model::CortexR5F, TRANSLATION_CACHE_BYTES)) {
        Ok(cpu) => cpu,
        Err(error) => return refused(format!("pinned CPU unavailable: {error}")),
    };
    if !cpu.features().access_refusal {
        return refused("pinned CPU lacks native access refusal".to_owned());
    }
    if let Err(error) = prepare(&cpu, case) {
        return refused(format!("case setup failed: {error}"));
    }
    let context = Context { cpu: &cpu, case, state: UnsafeCell::new(State::new(case)) };
    // SAFETY: execution has not started.
    unsafe { (*context.state.get()).captures.extend_from_slice(captures); }
    let data = (&context as *const Context<'_>).cast_mut().cast::<c_void>();
    let hooks = [
        Hook::Instruction { callback: instruction_entry, data, range: HookRange::All },
        Hook::Memory { callback: memory_entry, data, read: true, write: true, range: HookRange::All },
        Hook::Fault { callback: fault_entry, data },
        Hook::Exception { callback: exception_entry, data },
    ];
    // SAFETY: `context` outlives every run of `cpu`; the hooks are removed after the
    // run, and no run follows if removal fails.
    let installed = match unsafe { cpu.install_hooks(&hooks) } {
        Ok(installed) => installed,
        Err(error) => return refused(format!("hook installation failed: {error}")),
    };
    let started = Instant::now();
    // SAFETY: no callback runs outside `cpu.run`.
    unsafe { (*context.state.get()).started = started };
    let result = cpu.run(Run {
        begin: u64::from(case.entry.0),
        until: None,
        watchdog_us: case.wall.0.as_micros() as u64,
        instructions: Some(case.budget.0 as usize + 1),
    });
    let elapsed = started.elapsed();
    let removal = cpu.remove_hooks(&installed);
    // SAFETY: the run has ended, so no callback holds the state.
    let state = unsafe { &mut *context.state.get() };
    state.conclude(case, result, elapsed);
    if let Err(error) = removal {
        state.host_failure(format!("hook removal failed: {error}"));
    }
    snapshots.append(&mut state.snapshots);
    state.observe(&cpu, case, elapsed, entry_registers)
}

fn prepare(cpu: &dyn Cpu, case: &Case) -> Result<(), String> {
    let mut pages: BTreeMap<u64, u32> = BTreeMap::new();
    for region in case.regions.iter() {
        let mut page = u64::from(region.span.base) & !(PAGE - 1);
        while page < region.span.end() {
            *pages.entry(page).or_default() |= region.permission.page_bits();
            page += PAGE;
        }
    }
    let mut runs: Vec<(u64, u64, u32)> = Vec::new();
    for (&page, &bits) in &pages {
        match runs.last_mut() {
            Some((_, end, run_bits)) if *end == page && *run_bits == bits => *end += PAGE,
            _ => runs.push((page, page + PAGE, bits)),
        }
    }
    for (base, end, bits) in runs {
        cpu.map_zeroed(base, (end - base) as usize, bits)?;
    }
    for region in case.regions.iter() {
        cpu.write_memory(u64::from(region.span.base), &region.initial)?;
    }
    write(cpu, arm::CPSR, case.registers.cpsr.0)?;
    for (index, &value) in case.registers.core.iter().enumerate() {
        write(cpu, arm::CORE[index], value)?;
    }
    write(cpu, arm::PC, case.entry.0)
}

fn read(cpu: &dyn Cpu, register: Register) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    match cpu.read_register(register, &mut bytes)? {
        4 => Ok(u32::from_le_bytes(bytes)),
        size => Err(format!("{register:?} read returned {size} bytes")),
    }
}

fn write(cpu: &dyn Cpu, register: Register, value: u32) -> Result<(), String> {
    cpu.write_register(register, &value.to_le_bytes())
}

fn read_word(cpu: &dyn Cpu, address: u64) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    cpu.read_memory(address, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

struct Context<'a> {
    cpu: &'a dyn Cpu,
    case: &'a Case,
    state: UnsafeCell<State>,
}

impl Context<'_> {
    /// Runs one callback body; callbacks never nest, so the state borrow is unique.
    fn dispatch(&self, body: impl FnOnce(&mut State, &dyn Cpu, &Case) -> Result<(), Step>) {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: see `dispatch`.
            let state = unsafe { &mut *self.state.get() };
            match body(state, self.cpu, self.case) {
                Ok(()) => {}
                Err(Step::Stop(status)) => state.stop(self.cpu, status),
                Err(Step::Host(message)) => {
                    state.host_failure(message);
                    state.request_stop(self.cpu);
                }
            }
        }));
        if outcome.is_err() {
            // SAFETY: the unwound borrow has ended.
            let state = unsafe { &mut *self.state.get() };
            state.host_failure("native-case callback panicked".to_owned());
            state.request_stop(self.cpu);
        }
    }
}

unsafe fn instruction_entry(address: u64, size: u32, data: *mut c_void) {
    // SAFETY: `data` is the `Context` installed by `execute`.
    let context = unsafe { &*data.cast::<Context<'_>>() };
    context.dispatch(|state, cpu, case| state.on_instruction(cpu, case, address, size));
}

unsafe fn memory_entry(access: Access, address: u64, size: i32, value: i64, data: *mut c_void) {
    // SAFETY: as for `instruction_entry`.
    let context = unsafe { &*data.cast::<Context<'_>>() };
    context.dispatch(|state, cpu, case| state.on_access(cpu, case, access, address, size, value));
}

unsafe fn fault_entry(fault: Fault, address: u64, size: i32, value: i64, data: *mut c_void) -> bool {
    // SAFETY: as for `instruction_entry`.
    let context = unsafe { &*data.cast::<Context<'_>>() };
    context.dispatch(|state, _, _| {
        state.on_fault(fault, address, size, value);
        Ok(())
    });
    false
}

unsafe fn exception_entry(exception: arm::Exception, data: *mut c_void) {
    // SAFETY: as for `instruction_entry`.
    let context = unsafe { &*data.cast::<Context<'_>>() };
    context.dispatch(|state, _, _| state.on_exception(exception));
}

enum Step {
    Stop(Status),
    Host(String),
}

impl From<String> for Step {
    fn from(message: String) -> Self {
        Self::Host(message)
    }
}

/// Refuses the access whose memory hook is running.
fn refuse(cpu: &dyn Cpu) -> Result<(), Step> {
    cpu.refuse_access().map_err(|error| Step::Host(format!("access refusal failed: {error}")))
}

#[derive(Clone, Copy, Default)]
struct Frame {
    return_address: u32,
    caller: usize,
}

/// The shadow call state of one stack context.
#[derive(Clone, Copy)]
struct Shadow {
    current: usize,
    frames: [Frame; MAX_DEPTH],
    depth: usize,
    returning: bool,
}

impl Shadow {
    /// A context entered at `function` with no frames and no root return.
    fn at(function: usize) -> Self {
        Self { current: function, frames: [Frame::default(); MAX_DEPTH], depth: 0, returning: false }
    }
}

/// The active context's shadow state, and for an opted-in case the state of
/// every other declared context as last switched away from.
enum ShadowContexts {
    Single(Shadow),
    Declared { active: ContextId, states: Box<[Shadow]> },
}

impl ShadowContexts {
    fn new(case: &Case) -> Self {
        let shadow = Shadow::at(case.function_at(case.entry.0).unwrap_or(0));
        let Some(contexts) = &case.stack_contexts else {
            return Self::Single(shadow);
        };
        Self::Declared { active: contexts.initial, states: vec![shadow; contexts.len()].into_boxed_slice() }
    }

    fn state(&self) -> &Shadow {
        match self {
            Self::Single(shadow) => shadow,
            Self::Declared { active, states } => &states[active.index()],
        }
    }

    fn state_mut(&mut self) -> &mut Shadow {
        match self {
            Self::Single(shadow) => shadow,
            Self::Declared { active, states } => &mut states[active.index()],
        }
    }

    /// Resumes `dest` at `resume` with its saved frames, retiring a pending
    /// frame whose return address is `resume`. `dest` may be the active context.
    fn switch(&mut self, dest: ContextId, function: usize, resume: u32) {
        let Self::Declared { active, states } = self else {
            return;
        };
        *active = dest;
        let shadow = &mut states[dest.index()];
        match shadow.depth {
            0 => shadow.current = function,
            depth if shadow.frames[depth - 1].return_address == resume => {
                shadow.current = shadow.frames[depth - 1].caller;
                shadow.depth -= 1;
            }
            _ => shadow.current = function,
        }
    }
}

/// The words of an LDM/STM or LDRD/STRD, admitted
/// together at the first address, which the engine accesses first.
#[derive(Clone, Copy)]
struct Transfer {
    words: u32,
    need: Need,
}

impl Transfer {
    fn of(word: u32) -> Option<Self> {
        let load = |bit: bool| if bit { Need::Read } else { Need::Write };
        if word >> 25 & 7 == 4 {
            return Some(Self { words: (word & 0xffff).count_ones(), need: load(word & 1 << 20 != 0) });
        }
        (word & 0x0e10_00d0 == 0x0000_00d0).then(|| Self { words: 2, need: load(word & 0x20 == 0) })
    }

    fn admitted(self, case: &Case, first: u64) -> bool {
        (0..self.words).all(|index| case.admits((first + 4 * u64::from(index)) & 0xffff_ffff, 4, self.need))
    }
}

#[derive(Clone, Copy)]
struct Instruction {
    pc: u32,
    word: u32,
    transfer: Option<Transfer>,
}

/// The local exclusive monitor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Monitor {
    Open,
    Exclusive(u32),
}

impl Monitor {
    /// A store over the tagged word other than STREX, whose effect on the
    /// monitor is IMPLEMENTATION DEFINED.
    fn clobbered(self, transfer: Option<Transfer>, address: u64, size: u64) -> bool {
        let Self::Exclusive(tag) = self else {
            return false;
        };
        let tag = u64::from(tag);
        let overlaps = |start: u64, bytes: u64| start < tag + 4 && tag < start + bytes;
        match transfer {
            Some(transfer) => (0..u64::from(transfer.words)).any(|index| overlaps((address + 4 * index) & 0xffff_ffff, 4)),
            None => overlaps(address, size),
        }
    }
}

/// A kernel case's current TPIDRURW, CPACR and FPEXC.
#[derive(Clone, Copy)]
struct SystemState {
    tpidrurw: Option<Tpidrurw>,
    cpacr: Option<Cpacr>,
    fpexc: Option<Fpexc>,
}

impl SystemState {
    fn read(self, register: SystemRegister) -> Option<u32> {
        match register {
            SystemRegister::Tpidrurw => self.tpidrurw.map(|value| value.0),
            SystemRegister::Cpacr => self.cpacr.map(|value| value.0),
            SystemRegister::Fpexc => self.fpexc.map(|value| value.0),
        }
    }
}

struct State {
    status: Option<Status>,
    detail: Option<String>,
    unicorn_error: u32,
    started: Instant,
    entered: u64,
    shadow: ShadowContexts,
    events: Vec<usize>,
    trace: [TraceEntry; LOG_SIZE],
    accesses: [AccessEntry; LOG_SIZE],
    access_total: u64,
    provenance: Vec<LogEntry>,
    provenance_total: u64,
    instruction: Instruction,
    monitor: Monitor,
    reserved: Option<u32>,
    system: SystemState,
    captures: Vec<Capture>,
    snapshots: Vec<StubSnapshot>,
}

impl State {
    fn new(case: &Case) -> Self {
        let kernel = case.profile.kernel();
        Self {
            status: None,
            detail: None,
            unicorn_error: 0,
            started: Instant::now(),
            entered: 0,
            shadow: ShadowContexts::new(case),
            events: Vec::with_capacity(MAX_RECORDED_EVENTS),
            captures: Vec::new(),
            snapshots: Vec::new(),
            trace: [TraceEntry::default(); LOG_SIZE],
            accesses: [AccessEntry::default(); LOG_SIZE],
            access_total: 0,
            provenance: Vec::with_capacity(PROVENANCE_SIZE),
            provenance_total: 0,
            instruction: Instruction { pc: 0, word: 0, transfer: None },
            monitor: Monitor::Open,
            reserved: None,
            system: SystemState {
                tpidrurw: kernel.and_then(|kernel| kernel.tpidrurw),
                cpacr: kernel.and_then(|kernel| kernel.cpacr),
                fpexc: kernel.and_then(|kernel| kernel.fpexc),
            },
        }
    }

    fn stop(&mut self, cpu: &dyn Cpu, status: Status) {
        if self.status.is_none() {
            self.status = Some(status);
        }
        self.request_stop(cpu);
    }

    fn request_stop(&mut self, cpu: &dyn Cpu) {
        if let Err(error) = cpu.stop() {
            self.host_failure(format!("stop request failed: {error}"));
        }
    }

    fn host_failure(&mut self, message: String) {
        self.status = Some(Status::EngineError);
        self.detail.get_or_insert(message);
    }

    fn log(&mut self, kind: AccessType, address: u64, size: i32, value: i64) {
        self.accesses[(self.access_total % LOG_SIZE as u64) as usize] = AccessEntry {
            kind,
            address: Word(address as u32),
            size,
            value: Wide(value as u64),
        };
        self.access_total += 1;
    }

    fn record(&mut self, entry: LogEntry) {
        if self.provenance.len() < PROVENANCE_SIZE {
            self.provenance.push(entry);
        }
        self.provenance_total += 1;
    }

    fn record_access(&mut self, kind: ProvenanceKind, address: u32, size: i32, value: u64) {
        self.record(LogEntry::Access(ProvenanceEntry {
            pc: Word(self.instruction.pc),
            instruction: Word(self.instruction.word),
            kind,
            address: Word(address),
            size,
            value: Wide(value),
        }));
    }

    fn on_instruction(&mut self, cpu: &dyn Cpu, case: &Case, address: u64, size: u32) -> Result<(), Step> {
        if self.status.is_some() {
            return Ok(());
        }
        if self.started.elapsed() >= case.wall.0 {
            return Err(Step::Stop(Status::WallBudget));
        }
        let cpsr = read(cpu, arm::CPSR)?;
        if cpsr & CPSR_THUMB != 0 || cpsr & CPSR_MODE != case.profile.mode() {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        if size != 4 || !case.admits(address, 4, Need::Execute) {
            return Err(Step::Stop(Status::FetchGuard));
        }
        let pc = address as u32;
        if pc == case.exit.0 {
            let shadow = self.shadow.state();
            let status = if shadow.returning && shadow.depth == 0 { Status::Returned } else { Status::UnknownCall };
            return Err(Step::Stop(status));
        }
        if case.stops_at(pc) {
            return Err(Step::Stop(Status::StopAddress));
        }
        if !case.functions[self.shadow.state().current].contains(address, 4) {
            return Err(Step::Stop(Status::UnknownCall));
        }
        if self.entered >= u64::from(case.budget.0) {
            return Err(Step::Stop(Status::InstructionBudget));
        }
        let word = read_word(cpu, address)?;
        self.trace[(self.entered % LOG_SIZE as u64) as usize] = TraceEntry { pc: Word(pc), instruction: Word(word) };
        self.entered += 1;
        self.instruction = Instruction { pc, word, transfer: None };
        let class = case.profile.classify(word);
        let passes = arm::condition_passes(word >> 28, cpsr);
        match class {
            ProfileWord::Excluded => return Err(Step::Stop(Status::UnsupportedInstruction)),
            ProfileWord::IdentityRead(target, identity) => {
                if passes {
                    write(cpu, arm::CORE[target], identity.0)?;
                }
                write(cpu, arm::PC, pc.wrapping_add(4))?;
                return Ok(());
            }
            ProfileWord::Kernel(op) => {
                if word >> 28 == 15 || passes {
                    self.perform(cpu, case, op)?;
                }
                write(cpu, arm::PC, pc.wrapping_add(4))?;
                return Ok(());
            }
            ProfileWord::ControlWrite(_) | ProfileWord::Ordinary => {}
        }
        if !passes {
            return Ok(());
        }
        if let ProfileWord::ControlWrite(source) = class {
            let value = read(cpu, arm::CORE[source as usize])?;
            if value & CPSR_THUMB != 0 || value & CPSR_MODE != case.profile.mode() {
                return Err(Step::Stop(Status::UnsupportedInstruction));
            }
        }
        self.instruction.transfer = Transfer::of(word);
        self.track(cpu, case, pc, word)
    }

    /// Performs one condition-passing kernel word; the caller advances the PC.
    fn perform(&mut self, cpu: &dyn Cpu, case: &Case, op: KernelWord) -> Result<(), Step> {
        match op {
            KernelWord::Barrier(barrier) => {
                let (pc, instruction) = (Word(self.instruction.pc), Word(self.instruction.word));
                self.record(LogEntry::Barrier(BarrierEntry { pc, instruction, kind: BarrierKind::Barrier, barrier }));
            }
            KernelWord::ClearExclusive => self.monitor = Monitor::Open,
            KernelWord::LoadExclusive { target, base } => {
                let address = read(cpu, arm::CORE[base])?;
                self.log(AccessType::Read, u64::from(address), 4, 0);
                if !case.admits_exclusive(address, Need::Read) {
                    return Err(Step::Stop(Status::MemoryGuard));
                }
                let value = read_word(cpu, u64::from(address))?;
                self.record_access(ProvenanceKind::Read, address, 4, u64::from(value));
                write(cpu, arm::CORE[target], value)?;
                self.monitor = Monitor::Exclusive(address);
                self.reserved = Some(address);
            }
            KernelWord::StoreExclusive { status, source, base } => {
                let address = read(cpu, arm::CORE[base])?;
                let value = read(cpu, arm::CORE[source])?;
                if !case.admits_exclusive(address, Need::Write) {
                    self.log(AccessType::Write, u64::from(address), 4, i64::from(value));
                    return Err(Step::Stop(Status::MemoryGuard));
                }
                let stored = match self.monitor {
                    Monitor::Exclusive(tag) if tag == address => true,
                    Monitor::Exclusive(_) => return Err(Step::Stop(Status::UnsupportedInstruction)),
                    Monitor::Open => false,
                };
                if stored {
                    self.log(AccessType::Write, u64::from(address), 4, i64::from(value));
                    cpu.write_memory(u64::from(address), &value.to_le_bytes())?;
                    self.record_access(ProvenanceKind::Write, address, 4, u64::from(value));
                }
                write(cpu, arm::CORE[status], u32::from(!stored))?;
                self.monitor = Monitor::Open;
            }
            KernelWord::Read { target, register } => {
                let value = self.system.read(register).ok_or_else(|| Step::Host(format!("{register:?} is not declared")))?;
                write(cpu, arm::CORE[target], value)?;
            }
            KernelWord::Write { source, register } => {
                let value = read(cpu, arm::CORE[source])?;
                match register {
                    SystemRegister::Tpidrurw => self.system.tpidrurw = Some(Tpidrurw(value)),
                    SystemRegister::Fpexc => {
                        self.system.fpexc = Some(Fpexc::new(value).ok_or(Step::Stop(Status::UnsupportedInstruction))?);
                    }
                    SystemRegister::Cpacr => return Err(Step::Host("CPACR is read-only".to_owned())),
                }
                let (pc, instruction) = (Word(self.instruction.pc), Word(self.instruction.word));
                self.record(LogEntry::System(SystemEntry { pc, instruction, kind: SystemKind::System, register, value: Word(value) }));
            }
        }
        Ok(())
    }

    /// Applies the call contract of one condition-passing word before it executes.
    fn track(&mut self, cpu: &dyn Cpu, case: &Case, pc: u32, word: u32) -> Result<(), Step> {
        let next = pc.wrapping_add(4);
        match control(word) {
            Control::BranchLink => self.call(cpu, case, branch_target(pc, word), next),
            Control::Branch => self.tail(cpu, case, branch_target(pc, word)),
            Control::BranchLinkRegister(register) => {
                let target = arm_target(cpu, register)?;
                self.call(cpu, case, target, next)
            }
            Control::BranchRegister(14) => {
                let link = arm_target(cpu, 14)?;
                self.return_to(case, link)
            }
            Control::BranchRegister(register) => {
                let target = arm_target(cpu, register)?;
                self.tail(cpu, case, target)
            }
            Control::Dispatch(table) => self.dispatch(cpu, case, pc, word, table),
            Control::PopSingleReturn => self.pop_single_return(cpu, case),
            Control::LoadMultipleReturn => self.load_multiple_return(cpu, case, word),
            Control::UntrackedPcWrite if case.profile.kernel().is_some() && word & 0x0fff_ffff == MOV_PC_LR => {
                let link = arm_target(cpu, 14)?;
                self.return_to(case, link)
            }
            Control::UntrackedPcWrite => self.load_stop(cpu, case, pc, word),
            Control::None => Ok(()),
        }
    }

    /// `ldr pc, [Rn, #+/-imm12]`: the declared stop address, or the resume address of
    /// the stack context holding SP. An inadmissible load is left to its own refusal.
    fn load_stop(&mut self, cpu: &dyn Cpu, case: &Case, pc: u32, word: u32) -> Result<(), Step> {
        let Some(kernel) = case.profile.kernel() else {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        };
        if word & 0x0f70_f000 != 0x0510_f000 {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        let Some(contexts) = &case.stack_contexts else {
            let Some(stop) = kernel.stop else {
                return Err(Step::Stop(Status::UnsupportedInstruction));
            };
            let address = pc_load_address(cpu, pc, word)?;
            return if case.admits(u64::from(address), 4, Need::Read) && read_word(cpu, u64::from(address))? == stop.0 {
                Ok(())
            } else {
                Err(Step::Stop(Status::UnsupportedInstruction))
            };
        };
        let address = pc_load_address(cpu, pc, word)?;
        if !case.admits(u64::from(address), 4, Need::Read) {
            return Ok(());
        }
        let target = read_word(cpu, u64::from(address))?;
        if kernel.stop.is_some_and(|stop| stop.0 == target) {
            return Ok(());
        }
        if target % 4 != 0 {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        let Some(function) = case.function_at(target) else {
            return Err(Step::Stop(Status::UnknownCall));
        };
        let Some(dest) = context_of(&contexts.bounds, read(cpu, arm::CORE[13])?) else {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        };
        self.shadow.switch(dest, function, target);
        Ok(())
    }

    fn call(&mut self, cpu: &dyn Cpu, case: &Case, target: u32, return_address: u32) -> Result<(), Step> {
        if case.stops_at(target) {
            return Ok(());
        }
        if let Some(index) = case.stub_at(target) {
            self.apply_stub(cpu, &case.stubs[index], index)?;
            write(cpu, arm::CORE[14], return_address)?;
            write(cpu, arm::PC, return_address)?;
            return Ok(());
        }
        let Some(function) = case.function_at(target) else {
            return Err(Step::Stop(Status::UnknownCall));
        };
        let shadow = self.shadow.state_mut();
        if shadow.depth == MAX_DEPTH {
            return Err(Step::Stop(Status::UnknownCall));
        }
        shadow.frames[shadow.depth] = Frame { return_address, caller: shadow.current };
        shadow.depth += 1;
        shadow.current = function;
        Ok(())
    }

    /// B, BX or a dispatch: stays in or changes function; a stub returns to LR.
    fn tail(&mut self, cpu: &dyn Cpu, case: &Case, target: u32) -> Result<(), Step> {
        if case.functions[self.shadow.state().current].contains(u64::from(target), 4) {
            return Ok(());
        }
        if let Some(function) = case.function_at(target) {
            self.shadow.state_mut().current = function;
            return Ok(());
        }
        let Some(index) = case.stub_at(target) else {
            return Ok(());
        };
        let link = read(cpu, arm::CORE[14])?;
        if link != self.expected_return(case) {
            return Err(Step::Stop(Status::UnknownCall));
        }
        self.apply_stub(cpu, &case.stubs[index], index)?;
        self.return_to(case, link)?;
        write(cpu, arm::PC, link)?;
        Ok(())
    }

    fn expected_return(&self, case: &Case) -> u32 {
        let shadow = self.shadow.state();
        match shadow.depth {
            0 => case.exit.0,
            depth => shadow.frames[depth - 1].return_address,
        }
    }

    fn return_to(&mut self, case: &Case, target: u32) -> Result<(), Step> {
        if target != self.expected_return(case) {
            return if case.stops_at(target) { Ok(()) } else { Err(Step::Stop(Status::UnknownCall)) };
        }
        let shadow = self.shadow.state_mut();
        match shadow.depth {
            0 => shadow.returning = true,
            depth => {
                shadow.current = shadow.frames[depth - 1].caller;
                shadow.depth -= 1;
            }
        }
        Ok(())
    }

    fn apply_stub(&mut self, cpu: &dyn Cpu, stub: &Stub, index: usize) -> Result<(), Step> {
        if self.events.len() == MAX_RECORDED_EVENTS {
            return Err(Step::Stop(Status::StubContract));
        }
        for binding in stub.inputs.iter() {
            if read(cpu, binding.register.register())? != binding.value {
                return Err(Step::Stop(Status::StubContract));
            }
        }
        if self.captures.iter().any(|capture| capture.service == stub.address.0) {
            let mut registers = [0; 15];
            for (index, value) in registers.iter_mut().enumerate() { *value = read(cpu, arm::CORE[index])?; }
            let mut memory = Vec::new();
            for capture in self.captures.iter().filter(|capture| capture.service == stub.address.0) {
                if capture.bytes == 0 { continue; }
                let address = match capture.address {
                    CaptureAddress::Fixed(address) => address,
                    CaptureAddress::Register(index) => *registers.get(usize::from(index)).ok_or(Step::Stop(Status::StubContract))?,
                };
                if capture.bytes > 4096 { return Err(Step::Stop(Status::StubContract)); }
                let mut bytes = vec![0; capture.bytes as usize];
                cpu.read_memory(u64::from(address), &mut bytes)?;
                memory.push(CapturedMemory { address, bytes });
            }
            self.snapshots.push(StubSnapshot { service: stub.address.0, registers, memory });
        }
        self.events.push(index);
        for binding in stub.outputs.iter() {
            write(cpu, binding.register.register(), binding.value)?;
        }
        Ok(())
    }

    /// An inadmissible table entry is left to the refusal of the load itself.
    fn dispatch(&mut self, cpu: &dyn Cpu, case: &Case, pc: u32, word: u32, table: Table) -> Result<(), Step> {
        let index = read(cpu, arm::CORE[(word & 15) as usize])?;
        let entry = match table {
            Table::PcRelative => pc.wrapping_add(8).wrapping_add(index << 2),
            Table::Register => {
                let base = read(cpu, arm::CORE[(word >> 16 & 15) as usize])?;
                let amount = word >> 7 & 31;
                let offset = match word >> 5 & 3 {
                    0 => index << amount,
                    1 if amount == 0 => 0,
                    1 => index >> amount,
                    _ if amount == 0 => ((index as i32) >> 31) as u32,
                    _ => ((index as i32) >> amount) as u32,
                };
                if word & 1 << 23 != 0 { base.wrapping_add(offset) } else { base.wrapping_sub(offset) }
            }
        };
        let entry = u64::from(entry);
        if !case.admits(entry, 4, Need::Read) {
            return Ok(());
        }
        let target = read_word(cpu, entry)?;
        if target & 3 != 0 {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        self.tail(cpu, case, target)
    }

    fn pop_single_return(&mut self, cpu: &dyn Cpu, case: &Case) -> Result<(), Step> {
        let address = read(cpu, arm::CORE[13])?;
        if !(Transfer { words: 1, need: Need::Read }).admitted(case, u64::from(address)) {
            return Ok(());
        }
        let loaded = read_word(cpu, u64::from(address))?;
        self.return_to(case, loaded)
    }

    /// LDM with PC: the loaded PC must be the tracked return address. An
    /// inadmissible transfer is left to the refusal of its first access.
    fn load_multiple_return(&mut self, cpu: &dyn Cpu, case: &Case, word: u32) -> Result<(), Step> {
        let list = word & 0xffff;
        let base = read(cpu, arm::CORE[(word >> 16 & 15) as usize])?;
        let words = list.count_ones();
        let first = match (word & 1 << 23 != 0, word & 1 << 24 != 0) {
            (true, false) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(4 * words - 4),
            (false, true) => base.wrapping_sub(4 * words),
        };
        if !(Transfer { words, need: Need::Read }).admitted(case, u64::from(first)) {
            return Ok(());
        }
        let loaded = read_word(cpu, u64::from(first.wrapping_add(4 * (list & 0x7fff).count_ones())))?;
        self.return_to(case, loaded)
    }

    fn on_access(&mut self, cpu: &dyn Cpu, case: &Case, access: Access, address: u64, size: i32, value: i64) -> Result<(), Step> {
        let (kind, need) = match access {
            Access::Read => (AccessType::Read, Need::Read),
            Access::Write => (AccessType::Write, Need::Write),
            Access::Fetch => {
                self.host_failure("memory hook reported a fetch".to_owned());
                return refuse(cpu);
            }
        };
        self.log(kind, address, size, value);
        let transfer = self.instruction.transfer.take();
        match self.admit(case, need, address, size, transfer) {
            Ok(true) if need == Need::Write && self.monitor.clobbered(transfer, address, size as u64) => {
                self.status.get_or_insert(Status::UnsupportedInstruction);
                return refuse(cpu);
            }
            Ok(true) => {}
            Ok(false) => {
                self.status.get_or_insert(Status::MemoryGuard);
                return refuse(cpu);
            }
            Err(message) => {
                self.host_failure(message);
                return refuse(cpu);
            }
        }
        if self.entered == 0 {
            return Ok(());
        }
        let (kind, observed) = if need == Need::Write {
            (ProvenanceKind::Write, value as u64)
        } else {
            let mut bytes = [0u8; 8];
            cpu.read_memory(address, &mut bytes[..size as usize])?;
            (ProvenanceKind::Read, u64::from_le_bytes(bytes))
        };
        self.record_access(kind, address as u32, size, observed);
        Ok(())
    }

    /// Every access after the run's outcome is decided is refused.
    fn admit(&self, case: &Case, need: Need, address: u64, size: i32, transfer: Option<Transfer>) -> Result<bool, String> {
        let size = u64::try_from(size)
            .ok()
            .filter(|size| (1..=8).contains(size))
            .ok_or_else(|| format!("memory access of {size} bytes"))?;
        let transfer = match transfer {
            Some(transfer) if transfer.need != need => {
                return Err("the first access of a transfer has the wrong direction".to_owned());
            }
            Some(transfer) => transfer.admitted(case, address),
            None => true,
        };
        Ok(self.status.is_none() && transfer && case.admits(address, size, need))
    }

    fn on_fault(&mut self, fault: Fault, address: u64, size: i32, value: i64) {
        self.log(fault.into(), address, size, value);
        self.status.get_or_insert(match fault {
            Fault::FetchUnmapped | Fault::FetchProtected => Status::FetchGuard,
            Fault::ReadUnmapped | Fault::ReadProtected | Fault::WriteUnmapped | Fault::WriteProtected => Status::MemoryGuard,
        });
    }

    fn on_exception(&mut self, exception: arm::Exception) -> Result<(), Step> {
        let status = match exception {
            arm::Exception::DataAbort => Status::MemoryGuard,
            arm::Exception::PrefetchAbort => Status::FetchGuard,
            arm::Exception::UndefinedInstruction
            | arm::Exception::SupervisorCall
            | arm::Exception::Breakpoint
            | arm::Exception::Irq
            | arm::Exception::Fiq
            | arm::Exception::Unknown(_) => Status::UnsupportedInstruction,
        };
        Err(Step::Stop(status))
    }

    /// Resolves the run's status from the guard state and the backend's exit.
    fn conclude(&mut self, case: &Case, result: Result<Exit, ExecutionError>, elapsed: Duration) {
        match result {
            Ok(Exit::Stopped) => {
                if self.status.is_none() {
                    if elapsed >= case.wall.0 {
                        self.status = Some(Status::WallBudget);
                    } else {
                        self.host_failure("the backend stopped without a guard".to_owned());
                    }
                }
            }
            Ok(exit) => self.host_failure(format!("unexpected architectural exit {exit:?}")),
            Err(error) => {
                self.unicorn_error = unicorn_error(&error);
                match (self.status, &error) {
                    (None, ExecutionError::InvalidInstruction(message)) => {
                        self.status = Some(Status::UnsupportedInstruction);
                        self.detail = Some(message.clone());
                    }
                    (None | Some(Status::Returned), _) => {
                        self.status = None;
                        self.host_failure(format!("execution failed: {error}"));
                    }
                    (Some(_), _) => {
                        self.detail.get_or_insert_with(|| error.to_string());
                    }
                }
            }
        }
    }

    fn observe<'a>(&mut self, cpu: &dyn Cpu, case: &'a Case, elapsed: Duration, entry: FinalRegisters) -> Observation<'a> {
        let mut registers = entry;
        let mut failure = None;
        for (index, value) in registers.core.iter_mut().enumerate() {
            match read(cpu, arm::CORE[index]) {
                Ok(read) => *value = read,
                Err(error) => failure = Some(error),
            }
        }
        match (read(cpu, arm::CPSR), read(cpu, arm::PC)) {
            (Ok(cpsr), Ok(pc)) => {
                registers.cpsr = cpsr;
                registers.pc = pc;
            }
            (Err(error), _) | (_, Err(error)) => failure = Some(error),
        }
        if let Some(error) = failure {
            self.host_failure(format!("final register read failed: {error}"));
        }
        let mut memory = Vec::with_capacity(case.regions.len());
        for region in case.regions.iter() {
            let mut bytes = vec![0u8; region.span.bytes as usize];
            let bytes = match cpu.read_memory(u64::from(region.span.base), &mut bytes) {
                Ok(()) => Some(HexBytes(bytes)),
                Err(error) => {
                    self.host_failure(format!("final memory read failed: {error}"));
                    None
                }
            };
            memory.push(FinalRegion { address: Word(region.span.base), bytes });
        }
        let first = self.entered.saturating_sub(LOG_SIZE as u64);
        let first_access = self.access_total.saturating_sub(LOG_SIZE as u64);
        Observation {
            wire_version: 1,
            status: self.status.unwrap_or(Status::EngineError),
            detail: self.detail.take(),
            unicorn_error: self.unicorn_error,
            instructions: Count(self.entered),
            elapsed_ns: Count(elapsed.as_nanos() as u64),
            registers: registers.into(),
            memory,
            events: self
                .events
                .iter()
                .map(|&index| {
                    let stub = &case.stubs[index];
                    Event { address: Word(stub.address.0), inputs: Bindings(&stub.inputs), outputs: Bindings(&stub.outputs) }
                })
                .collect(),
            trace_total: Count(self.entered),
            trace: (first..self.entered).map(|index| self.trace[(index % LOG_SIZE as u64) as usize]).collect(),
            access_total: Count(self.access_total),
            accesses: (first_access..self.access_total)
                .map(|index| self.accesses[(index % LOG_SIZE as u64) as usize])
                .collect(),
            memory_provenance_total: Count(self.provenance_total),
            memory_provenance: std::mem::take(&mut self.provenance),
            kernel: case.profile.kernel().map(|_| KernelRegisters {
                tpidrurw: self.system.tpidrurw.map(|value| Word(value.0)),
                fpexc: self.system.fpexc.map(|value| Word(value.0)),
                reserved: self.reserved.map(Word),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// A32 control-flow and profile words

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Table {
    /// `LDR PC, [PC, Rm, LSL #2]`.
    PcRelative,
    /// `LDR PC, [Rn, Rm{, LSL|LSR|ASR #n}]`.
    Register,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Branch,
    BranchLink,
    BranchRegister(u32),
    BranchLinkRegister(u32),
    Dispatch(Table),
    PopSingleReturn,
    LoadMultipleReturn,
    UntrackedPcWrite,
    None,
}

/// Classifies a condition-passing A32 word with cond != 0b1111.
fn control(word: u32) -> Control {
    let top = word >> 25 & 7;
    let destination = word >> 12 & 15;
    let load = word & 1 << 20 != 0;
    if word & 0x0fff_ffff == 0x049d_f004 {
        return Control::PopSingleReturn;
    }
    match word & 0x0f00_0000 {
        0x0a00_0000 => return Control::Branch,
        0x0b00_0000 => return Control::BranchLink,
        _ => {}
    }
    match word & 0x0fff_fff0 {
        0x012f_ff10 => return Control::BranchRegister(word & 15),
        0x012f_ff30 => return Control::BranchLinkRegister(word & 15),
        0x012f_ff20 => return Control::UntrackedPcWrite,
        _ => {}
    }
    if top == 3 && load && destination == 15 && word & 0x0170_0010 == 0x0110_0000 && word & 15 != 15 {
        let base = word >> 16 & 15;
        let shift = word >> 5 & 3;
        if base == 15 && word & 1 << 23 != 0 && shift == 0 && word >> 7 & 31 == 2 {
            return Control::Dispatch(Table::PcRelative);
        }
        if base != 15 && shift != 3 {
            return Control::Dispatch(Table::Register);
        }
    }
    if top == 4 && load && word & 1 << 15 != 0 {
        return Control::LoadMultipleReturn;
    }
    let data_processing = top == 1 || (top == 0 && word & 0x90 != 0x90);
    let opcode = word >> 21 & 15;
    let misc = (8..=11).contains(&opcode) && !load;
    let compare = (8..=11).contains(&opcode) && load;
    let moves = top == 1 && word & 0x0fb0_0000 == 0x0300_0000;
    let pc_write = destination == 15
        && ((data_processing && !misc && !compare)
            || moves
            || ((top == 2 || top == 3) && load)
            || (top == 0 && word & 0x90 == 0x90 && word & 0x60 != 0 && load));
    let doubleword_into_pc = top == 0 && word & 0xf0 == 0xd0 && !load && destination == 14;
    // MRS, CLZ and QADD/QSUB/QDADD/QDSUB with Rd = PC.
    let misc_into_pc = word & 0x0fbf_ffff == 0x010f_f000 || word & 0x0fff_fff0 == 0x016f_ff10 || word & 0x0f90_f0f0 == 0x0100_f050;
    // SMULxy/SMLAxy/SMULWy/SMLAWy write Rd at bits 19-16; SMLALxy also writes RdLo.
    let halfword_multiply_into_pc = word & 0x0f90_0090 == 0x0100_0080
        && (word >> 16 & 15 == 15 || (word & 0x0060_0000 == 0x0040_0000 && destination == 15));
    // Media signed multiplies and USAD8/USADA8 write Rd at bits 19-16; the rest at bits 15-12.
    let media_into_pc = top == 3
        && word & 0x10 != 0
        && if word & 0x0f80_0000 == 0x0700_0000 || word & 0x0ff0_00f0 == 0x0780_0010 {
            word >> 16 & 15 == 15
        } else {
            destination == 15
        };
    if pc_write || doubleword_into_pc || misc_into_pc || halfword_multiply_into_pc || media_into_pc {
        Control::UntrackedPcWrite
    } else {
        Control::None
    }
}

/// Words refused regardless of condition: Cortex-R4F exclusions, ARMv7
/// UNPREDICTABLE transfer and multiply forms, and the user-ARM case profile.
fn excluded(word: u32) -> bool {
    let unconditional = word >> 28 == 15;
    let coprocessor_or_supervisor = word >> 25 & 7 >= 6;
    // MSR immediate with an empty mask is the hint space (NOP, YIELD, WFE, WFI, SEV, DBG).
    let register_status_write = word & 0x0fb0_fff0 == 0x0120_f000;
    let user_flags = matches!(word & 0x0fff_fff0, 0x0128_f000 | 0x012c_f000) && word & 15 != 15;
    let status_write_or_hint = word & 0x0fb0_f000 == 0x0320_f000 || (register_status_write && !user_flags);
    let synchronization = word & 0x0f00_00f0 == 0x0100_0090;
    unconditional
        || coprocessor_or_supervisor
        || status_write_or_hint
        || synchronization
        || unpredictable(word)
        || r4f::classify_a32(word).is_some()
}

/// Words a privileged profile refuses beyond `excluded`: MRS SPSR, banked
/// MRS/MSR, LDM/STM with the user-bank bit and ERET.
fn privileged_refused(word: u32) -> bool {
    let spsr_read = word & 0x0fff_0fff == 0x014f_0000;
    let banked = word & 0x0fb0_0eff == 0x0100_0200 || word & 0x0fb0_fef0 == 0x0120_f200;
    let user_bank = word >> 25 & 7 == 4 && word & 1 << 22 != 0;
    let exception_return = word & 0x0fff_ffff == 0x0160_006e;
    spsr_read || banked || user_bank || exception_return
}

/// ARMv7 UNPREDICTABLE or UNKNOWN-result encodings of the transfer and multiply forms.
fn unpredictable(word: u32) -> bool {
    let top = word >> 25 & 7;
    let (rn, rt, rs, rm) = (word >> 16 & 15, word >> 12 & 15, word >> 8 & 15, word & 15);
    let load = word & 1 << 20 != 0;
    let writeback = word & 1 << 24 == 0 || word & 1 << 21 != 0;
    let writeback_conflict = |transferred: &[u32]| writeback && (rn == 15 || transferred.contains(&rn));
    match top {
        2 | 3 if top == 2 || word & 0x10 == 0 => {
            let byte = word & 1 << 22 != 0;
            writeback_conflict(&[rt]) || (top == 3 && rm == 15) || (byte && rt == 15)
        }
        0 if word & 0x90 == 0x90 && word & 0x60 != 0 => {
            let register = word & 1 << 22 == 0;
            if !load && word & 0x40 != 0 {
                let load_dual = word & 0x20 == 0;
                rt == 14
                    || word & 0x0120_0000 == 0x0020_0000
                    || writeback_conflict(&[rt, rt + 1])
                    || (register && (rm == 15 || (load_dual && (rm == rt || rm == rt + 1))))
            } else {
                rt == 15 || writeback_conflict(&[rt]) || (register && rm == 15)
            }
        }
        0 if word & 0x0f00_00f0 == 0x0000_0090 => {
            let accumulates = word & 0x00e0_0000 != 0;
            let pair = word & 0x0080_0000 != 0 || word & 0x00f0_0000 == 0x0040_0000;
            [rn, rs, rm].contains(&15) || (accumulates && rt == 15) || (pair && rn == rt)
        }
        4 => {
            let list = word & 0xffff;
            let base_listed = word & 1 << 21 != 0 && list & 1 << rn != 0;
            list == 0 || rn == 15 || (base_listed && (load || list & list.wrapping_neg() != 1 << rn))
        }
        _ => false,
    }
}

fn branch_target(pc: u32, word: u32) -> u32 {
    pc.wrapping_add(8).wrapping_add((((word << 8) as i32) >> 6) as u32)
}

/// The ARM-state target of BX/BLX Rm.
fn arm_target(cpu: &dyn Cpu, register: u32) -> Result<u32, Step> {
    if register == 15 {
        return Err(Step::Stop(Status::UnsupportedInstruction));
    }
    let target = read(cpu, arm::CORE[register as usize])?;
    if target & 3 != 0 {
        return Err(Step::Stop(Status::UnsupportedInstruction));
    }
    Ok(target)
}

/// The address `ldr pc, [Rn, #+/-imm12]` reads.
fn pc_load_address(cpu: &dyn Cpu, pc: u32, word: u32) -> Result<u32, Step> {
    let base = match word >> 16 & 15 {
        15 => pc.wrapping_add(8),
        register => read(cpu, arm::CORE[register as usize])?,
    };
    let offset = word & 0xfff;
    Ok(if word & 1 << 23 != 0 { base.wrapping_add(offset) } else { base.wrapping_sub(offset) })
}
