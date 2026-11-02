use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{CpuDataAttribution, FetchAttribution, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::machine::{Machine, RunOutcome};
use fujimod_xm1::provenance::Guest;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::num::{NonZeroU16, NonZeroUsize};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

#[path = "startup_consumers_shared.rs"]
pub(crate) mod shared;

const USAGE: &str = "startup-consumers --role retained|installer|joined --run RUN --update DAT --canonical-dat DAT --boot-rom ROM --analysis ROOT --installer-image BIN --installer-symbols SYMBOLS --installer-contract CONTRACT --dispatcher-image BIN --dispatcher-symbols SYMBOLS --dispatcher-contract CONTRACT --joined-contract CONTRACT --output NEW_DIR";
const BOOT_LIMIT: u64 = 8_000_000;
const LIMITS: Limits = Limits {
    instructions: 80_000_000, segment_instructions: 256, scoped_segment_instructions: InstructionBudget(4096), watch_records: 65_536,
    probes: 131_072, resources: 512, resource_bytes: 4_194_304, invocations: 64,
    records: 1_048_576, snapshot_bytes: 134_217_728, snapshots: 65_536, segments: 32_768,
};

macro_rules! scalar {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        struct $name($inner);
    };
}
scalar!(Address, u32);
scalar!(Word, u32);
scalar!(StackBytes, u32);
scalar!(CoreInstructions, u64);
scalar!(InstructionBudget, u64);
scalar!(InvocationId, u64);
scalar!(ResourceId, u64);
scalar!(ProbeSequence, u64);
scalar!(SegmentId, u64);
#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
struct Bytes(Vec<u8>);
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String")]
struct Digest(String);
impl TryFrom<String> for Digest {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        require(value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "invalid SHA256")?;
        Ok(Self(value))
    }
}
fn digest(bytes: &[u8]) -> Digest { Digest(sha256(bytes)) }
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "usize", into = "usize")]
enum Core { Primary, Secondary }
impl TryFrom<usize> for Core {
    type Error = String;
    fn try_from(value: usize) -> Result<Self, String> {
        match value { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("expected one of two cores".into()) }
    }
}
impl From<Core> for usize {
    fn from(value: Core) -> usize { match value { Core::Primary => 0, Core::Secondary => 1 } }
}
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Retained, Installer, Joined }
#[derive(Clone, Copy)]
enum KernelDomain { Primary, Secondary }
impl KernelDomain {
    fn task_target(target: Address) -> Result<Self, String> {
        match target.0 {
            0x4d39c | 0x4d094 | 0x44f2c => Ok(Self::Primary),
            0xdd360 | 0xdd054 | 0xd4eec => Ok(Self::Secondary),
            _ => Err("task service target has no admitted kernel layout".into()),
        }
    }
    fn message_target(target: Address) -> Result<Self, String> {
        match target.0 {
            0x411cc | 0x49308 | 0x49528 | 0x4a56c | 0x4bab8 => Ok(Self::Primary),
            0xd11cc | 0xd92c8 | 0xd94e8 | 0xda52c | 0xdba78 => Ok(Self::Secondary),
            _ => Err("message service target has no admitted kernel layout".into()),
        }
    }
    fn domains(self) -> Address {
        Address(match self { Self::Primary => 0x400545cc, Self::Secondary => 0x400e458c })
    }
}
#[derive(Clone, Copy)]
enum DomainSelection { Current, First, Second }
#[derive(Clone, Copy)]
struct MessageIndex(NonZeroU16);
struct MessageReference { selection: DomainSelection, index: MessageIndex }
impl MessageReference {
    fn parse(encoded: Word) -> Option<Self> {
        let selection = match (encoded.0 as i32) >> 16 {
            0 => DomainSelection::Current, 1 => DomainSelection::First, 2 => DomainSelection::Second,
            _ => return None,
        };
        let index = encoded.0 as u16 as i16;
        if index <= 0 { return None; }
        Some(Self { selection, index: MessageIndex(NonZeroU16::new(index as u16)?) })
    }
}
#[derive(Clone, Copy)]
enum MessageAccess { Acquire, Submit, Receive }
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Family { TaskWrapper, TaskReserve, StorageReserve, MessageConstruct, MessageSend, MessageReceive }
const FAMILIES: [Family; 6] = [Family::TaskWrapper, Family::TaskReserve, Family::StorageReserve, Family::MessageConstruct, Family::MessageSend, Family::MessageReceive];
impl Family {
    fn entry(self) -> u32 {
        match self {
            Self::TaskWrapper => 0x725dd8, Self::TaskReserve => 0x725c6c,
            Self::StorageReserve => 0x723470, Self::MessageConstruct => 0x724cdc,
            Self::MessageSend => 0x72503c, Self::MessageReceive => 0x725318,
        }
    }
    fn layout(self) -> FrameLayout {
        let (saved, locals) = match self {
            Self::TaskWrapper => (24,32),
            Self::TaskReserve | Self::StorageReserve => (32,0),
            Self::MessageConstruct => (36,36),
            Self::MessageSend => (24,8),
            Self::MessageReceive => (32,8),
        };
        FrameLayout { saved: StackBytes(saved), locals: StackBytes(locals) }
    }
    fn epilogue(self) -> Address {
        Address(match self {
            Self::TaskWrapper => 0x726094, Self::TaskReserve => 0x725dcc,
            Self::StorageReserve => 0x7235cc, Self::MessageConstruct => 0x724f04,
            Self::MessageSend => 0x725210, Self::MessageReceive => 0x725420,
        })
    }
    fn error_continuations(self) -> &'static [u32] {
        match self {
            Self::TaskWrapper => &[0x725eac,0x725fe4,0x726058,0x72608c],
            Self::MessageConstruct => &[0x724d50,0x724dd4,0x724e64,0x724efc],
            Self::MessageSend => &[0x7250b4,0x72513c,0x725208],
            Self::MessageReceive => &[0x7253b8],
            Self::TaskReserve | Self::StorageReserve => &[],
        }
    }
    fn points(self) -> &'static [u32] {
        match self {
            Self::TaskWrapper => &[
                0x725df8,0x725e1c,0x725e34,0x725e50,0x725e74,0x725e78,
                0x725f08,0x725f0c,0x725f68,0x725f6c,0x725f78,0x725f7c,
                0x725fa8,0x725fac,0x725fb4,0x725fb8,0x726000,0x726004,
                0x726024,0x726028,0x726094,
            ],
            Self::TaskReserve => &[
                0x725c94,0x725c98,0x725ca4,0x725ca8,0x725cc0,0x725cc4,
                0x725cd4,0x725cf0,0x725d0c,0x725d1c,0x725d30,0x725d44,
                0x725d54,0x725d64,0x725d7c,0x725d94,0x725d98,0x725da0,
                0x725dac,0x725db0,0x725dc4,0x725dc8,0x725dcc,
            ],
            Self::StorageReserve => &[
                0x723498,0x72349c,0x7234a8,0x7234ac,0x7234c4,0x7234c8,
                0x7234d8,0x7234f4,0x723510,0x723520,0x723534,0x723548,
                0x723558,0x723564,0x72357c,0x723594,0x723598,0x7235a0,
                0x7235ac,0x7235b0,0x7235c4,0x7235c8,0x7235cc,
            ],
            Self::MessageConstruct => &[
                0x724d00,0x724d20,0x724d24,0x724d70,0x724d98,0x724d9c,
                0x724de4,0x724dec,0x724e04,0x724e20,0x724e34,0x724e38,0x724ea4,0x724ea8,
                0x724ee4,0x724ee8,0x724f04,
            ],
            Self::MessageSend => &[
                0x725064,0x725068,0x725078,0x725084,0x7250c4,0x7250cc,
                0x7250d0,0x725100,0x725104,0x72514c,0x725154,0x72515c,
                0x725164,0x72517c,0x725180,0x725190,0x725194,0x7251b4,
                0x7251b8,0x7251f0,0x7251f4,0x725210,
            ],
            Self::MessageReceive => &[
                0x725344,0x725348,0x725378,0x72537c,0x7253c0,0x7253c8,
                0x7253d0,0x7253d8,0x7253e0,0x7253f8,0x7253fc,
                0x725414,0x725418,0x725420,
            ],
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProbeKind {
    OuterEntry, OuterReturn, InstallerEntry, InstallerReturn, CacheEntry, CacheReturn,
    CompletionStore, CompletionBeforeDsb, CompletionAfterDsb, PeerPoll, PeerPostload,
    PeerBranch, InitializerCall, InitializerEntry, InitializerReturn, CriticalEntry,
    CriticalIdentityCall, CriticalIdentityReturn, CriticalRootLoad, CriticalSlotLoad,
    CriticalCall, CriticalReturn, FamilyEntry, Source, DynamicTarget, DynamicReturn, FamilyReturn,
}
#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ResourceKind {
    Roots, RootObject, ServiceStorage, TaskStorage, Stack, Code, Completion, Descriptor,
    Record, Cursor, Output, Auxiliary, Payload, Buffer, CallbackCell, ErrorState, Header, Carrier,
}
#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Lifetime { Observation, Invocation, Allocation, ReceiveRelease }
#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Status { Returned, NotObserved, Unclosed }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome { Observed, NonObservation, Refused }
#[derive(Clone, Serialize)]
struct Context {
    core: Core, pc: Address, registers: [Word; 13], sp: Address, lr: Address, cpsr: Word,
    counts: [u64; 2], tcm: [[Word; 2]; 2], identities: [Word; 2],
}
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Source { CpuRead, CpuWrite, CpuFetch, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ByteOrder { Little, Big }
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FetchSource { CpuFetch }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LatestFetch {
    source: FetchSource, address: u64, physical: Option<u64>, size: u64,
    value: Option<u64>, before: Option<Bytes>, byte_order: Option<ByteOrder>,
    core: Option<Core>, pc: Option<Address>, instructions: Option<u64>, tick: Option<u64>,
    core_instructions: CoreInstructions,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MemoryEvent {
    source: Source, address: u64, physical: Option<u64>, size: u64,
    value: Option<u64>, before: Option<Bytes>, byte_order: Option<ByteOrder>,
    core: Option<Core>, pc: Option<Address>, instructions: Option<u64>, tick: Option<u64>,
    #[serde(default)]
    core_instructions: Option<CoreInstructions>,
    #[serde(default)]
    latest_fetch: Option<LatestFetch>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WatchClass { range: [u64; 2], kinds: Vec<Source> }
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ViewsPolicy { Fixed }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WatchState {
    enabled: bool, ranges: Vec<[u64; 2]>, classes: Vec<WatchClass>, views: ViewsPolicy,
    fetch_attribution: AttributionConfig,
    records: Vec<MemoryEvent>, violation: Option<String>,
}
#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CpuDataSource { CpuRead, CpuWrite }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AttributionConfig { sources: Vec<CpuDataSource>, ranges: Vec<FetchRange> }
#[derive(Serialize)]
struct View { core: Core, guest: Address, physical: Address, bytes: Bytes }
#[derive(Serialize)]
struct Resource {
    id: ResourceId, kind: ResourceKind, lifetime: Lifetime, owner: Option<InvocationId>,
    physical: Address, size: u64, admitted: Context, initial: Vec<View>,
    closed: Option<Context>, final_views: Vec<View>,
}
#[derive(Serialize)]
struct Snapshot { resource: ResourceId, views: Vec<View> }
#[derive(Serialize)]
struct Probe {
    sequence: ProbeSequence, kind: ProbeKind, family: Option<Family>, invocation: Option<InvocationId>,
    context: Context, source_word: Word, snapshots: Vec<Snapshot>,
}
#[derive(Serialize)]
struct Invocation {
    id: InvocationId, family: Family, core: Core, entry_probe: Option<ProbeSequence>,
    return_probe: Option<ProbeSequence>, parent: Option<InvocationId>, status: Status,
    entry_lr: Address, entry_sp: Address,
}
#[derive(Serialize)]
struct Census { family: Family, core: Core, entries: u64, first_probe: Option<ProbeSequence> }
#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FetchScope { Complete, Scoped }
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "[u64; 2]", into = "[u64; 2]")]
struct FetchRange([u64; 2]);
impl TryFrom<[u64; 2]> for FetchRange {
    type Error = String;
    fn try_from(range: [u64; 2]) -> Result<Self, String> {
        require(range[0] < range[1] && range[1] <= 1u64 << 32, "invalid physical fetch span")?;
        Ok(Self(range))
    }
}
impl From<FetchRange> for [u64; 2] {
    fn from(range: FetchRange) -> Self { range.0 }
}
struct WatchPlan {
    classes: Vec<WatchRange>, fetch_scope: FetchScope, fetch_ranges: Vec<FetchRange>,
    armed_sites: Vec<Address>,
}
#[derive(Serialize)]
struct StopPoint { core: Core, pc: Address, instructions: u64 }
#[derive(Serialize)]
struct Segment {
    id: SegmentId, start_counts: [u64; 2], end_counts: [u64; 2],
    start_contexts: [Context; 2], end_contexts: [Context; 2],
    fetch_scope: FetchScope, fetch_ranges: Vec<FetchRange>, armed_sites: Vec<Address>,
    probe_range: [ProbeSequence; 2],
    watch: WatchState, end_error: Option<String>, stops: Vec<StopPoint>,
}
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum FetchClassification { BoundaryAttempt }
#[derive(Serialize)]
struct FetchAttempt {
    segment: SegmentId, row: u64, classification: FetchClassification,
}
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum PayloadState { Empty, Admitted }
#[derive(Serialize)]
struct PayloadAdmission {
    owner: InvocationId, context: Context, pointer: Address, length: u64,
    state: PayloadState, resource: Option<ResourceId>,
}
#[derive(Clone, Copy, Serialize)]
struct Limits {
    instructions: u64, segment_instructions: u64, watch_records: u64,
    scoped_segment_instructions: InstructionBudget,
    probes: u64, resources: u64, resource_bytes: u64, invocations: u64,
    records: u64, snapshot_bytes: u64, snapshots: u64, segments: u64,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ComponentKind { Installer, Dispatcher }
#[derive(Serialize)]
struct ComponentBinding { kind: ComponentKind, image: Digest, symbols: Digest, contract: Digest }
#[derive(Serialize)]
struct Identity {
    run: Digest, update: Digest, canonical_dat: Digest, image4: Digest, loader_a: Digest,
    loader_b: Digest, boot_rom: Digest, observer: Digest, joined_contract: Digest,
    components: Vec<ComponentBinding>,
}
#[derive(Serialize)]
struct Receipt {
    schema: u32, role: Role, outcome: Outcome, error: Option<String>, machine_failure: Option<String>,
    identity: Identity, checkpoint_sha256: Digest, checkpoint_reused: bool, instructions: u64,
    limits: Limits, probes: Vec<Probe>, invocations: Vec<Invocation>, resources: Vec<Resource>,
    segments: Vec<Segment>, census: Vec<Census>, fetch_attempts: Vec<FetchAttempt>,
    payloads: Vec<PayloadAdmission>,
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}
fn reg(machine: &Machine, core: Core, register: arm::Register) -> Result<u32, String> {
    u32::try_from(machine.session.board.read_register(core.into(), Register::Arm32(register))?).map_err(|_| "register exceeds word".into())
}
fn counts(machine: &Machine) -> Result<[u64; 2], String> {
    machine.session.board.instruction_counts().try_into().map_err(|_| "expected two instruction clocks".into())
}
fn context(machine: &Machine, core: Core) -> Result<Context, String> {
    let mut registers = [Word(0); 13];
    for (index, value) in registers.iter_mut().enumerate() { *value = Word(reg(machine, core, arm::Register::R(index as u8))?); }
    let mut tcm = [[Word(0); 2]; 2];
    let mut identities = [Word(0); 2];
    for index in 0..2 {
        let cpu = machine.session.board.cpu(index)?;
        identities[index] = Word(u32::try_from(cpu.rprofile_identity(None)?).map_err(|_| "identity exceeds word")?);
        for (bank, opc2) in [1, 0].into_iter().enumerate() {
            tcm[index][bank] = Word(u32::try_from(cpu.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2, secure: 0, wide: false,
            }, None)?).map_err(|_| "TCM register exceeds word")?);
        }
    }
    Ok(Context {
        core, pc: Address(reg(machine, core, arm::Register::R(15))?), registers,
        sp: Address(reg(machine, core, arm::Register::R(13))?), lr: Address(reg(machine, core, arm::Register::R(14))?),
        cpsr: Word(reg(machine, core, arm::Register::Cpsr)?), counts: counts(machine)?, tcm, identities,
    })
}
fn contexts(machine: &Machine) -> Result<[Context; 2], String> {
    Ok([context(machine, Core::Primary)?, context(machine, Core::Secondary)?])
}
fn physical(maps: &[MemoryMap], state: &Context, guest: Address, size: u64) -> Result<Address, String> {
    require(size != 0 && size <= LIMITS.resource_bytes, "resource extent exceeds admission bound")?;
    let end = u64::from(guest.0).checked_add(size).ok_or("resource extent overflow")?;
    let span = static_span(maps, MemoryCore(state.core.into()), MemoryAddress(u64::from(guest.0))..MemoryAddress(end))?;
    for bank in state.tcm[usize::from(state.core)] {
        if let Some((low, high)) = tcm_window(TcmRegister(bank.0))? {
            require(end <= low || u64::from(guest.0) >= high, "resource aliases active TCM")?;
        }
    }
    u32::try_from(span.physical.0).map(Address).map_err(|_| "physical address exceeds word".into())
}
fn views(machine: &Machine, maps: &[MemoryMap], state: &Context, base: Address, size: u64) -> Result<Vec<View>, String> {
    require(u64::from(base.0) + size <= 0x4000_0000, "resource lacks low/high physical aliases")?;
    let mut physical_bytes = vec![0; usize::try_from(size).map_err(|_| "resource size exceeds host")?];
    machine.session.board.read_physical(u64::from(base.0), &mut physical_bytes)?;
    let mut result = Vec::with_capacity(4);
    for core in [Core::Primary, Core::Secondary] {
        let mut local = state.clone();
        local.core = core;
        for alias in [0, 0x4000_0000] {
            let guest = Address(base.0 + alias);
            require(physical(maps, &local, guest, size)? == base, "resource core/alias translation differs")?;
            let mut bytes = vec![0; physical_bytes.len()];
            machine.session.board.read_memory(core.into(), u64::from(guest.0), &mut bytes)?;
            require(bytes == physical_bytes, "resource guest and physical views differ")?;
            result.push(View { core, guest, physical: base, bytes: Bytes(bytes) });
        }
    }
    Ok(result)
}
fn word(machine: &Machine, maps: &[MemoryMap], state: &Context, guest: Address) -> Result<Word, String> {
    let base = physical(maps, state, guest, 4)?;
    let samples = views(machine, maps, state, base, 4)?;
    let bytes = samples.first().ok_or("word views absent")?.bytes.0.as_slice();
    Ok(Word(u32::from_le_bytes(bytes.try_into().map_err(|_| "word extent differs")?)))
}
fn add(base: Address, offset: u32) -> Result<Address, String> {
    base.0.checked_add(offset).map(Address).ok_or_else(|| "guest address overflow".into())
}

#[derive(Clone, Copy)]
struct FrameLayout { saved: StackBytes, locals: StackBytes }
impl FrameLayout {
    const LINK: Self = Self { saved: StackBytes(8), locals: StackBytes(0) };
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum ProcessorMode { User, Fiq, Irq, Supervisor, Monitor, Abort, Hypervisor, Undefined, System }
impl ProcessorMode {
    fn from_cpsr(cpsr: Word) -> Result<Self, String> {
        match cpsr.0 & 0x1f {
            0x10 => Ok(Self::User), 0x11 => Ok(Self::Fiq), 0x12 => Ok(Self::Irq),
            0x13 => Ok(Self::Supervisor), 0x16 => Ok(Self::Monitor), 0x17 => Ok(Self::Abort),
            0x1a => Ok(Self::Hypervisor), 0x1b => Ok(Self::Undefined), 0x1f => Ok(Self::System),
            _ => Err("unsupported processor mode".into()),
        }
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct ExecutionFrame { core: Core, mode: ProcessorMode, fp: Address, sp: Address }
impl ExecutionFrame {
    fn observed(state: &Context) -> Result<Self, String> {
        Ok(Self { core: state.core, mode: ProcessorMode::from_cpsr(state.cpsr)?,
            fp: Address(state.registers[11].0), sp: state.sp })
    }
    fn entered(self, layout: FrameLayout) -> Result<Self, String> {
        let bytes = layout.saved.0.checked_add(layout.locals.0).ok_or("frame extent overflow")?;
        Ok(Self { fp: Address(self.sp.0.checked_sub(4).ok_or("frame pointer underflow")?),
            sp: Address(self.sp.0.checked_sub(bytes).ok_or("body stack underflow")?), ..self })
    }
}
#[derive(Clone, Copy)]
struct Frame {
    id: InvocationId, family: Family, entry_lr: Address, caller: ExecutionFrame,
    body: ExecutionFrame, pop_sp: Address,
    descriptor: Option<Address>, record: Option<Address>, output: Option<Address>,
    message: Option<Address>, active: bool,
}
impl Frame {
    fn source_key(self, pc: Address) -> ExecutionFrame {
        ExecutionFrame { sp: if pc == self.family.epilogue() { self.pop_sp } else { self.body.sp }, ..self.body }
    }
}
struct Call {
    owner: Option<InvocationId>, target: Address, continuation: Address,
    caller_frame: ExecutionFrame, entered: bool,
}
impl Call {
    fn targets(&self, state: &Context, key: ExecutionFrame) -> bool {
        !self.entered && self.target == state.pc && self.continuation == state.lr && self.caller_frame == key
    }
    fn returns(&self, pc: Address, key: ExecutionFrame) -> bool {
        self.entered && self.continuation == pc && self.caller_frame == key
    }
}
struct Runtime {
    receipt: Receipt, maps: Vec<MemoryMap>, frames: Vec<Frame>, calls: Vec<Call>,
    global_returns: BTreeMap<(Core, Address), ProbeKind>,
    globals_seen: BTreeSet<(Core, ProbeKind)>, installed: bool, outer_returned: bool,
    carriers: Vec<WatchRange>,
    peer_poll: Option<ProbeSequence>, peer_ready: bool,
    retained_records: u64, captured_bytes: u64, captured_views: u64,
    shared_sites: Option<shared::Sites>,
}
impl Runtime {
    fn capture_views(&mut self, machine: &Machine, state: &Context, base: Address, size: u64) -> Result<Vec<View>, String> {
        let bytes = size.checked_mul(4).ok_or("snapshot byte count overflow")?;
        let total = self.captured_bytes.checked_add(bytes).ok_or("aggregate snapshot byte count overflow")?;
        require(total <= LIMITS.snapshot_bytes && self.captured_views < LIMITS.snapshots,
            "aggregate snapshot capture bound reached")?;
        self.captured_bytes = total;
        self.captured_views += 1;
        views(machine, &self.maps, state, base, size)
    }
    fn watch_ranges(&self, machine: &Machine, backings: &[WatchRange], states: &[Context; 2]) -> Result<WatchPlan, String> {
        let armed_sites = machine.traces.sites.keys().copied().map(Address).collect::<Vec<_>>();
        if !self.outer_returned {
            return Ok(WatchPlan { classes: backings.to_vec(), fetch_scope: FetchScope::Complete,
                fetch_ranges: backings.iter().map(|range| FetchRange([range.start, range.end])).collect(), armed_sites });
        }
        let data_kinds = WatchKinds::CPU_READ.union(WatchKinds::WRITES).union(WatchKinds::HOST_READ).union(WatchKinds::MODEL_BUS_READ);
        let data = self.receipt.resources.iter().filter(|resource| resource.closed.is_none() && resource.kind != ResourceKind::Carrier)
            .map(|resource| WatchRange { start: u64::from(resource.physical.0),
                end: u64::from(resource.physical.0) + resource.size, kinds: data_kinds }).collect();
        let mut classes = merge_watch_ranges(data);
        let words = FAMILIES.iter().map(|family| Address(family.entry()))
            .chain([0x427d4,0x427e4,0x42800,0x42810,0xd27d4,0xd27e4,0xd2800,0xd2810].into_iter().map(Address))
            .chain([0x4b430,0xdb3f0,0x48fb8,0xd8f78,0x48f04,0xd8ec4,0x48f58,0xd8f18].into_iter().map(Address))
            .chain(armed_sites.iter().copied()).map(|pc| (pc, 4));
        let mut fetches = Vec::new();
        for (guest, size) in [(Address(0x69cdf8),0x18c),(Address(0x725a04),0x150)].into_iter().chain(words) {
            let start = physical(&self.maps, &states[0], guest, size)?;
            require(physical(&self.maps, &states[1], guest, size)? == start, "fetch scope differs between processor views")?;
            fetches.push(WatchRange { start: u64::from(start.0), end: u64::from(start.0) + size, kinds: WatchKinds::CPU_FETCH });
        }
        let fetches = merge_watch_ranges(fetches);
        let fetch_ranges = fetches.iter().map(|range| FetchRange([range.start, range.end])).collect();
        classes.extend(fetches);
        classes.extend(self.carriers.iter().cloned());
        require(classes.len() <= 256, "classified resource range bound reached")?;
        Ok(WatchPlan { classes, fetch_scope: FetchScope::Scoped, fetch_ranges, armed_sites })
    }
    fn admit(&mut self, machine: &Machine, state: &Context, kind: ResourceKind, lifetime: Lifetime,
        owner: Option<InvocationId>, guest: Address, size: u64) -> Result<Option<ResourceId>, String> {
        if size == 0 { return Ok(None); }
        let base = physical(&self.maps, state, guest, size)?;
        if let Some(existing) = self.receipt.resources.iter().find(|r| r.closed.is_none() && r.kind == kind
            && r.owner == owner && r.physical == base && r.size == size && r.lifetime == lifetime) {
            return Ok(Some(existing.id));
        }
        require(self.receipt.resources.len() < LIMITS.resources as usize, "resource count bound reached")?;
        let initial = self.capture_views(machine, state, base, size)?;
        let id = ResourceId(self.receipt.resources.len() as u64);
        self.receipt.resources.push(Resource {
            id, kind, lifetime, owner, physical: base, size, admitted: state.clone(), initial,
            closed: None, final_views: Vec::new(),
        });
        Ok(Some(id))
    }
    fn snapshot(&mut self, machine: &Machine, state: &Context, owner: Option<InvocationId>) -> Result<Vec<Snapshot>, String> {
        let mut snapshots = Vec::new();
        let poll=self.shared_sites.as_ref().map_or([0xd0a44,0xd0a48,0xd0a4c],|sites|sites.poll);
        for index in 0..self.receipt.resources.len() {
            let resource = &self.receipt.resources[index];
            if resource.closed.is_some() || (resource.owner.is_some() && resource.owner != owner)
                || resource.kind == ResourceKind::Carrier
                || (resource.kind == ResourceKind::Buffer && resource.lifetime == Lifetime::Observation)
                || (resource.owner.is_none() && resource.lifetime == Lifetime::Observation && resource.size > 4
                    && matches!(resource.kind, ResourceKind::Record | ResourceKind::Auxiliary))
                || (poll.contains(&state.pc.0) && resource.kind != ResourceKind::Completion) { continue; }
            let (id, base, size) = (resource.id, resource.physical, resource.size);
            let views = self.capture_views(machine, state, base, size)?;
            snapshots.push(Snapshot { resource: id, views });
        }
        Ok(snapshots)
    }
    fn probe(&mut self, machine: &Machine, state: &Context, kind: ProbeKind,
        family: Option<Family>, owner: Option<InvocationId>) -> Result<ProbeSequence, String> {
        require(self.receipt.probes.len() < LIMITS.probes as usize, "probe bound reached")?;
        let source_word = word(machine, &self.maps, state, state.pc)?;
        let sequence = ProbeSequence(self.receipt.probes.len() as u64);
        self.receipt.probes.push(Probe {
            sequence, kind, family, invocation: owner, context: state.clone(), source_word, snapshots: Vec::new(),
        });
        let snapshots = if kind == ProbeKind::FamilyEntry && owner.is_none() { Vec::new() }
            else { self.snapshot(machine, state, owner)? };
        self.receipt.probes[sequence.0 as usize].snapshots = snapshots;
        Ok(sequence)
    }
    fn close(&mut self, machine: &Machine, state: &Context, owner: Option<InvocationId>, lifetime: Lifetime) -> Result<(), String> {
        for index in 0..self.receipt.resources.len() {
            let resource = &self.receipt.resources[index];
            if resource.closed.is_some() || resource.owner != owner || resource.lifetime != lifetime { continue; }
            let (base, size) = (resource.physical, resource.size);
            let final_views = self.capture_views(machine, state, base, size)?;
            self.receipt.resources[index].final_views = final_views;
            self.receipt.resources[index].closed = Some(state.clone());
        }
        Ok(())
    }
    fn kernel_records(&mut self, machine: &Machine, state: &Context, lifetime: Lifetime,
        owner: Option<InvocationId>) -> Result<(), String> {
        for (kind, bases, size) in [
            (ResourceKind::Record,[0x4005beac,0x400ebe6c],40*120),
            (ResourceKind::Record,[0x4005ff0c,0x400efecc],56*120),
            (ResourceKind::Record,[0x400622d4,0x400f2294],4*120),
            (ResourceKind::Auxiliary,[0x4005d178,0x400ed138],12*120),
            (ResourceKind::Auxiliary,[0x400624b4,0x400f2474],16*120),
            (ResourceKind::Record,[0x400545a4,0x400e4564],0x1c8),
        ] {
            for base in bases {
                self.admit(machine, state, kind, lifetime, owner, Address(base), size)?;
            }
        }
        Ok(())
    }
    fn initial_resources(&mut self, machine: &Machine, state: &Context) -> Result<(), String> {
        for (kind, base, size) in [
            (ResourceKind::Roots,0x96bd10,64),
            (ResourceKind::RootObject,0x4dae0,0x162c),
            (ResourceKind::RootObject,0xddaa0,0x162c),
            (ResourceKind::ServiceStorage,0x9c34f0,0xf64),
            (ResourceKind::TaskStorage,0xa0f85c,0x998),
            (ResourceKind::Record,0xa0f464,8*120),
            (ResourceKind::Record,0x9c280c,4),
            (ResourceKind::Cursor,0x89ea90,12),
            (ResourceKind::CallbackCell,0x89ea94,4),
            (ResourceKind::CallbackCell,0x89ea8c,4),
            (ResourceKind::ErrorState,0xa0f82c,4),
            (ResourceKind::Completion,0xa00c4,4),
            (ResourceKind::Code,0x69cdf8,0x18c),
            (ResourceKind::Code,0x725a04,0x150),
            (ResourceKind::Code,0xd0430,0x680),
            (ResourceKind::Code,0x42634,0x74),
            (ResourceKind::Code,0x723318,0x2d80),
            (ResourceKind::Code,0x722054,0x198),
            (ResourceKind::Code,0x412bc,0x60),
            (ResourceKind::Code,0x427b8,0x70),
            (ResourceKind::Code,0xd27b8,0x70),
            (ResourceKind::Code,0xd1140,0x48),
            (ResourceKind::Header,0x40010,4),
            (ResourceKind::Header,0xd0010,4),
        ] {
            self.admit(machine, state, kind, Lifetime::Observation, None, Address(base), size)?;
        }
        self.kernel_records(machine, state, Lifetime::Observation, None)?;
        for (bases, size) in [
            ([0x4005988c,0x400e984c],16*120),
            ([0x4005a00c,0x400e9fcc],32*120),
            ([0x4006194c,0x400f190c],20*120),
            ([0x4005f584,0x400ef544],20*120),
        ] {
            for base in bases {
                self.admit(machine, state, ResourceKind::Record, Lifetime::Observation, None, Address(base), size)?;
            }
        }
        for (base, size) in [(0x40059284,4),(0x400e9244,4),(0x40062c4c,8),(0x400f2c0c,8)] {
            self.admit(machine, state, ResourceKind::Buffer, Lifetime::Observation, None, Address(base), size)?;
        }
        let depth=if self.shared_sites.as_ref().is_some_and(|sites|sites.owned) {104} else {136};
        let low = state.sp.0.checked_sub(depth).ok_or("outer scratch underflow")?;
        self.admit(machine, state, ResourceKind::Stack, Lifetime::Observation, None, Address(low), u64::from(depth+32))?;
        self.admit(machine, state, ResourceKind::Buffer, Lifetime::Observation, None, Address(0x409c4460), 0x4b000)?;
        self.admit(machine, state, ResourceKind::Buffer, Lifetime::Observation, None, Address(0x40a10200), 0x4b000)?;
        for index in 0..self.carriers.len() {
            let carrier = self.carriers[index];
            let mut cursor = carrier.start;
            while cursor < carrier.end {
                let size = (carrier.end-cursor).min(1<<20);
                self.admit(machine, state, ResourceKind::Carrier, Lifetime::Observation, None,
                    Address(u32::try_from(cursor).map_err(|_| "carrier address exceeds word")?), size)?;
                cursor += size;
            }
        }
        Ok(())
    }
    fn family_entry(&mut self, machine: &mut Machine, state: &Context, family: Family, key: ExecutionFrame) -> Result<(), String> {
        let selected = !self.receipt.invocations.iter().any(|i| i.family == family && i.entry_probe.is_some());
        let owner = if selected { Some(InvocationId(self.receipt.invocations.len() as u64)) } else { None };
        let probe = self.probe(machine, state, ProbeKind::FamilyEntry, Some(family), owner)?;
        let census = self.receipt.census.iter_mut().find(|c| c.family == family && c.core == state.core).ok_or("family census absent")?;
        census.entries = census.entries.checked_add(1).ok_or("census overflow")?;
        census.first_probe.get_or_insert(probe);
        if !selected {
            require(!self.frames.iter().any(|frame| frame.active && frame.family == family && frame.caller == key),
                "selected family frame re-entered before its first return")?;
            return Ok(());
        }
        require(self.receipt.invocations.len() < LIMITS.invocations as usize, "invocation bound reached")?;
        let id = owner.ok_or("selected invocation ID absent")?;
        let mut parent = None;
        let mut ambiguous_parent = false;
        for call in self.calls.iter().filter(|call| call.targets(state,key)) {
            if let Some(owner) = call.owner.filter(|id| self.frames.iter().any(|frame| frame.active && frame.id == *id && frame.body == key)) {
                ambiguous_parent |= parent.replace(owner).is_some();
            }
        }
        self.receipt.invocations.push(Invocation {
            id, family, core: state.core, entry_probe: Some(probe), return_probe: None,
            parent: if ambiguous_parent { None } else { parent },
            status: Status::Unclosed, entry_lr: state.lr, entry_sp: state.sp,
        });
        require(!ambiguous_parent, "immediate parent frame is ambiguous")?;
        let layout = family.layout();
        let mut frame = Frame {
            id, family, entry_lr: state.lr, caller: key, body: key.entered(layout)?,
            pop_sp: Address(key.sp.0.checked_sub(layout.saved.0).ok_or("saved frame underflow")?),
            descriptor: None, record: None, output: None, message: None, active: true,
        };
        self.admit(machine, state, ResourceKind::Stack, Lifetime::Invocation, Some(id),
            Address(state.sp.0.checked_sub(256).ok_or("local stack underflow")?), 288)?;
        let r = state.registers;
        match family {
            Family::TaskWrapper => {
                let descriptor = Address(r[0].0);
                self.admit(machine, state, ResourceKind::Descriptor, Lifetime::Invocation, Some(id), descriptor, 28)?;
                frame.descriptor = Some(descriptor);
                let ident = word(machine, &self.maps, state, descriptor)?.0;
                if (1..=120).contains(&ident) {
                    self.admit(machine, state, ResourceKind::Output, Lifetime::Invocation, Some(id),
                        Address(0x40a5b204 + 8*(ident-1)), 8)?;
                    self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(id),
                        Address(0x40a0fc44 + 12*(ident-1)), 12)?;
                    self.kernel_records(machine, state, Lifetime::Allocation, Some(id))?;
                }
            }
            Family::TaskReserve | Family::StorageReserve => {
                let index = r[0].0;
                let maximum = if family == Family::TaskReserve { 120 } else { 240 };
                if index < maximum {
                    let base = if family == Family::TaskReserve { 0x40a0fc44 } else { 0x409c3904 };
                    let record = Address(base + 12 * index);
                    self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(id), record, 12)?;
                    frame.record = Some(record);
                }
                let output = Address(r[2].0);
                self.admit(machine, state, ResourceKind::Output, Lifetime::Invocation, Some(id), output, 4)?;
                frame.output = Some(output);
            }
            Family::MessageConstruct => {
                let queue = Address(r[1].0);
                self.admit(machine, state, ResourceKind::Descriptor, Lifetime::Invocation, Some(id), queue, 8)?;
            }
            Family::MessageSend | Family::MessageReceive => {
                let message = Address(r[1].0);
                self.admit(machine, state, ResourceKind::Descriptor, Lifetime::Invocation, Some(id), message, 12)?;
                frame.message = Some(message);
            }
        }
        self.frames.push(frame);
        require(state.lr.0 & 3 == 0, "family caller return is not A32 aligned")?;
        let mut sites: Vec<u32> = family.points().iter().chain(COMMON_PCS).copied().collect();
        sites.push(state.lr.0);
        arm_sites(machine, sites)?;
        let snapshots = self.snapshot(machine, state, Some(id))?;
        self.receipt.probes[probe.0 as usize].snapshots = snapshots;
        Ok(())
    }
    fn payload(&mut self, machine: &Machine, state: &Context, frame: &Frame, pointer: Address,
        length: u64, lifetime: Lifetime) -> Result<(), String> {
        require(self.receipt.payloads.len() < LIMITS.probes as usize, "payload admission bound reached")?;
        let resource = self.admit(machine, state, ResourceKind::Payload, lifetime, Some(frame.id), pointer, length)?;
        self.receipt.payloads.push(PayloadAdmission {
            owner: frame.id, context: state.clone(), pointer, length, resource,
            state: if length == 0 { PayloadState::Empty } else { PayloadState::Admitted },
        });
        Ok(())
    }
    fn message_payload(&mut self, machine: &Machine, state: &Context, frame: &Frame) -> Result<(), String> {
        let message = frame.message.ok_or("message descriptor absent")?;
        let length = word(machine, &self.maps, state, add(message, 4)?)?.0;
        let payload = Address(word(machine, &self.maps, state, add(message, 8)?)?.0);
        self.payload(machine, state, frame, payload, u64::from(length), Lifetime::Invocation)?;
        Ok(())
    }
    fn allocation(&mut self, machine: &Machine, state: &Context, frame: &Frame) -> Result<(), String> {
        let output = frame.output.ok_or("reservation output absent")?;
        let pointer = Address(word(machine, &self.maps, state, output)?.0);
        let record = frame.record.ok_or("successful reservation record outside admitted domain")?;
        let stored_base = Address(word(machine, &self.maps, state, record)?.0);
        let stored_size = word(machine, &self.maps, state, add(record, 4)?)?.0;
        if stored_size == 0 { return Ok(()); }
        let mapped_output = physical(&self.maps, state, pointer, 1)?;
        let mapped_record = physical(&self.maps, state, stored_base, 1)?;
        require(mapped_output == mapped_record, "reservation output and record backing differ")?;
        self.admit(machine, state, ResourceKind::Buffer, Lifetime::Allocation, Some(frame.id), pointer, u64::from(stored_size))?;
        Ok(())
    }
    fn task_domains(&mut self, machine: &Machine, state: &Context, owner: InvocationId, domain: KernelDomain) -> Result<(), String> {
        for offset in [0,132] {
            let descriptor = add(domain.domains(), offset)?;
            self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(owner), descriptor, 132)?;
            for (field, size) in [(0x20,128),(0x24,4),(0x28,4),(0x2c,4)] {
                let target = Address(word(machine, &self.maps, state, add(descriptor, field)?)?.0);
                require(target.0 != 0, "task domain resource pointer is null")?;
                self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(owner), target, size)?;
            }
        }
        Ok(())
    }
    fn message_inputs(&mut self, machine: &Machine, state: &Context, frame: &Frame,
        domain: KernelDomain, access: MessageAccess) -> Result<(), String> {
        let Some(reference) = MessageReference::parse(state.registers[0]) else { return Ok(()); };
        let first = domain.domains();
        let second = add(first,132)?;
        let descriptor = match reference.selection {
            DomainSelection::Current => {
                let selected = Address(word(machine, &self.maps, state, Address(first.0-4))?.0);
                require(selected == first || selected == second, "message current domain has no admitted layout")?;
                selected
            }
            DomainSelection::First => first,
            DomainSelection::Second => second,
        };
        self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(frame.id), descriptor, 132)?;
        let (table, count_offset, stride, construction_bytes) = match access {
            MessageAccess::Acquire => (0x6c,0x70,20,20),
            MessageAccess::Submit | MessageAccess::Receive => (0x5c,0x60,32,16),
        };
        let count = word(machine, &self.maps, state, add(descriptor,count_offset)?)?.0 as i32;
        let index = u32::from(reference.index.0.get());
        if index as i32 > count { return Ok(()); }
        let base = Address(word(machine, &self.maps, state, add(descriptor,table)?)?.0);
        require(base.0 != 0, "message node table pointer is null")?;
        let node = add(base,(index-1).checked_mul(stride).ok_or("message node offset overflow")?)?;
        self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(frame.id), node, u64::from(stride))?;
        let construction = Address(word(machine, &self.maps, state, add(node,8)?)?.0);
        require(construction.0 != 0, "message construction record pointer is null")?;
        self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(frame.id), construction, construction_bytes)?;
        if matches!(access, MessageAccess::Acquire) {
            let head = Address(word(machine, &self.maps, state, add(node,16)?)?.0);
            let prefix = if head.0 != 0 { Some(head) } else {
                let cursor = Address(word(machine, &self.maps, state, add(node,12)?)?.0);
                let end = Address(word(machine, &self.maps, state, add(construction,12)?)?.0);
                (cursor.0 < end.0).then_some(cursor)
            };
            if let Some(prefix) = prefix {
                require(prefix.0 != 0, "message free block pointer is null")?;
                self.admit(machine, state, ResourceKind::Buffer, Lifetime::Allocation, Some(frame.id), prefix, 4)?;
            }
            return Ok(());
        }
        if word(machine, &self.maps, state, add(construction,12)?)? == Word(0) { return Ok(()); }
        let capacity = word(machine, &self.maps, state, add(construction,4)?)?.0;
        if capacity == 0 { return Ok(()); }
        let cursor_field = if matches!(access, MessageAccess::Submit) { 0x1c } else { 0x18 };
        let cursor = word(machine, &self.maps, state, add(node,cursor_field)?)?.0;
        require(cursor < capacity, "message ring cursor exceeds capacity")?;
        let ring = Address(word(machine, &self.maps, state, add(construction,8)?)?.0);
        require(ring.0 != 0, "message ring pointer is null")?;
        let slot = add(ring,cursor.checked_mul(4).ok_or("message ring slot overflow")?)?;
        self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(frame.id), slot, 4)?;
        Ok(())
    }
    fn source_resources(&mut self, machine: &Machine, state: &Context, frame: &Frame) -> Result<(), String> {
        let pc = state.pc.0;
        if frame.family == Family::TaskWrapper && [0x725f08,0x725fa8,0x726024].contains(&pc) {
            let instruction = word(machine, &self.maps, state, state.pc)?;
            let target = call_target(state, instruction)?.ok_or("task service call does not execute")?;
            self.task_domains(machine, state, frame.id, KernelDomain::task_target(target)?)?;
        }
        if (frame.family == Family::MessageSend && [0x725100,0x7251b4,0x7251f0].contains(&pc))
            || (frame.family == Family::MessageReceive && [0x725378,0x725414].contains(&pc)) {
            let instruction = word(machine, &self.maps, state, state.pc)?;
            let target = call_target(state, instruction)?.ok_or("message service call does not execute")?;
            let domain = KernelDomain::message_target(target)?;
            self.task_domains(machine, state, frame.id, domain)?;
            let access = match pc {
                0x725100 => Some(MessageAccess::Acquire), 0x7251b4 => Some(MessageAccess::Submit),
                0x725378 => Some(MessageAccess::Receive), _ => None,
            };
            if let Some(access) = access { self.message_inputs(machine, state, frame, domain, access)?; }
        }
        if matches!(frame.family, Family::TaskReserve | Family::StorageReserve)
            && [0x725dac,0x7235ac].contains(&pc) {
            let wrote_output = self.receipt.probes.iter().any(|probe| probe.invocation == Some(frame.id)
                && probe.kind == ProbeKind::Source && [0x725d1c,0x725d64,0x723520,0x723564].contains(&probe.context.pc.0));
            if wrote_output { self.allocation(machine, state, frame)?; }
        }
        if frame.family == Family::TaskWrapper && pc == 0x725f68 {
            self.admit(machine, state, ResourceKind::Buffer, Lifetime::Allocation, Some(frame.id),
                Address(state.registers[0].0), u64::from(state.registers[2].0))?;
        }
        if frame.family == Family::MessageConstruct && [0x724d24,0x724e38].contains(&pc)
            && state.registers[0] == Word(0) {
            let call_pc = if pc == 0x724d24 { 0x724d20 } else { 0x724e34 };
            let call = self.receipt.probes.iter().rev().find(|probe| probe.invocation == Some(frame.id)
                && probe.kind == ProbeKind::Source && probe.context.pc == Address(call_pc)).ok_or("constructor reservation call absent")?;
            let output = Address(call.context.registers[2].0);
            let size = u64::from(call.context.registers[1].0);
            let pointer = Address(word(machine, &self.maps, state, output)?.0);
            self.admit(machine, state, ResourceKind::Buffer, Lifetime::Allocation, Some(frame.id), pointer, size)?;
        }
        if frame.family == Family::TaskWrapper && pc == 0x725df8 {
            let descriptor = frame.descriptor.ok_or("task descriptor absent")?;
            let id = word(machine, &self.maps, state, descriptor)?.0;
            if (1..=120).contains(&id) {
                self.admit(machine, state, ResourceKind::Record, Lifetime::Allocation, Some(frame.id), Address(0x40a0fc44 + 12*(id-1)), 12)?;
                self.admit(machine, state, ResourceKind::Output, Lifetime::Invocation, Some(frame.id), Address(0x40a5b204 + 8*(id-1)), 8)?;
            }
        }
        if frame.family == Family::TaskWrapper && [0x725f08,0x725fa8].contains(&pc) {
            self.admit(machine, state, ResourceKind::Auxiliary, Lifetime::Invocation, Some(frame.id), Address(state.registers[1].0), if pc == 0x725fa8 { 24 } else { 8 })?;
        }
        if frame.family == Family::MessageConstruct && [0x724d98,0x724ea4].contains(&pc) {
            self.admit(machine, state, ResourceKind::Auxiliary, Lifetime::Invocation, Some(frame.id), Address(state.registers[1].0), if pc == 0x724d98 { 12 } else { 16 })?;
        }
        if frame.family == Family::MessageSend {
            if [0x7250c4,0x7250cc,0x7250d0].contains(&pc) { self.message_payload(machine, state, frame)?; }
            if pc == 0x725100 {
                self.admit(machine, state, ResourceKind::Output, Lifetime::Invocation, Some(frame.id), Address(state.registers[1].0), 4)?;
            }
            if pc == 0x725104 && state.registers[0].0 == 0 {
                self.service_buffer(machine, state, frame, 0x725100, Lifetime::Invocation)?;
            }
            if pc == 0x72517c {
                let bytes = u64::from(state.registers[2].0);
                self.payload(machine, state, frame, Address(state.registers[1].0), bytes, Lifetime::Invocation)?;
                self.admit(machine, state, ResourceKind::Buffer, Lifetime::Invocation, Some(frame.id), Address(state.registers[0].0), bytes)?;
            }
        }
        if frame.family == Family::MessageReceive {
            if pc == 0x725378 {
                self.admit(machine, state, ResourceKind::Output, Lifetime::Invocation, Some(frame.id), Address(state.registers[1].0), 4)?;
            }
            if pc == 0x72537c && state.registers[0].0 == 0 {
                self.service_buffer(machine, state, frame, 0x725378, Lifetime::ReceiveRelease)?;
            }
            if pc == 0x7253f8 {
                let bytes = u64::from(state.registers[2].0);
                self.payload(machine, state, frame, Address(state.registers[0].0), bytes, Lifetime::ReceiveRelease)?;
                self.admit(machine, state, ResourceKind::Buffer, Lifetime::ReceiveRelease, Some(frame.id), Address(state.registers[1].0), bytes)?;
            }
        }
        Ok(())
    }
    fn service_buffer(&mut self, machine: &Machine, state: &Context, frame: &Frame, call_pc: u32, lifetime: Lifetime) -> Result<(), String> {
        let call = self.receipt.probes.iter().rev().find(|p| p.invocation == Some(frame.id) && p.kind == ProbeKind::Source && p.context.pc == Address(call_pc)).ok_or("buffer service call absent")?;
        let output = Address(call.context.registers[1].0);
        let pointer = Address(word(machine, &self.maps, state, output)?.0);
        self.admit(machine, state, ResourceKind::Buffer, lifetime, Some(frame.id), pointer, 12)?;
        if frame.family == Family::MessageSend {
            let message = frame.message.ok_or("send descriptor absent")?;
            let length = u64::from(word(machine, &self.maps, state, add(message, 4)?)?.0);
            self.admit(machine, state, ResourceKind::Buffer, lifetime, Some(frame.id), pointer, length.checked_add(12).ok_or("send buffer extent overflow")?)?;
        } else {
            let length = u64::from(word(machine, &self.maps, state, add(pointer, 8)?)?.0);
            self.admit(machine, state, ResourceKind::Buffer, lifetime, Some(frame.id), pointer, length.checked_add(12).ok_or("receive buffer extent overflow")?)?;
            let message = frame.message.ok_or("receive descriptor absent")?;
            let destination = Address(word(machine, &self.maps, state, add(message, 8)?)?.0);
            self.payload(machine, state, frame, destination, length, Lifetime::ReceiveRelease)?;
        }
        Ok(())
    }
    fn dynamic_call(&mut self, machine: &mut Machine, state: &Context, owner: Option<InvocationId>,
        instruction: Word, caller_frame: ExecutionFrame) -> Result<(), String> {
        let Some(target) = call_target(state, instruction)? else { return Ok(()); };
        require(self.calls.len() < LIMITS.probes as usize, "dynamic call bound reached")?;
        self.calls.push(Call { owner, target, continuation: add(state.pc,4)?, caller_frame, entered: false });
        arm_sites(machine, vec![target.0])
    }
    fn source_owner(&self, machine: &Machine, state: &Context, key: ExecutionFrame,
        returning_owner: Option<InvocationId>) -> Result<Option<Frame>, String> {
        let mut selected = None;
        let mut helper_link = None;
        for frame in self.frames.iter().filter(|frame| frame.active) {
            let in_body = frame.family.entry() <= state.pc.0 && state.pc.0 <= frame.family.epilogue().0;
            let mut owns = key == frame.source_key(state.pc)
                && (frame.family.points().contains(&state.pc.0) || (in_body && returning_owner == Some(frame.id)));
            if !owns && COMMON_PCS.contains(&state.pc.0) && !frame.family.error_continuations().is_empty()
                && key == frame.body.entered(FrameLayout::LINK)? {
                let (saved_fp, saved_lr) = match helper_link {
                    Some(link) => link,
                    None => {
                        let link = (Address(word(machine, &self.maps, state, key.sp)?.0),
                            Address(word(machine, &self.maps, state, add(key.sp,4)?)?.0));
                        helper_link = Some(link);
                        link
                    }
                };
                owns = saved_fp == frame.body.fp && frame.family.error_continuations().contains(&saved_lr.0);
            }
            if owns {
                require(selected.is_none(), "source frame is ambiguous")?;
                selected = Some(*frame);
            }
        }
        Ok(selected)
    }
    fn identity_source_frame(&self, key: ExecutionFrame) -> Result<bool, String> {
        for call in self.calls.iter().filter(|call| call.owner.is_none() && call.entered && call.target == Address(0x412bc)) {
            if key == call.caller_frame.entered(FrameLayout::LINK)? { return Ok(true); }
        }
        Ok(false)
    }
    fn handle(&mut self, machine: &mut Machine, state: &Context) -> Result<(), String> {
        let key = ExecutionFrame::observed(state)?;
        if let Some(family) = FAMILIES.into_iter().find(|family| family.entry() == state.pc.0) {
            self.family_entry(machine, state, family, key)?;
        }
        let mut returning_call = None;
        let mut target_call = None;
        for (index, call) in self.calls.iter().enumerate() {
            if call.returns(state.pc,key) {
                require(returning_call.replace(index).is_none(), "dynamic return frame is ambiguous")?;
            }
            if call.targets(state,key) {
                require(target_call.replace(index).is_none(), "dynamic target frame is ambiguous")?;
            }
        }
        let returning_owner = returning_call.and_then(|index| self.calls[index].owner);
        let source_frame = self.source_owner(machine, state, key, returning_owner)?;
        if let Some(frame) = &source_frame { self.source_resources(machine, state, frame)?; }
        if let Some(index) = target_call {
            let owner = self.calls[index].owner;
            let family = owner.and_then(|id| self.frames.iter().find(|frame| frame.id == id).map(|frame| frame.family));
            self.probe(machine, state, ProbeKind::DynamicTarget, family, owner)?;
            self.calls[index].entered = true;
            arm_sites(machine, vec![self.calls[index].continuation.0])?;
        }
        if let Some(index) = returning_call {
            let owner = self.calls[index].owner;
            let family = owner.and_then(|id| self.frames.iter().find(|frame| frame.id == id).map(|frame| frame.family));
            self.probe(machine, state, ProbeKind::DynamicReturn, family, owner)?;
            let target = self.calls[index].target;
            let continuation = self.calls[index].continuation;
            self.calls.remove(index);
            remove_if_unused(machine, target.0, &self.frames, &self.calls, &self.global_returns)?;
            remove_if_unused(machine, continuation.0, &self.frames, &self.calls, &self.global_returns)?;
        }
        if let Some(kind) = self.global_returns.remove(&(state.core, state.pc)) {
            self.probe(machine, state, kind, None, None)?;
            self.globals_seen.insert((state.core, kind));
            if kind == ProbeKind::InstallerReturn { self.installed = true; }
            if kind == ProbeKind::OuterReturn { self.outer_returned = true; }
        }
        let poll=self.shared_sites.as_ref().map_or([0xd0a44,0xd0a48,0xd0a4c],|sites|sites.poll);
        let destination=self.shared_sites.as_ref().map_or(3,|sites|sites.poll_destination);
        if state.core == Core::Secondary && poll.contains(&state.pc.0) && !self.peer_ready {
            if state.pc == Address(poll[0]) {
                self.peer_poll = Some(self.probe(machine, state, ProbeKind::Source, None, None)?);
            } else if state.pc == Address(poll[1]) && state.registers[usize::from(destination)].0 != 0 {
                let poll = self.peer_poll.ok_or("successful peer load lacks preceding poll")?;
                self.receipt.probes[poll.0 as usize].kind = ProbeKind::PeerPoll;
                self.probe(machine, state, ProbeKind::PeerPostload, None, None)?;
                self.peer_ready = true;
            } else {
                self.probe(machine, state, ProbeKind::Source, None, None)?;
            }
        } else if let Some(kind) = self.shared_sites.as_ref().map_or_else(||global_kind(state.pc.0),|sites|sites.kind(state.pc.0)) {
            let primary = matches!(kind, ProbeKind::OuterEntry | ProbeKind::InstallerEntry | ProbeKind::CacheEntry
                | ProbeKind::CompletionStore | ProbeKind::CompletionBeforeDsb | ProbeKind::CompletionAfterDsb);
            let selected = state.core == if primary { Core::Primary } else { Core::Secondary };
            if selected && self.globals_seen.insert((state.core, kind)) {
                self.probe(machine, state, kind, None, None)?;
                let returned = match kind {
                    ProbeKind::OuterEntry if state.registers[0] == Word(0) => Some(ProbeKind::OuterReturn),
                    ProbeKind::InstallerEntry => Some(ProbeKind::InstallerReturn),
                    ProbeKind::CacheEntry if !self.installed => Some(ProbeKind::CacheReturn),
                    ProbeKind::InitializerEntry => Some(ProbeKind::InitializerReturn),
                    _ => None,
                };
                if let Some(returned) = returned {
                    require(state.lr.0 & 3 == 0, "global caller return is not A32 aligned")?;
                    self.global_returns.insert((state.core, state.lr), returned);
                    arm_sites(machine, vec![state.lr.0])?;
                }
                if matches!(kind, ProbeKind::CriticalIdentityCall | ProbeKind::CriticalCall) {
                    let instruction = word(machine, &self.maps, state, state.pc)?;
                    self.dynamic_call(machine, state, None, instruction, key)?;
                }
                if kind == ProbeKind::InitializerCall {
                    for pc in poll {
                        if self.shared_sites.is_none() && machine.traces.sites.contains_key(&pc) { machine.traces.remove_sites(&machine.session, &[Guest(pc)])?; }
                    }
                }
            }
        }
        if let Some(frame) = source_frame {
            let sequence = self.probe(machine, state, ProbeKind::Source, Some(frame.family), Some(frame.id))?;
            let instruction = self.receipt.probes[sequence.0 as usize].source_word;
            self.dynamic_call(machine, state, Some(frame.id), instruction, key)?;
            if frame.family == Family::MessageReceive && state.pc == Address(0x725418) {
                self.close(machine, state, Some(frame.id), Lifetime::ReceiveRelease)?;
            }
        } else if state.core == Core::Secondary && [0x412c4,0x412c8,0x427b8].contains(&state.pc.0)
            && self.identity_source_frame(key)? {
            let sequence = self.probe(machine, state, ProbeKind::Source, None, None)?;
            self.dynamic_call(machine, state, None, self.receipt.probes[sequence.0 as usize].source_word, key)?;
        }
        let mut returned = None;
        for (index, frame) in self.frames.iter().enumerate() {
            if frame.active && frame.entry_lr == state.pc && frame.caller == key {
                require(returned.replace(index).is_none(), "family return frame is ambiguous")?;
            }
        }
        if let Some(index) = returned {
            let frame = self.frames[index];
            if matches!(frame.family, Family::TaskReserve | Family::StorageReserve) && state.registers[0] == Word(0) {
                self.allocation(machine, state, &frame)?;
            }
            let probe = self.probe(machine, state, ProbeKind::FamilyReturn, Some(frame.family), Some(frame.id))?;
            self.receipt.invocations[frame.id.0 as usize].return_probe = Some(probe);
            self.receipt.invocations[frame.id.0 as usize].status = Status::Returned;
            self.close(machine, state, Some(frame.id), Lifetime::Invocation)?;
            self.frames[index].active = false;
            for pc in frame.family.points().iter().chain(COMMON_PCS) {
                remove_if_unused(machine, *pc, &self.frames, &self.calls, &self.global_returns)?;
            }
        }
        Ok(())
    }
    fn finished(&self) -> bool {
        FAMILIES.iter().all(|family| self.receipt.invocations.iter().any(|i| i.family == *family && i.status == Status::Returned))
            && self.global_returns.is_empty() && self.calls.is_empty()
            && self.receipt.probes.iter().any(|probe| probe.kind == ProbeKind::InitializerReturn && probe.context.core == Core::Secondary)
            && self.receipt.probes.iter().any(|probe| probe.kind == ProbeKind::CriticalReturn && probe.context.core == Core::Secondary)
    }
    fn absent(&mut self) {
        for family in FAMILIES {
            if !self.receipt.invocations.iter().any(|i| i.family == family) {
                self.receipt.invocations.push(Invocation {
                    id: InvocationId(self.receipt.invocations.len() as u64), family, core: Core::Primary,
                    entry_probe: None, return_probe: None, parent: None, status: Status::NotObserved,
                    entry_lr: Address(0), entry_sp: Address(0),
                });
            }
        }
    }
}

fn global_kind(pc: u32) -> Option<ProbeKind> {
    Some(match pc {
        0x69cdf8 => ProbeKind::OuterEntry, 0x725a04 => ProbeKind::InstallerEntry,
        0x42634 => ProbeKind::CacheEntry, 0x40a08 => ProbeKind::CompletionStore,
        0x40a0c => ProbeKind::CompletionBeforeDsb, 0x40a10 => ProbeKind::CompletionAfterDsb,
        0xd0a44 => ProbeKind::PeerPoll, 0xd0a48 => ProbeKind::PeerPostload,
        0xd0a4c => ProbeKind::PeerBranch, 0xd0aac => ProbeKind::InitializerCall,
        0xd0430 => ProbeKind::InitializerEntry, 0xd0ab0 => ProbeKind::InitializerReturn,
        0x7235d8 => ProbeKind::CriticalEntry, 0x7235f4 => ProbeKind::CriticalIdentityCall,
        0x7235f8 => ProbeKind::CriticalIdentityReturn, 0x7235fc => ProbeKind::CriticalRootLoad,
        0x723600 => ProbeKind::CriticalSlotLoad, 0x723604 => ProbeKind::CriticalCall,
        0x723608 => ProbeKind::CriticalReturn, _ => return None,
    })
}
const COMMON_PCS: &[u32] = &[0x7233ac,0x7233b0];
const GLOBAL_PCS: &[u32] = &[
    0x69cdf8,0x725a04,0x42634,0x40a08,0x40a0c,0x40a10,0xd0a44,0xd0a48,0xd0a4c,
    0xd0aac,0xd0430,0xd0ab0,0x7235d8,0x7235f4,0x7235f8,0x7235fc,0x723600,0x723604,0x723608,
    0x412c4,0x412c8,0x427b8,
];
fn arm_sites(machine: &mut Machine, pcs: Vec<u32>) -> Result<(), String> {
    let sites = pcs.into_iter().filter(|pc| !machine.traces.sites.contains_key(pc))
        .map(|pc| (pc, format!("consumer-{pc:x}"))).collect::<BTreeMap<_, _>>();
    if !sites.is_empty() { machine.traces.add_sites(&machine.session, sites)?; }
    machine.traces.stop_labels = machine.traces.sites.values().cloned().collect();
    Ok(())
}
fn remove_if_unused(machine: &mut Machine, pc: u32, frames: &[Frame], calls: &[Call], returns: &BTreeMap<(Core, Address), ProbeKind>) -> Result<(), String> {
    if FAMILIES.iter().any(|family| family.entry() == pc)
        || GLOBAL_PCS.contains(&pc)
        || frames.iter().any(|frame| frame.active && (frame.entry_lr == Address(pc) || frame.family.points().contains(&pc) || COMMON_PCS.contains(&pc)))
        || calls.iter().any(|call| call.target == Address(pc) || call.continuation == Address(pc))
        || returns.keys().any(|(_, address)| *address == Address(pc)) { return Ok(()); }
    if machine.traces.sites.get(&pc).is_some_and(|label|!label.starts_with("consumer-") || label.starts_with("consumer-global-")) { return Ok(()); }
    if machine.traces.sites.contains_key(&pc) { machine.traces.remove_sites(&machine.session, &[Guest(pc)])?; }
    Ok(())
}
fn condition(state: &Context, instruction: u32) -> bool {
    let flags = state.cpsr.0;
    let (n,z,c,v) = (flags & (1<<31) != 0, flags & (1<<30) != 0, flags & (1<<29) != 0, flags & (1<<28) != 0);
    match instruction >> 28 {
        0 => z, 1 => !z, 2 => c, 3 => !c, 4 => n, 5 => !n, 6 => v, 7 => !v,
        8 => c && !z, 9 => !c || z, 10 => n == v, 11 => n != v,
        12 => !z && n == v, 13 => z || n != v, 14 => true, _ => false,
    }
}
fn call_target(state: &Context, instruction: Word) -> Result<Option<Address>, String> {
    let code = instruction.0;
    if !condition(state, code) { return Ok(None); }
    let target = if code & 0x0f00_0000 == 0x0b00_0000 {
        let offset = ((code << 8) as i32 >> 6) as u32;
        state.pc.0.wrapping_add(8).wrapping_add(offset)
    } else if code & 0x0fff_fff0 == 0x012f_ff30 {
        let register = (code & 15) as usize;
        match register { 0..=12 => state.registers[register].0, 13 => state.sp.0, 14 => state.lr.0, _ => state.pc.0.wrapping_add(8) }
    } else { return Ok(None); };
    require(target & 3 == 0, "dynamic call target is not A32 aligned")?;
    Ok(Some(Address(target)))
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Flag {
    Role, Run, Update, Canonical, Rom, Analysis, InstallerImage, InstallerSymbols,
    InstallerContract, DispatcherImage, DispatcherSymbols, DispatcherContract, JoinedContract, Output,
}
fn arguments() -> Result<(Role, BTreeMap<Flag, PathBuf>), String> {
    let mut values = BTreeMap::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let key = match flag.to_str().ok_or(USAGE)? {
            "--role" => Flag::Role, "--run" => Flag::Run, "--update" => Flag::Update,
            "--canonical-dat" => Flag::Canonical, "--boot-rom" => Flag::Rom,
            "--analysis" => Flag::Analysis, "--installer-image" => Flag::InstallerImage,
            "--installer-symbols" => Flag::InstallerSymbols, "--installer-contract" => Flag::InstallerContract,
            "--dispatcher-image" => Flag::DispatcherImage, "--dispatcher-symbols" => Flag::DispatcherSymbols,
            "--dispatcher-contract" => Flag::DispatcherContract, "--joined-contract" => Flag::JoinedContract,
            "--output" => Flag::Output, _ => return Err(USAGE.into()),
        };
        require(values.insert(key, PathBuf::from(args.next().ok_or(USAGE)?)).is_none(), "duplicate argument")?;
    }
    let role = match values.remove(&Flag::Role).as_deref().and_then(Path::to_str) {
        Some("retained") => Role::Retained, Some("installer") => Role::Installer, Some("joined") => Role::Joined,
        _ => return Err(USAGE.into()),
    };
    require(values.len() == 13, USAGE)?;
    Ok((role, values))
}
#[derive(Deserialize)]
struct Artifact {
    schema: u32, linked_sha256: Digest, symbols_sha256: Digest, patch: [Address; 2],
}
#[derive(Deserialize)]
struct InstallerContract { schema: u32, candidate_dat_sha256: Digest, kernel_install: Artifact }
#[derive(Deserialize)]
struct DispatcherContract { schema: u32, startup_dispatch: Artifact }
#[derive(Deserialize)]
struct Composition { schema: u32, kernel_install: Artifact, startup_dispatch: Artifact, joined_image4_sha256: Digest }
#[derive(Deserialize)]
struct JoinedContract { schema: u32, candidate_dat_sha256: Digest, candidate_image4_sha256: Digest, composition: Composition }
fn bind_artifact(artifact: &Artifact, image: &[u8], symbols: &[u8], patch: [Address; 2]) -> Result<(), String> {
    require(artifact.schema == 1 && artifact.patch == patch && artifact.linked_sha256 == digest(image)
        && artifact.symbols_sha256 == digest(symbols) && !image.is_empty() && image.len()%4 == 0
        && image.len() <= (patch[1].0-patch[0].0) as usize, "component artifact binding differs")
}
fn patch_image(image4: &mut [u8], image: &[u8], patch: [Address; 2]) -> Result<(), String> {
    let start = patch[0].0.checked_sub(0x61c400).ok_or("patch below image4")? as usize;
    let end = patch[1].0.checked_sub(0x61c400).ok_or("patch below image4")? as usize;
    let window = image4.get_mut(start..end).ok_or("patch outside image4")?;
    window[..image.len()].copy_from_slice(image);
    for word in window[image.len()..].chunks_exact_mut(4) { word.copy_from_slice(&0xe320_f000u32.to_le_bytes()); }
    Ok(())
}
fn pinned(root: &Path, path: &str, expected: &str) -> Result<Vec<u8>, String> {
    let bytes = read(&root.join(path))?;
    require(sha256(&bytes) == expected, "canonical analysis input differs")?;
    Ok(bytes)
}
fn merge_watch_ranges(mut ranges: Vec<WatchRange>) -> Vec<WatchRange> {
    ranges.sort_unstable_by_key(|range| range.start);
    let mut result: Vec<WatchRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = result.last_mut().filter(|last| range.start <= last.end) { last.end = last.end.max(range.end); }
        else { result.push(range); }
    }
    result
}
fn backed_ranges(document: &RunDocument) -> Result<Vec<WatchRange>, String> {
    let mut regions = run::memory_backings(document)?.into_iter().map(|region| {
        let end = region.base.checked_add(region.size as u64).ok_or("backing overflow")?;
        require(region.base < end && end <= 1u64<<32, "backing extent differs")?;
        Ok(WatchRange { start: region.base, end, kinds: WatchKinds::ALL })
    }).collect::<Result<Vec<_>, String>>()?;
    regions.sort_unstable_by_key(|region| region.start);
    let mut result: Vec<WatchRange> = Vec::with_capacity(regions.len());
    for region in regions {
        if let Some(last) = result.last_mut().filter(|last| region.start <= last.end) { last.end = last.end.max(region.end); }
        else { result.push(region); }
    }
    require(!result.is_empty() && result.len() <= 64, "fetch attribution backing range bound differs")?;
    Ok(result)
}
fn stopped_points(machine: &Machine) -> Result<Vec<StopPoint>, String> {
    machine.traces.hits.iter().map(|hit| {
        let raw = hit.address.strip_prefix("0x").ok_or("instruction point address is not hexadecimal")?;
        let pc = u32::from_str_radix(raw, 16).map_err(|error| error.to_string())?;
        Ok(StopPoint { core: Core::try_from(hit.core)?, pc: Address(pc), instructions: hit.instructions })
    }).collect()
}
fn record_fetches(maps: &[MemoryMap], receipt: &mut Receipt, segment: &Segment) -> Result<(), String> {
    let mut progress = segment.start_counts;
    let begin = segment.start_counts.iter().sum::<u64>();
    let settled = segment.end_counts.iter().sum::<u64>();
    let mut boundary_cores = BTreeSet::new();
    let mut boundary_rows = BTreeSet::new();
    for core in [Core::Primary, Core::Secondary] {
        let index = usize::from(core);
        require(segment.start_counts[index] <= segment.end_counts[index], "per-core counter decreased")?;
        require(segment.start_contexts[index].counts == segment.start_counts
            && segment.end_contexts[index].counts == segment.end_counts, "fetch endpoint context counts differ")?;
    }
    for point in &segment.stops {
        require(boundary_cores.insert(point.core), "duplicate stopped core")?;
        let end = &segment.end_contexts[usize::from(point.core)];
        require(point.pc == end.pc && point.instructions == settled, "instruction point differs from settled endpoint")?;
        let index = segment.watch.records.iter().rposition(|row| row.source == Source::CpuFetch
            && row.core == Some(point.core) && row.pc == Some(point.pc) && row.instructions == Some(point.instructions))
            .ok_or("stopped instruction point fetch absent")?;
        require(boundary_rows.insert(index), "stopped instruction points share a fetch row")?;
    }
    let mut resumes = [false; 2];
    for core in [Core::Primary, Core::Secondary] {
        let index = usize::from(core);
        let start = &segment.start_contexts[index];
        let base = u64::from(physical(maps, start, start.pc, 4)?.0);
        resumes[index] = segment.fetch_ranges.iter().any(|range| range.0[0] <= base && base + 4 <= range.0[1])
            && segment.start_counts[index] < segment.end_counts[index];
    }
    let mut last_clock = begin;
    let mut last_tick = None;
    for (index, row) in segment.watch.records.iter().enumerate().filter(|(_, row)| row.source == Source::CpuFetch) {
        let core = row.core.ok_or("fetch core absent")?;
        let cpu = usize::from(core);
        let clock = row.instructions.ok_or("fetch clock absent")?;
        let ordinal = row.core_instructions.ok_or("fetch own-core count absent")?.0;
        let tick = row.tick.ok_or("fetch tick absent")?;
        let base = row.physical.ok_or("fetch physical projection absent")?;
        let end = base.checked_add(row.size).ok_or("fetch physical extent overflow")?;
        require(row.size != 0 && segment.fetch_ranges.iter().any(|range| range.0[0] <= base && end <= range.0[1]),
            "fetch falls outside declared scope")?;
        let peer_count = clock.checked_sub(ordinal).ok_or("fetch own-core count exceeds global clock")?;
        require(clock >= last_clock && clock <= settled && ordinal <= segment.end_counts[cpu]
            && peer_count >= progress[1-cpu] && peer_count <= segment.end_counts[1-cpu]
            && last_tick.is_none_or(|previous| tick >= previous), "fetch clocks or per-core counts exceed actual endpoints")?;
        last_tick = Some(tick);
        if boundary_rows.contains(&index) {
            require(ordinal == segment.end_counts[cpu], "stopped fetch own-core count differs")?;
            receipt.fetch_attempts.push(FetchAttempt {
                segment: segment.id, row: index as u64, classification: FetchClassification::BoundaryAttempt,
            });
            continue;
        }
        if resumes[cpu] {
            require(row.pc == Some(segment.start_contexts[cpu].pc)
                && ordinal == segment.start_counts[cpu] + 1, "in-scope resumed fetch differs from paused entry")?;
            resumes[cpu] = false;
        }
        require(clock > last_clock && ordinal > progress[cpu], "executed fetch clock or own-core count does not advance")?;
        if segment.fetch_scope == FetchScope::Complete {
            require(clock == progress.iter().sum::<u64>() + 1 && ordinal == progress[cpu] + 1,
                "fetch clock does not advance exact scheduler")?;
        }
        progress[cpu] = ordinal;
        progress[1-cpu] = peer_count;
        last_clock = clock;
    }
    require(!resumes.into_iter().any(|pending| pending), "in-scope executed resumption absent")?;
    if segment.fetch_scope == FetchScope::Complete {
        require(progress == segment.end_counts, "executed fetches do not reconcile per-core counts")?;
    }
    Ok(())
}
fn verify_loaded(machine: &Machine, maps: &[MemoryMap], state: &Context, image4: &[u8], loader_a: &[u8], loader_b: &[u8]) -> Result<(), String> {
    for (base, expected) in [
        (0x61c400,image4),
        (0x42634,loader_a.get(0x2634..0x26a8).ok_or("loader A cache source absent")?),
        (0xd0430,loader_b.get(0x430..0xab4).ok_or("loader B initializer source absent")?),
        (0xd1140,loader_b.get(0x1140..0x1188).ok_or("loader B selected service source absent")?),
    ] {
        for sample in views(machine, maps, state, Address(base), expected.len() as u64)? {
            require(sample.bytes.0 == expected, "loaded native/source code differs")?;
        }
    }
    Ok(())
}
fn observe(runtime: &mut Runtime, machine: &mut Machine, image4: &[u8], loader_a: &[u8], loader_b: &[u8], output: &Path) -> Result<(), String> {
    machine.traces.bound_full_registers(NonZeroUsize::new(LIMITS.probes as usize).ok_or("zero trace bound")?);
    let initial_sites = FAMILIES.iter().map(|family| family.entry()).chain(GLOBAL_PCS.iter().copied())
        .map(|pc| (pc, format!("consumer-{pc:x}"))).collect();
    machine.traces.arm(&machine.session, initial_sites)?;
    machine.traces.stop_labels = machine.traces.sites.values().cloned().collect();
    let boot_end = machine.instructions().checked_add(BOOT_LIMIT).ok_or("boot deadline overflow")?;
    loop {
        require(machine.instructions() < boot_end, "outer phase0 entry not observed before boot limit")?;
        machine.traces.hits.clear();
        let result = machine.run(boot_end-machine.instructions());
        require(result == RunOutcome::Stopped, "natural startup did not reach an armed source boundary")?;
        let cores = machine.traces.hits.iter().map(|hit| Core::try_from(hit.core)).collect::<Result<Vec<_>,_>>()?;
        let mut outer = None;
        for core in cores {
            let state = context(machine, core)?;
            if core == Core::Primary && state.pc == Address(0x69cdf8) && state.registers[0] == Word(0) { outer = Some(state); }
            else if let Some(family) = FAMILIES.into_iter().find(|family| family.entry() == state.pc.0) {
                runtime.family_entry(machine, &state, family, ExecutionFrame::observed(&state)?)?;
                return Err("first consumer entered before fixed-view observation boundary".into());
            }
        }
        if let Some(state) = outer {
            require(state.sp == Address(0x40063450) && state.lr == Address(0x40a00), "outer natural caller differs")?;
            verify_loaded(machine, &runtime.maps, &state, image4, loader_a, loader_b)?;
            arm_sites(machine, vec![state.lr.0])?;
            break;
        }
    }
    let ranges = backed_ranges(&machine.document)?;
    let mut initialized = false;
    while machine.instructions() < LIMITS.instructions {
        require(runtime.receipt.segments.len() < LIMITS.segments as usize, "aggregate segment bound reached")?;
        let available = LIMITS.records.checked_sub(runtime.retained_records).ok_or("aggregate record count overflow")?;
        require(available != 0, "aggregate memory record bound reached")?;
        let start_contexts = contexts(machine)?;
        let start_counts = counts(machine)?;
        let plan = runtime.watch_ranges(machine, &ranges, &start_contexts)?;
        let segment_instructions = match plan.fetch_scope {
            FetchScope::Complete => InstructionBudget(LIMITS.segment_instructions),
            FetchScope::Scoped => LIMITS.scoped_segment_instructions,
        };
        let probe_begin = ProbeSequence(runtime.receipt.probes.len() as u64);
        let attribution = FetchAttribution::new(CpuDataAttribution::ReadsAndWrites,
            ranges.iter().map(|range| WatchRange { start: range.start, end: range.end, kinds: WatchKinds::CPU_FETCH }).collect())?;
        let mut watch = machine.session.board.memory_watch_begin_with_fetch_attribution(
            plan.classes, attribution, LIMITS.watch_records.min(available) as usize, runtime.maps.clone(), ViewPolicy::Fixed)?;
        let mut stops = Vec::new();
        let body = (|| -> Result<(), String> {
            if !initialized {
                runtime.initial_resources(machine, &start_contexts[0])?;
                runtime.handle(machine, &start_contexts[0])?;
                initialized = true;
            }
            machine.traces.hits.clear();
            let result = machine.run(segment_instructions.0.min(LIMITS.instructions-machine.instructions()));
            stops = stopped_points(machine)?;
            let cores = machine.traces.hits.iter().map(|hit| Core::try_from(hit.core)).collect::<Result<Vec<_>,_>>()?;
            for core in cores {
                let state = context(machine, core)?;
                runtime.handle(machine, &state)?;
            }
            require(matches!(result, RunOutcome::Stopped | RunOutcome::Completed), "native observation stopped with a machine/capture failure")?;
            if runtime.finished() || machine.instructions() == LIMITS.instructions {
                let terminal = context(machine, Core::Primary)?;
                for owner in runtime.frames.iter().map(|frame| Some(frame.id)).chain(std::iter::once(None)).collect::<Vec<_>>() {
                    runtime.close(machine, &terminal, owner, Lifetime::Allocation)?;
                }
                runtime.close(machine, &terminal, None, Lifetime::Observation)?;
            }
            Ok(())
        })();
        let end_contexts = contexts(machine);
        let end_counts = counts(machine);
        let raw = serde_json::to_vec(&watch.records.borrow().state()).map_err(|error| error.to_string());
        let end_error = machine.session.board.memory_watch_end(&mut watch).err();
        let raw = raw?;
        let state: WatchState = match serde_json::from_slice(&raw) {
            Ok(state) => state,
            Err(error) => {
                write(&output.join(format!("unparsed-watch-{}.json", runtime.receipt.segments.len())), &raw)?;
                return Err(format!("watch serialization: {error}; teardown: {end_error:?}"));
            }
        };
        if end_contexts.is_err() || end_counts.is_err() {
            write(&output.join(format!("unclosed-watch-{}.json", runtime.receipt.segments.len())), &raw)?;
        }
        let segment = Segment {
            id: SegmentId(runtime.receipt.segments.len() as u64), start_counts, end_counts: end_counts?,
            start_contexts, end_contexts: end_contexts?, watch: state, end_error, stops,
            fetch_scope: plan.fetch_scope, fetch_ranges: plan.fetch_ranges, armed_sites: plan.armed_sites,
            probe_range: [probe_begin, ProbeSequence(runtime.receipt.probes.len() as u64)],
        };
        runtime.retained_records = runtime.retained_records.checked_add(segment.watch.records.len() as u64).ok_or("aggregate record count overflow")?;
        let refusal = segment.watch.violation.as_ref().map(|error| format!("continuous memory watch refused: {error}"))
            .or_else(|| segment.end_error.as_ref().map(|error| format!("continuous memory watch teardown: {error}")))
            .or_else(|| (!segment.watch.enabled).then(|| "continuous memory watch disabled".to_owned()));
        let fetches = if refusal.is_none() && body.is_ok() { record_fetches(&runtime.maps, &mut runtime.receipt, &segment) } else { Ok(()) };
        runtime.receipt.segments.push(segment);
        if let Err(error) = body {
            return Err(match refusal { Some(watch) => format!("{error}; {watch}"), None => error });
        }
        if let Some(error) = refusal { return Err(error); }
        fetches?;
        if runtime.finished() { return Ok(()); }
    }
    Ok(())
}
fn drive() -> Result<(), String> {
    let (role, mut args) = arguments()?;
    let mut take = |key| args.remove(&key).ok_or_else(|| USAGE.to_owned());
    let run_bytes = read(&take(Flag::Run)?)?;
    let update = read(&take(Flag::Update)?)?;
    let canonical = read(&take(Flag::Canonical)?)?;
    let rom = read(&take(Flag::Rom)?)?;
    let analysis = take(Flag::Analysis)?;
    let installer_image = read(&take(Flag::InstallerImage)?)?;
    let installer_symbols = read(&take(Flag::InstallerSymbols)?)?;
    let installer_contract_bytes = read(&take(Flag::InstallerContract)?)?;
    let dispatcher_image = read(&take(Flag::DispatcherImage)?)?;
    let dispatcher_symbols = read(&take(Flag::DispatcherSymbols)?)?;
    let dispatcher_contract_bytes = read(&take(Flag::DispatcherContract)?)?;
    let joined_contract_bytes = read(&take(Flag::JoinedContract)?)?;
    let output = take(Flag::Output)?;
    let installer_contract: InstallerContract = serde_json::from_slice(&installer_contract_bytes).map_err(|error| error.to_string())?;
    let dispatcher_contract: DispatcherContract = serde_json::from_slice(&dispatcher_contract_bytes).map_err(|error| error.to_string())?;
    let joined_contract: JoinedContract = serde_json::from_slice(&joined_contract_bytes).map_err(|error| error.to_string())?;
    require(installer_contract.schema == 1 && dispatcher_contract.schema == 1 && joined_contract.schema == 1 && joined_contract.composition.schema == 1, "native contract schema differs")?;
    let installer_patch = [Address(0x725a04),Address(0x725b54)];
    let dispatcher_patch = [Address(0x69cdf8),Address(0x69cf84)];
    bind_artifact(&installer_contract.kernel_install, &installer_image, &installer_symbols, installer_patch)?;
    bind_artifact(&dispatcher_contract.startup_dispatch, &dispatcher_image, &dispatcher_symbols, dispatcher_patch)?;
    bind_artifact(&joined_contract.composition.kernel_install, &installer_image, &installer_symbols, installer_patch)?;
    bind_artifact(&joined_contract.composition.startup_dispatch, &dispatcher_image, &dispatcher_symbols, dispatcher_patch)?;
    require(sha256(&canonical) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490", "canonical DAT differs")?;
    require(digest(&update) == match role {
        Role::Retained => digest(&canonical), Role::Installer => installer_contract.candidate_dat_sha256,
        Role::Joined => joined_contract.candidate_dat_sha256,
    }, "update differs from role binding")?;
    require(sha256(&run_bytes) == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && sha256(&rom) == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "run/ROM binding differs")?;
    let canonical_image4 = pinned(&analysis,"lz/id4-0061c400.bin","f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")?;
    let loader_a = pinned(&analysis,"decode/loader-40000.bin","21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")?;
    let loader_b = pinned(&analysis,"decode/loader-d0000.bin","8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")?;
    let mut image4 = canonical_image4.clone();
    if role != Role::Retained { patch_image(&mut image4, &installer_image, installer_patch)?; }
    if role == Role::Joined { patch_image(&mut image4, &dispatcher_image, dispatcher_patch)?; }
    if role == Role::Joined {
        require(digest(&image4) == joined_contract.candidate_image4_sha256 && digest(&image4) == joined_contract.composition.joined_image4_sha256, "joined image4 binding differs")?;
    }
    let identity = Identity {
        run: digest(&run_bytes), update: digest(&update), canonical_dat: digest(&canonical),
        image4: digest(&canonical_image4), loader_a: digest(&loader_a), loader_b: digest(&loader_b),
        boot_rom: digest(&rom), joined_contract: digest(&joined_contract_bytes),
        observer: digest(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?),
        components: vec![
            ComponentBinding { kind: ComponentKind::Installer, image: digest(&installer_image), symbols: digest(&installer_symbols), contract: digest(&installer_contract_bytes) },
            ComponentBinding { kind: ComponentKind::Dispatcher, image: digest(&dispatcher_image), symbols: digest(&dispatcher_symbols), contract: digest(&dispatcher_contract_bytes) },
        ],
    };
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    write(&output.join("reset.fujimod"), &checkpoint)?;
    let imported = machine.session.import_snapshot(&read(&output.join("reset.fujimod"))?)?;
    machine.session.restore(&imported)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "checkpoint is not fresh reset")?;
    let receipt = Receipt {
        schema: 1, role, outcome: Outcome::NonObservation, error: None, machine_failure: None,
        identity, checkpoint_sha256: digest(&checkpoint), checkpoint_reused: false, instructions: 0,
        limits: LIMITS, probes: Vec::new(), invocations: Vec::new(), resources: Vec::new(),
        segments: Vec::new(), fetch_attempts: Vec::new(), payloads: Vec::new(),
        census: FAMILIES.into_iter().flat_map(|family| [Core::Primary, Core::Secondary].into_iter()
            .map(move |core| Census { family, core, entries: 0, first_probe: None })).collect(),
    };
    let mut runtime = Runtime {
        receipt, maps, frames: Vec::new(), calls: Vec::new(), global_returns: BTreeMap::new(),
        globals_seen: BTreeSet::new(), installed: false, outer_returned: false,
        carriers: [(0x61c400,image4.len()),(0x40000,loader_a.len()),(0xd0000,loader_b.len())].into_iter()
            .map(|(base, size)| WatchRange { start: base, end: base + size as u64, kinds: WatchKinds::WRITES }).collect(),
        peer_poll: None, peer_ready: false,
        retained_records: 0, captured_bytes: 0, captured_views: 0,
        shared_sites: None,
    };
    let result = observe(&mut runtime, &mut machine, &image4, &loader_a, &loader_b, &output);
    runtime.receipt.outcome = if result.is_err() { Outcome::Refused } else if runtime.finished() { Outcome::Observed } else { Outcome::NonObservation };
    runtime.absent();
    runtime.receipt.error = result.as_ref().err().cloned();
    runtime.receipt.machine_failure = machine.failure.clone();
    runtime.receipt.instructions = machine.instructions();
    let file = std::fs::File::create_new(output.join("consumers.json")).map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &runtime.receipt).map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    result
}
fn main() -> ExitCode {
    match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}
