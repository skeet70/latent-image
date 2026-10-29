//! Bounded native cases on the pinned FujiMod Cortex-R5F core with the
//! Cortex-R4F exclusions of `r4f::classify_a32`. Input is the
//! `FUJI_NATIVE_V1` wire written by `Emulation.NativeCase`; output is the
//! runner observation read by `Emulation.NativeObservation`.
//!
//! Engine pages carry the union of the permissions of the regions they touch.
//! The memory hook admits each access against exact logical regions and
//! alignment, and refuses any other through `Cpu::refuse_access` before the
//! access happens. At the first access of an LDM/STM or an immediate
//! pre-indexed LDRD/STRD it admits the instruction's whole transfer, so a
//! refused transfer has no effect. Unmapped accesses fault and are refused.
//!
//! The case's call contract is tracked from the decoded control-flow words
//! (B, BL, BX, BLX, LDM with PC and the two LDR-to-PC dispatch forms). All
//! other instruction semantics belong to the backend, which executes the
//! Cortex-R5F instruction set. The user-ARM case profile refuses coprocessor,
//! supervisor-call, unconditional-space, status-register write, hint and
//! synchronization-primitive words, and ARMv7 UNPREDICTABLE transfer and
//! multiply forms.

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
const MAX_REGIONS: u32 = 16;
const MAX_REGION_BYTES: u32 = 2 * 1024 * 1024;
const MAX_MAPPED_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FUNCTIONS: u32 = 256;
const MAX_STUBS: u32 = 32;
const MAX_INSTRUCTIONS: u32 = 4_000_000;
const MAX_WALL_MS: u32 = 10_000;
/// Engine limit on recorded stub events, separate from the 128 expected events.
const MAX_RECORDED_EVENTS: usize = 512;
const MAX_DEPTH: usize = 32;
const LOG_SIZE: usize = 64;
const PROVENANCE_SIZE: usize = 4096;
const PAGE: u64 = 4096;
const TRANSLATION_CACHE_BYTES: u32 = 4 * 1024 * 1024;
/// The PMSAv7 default map is execute-never from 0x80000000.
const EXECUTABLE_END: u64 = 0x7fff_fffc;
const USER_MODE: u32 = 0x10;
const CPSR_MODE: u32 = 0x1f;
const CPSR_THUMB: u32 = 1 << 5;

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
    let observation = match &case {
        Ok(case) => execute(case),
        Err(error) => Observation::invalid(*error),
    };
    match serde_json::to_vec(&observation) {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Permission {
    Read,
    ReadWrite,
    ReadExecute,
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
            Need::Read => true,
            Need::Write => self == Self::ReadWrite,
            Need::Execute => self == Self::ReadExecute,
        }
    }

    fn page_bits(self) -> u32 {
        match self {
            Self::Read => PAGE_READ,
            Self::ReadWrite => PAGE_READ | PAGE_WRITE,
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

/// User-mode ARM CPSR: mode 0x10 with explicit flags only.
#[derive(Clone, Copy, Debug)]
struct Cpsr(u32);

/// r0-r14 at entry; r13 is SP and r14 is LR.
#[derive(Clone, Copy, Debug)]
struct EntryRegisters {
    core: [u32; 15],
    cpsr: Cpsr,
}

#[derive(Clone, Copy, Debug)]
struct InstructionBudget(u32);

#[derive(Clone, Copy, Debug)]
struct WallBudget(Duration);

struct Case {
    entry: Address,
    exit: Address,
    registers: EntryRegisters,
    budget: InstructionBudget,
    wall: WallBudget,
    regions: Box<[Region]>,
    functions: Box<[Span]>,
    stubs: Box<[Stub]>,
}

impl Case {
    /// Containment of the whole range in one region granting `need`.
    fn covers(&self, address: u64, bytes: u64, need: Need) -> bool {
        self.regions
            .iter()
            .any(|region| region.permission.grants(need) && region.span.contains(address, bytes))
    }

    /// One guest access: covered and, wider than a byte, naturally aligned.
    fn admits(&self, address: u64, size: u64, need: Need) -> bool {
        (size <= 1 || address % size == 0) && self.covers(address, size, need)
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
        let token = self.next()?;
        if token.is_empty() || token.len() > 8 {
            return Err(WireError("hexadecimal word must have one to eight digits"));
        }
        token.iter().try_fold(0u32, |value, &digit| {
            nibble(digit)
                .map(|digit| value << 4 | u32::from(digit))
                .ok_or(WireError("hexadecimal word must be lowercase hex"))
        })
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
}

fn nibble(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

fn parse(wire: &[u8]) -> Result<Case, WireError> {
    if wire.len() > MAX_WIRE_BYTES {
        return Err(WireError("wire exceeds its byte limit"));
    }
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
    if cpsr & 0x0fff_ffff != USER_MODE {
        return Err(WireError("CPSR must be user-mode ARM with flags only"));
    }
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
        let permission = match tokens.count(7)? {
            1 => Permission::Read,
            3 => Permission::ReadWrite,
            5 => Permission::ReadExecute,
            _ => return Err(WireError("region permission must be 1, 3 or 5")),
        };
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
        let initial = tokens.bytes(bytes)?;
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

    let case = Case {
        entry,
        exit,
        registers: EntryRegisters { core, cpsr: Cpsr(cpsr) },
        budget: InstructionBudget(budget),
        wall: WallBudget(Duration::from_millis(u64::from(wall_ms))),
        regions: regions.into(),
        functions: functions.into(),
        stubs: stubs.into(),
    };
    validate(&case)?;
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
    Ok(())
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
    memory_provenance: Vec<ProvenanceEntry>,
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

/// The whole span of an LDM/STM or immediate pre-indexed LDRD/STRD, admitted
/// at its lowest address, which the engine accesses first.
#[derive(Clone, Copy)]
struct Transfer {
    bytes: u64,
    need: Need,
}

impl Transfer {
    fn of(word: u32) -> Option<Self> {
        let load = |bit: bool| if bit { Need::Read } else { Need::Write };
        if word >> 25 & 7 == 4 {
            return Some(Self { bytes: 4 * u64::from((word & 0xffff).count_ones()), need: load(word & 1 << 20 != 0) });
        }
        (word & 0x0f50_00d0 == 0x0140_00d0).then(|| Self { bytes: 8, need: load(word & 0x20 == 0) })
    }
}

#[derive(Clone, Copy)]
struct Instruction {
    pc: u32,
    word: u32,
    transfer: Option<Transfer>,
}

struct State {
    status: Option<Status>,
    detail: Option<String>,
    unicorn_error: u32,
    started: Instant,
    entered: u64,
    current: usize,
    frames: [Frame; MAX_DEPTH],
    depth: usize,
    returning: bool,
    events: Vec<usize>,
    trace: [TraceEntry; LOG_SIZE],
    accesses: [AccessEntry; LOG_SIZE],
    access_total: u64,
    provenance: Vec<ProvenanceEntry>,
    provenance_total: u64,
    instruction: Instruction,
}

impl State {
    fn new(case: &Case) -> Self {
        Self {
            status: None,
            detail: None,
            unicorn_error: 0,
            started: Instant::now(),
            entered: 0,
            current: case.function_at(case.entry.0).unwrap_or(0),
            frames: [Frame::default(); MAX_DEPTH],
            depth: 0,
            returning: false,
            events: Vec::with_capacity(MAX_RECORDED_EVENTS),
            trace: [TraceEntry::default(); LOG_SIZE],
            accesses: [AccessEntry::default(); LOG_SIZE],
            access_total: 0,
            provenance: Vec::with_capacity(PROVENANCE_SIZE),
            provenance_total: 0,
            instruction: Instruction { pc: 0, word: 0, transfer: None },
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

    fn on_instruction(&mut self, cpu: &dyn Cpu, case: &Case, address: u64, size: u32) -> Result<(), Step> {
        if self.status.is_some() {
            return Ok(());
        }
        if self.started.elapsed() >= case.wall.0 {
            return Err(Step::Stop(Status::WallBudget));
        }
        let cpsr = read(cpu, arm::CPSR)?;
        if cpsr & CPSR_THUMB != 0 || cpsr & CPSR_MODE != USER_MODE {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        if size != 4 || !case.admits(address, 4, Need::Execute) {
            return Err(Step::Stop(Status::FetchGuard));
        }
        let pc = address as u32;
        if pc == case.exit.0 {
            let status = if self.returning && self.depth == 0 { Status::Returned } else { Status::UnknownCall };
            return Err(Step::Stop(status));
        }
        if !case.functions[self.current].contains(address, 4) {
            return Err(Step::Stop(Status::UnknownCall));
        }
        if self.entered >= u64::from(case.budget.0) {
            return Err(Step::Stop(Status::InstructionBudget));
        }
        let word = read_word(cpu, address)?;
        self.trace[(self.entered % LOG_SIZE as u64) as usize] = TraceEntry { pc: Word(pc), instruction: Word(word) };
        self.entered += 1;
        self.instruction = Instruction { pc, word, transfer: None };
        if excluded(word) {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        if !arm::condition_passes(word >> 28, cpsr) {
            return Ok(());
        }
        self.instruction.transfer = Transfer::of(word);
        self.track(cpu, case, pc, word)
    }

    /// Applies the call contract of one condition-passing word before it executes.
    fn track(&mut self, cpu: &dyn Cpu, case: &Case, pc: u32, word: u32) -> Result<(), Step> {
        let next = pc.wrapping_add(4);
        match control(word) {
            Control::BranchLink => self.call(cpu, case, branch_target(pc, word)?, next),
            Control::Branch => self.tail(cpu, case, branch_target(pc, word)?),
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
            Control::LoadMultipleReturn => self.load_multiple_return(cpu, case, word),
            Control::UntrackedPcWrite => Err(Step::Stop(Status::UnsupportedInstruction)),
            Control::None => Ok(()),
        }
    }

    fn call(&mut self, cpu: &dyn Cpu, case: &Case, target: u32, return_address: u32) -> Result<(), Step> {
        if let Some(index) = case.stub_at(target) {
            self.apply_stub(cpu, &case.stubs[index], index)?;
            write(cpu, arm::CORE[14], return_address)?;
            write(cpu, arm::PC, return_address)?;
            return Ok(());
        }
        let Some(function) = case.function_at(target) else {
            return Err(Step::Stop(Status::UnknownCall));
        };
        if self.depth == MAX_DEPTH {
            return Err(Step::Stop(Status::UnknownCall));
        }
        self.frames[self.depth] = Frame { return_address, caller: self.current };
        self.depth += 1;
        self.current = function;
        Ok(())
    }

    /// B, BX or a dispatch: stays in or changes function; a stub returns to LR.
    fn tail(&mut self, cpu: &dyn Cpu, case: &Case, target: u32) -> Result<(), Step> {
        if case.functions[self.current].contains(u64::from(target), 4) {
            return Ok(());
        }
        if let Some(function) = case.function_at(target) {
            self.current = function;
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
        match self.depth {
            0 => case.exit.0,
            depth => self.frames[depth - 1].return_address,
        }
    }

    fn return_to(&mut self, case: &Case, target: u32) -> Result<(), Step> {
        if target != self.expected_return(case) {
            return Err(Step::Stop(Status::UnknownCall));
        }
        match self.depth {
            0 => self.returning = true,
            depth => {
                self.current = self.frames[depth - 1].caller;
                self.depth -= 1;
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
        self.events.push(index);
        for binding in stub.outputs.iter() {
            write(cpu, binding.register.register(), binding.value)?;
        }
        Ok(())
    }

    fn dispatch(&mut self, cpu: &dyn Cpu, case: &Case, pc: u32, word: u32, table: Table) -> Result<(), Step> {
        let index = read(cpu, arm::CORE[(word & 15) as usize])?;
        let entry = match table {
            Table::PcRelative => u64::from(pc + 8) + (u64::from(index) << 2),
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
                if word & 1 << 23 != 0 {
                    u64::from(base) + u64::from(offset)
                } else {
                    u64::from(base.wrapping_sub(offset))
                }
            }
        };
        if !case.admits(entry, 4, Need::Read) {
            self.log(AccessType::Read, entry, 4, 0);
            return Err(Step::Stop(Status::MemoryGuard));
        }
        let target = read_word(cpu, entry)?;
        if target & 3 != 0 {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        self.tail(cpu, case, target)
    }

    /// LDM with PC: the loaded PC must be the tracked return address.
    fn load_multiple_return(&mut self, cpu: &dyn Cpu, case: &Case, word: u32) -> Result<(), Step> {
        let base_register = word >> 16 & 15;
        let list = word & 0xffff;
        if base_register == 15 || (word & 1 << 21 != 0 && list & 1 << base_register != 0) {
            return Err(Step::Stop(Status::UnsupportedInstruction));
        }
        let base = u64::from(read(cpu, arm::CORE[base_register as usize])?);
        let words = u64::from(list.count_ones());
        let before = u64::from((list & 0x7fff).count_ones());
        let increment = word & 1 << 23 != 0;
        let before_transfer = word & 1 << 24 != 0;
        let first = if increment {
            base.checked_add(if before_transfer { 4 } else { 0 })
        } else {
            base.checked_sub(4 * words - if before_transfer { 0 } else { 4 })
        };
        let Some(first) = first.filter(|&first| first % 4 == 0 && case.covers(first, 4 * words, Need::Read)) else {
            self.log(AccessType::Read, first.unwrap_or(base), (4 * words) as i32, 0);
            return Err(Step::Stop(Status::MemoryGuard));
        };
        let loaded = read_word(cpu, first + 4 * before)?;
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
        match self.admit(case, need, address, size) {
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
        if self.provenance.len() < PROVENANCE_SIZE {
            self.provenance.push(ProvenanceEntry {
                pc: Word(self.instruction.pc),
                instruction: Word(self.instruction.word),
                kind,
                address: Word(address as u32),
                size,
                value: Wide(observed),
            });
        }
        self.provenance_total += 1;
        Ok(())
    }

    /// Every access after the run's outcome is decided is refused.
    fn admit(&mut self, case: &Case, need: Need, address: u64, size: i32) -> Result<bool, String> {
        let size = u64::try_from(size)
            .ok()
            .filter(|size| (1..=8).contains(size))
            .ok_or_else(|| format!("memory access of {size} bytes"))?;
        let transfer = match self.instruction.transfer.take() {
            Some(transfer) if transfer.need != need => {
                return Err("the first access of a transfer has the wrong direction".to_owned());
            }
            Some(transfer) => case.covers(address, transfer.bytes, need),
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
    LoadMultipleReturn,
    UntrackedPcWrite,
    None,
}

/// Classifies a condition-passing A32 word with cond != 0b1111.
fn control(word: u32) -> Control {
    let top = word >> 25 & 7;
    let destination = word >> 12 & 15;
    let load = word & 1 << 20 != 0;
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
    if pc_write || doubleword_into_pc || misc_into_pc { Control::UntrackedPcWrite } else { Control::None }
}

/// Words refused regardless of condition: Cortex-R4F exclusions, ARMv7
/// UNPREDICTABLE transfer and multiply forms, and the user-ARM case profile.
fn excluded(word: u32) -> bool {
    let unconditional = word >> 28 == 15;
    let coprocessor_or_supervisor = word >> 25 & 7 >= 6;
    // MSR immediate with an empty mask is the hint space (NOP, YIELD, WFE, WFI, SEV, DBG).
    let status_write_or_hint = word & 0x0fb0_f000 == 0x0320_f000 || word & 0x0fb0_fff0 == 0x0120_f000;
    let synchronization = word & 0x0f00_00f0 == 0x0100_0090;
    unconditional
        || coprocessor_or_supervisor
        || status_write_or_hint
        || synchronization
        || unpredictable(word)
        || r4f::classify_a32(word).0.is_some()
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

fn branch_target(pc: u32, word: u32) -> Result<u32, Step> {
    let offset = i64::from(((word << 8) as i32) >> 6);
    u32::try_from(i64::from(pc) + 8 + offset).map_err(|_| Step::Stop(Status::UnknownCall))
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
