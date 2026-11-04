//! Attach-boundary state: the contract's comparison rules, the observer's capture and the retained/composed
//! comparison of two captures.
use crate::composed_catalog::{Bank, Boundary, Citation, Core, Label, PieceKind, Reason, Sha256, Target, TargetSpan};
use crate::attach_relations::{self, AttachSources, RelationFailure, RoleInput, RoleRelationSummary, RoleRelations, Relations};
use crate::run::{self, RunDocument};
use crate::composed_receipt::{Difference, Row, Word};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use fujimod_emulator::board::{Board, factory::Graph};
use fujimod_emulator_cpu::{Register, arm};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

pub const PAGE: usize = 4096;
pub const NONZERO_PAGE_LIMIT: u64 = 65_536;
pub const COMPRESSED_LIMIT: u64 = 256 << 20;
pub const BOARD_STATE_LIMIT: u64 = 64 << 20;
/// The largest TCM region encoding, 512 << 14.
pub const TCM_WINDOW_LIMIT: u64 = 8 << 20;
pub const DECLARED_SPAN_LIMIT: u64 = 64 << 10;
pub const ALLOWANCE_LIMIT: usize = 256;
pub const REGISTER_LIMIT: usize = 32;
pub const BOARD_LIMIT: usize = 256;
/// Absorbed ranges recorded per allowance in the verdict.
pub const ABSORBED_RANGE_LIMIT: usize = 64;
/// Bytes of each role recorded per allowance in the verdict.
pub const ABSORBED_BYTE_LIMIT: usize = 4096;
/// JSON bytes of each role's value recorded in a board difference.
pub const BOARD_VALUE_LIMIT: usize = 4096;
/// Per `AttachMemory` difference.
pub const DIFFERING_RUN_LIMIT: usize = 64;
pub const ATTACH_DIFFERENCE_LIMIT: usize = 256;
pub const MEMORY_FILE: &str = "attach-state.bin.gz";
pub const BOARD_FILE: &str = "attach-board.json.gz";
const MAGIC: &[u8] = b"xm1-attach-state\x01";
const END_MARK: u32 = u32::MAX;
const ALIAS_BASE: u64 = 0x4000_0000;
const ALIAS_END: u64 = 0x6000_0000;
const ADDRESS_END: u64 = 1 << 32;
const READ_CHUNK: usize = 1 << 20;

/// Half-open, nonempty and within the 32-bit address space.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "RawSpan", into = "RawSpan")]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawSpan {
    start: u64,
    end: u64,
}

impl TryFrom<RawSpan> for Span {
    type Error = String;
    fn try_from(raw: RawSpan) -> Result<Self, Self::Error> { Span::new(raw.start, raw.end) }
}

impl From<Span> for RawSpan {
    fn from(span: Span) -> Self { Self { start: span.start, end: span.end } }
}

impl Span {
    pub fn new(start: u64, end: u64) -> Result<Self, String> {
        if start < end && end <= ADDRESS_END { Ok(Self { start, end }) } else { Err("span is empty or exceeds 32 bits".into()) }
    }
    pub fn holds(self, address: u64) -> bool { self.start <= address && address < self.end }
    pub fn contains(self, other: Span) -> bool { self.start <= other.start && other.end <= self.end }
    pub fn intersects(self, other: Span) -> bool { other.start < self.end && self.start < other.end }
    pub fn len(self) -> u64 { self.end - self.start }
}

/// A physical address range, or a range of offsets within one core's TCM bank.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "target", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Space {
    Physical,
    Tcm { core: Core, bank: Bank },
}

/// CPSR[4:0].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArmMode {
    Usr,
    Fiq,
    Irq,
    Svc,
    Abt,
    Und,
    Sys,
}

impl ArmMode {
    pub fn of(cpsr: Word) -> Option<Self> {
        Some(match cpsr.0 & 0x1f {
            0x10 => Self::Usr,
            0x11 => Self::Fiq,
            0x12 => Self::Irq,
            0x13 => Self::Svc,
            0x17 => Self::Abt,
            0x1b => Self::Und,
            0x1f => Self::Sys,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextRegister {
    R0,
    R1,
    R2,
    R3,
    R4,
    R5,
    R6,
    R7,
    R8,
    R9,
    R10,
    R11,
    R12,
    Sp,
    Lr,
    Pc,
    Cpsr,
    Spsr,
    Cpacr,
    Fpexc,
    Fpscr,
    Sctlr,
    TcmData,
    TcmInstruction,
}

impl ContextRegister {
    const GENERAL: [Self; 13] =
        [Self::R0, Self::R1, Self::R2, Self::R3, Self::R4, Self::R5, Self::R6, Self::R7, Self::R8, Self::R9, Self::R10, Self::R11, Self::R12];
    /// Compared on every core.
    const SYSTEM: [Self; 5] = [Self::TcmData, Self::TcmInstruction, Self::Sctlr, Self::Cpacr, Self::Fpexc];
    fn masked(self) -> bool { matches!(self, Self::Cpsr | Self::Spsr) }
}

/// One core's readable state at the attach stop.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreContext {
    pub core: Core,
    pub r: [Word; 13],
    pub sp: Word,
    pub lr: Word,
    pub pc: Word,
    pub cpsr: Word,
    pub spsr: Word,
    pub cpacr: Word,
    pub fpexc: Word,
    pub fpscr: Word,
    pub sctlr: Word,
    /// CP15 c9,c1,0 (data) and c9,c1,1 (instruction).
    pub tcm: [Word; 2],
}

impl CoreContext {
    pub fn get(&self, register: ContextRegister) -> Word {
        use ContextRegister as R;
        match register {
            R::R0 | R::R1 | R::R2 | R::R3 | R::R4 | R::R5 | R::R6 | R::R7 | R::R8 | R::R9 | R::R10 | R::R11 | R::R12 => {
                self.r[R::GENERAL.iter().position(|general| *general == register).unwrap_or_default()]
            }
            R::Sp => self.sp,
            R::Lr => self.lr,
            R::Pc => self.pc,
            R::Cpsr => self.cpsr,
            R::Spsr => self.spsr,
            R::Cpacr => self.cpacr,
            R::Fpexc => self.fpexc,
            R::Fpscr => self.fpscr,
            R::Sctlr => self.sctlr,
            R::TcmData => self.tcm[0],
            R::TcmInstruction => self.tcm[1],
        }
    }
    /// The enabled window `[base, end)` of `bank`.
    fn tcm_window(&self, bank: Bank) -> Option<(u64, u64)> {
        tcm_window(self.tcm[bank_index(bank)].0)
    }
}

fn bank_index(bank: Bank) -> usize {
    match bank {
        Bank::Data => 0,
        Bank::Instruction => 1,
    }
}

/// Cortex-R TCM region register: base [31:12], size code [6:2], enable [0].
fn tcm_window(register: u32) -> Option<(u64, u64)> {
    let code = (register >> 2) & 0x1f;
    if register & 1 == 0 || !(3..=14).contains(&code) {
        return None;
    }
    let base = u64::from(register & 0xffff_f000);
    let end = base + (512u64 << code);
    (end <= ADDRESS_END).then_some((base, end))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegionRecord {
    pub space: Space,
    /// Physical address, or 0 for a TCM bank.
    pub start: u64,
    pub end: u64,
    /// Tile the region in order.
    pub reads: Vec<RegionRead>,
    /// Over every byte of the region, zero pages included.
    pub sha256: Sha256,
    pub nonzero_pages: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    /// Relative to the receipt's directory.
    pub path: String,
    pub sha256: Sha256,
    pub compressed_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachState {
    pub instructions: [u64; 2],
    pub cores: [CoreContext; 2],
    pub regions: Vec<RegionRecord>,
    /// RAM backing extents that no stable physical-bus mapping and no stable processor mapping reaches; not captured.
    pub unreachable: Vec<Span>,
    pub memory_file: FileRecord,
    pub board_file: FileRecord,
}

macro_rules! kebab_label {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let kebab = !value.is_empty() && !value.starts_with('-') && !value.ends_with('-')
                    && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
                if kebab { Ok(Self(value)) } else { Err("label is not a kebab-case name".into()) }
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self { value.0 }
        }
        impl $name {
            pub fn as_str(&self) -> &str { &self.0 }
        }
    };
}

kebab_label!(AllowanceLabel);
kebab_label!(SiteLabel);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transform {
    Identity,
    Complement,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum CopySource {
    Dat { offset: u64, transform: Transform },
    Target { target: Target, start: u64 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "RawWindow", into = "RawWindow")]
pub struct Window {
    pub space: Space,
    pub span: Span,
    pub citation: Citation,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    space: Space,
    start: u64,
    end: u64,
    citation: Citation,
}

impl TryFrom<RawWindow> for Window {
    type Error = String;
    fn try_from(raw: RawWindow) -> Result<Self, Self::Error> {
        Ok(Self { space: raw.space, span: Span::new(raw.start, raw.end)?, citation: raw.citation })
    }
}

impl From<Window> for RawWindow {
    fn from(window: Window) -> Self {
        Self { space: window.space, start: window.span.start, end: window.span.end, citation: window.citation }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AllowanceKind {
    Copy { source: CopySource, canonical_sha256: Sha256, composed_sha256: Sha256 },
    DeadStack { core: Core, mode: ArmMode, window: Window },
    DeadStackAtBoundary { core: Core, boundary: Label, window: Window },
    PeerLiveStack { core: Core, window: Window },
    Declared { citation: Citation },
}

impl AllowanceKind {
    fn tag(&self) -> AllowanceKindTag {
        match self {
            Self::Copy { .. } => AllowanceKindTag::Copy,
            Self::DeadStack { .. } => AllowanceKindTag::DeadStack,
            Self::DeadStackAtBoundary { .. } => AllowanceKindTag::DeadStackAtBoundary,
            Self::PeerLiveStack { .. } => AllowanceKindTag::PeerLiveStack,
            Self::Declared { .. } => AllowanceKindTag::Declared,
        }
    }
}

/// The holder of a span that another span overlaps.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "name", rename_all = "kebab-case")]
pub enum Occupant {
    Allowance(AllowanceLabel),
    Carrier(PieceKind),
}

/// The stack-allowance check that failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StackCheck {
    Mode,
    SpSpace,
    SpOutsideWindow,
    AboveSp,
    BoundaryRow,
    StillCurrent,
    BelowPeerSp,
}

/// An allowance entry that absorbed no difference.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "entry", rename_all = "kebab-case")]
pub enum UnusedEntry {
    Memory(AllowanceLabel),
    Register { core: Core, register: ContextRegister },
    Board(BoardPointer),
}

/// An RFC 6901 pointer to one leaf of `Board::state()`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BoardLeaf(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Allowance {
    pub label: AllowanceLabel,
    pub space: Space,
    pub span: Span,
    pub reason: Reason,
    pub kind: AllowanceKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AllowanceKindTag {
    Copy,
    DeadStack,
    DeadStackAtBoundary,
    PeerLiveStack,
    Declared,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAllowance {
    label: AllowanceLabel,
    kind: AllowanceKindTag,
    space: Space,
    start: u64,
    end: u64,
    reason: Reason,
    #[serde(default)]
    citation: Option<Citation>,
    #[serde(default)]
    source: Option<CopySource>,
    #[serde(default)]
    canonical_sha256: Option<Sha256>,
    #[serde(default)]
    composed_sha256: Option<Sha256>,
    #[serde(default)]
    core: Option<Core>,
    #[serde(default)]
    mode: Option<ArmMode>,
    #[serde(default)]
    boundary: Option<Label>,
    #[serde(default)]
    window: Option<Window>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub core: Core,
    pub retained_pc: Vec<Span>,
    pub composed_pc: Vec<Span>,
    pub reason: Reason,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "residency", rename_all = "kebab-case")]
pub enum Residency {
    Resident { canonical_sha256: Sha256, composed_sha256: Sha256 },
    Absent { reason: Reason },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RawResidency {
    Resident,
    Absent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCarrierPart {
    start: u64,
    end: u64,
    residency: RawResidency,
    #[serde(default)]
    reason: Option<Reason>,
    #[serde(default)]
    canonical_sha256: Option<Sha256>,
    #[serde(default)]
    composed_sha256: Option<Sha256>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCarrier {
    piece: PieceKind,
    target: Target,
    start: u64,
    end: u64,
    parts: Vec<RawCarrierPart>,
}

/// A carrier sub-span in physical addresses.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CarrierPart {
    pub piece: PieceKind,
    pub target: Target,
    pub physical: Span,
    pub residency: Residency,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterAllowance {
    pub core: Core,
    pub register: ContextRegister,
    pub mask: Word,
    pub retained: Word,
    pub composed: Word,
    pub reason: Reason,
}

/// Top-level keys of `Board::state()`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BoardField {
    ExecutionMode,
    ExecutionPolicy,
    CountsAsBoot,
    Timeline,
    Instructions,
    Released,
    Waiting,
    WaitingEvent,
    Events,
    NextCore,
    SliceLeft,
    Quantum,
    Failure,
    MemoryDecoders,
    DmaMappings,
    Gic,
    CoreInterruptDevices,
    Outputs,
    ImplementationCluster,
    UnmodeledSystem,
    Aprofile,
    Mprofile,
    Bus,
    DiscoveryCalleeStubs,
}

impl BoardField {
    const ALL: [(Self, &'static str); 24] = [
        (Self::ExecutionMode, "execution_mode"),
        (Self::ExecutionPolicy, "execution_policy"),
        (Self::CountsAsBoot, "counts_as_boot"),
        (Self::Timeline, "timeline"),
        (Self::Instructions, "instructions"),
        (Self::Released, "released"),
        (Self::Waiting, "waiting"),
        (Self::WaitingEvent, "waiting_event"),
        (Self::Events, "events"),
        (Self::NextCore, "next_core"),
        (Self::SliceLeft, "slice_left"),
        (Self::Quantum, "quantum"),
        (Self::Failure, "failure"),
        (Self::MemoryDecoders, "memory_decoders"),
        (Self::DmaMappings, "dma_mappings"),
        (Self::Gic, "gic"),
        (Self::CoreInterruptDevices, "core_interrupt_devices"),
        (Self::Outputs, "outputs"),
        (Self::ImplementationCluster, "implementation_cluster"),
        (Self::UnmodeledSystem, "unmodeled_system"),
        (Self::Aprofile, "aprofile"),
        (Self::Mprofile, "mprofile"),
        (Self::Bus, "bus"),
        (Self::DiscoveryCalleeStubs, "discovery_callee_stubs"),
    ];
    fn parse(key: &str) -> Option<Self> { Self::ALL.iter().find(|(_, name)| *name == key).map(|(field, _)| *field) }
    /// Left out of the comparison.
    fn excluded(self) -> bool { matches!(self, Self::Timeline | Self::Instructions | Self::NextCore | Self::SliceLeft) }
    /// Compared without allowances.
    fn fixed(self) -> bool {
        matches!(self, Self::Quantum | Self::ExecutionMode | Self::ExecutionPolicy | Self::CountsAsBoot | Self::Failure | Self::DiscoveryCalleeStubs)
    }
}

/// An RFC 6901 pointer into `Board::state()` whose first segment is a `BoardField`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct BoardPointer(String);

impl TryFrom<String> for BoardPointer {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let pointer = Self(value);
        let segments = pointer.segments();
        let field = segments.first().and_then(|first| BoardField::parse(first)).ok_or("board pointer names no board field")?;
        if field.excluded() || field.fixed() {
            return Err("board pointer names a field outside the allowance set".into());
        }
        let deep_enough = match (field, segments.get(1).map(String::as_str)) {
            // `/bus/devices/<n>/state/<field>` or `/bus/devices/<n>/<field>` other than `state`.
            (BoardField::Bus, Some("devices")) => segments.len() >= 5 || (segments.len() == 4 && segments[3] != "state"),
            // An entry and one of its fields.
            (BoardField::Bus, Some("frontier" | "unmodeled")) => segments.len() >= 4,
            _ => segments.len() >= 2,
        };
        if !deep_enough {
            return Err("board pointer is above field depth".into());
        }
        Ok(pointer)
    }
}

impl From<BoardPointer> for String {
    fn from(value: BoardPointer) -> Self { value.0 }
}

impl BoardPointer {
    pub fn as_str(&self) -> &str { &self.0 }
    fn segments(&self) -> Vec<String> {
        match self.0.strip_prefix('/') {
            Some(rest) => rest.split('/').map(|segment| segment.replace("~1", "/").replace("~0", "~")).collect(),
            None => vec![],
        }
    }
    fn covers(&self, leaf: &str) -> bool { leaf == self.0 || leaf.strip_prefix(&self.0).is_some_and(|rest| rest.starts_with('/')) }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BoardAllowanceKind {
    Exact { retained: Value, composed: Value },
    Declared { citation: Citation },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BoardAllowance {
    pub pointer: BoardPointer,
    pub kind: BoardAllowanceKind,
    pub reason: Reason,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RawBoardKind {
    Exact,
    Declared,
}

/// A present key, `null` included.
fn present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBoardAllowance {
    pointer: BoardPointer,
    kind: RawBoardKind,
    #[serde(default, deserialize_with = "present")]
    retained: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    composed: Option<Value>,
    reason: Reason,
    #[serde(default)]
    citation: Option<Citation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawAttachRules {
    peers: Vec<Peer>,
    carriers: Vec<RawCarrier>,
    allowances: Vec<RawAllowance>,
    registers: Vec<RegisterAllowance>,
    board: Vec<RawBoardAllowance>,
    relations: Relations,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AttachRules {
    pub peers: Vec<Peer>,
    pub carriers: Vec<CarrierPart>,
    pub allowances: Vec<Allowance>,
    pub registers: Vec<RegisterAllowance>,
    pub board: Vec<BoardAllowance>,
    pub relations: Relations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttachRuleRefusal {
    Bound,
    /// Allowance kind fields missing or present for another kind.
    AllowanceShape(AllowanceLabel),
    DuplicateLabel(AllowanceLabel),
    Overlap { first: Occupant, second: Occupant },
    OutsideWindow(AllowanceLabel),
    WindowSpace(AllowanceLabel),
    PeerAttachCore(AllowanceLabel),
    DeclaredSize(AllowanceLabel),
    ForeignCore(Core),
    BoundaryUnknown(AllowanceLabel),
    Peers,
    PeerSpans(Core),
    /// Carrier entries do not match the contract's carriers one to one.
    Carriers,
    CarrierTiling { piece: PieceKind },
    CarrierPartShape { piece: PieceKind },
    RegisterMask { core: Core, register: ContextRegister },
    DuplicateRegister { core: Core, register: ContextRegister },
    BoardShape(BoardPointer),
    BoardOverlap(BoardPointer),
    /// The log span is unaligned, longer than 4 KiB or outside DRAM.
    LogSpan,
    /// A log site's store is not the instruction after its read, or its key or label repeats.
    LogSite(SiteLabel),
    LogSpanOverlap(AllowanceLabel),
    /// The LZ submitted word is unaligned, outside DRAM, over an allowance or the log span, or a record length is
    /// shorter than its header.
    LzSubmittedWord,
}

/// Contract facts the attach rules are checked against.
pub(crate) struct RuleContext<'a> {
    pub attach_core: Core,
    /// Cores with at least one boundary.
    pub cores: BTreeSet<Core>,
    pub boundaries: &'a BTreeMap<Label, Boundary>,
    /// Every piece carrier with its physical span.
    pub carriers: Vec<(PieceKind, TargetSpan, Span)>,
}

fn space_name(space: Space) -> String {
    match space {
        Space::Physical => "physical".into(),
        Space::Tcm { core, bank } => format!("tcm-{}-{bank:?}", core.index()),
    }
}

impl AttachRules {
    pub(crate) fn parse(raw: RawAttachRules, context: &RuleContext) -> Result<Self, AttachRuleRefusal> {
        use AttachRuleRefusal as Refuse;
        if raw.allowances.len() > ALLOWANCE_LIMIT || raw.registers.len() > REGISTER_LIMIT || raw.board.len() > BOARD_LIMIT {
            return Err(Refuse::Bound);
        }
        let peers: BTreeSet<Core> = raw.peers.iter().map(|peer| peer.core).collect();
        let expected: BTreeSet<Core> = Core::ALL.into_iter().filter(|core| *core != context.attach_core).collect();
        if peers != expected || raw.peers.len() != expected.len() {
            return Err(Refuse::Peers);
        }
        for peer in &raw.peers {
            if peer.retained_pc.is_empty() || peer.composed_pc.is_empty() {
                return Err(Refuse::PeerSpans(peer.core));
            }
            for spans in [&peer.retained_pc, &peer.composed_pc] {
                if spans.windows(2).any(|pair| pair[1].start < pair[0].end) {
                    return Err(Refuse::PeerSpans(peer.core));
                }
            }
        }
        let carriers = carrier_parts(raw.carriers, context)?;
        let mut allowances = Vec::with_capacity(raw.allowances.len());
        let mut labels = BTreeSet::new();
        for raw in raw.allowances {
            if !labels.insert(raw.label.clone()) {
                return Err(Refuse::DuplicateLabel(raw.label));
            }
            allowances.push(allowance(raw, context)?);
        }
        // Allowances are disjoint from each other and from resident carrier parts, per space.
        let mut occupied: Vec<(Space, Span, Occupant)> = allowances.iter()
            .map(|allowance| (allowance.space, allowance.span, Occupant::Allowance(allowance.label.clone())))
            .chain(carriers.iter().filter(|part| matches!(part.residency, Residency::Resident { .. }))
                .map(|part| (Space::Physical, part.physical, Occupant::Carrier(part.piece.clone()))))
            .collect();
        occupied.sort_by_key(|(space, span, _)| (*space, span.start));
        for pair in occupied.windows(2) {
            if pair[0].0 == pair[1].0 && pair[0].1.intersects(pair[1].1) {
                return Err(Refuse::Overlap { first: pair[0].2.clone(), second: pair[1].2.clone() });
            }
        }
        let mut registers = BTreeSet::new();
        for entry in &raw.registers {
            if !registers.insert((entry.core, entry.register)) {
                return Err(Refuse::DuplicateRegister { core: entry.core, register: entry.register });
            }
            let mask = entry.mask.0;
            if mask == 0 || (!entry.register.masked() && mask != u32::MAX) || (entry.retained.0 | entry.composed.0) & !mask != 0 {
                return Err(Refuse::RegisterMask { core: entry.core, register: entry.register });
            }
        }
        let mut board = Vec::with_capacity(raw.board.len());
        for entry in raw.board {
            let kind = match (entry.kind, entry.retained, entry.composed, entry.citation) {
                (RawBoardKind::Exact, Some(retained), Some(composed), None) => BoardAllowanceKind::Exact { retained, composed },
                (RawBoardKind::Declared, None, None, Some(citation)) => BoardAllowanceKind::Declared { citation },
                _ => return Err(Refuse::BoardShape(entry.pointer)),
            };
            board.push(BoardAllowance { pointer: entry.pointer, kind, reason: entry.reason });
        }
        for (index, entry) in board.iter().enumerate() {
            if let Some(other) = board.iter().enumerate()
                .find(|(other, allowance)| *other != index && allowance.pointer.covers(entry.pointer.as_str()))
            {
                return Err(Refuse::BoardOverlap(other.1.pointer.clone()));
            }
        }
        raw.relations.validate(&allowances)?;
        Ok(Self { peers: raw.peers, carriers, allowances, registers: raw.registers, board, relations: raw.relations })
    }
}

fn carrier_parts(raw: Vec<RawCarrier>, context: &RuleContext) -> Result<Vec<CarrierPart>, AttachRuleRefusal> {
    use AttachRuleRefusal as Refuse;
    if raw.len() != context.carriers.len() {
        return Err(Refuse::Carriers);
    }
    let mut parts = Vec::new();
    for (entry, (piece, carrier, physical)) in raw.into_iter().zip(&context.carriers) {
        if entry.piece != *piece || entry.target != carrier.target || (entry.start, entry.end) != (carrier.span.start, carrier.span.end) {
            return Err(Refuse::Carriers);
        }
        let mut cursor = entry.start;
        for part in entry.parts {
            if part.start != cursor || part.end <= part.start || part.end > entry.end {
                return Err(Refuse::CarrierTiling { piece: piece.clone() });
            }
            cursor = part.end;
            let residency = match (part.residency, part.reason, part.canonical_sha256, part.composed_sha256) {
                (RawResidency::Resident, None, Some(canonical_sha256), Some(composed_sha256)) => {
                    Residency::Resident { canonical_sha256, composed_sha256 }
                }
                (RawResidency::Absent, Some(reason), None, None) => Residency::Absent { reason },
                _ => return Err(Refuse::CarrierPartShape { piece: piece.clone() }),
            };
            let offset = physical.start - entry.start;
            parts.push(CarrierPart {
                piece: piece.clone(),
                target: carrier.target,
                physical: Span { start: part.start + offset, end: part.end + offset },
                residency,
            });
        }
        if cursor != entry.end {
            return Err(Refuse::CarrierTiling { piece: piece.clone() });
        }
    }
    Ok(parts)
}

fn allowance(raw: RawAllowance, context: &RuleContext) -> Result<Allowance, AttachRuleRefusal> {
    use AttachRuleRefusal as Refuse;
    let shape = || Refuse::AllowanceShape(raw.label.clone());
    let span = Span::new(raw.start, raw.end).map_err(|_| shape())?;
    let in_core = |core: Core| if context.cores.contains(&core) { Ok(()) } else { Err(Refuse::ForeignCore(core)) };
    if let Space::Tcm { core, .. } = raw.space {
        in_core(core)?;
    }
    let windowed = |window: &Window| {
        if let Space::Tcm { core, .. } = window.space {
            in_core(core)?;
        }
        if window.space != raw.space {
            return Err(Refuse::WindowSpace(raw.label.clone()));
        }
        if !window.span.contains(span) {
            return Err(Refuse::OutsideWindow(raw.label.clone()));
        }
        Ok(())
    };
    let kind = match raw.kind {
        AllowanceKindTag::Copy => match (&raw.source, &raw.canonical_sha256, &raw.composed_sha256, &raw.citation, raw.core, raw.mode, &raw.boundary, &raw.window) {
            (Some(source), Some(canonical), Some(composed), None, None, None, None, None) => {
                AllowanceKind::Copy { source: *source, canonical_sha256: canonical.clone(), composed_sha256: composed.clone() }
            }
            _ => return Err(shape()),
        },
        AllowanceKindTag::DeadStack => match (&raw.source, &raw.canonical_sha256, &raw.composed_sha256, &raw.citation, raw.core, raw.mode, &raw.boundary, &raw.window) {
            (None, None, None, None, Some(core), Some(mode), None, Some(window)) => {
                windowed(window)?;
                AllowanceKind::DeadStack { core, mode, window: window.clone() }
            }
            _ => return Err(shape()),
        },
        AllowanceKindTag::DeadStackAtBoundary => match (&raw.source, &raw.canonical_sha256, &raw.composed_sha256, &raw.citation, raw.core, raw.mode, &raw.boundary, &raw.window) {
            (None, None, None, None, Some(core), None, Some(boundary), Some(window)) => {
                windowed(window)?;
                let known = context.boundaries.get(boundary).is_some_and(|known| known.cores.contains(core) && known.record_limit >= 1);
                if !known {
                    return Err(Refuse::BoundaryUnknown(raw.label.clone()));
                }
                AllowanceKind::DeadStackAtBoundary { core, boundary: boundary.clone(), window: window.clone() }
            }
            _ => return Err(shape()),
        },
        AllowanceKindTag::PeerLiveStack => match (&raw.source, &raw.canonical_sha256, &raw.composed_sha256, &raw.citation, raw.core, raw.mode, &raw.boundary, &raw.window) {
            (None, None, None, None, Some(core), None, None, Some(window)) => {
                if core == context.attach_core {
                    return Err(Refuse::PeerAttachCore(raw.label.clone()));
                }
                windowed(window)?;
                AllowanceKind::PeerLiveStack { core, window: window.clone() }
            }
            _ => return Err(shape()),
        },
        AllowanceKindTag::Declared => match (&raw.source, &raw.canonical_sha256, &raw.composed_sha256, &raw.citation, raw.core, raw.mode, &raw.boundary, &raw.window) {
            (None, None, None, Some(citation), None, None, None, None) => {
                if span.len() > DECLARED_SPAN_LIMIT {
                    return Err(Refuse::DeclaredSize(raw.label.clone()));
                }
                AllowanceKind::Declared { citation: citation.clone() }
            }
            _ => return Err(shape()),
        },
    };
    Ok(Allowance { label: raw.label, space: raw.space, span, reason: raw.reason, kind })
}

/// Updates a SHA-256 and a byte count with everything written through it.
pub(crate) struct Hashing<W> {
    pub(crate) inner: W,
    pub(crate) hasher: sha2::Sha256,
    pub(crate) bytes: u64,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..written]);
        self.bytes += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> { self.inner.flush() }
}

pub(crate) fn hex(hasher: sha2::Sha256) -> Result<Sha256, String> { Sha256::try_from(format!("{:x}", hasher.finalize())) }

pub(crate) fn io(error: std::io::Error) -> String { error.to_string() }

/// A physical-bus address, translated to a backing by the bus mappings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BusAddress(pub u64);

/// A processor address on one core, translated by that core's mappings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProcessorAddress(pub u64);

/// The address a stretch's first byte is read at.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "via", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReadPath {
    Bus { address: BusAddress },
    Core { core: Core, address: ProcessorAddress },
}

/// Region offsets `[start, end)` read through `path`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegionRead {
    pub start: u64,
    pub end: u64,
    pub path: ReadPath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureRegion {
    pub space: Space,
    pub start: u64,
    pub end: u64,
    pub reads: Vec<RegionRead>,
}

/// The physical part of a capture, derived from the run's memory graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturePlan {
    pub regions: Vec<CaptureRegion>,
    pub unreachable: Vec<Span>,
}

/// Plans the capture of every RAM backing of the session `run::open` builds.
pub fn capture_plan(document: &RunDocument) -> Result<CapturePlan, String> {
    plan_backings(&run::graph(document)?.1)
}

/// Each backing byte is read through a stable physical-bus mapping if one reaches it, else through a stable
/// processor mapping, else recorded as unreachable. Mappings a memory decoder can select are not stable.
fn plan_backings(graph: &Graph) -> Result<CapturePlan, String> {
    let switched_bus: Vec<_> = graph.decoders.iter().flat_map(|decoder| &decoder.choices).flat_map(|choice| &choice.dma).collect();
    let switched_cpu: Vec<_> = graph.decoders.iter().flat_map(|decoder| &decoder.choices).flat_map(|choice| &choice.processors).collect();
    // (physical start, physical end, path to the first byte)
    let bus: Vec<(u64, u64, ReadPath)> = graph.dma_mappings.iter().filter(|map| !switched_bus.contains(map))
        .map(|map| (map.physical, map.physical + map.size as u64, ReadPath::Bus { address: BusAddress(map.address) })).collect();
    let mut cpu: Vec<(u64, u64, ReadPath)> = Vec::new();
    for map in graph.mappings.iter().filter(|map| !switched_cpu.contains(map)) {
        let core = *Core::ALL.get(map.core).ok_or("processor mapping names an absent core")?;
        cpu.push((map.physical, map.physical + map.size as u64, ReadPath::Core { core, address: ProcessorAddress(map.address) }));
    }
    let offset = |path: ReadPath, by: u64| match path {
        ReadPath::Bus { address } => ReadPath::Bus { address: BusAddress(address.0 + by) },
        ReadPath::Core { core, address } => ReadPath::Core { core, address: ProcessorAddress(address.0 + by) },
    };
    let covering = |maps: &[(u64, u64, ReadPath)], at: u64| maps.iter().find(|(start, end, _)| *start <= at && at < *end).copied();
    let next_start = |maps: &[(u64, u64, ReadPath)], at: u64| maps.iter().map(|(start, ..)| *start).filter(|start| *start > at).min();
    let mut backings: Vec<(u64, u64)> = graph.ram.iter().map(|ram| (ram.base, ram.base + ram.size as u64)).collect();
    backings.sort_unstable();
    let mut plan = CapturePlan { regions: Vec::new(), unreachable: Vec::new() };
    for (base, end) in backings {
        let mut region: Option<CaptureRegion> = None;
        let mut cursor = base;
        while cursor < end {
            let bus_next = next_start(&bus, cursor).unwrap_or(end).min(end);
            let reached = covering(&bus, cursor).map(|(start, stop, path)| (stop.min(end), offset(path, cursor - start)))
                .or_else(|| covering(&cpu, cursor).map(|(start, stop, path)| (stop.min(bus_next), offset(path, cursor - start))));
            let stop = match reached {
                Some((stop, path)) => {
                    let region = region.get_or_insert_with(|| CaptureRegion { space: Space::Physical, start: cursor, end: cursor, reads: Vec::new() });
                    region.reads.push(RegionRead { start: cursor - region.start, end: stop - region.start, path });
                    region.end = stop;
                    stop
                }
                None => {
                    plan.regions.extend(region.take());
                    let stop = bus_next.min(next_start(&cpu, cursor).unwrap_or(end)).min(end);
                    match plan.unreachable.last_mut() {
                        Some(span) if span.end == cursor => span.end = stop,
                        _ => plan.unreachable.push(Span::new(cursor, stop)?),
                    }
                    stop
                }
            };
            if stop % PAGE as u64 != 0 {
                return Err(format!("capture plan boundary {stop:#x} is not page aligned"));
            }
            cursor = stop;
        }
        plan.regions.extend(region);
    }
    Ok(plan)
}

pub(crate) fn read_region(board: &Board, region: &CaptureRegion, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
    let end = offset + buffer.len() as u64;
    for read in region.reads.iter().filter(|read| read.start < end && offset < read.end) {
        let from = read.start.max(offset);
        let to = read.end.min(end);
        let bytes = &mut buffer[(from - offset) as usize..(to - offset) as usize];
        let into = from - read.start;
        match read.path {
            ReadPath::Bus { address } => board.read_physical(address.0 + into, bytes)?,
            ReadPath::Core { core, address } => board.read_memory(core.index(), address.0 + into, bytes)?,
        }
    }
    Ok(())
}

/// A range in one space.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SpaceSpan {
    pub space: Space,
    pub start: u64,
    pub end: u64,
}

/// Writes the memory snapshot of `regions`, reading each through `read`. Pages are 4096 bytes.
pub fn write_memory_snapshot(
    path: &Path,
    regions: &[CaptureRegion],
    mut read: impl FnMut(&CaptureRegion, u64, &mut [u8]) -> Result<(), String>,
) -> Result<(Vec<RegionRecord>, Sha256, u64), String> {
    let file = File::create_new(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let hashing = Hashing { inner: BufWriter::new(file), hasher: sha2::Sha256::new(), bytes: 0 };
    let mut encoder = GzEncoder::new(hashing, Compression::fast());
    encoder.write_all(MAGIC).map_err(io)?;
    encoder.write_all(&u32::try_from(regions.len()).map_err(|_| "too many regions")?.to_le_bytes()).map_err(io)?;
    for region in regions {
        let length = region.end.checked_sub(region.start).filter(|length| *length > 0 && length % PAGE as u64 == 0)
            .ok_or("capture region is empty or not a page multiple")?;
        let tiled = region.reads.first().is_some_and(|read| read.start == 0)
            && region.reads.windows(2).all(|pair| pair[0].end == pair[1].start)
            && region.reads.iter().all(|read| read.start < read.end)
            && region.reads.last().is_some_and(|read| read.end == length);
        if !tiled {
            return Err("capture region reads do not tile the region".into());
        }
        let (space, core, bank) = match region.space {
            Space::Physical => (0u8, 0u8, 0u8),
            Space::Tcm { core, bank } => (1, core.index() as u8, bank_index(bank) as u8),
        };
        encoder.write_all(&[space, core, bank]).map_err(io)?;
        encoder.write_all(&region.start.to_le_bytes()).map_err(io)?;
        encoder.write_all(&length.to_le_bytes()).map_err(io)?;
    }
    let mut records = Vec::with_capacity(regions.len());
    let mut nonzero_total = 0u64;
    let mut buffer = vec![0u8; READ_CHUNK];
    for (index, region) in regions.iter().enumerate() {
        let mut hasher = sha2::Sha256::new();
        let mut nonzero = 0u64;
        let mut offset = 0u64;
        let length = region.end - region.start;
        while offset < length {
            let chunk = (length - offset).min(READ_CHUNK as u64) as usize;
            read(region, offset, &mut buffer[..chunk])?;
            hasher.update(&buffer[..chunk]);
            for (page_index, page) in buffer[..chunk].chunks(PAGE).enumerate() {
                if page.iter().any(|byte| *byte != 0) {
                    nonzero += 1;
                    nonzero_total += 1;
                    if nonzero_total > NONZERO_PAGE_LIMIT {
                        return Err("attach-state capture exceeds its nonzero page bound".into());
                    }
                    let page_offset = u32::try_from(offset + (page_index * PAGE) as u64).map_err(|_| "page offset exceeds 32 bits")?;
                    encoder.write_all(&u32::try_from(index).map_err(|_| "region index")?.to_le_bytes()).map_err(io)?;
                    encoder.write_all(&page_offset.to_le_bytes()).map_err(io)?;
                    encoder.write_all(page).map_err(io)?;
                }
            }
            offset += chunk as u64;
        }
        records.push(RegionRecord {
            space: region.space, start: region.start, end: region.end, reads: region.reads.clone(), sha256: hex(hasher)?, nonzero_pages: nonzero,
        });
    }
    encoder.write_all(&END_MARK.to_le_bytes()).map_err(io)?;
    encoder.write_all(&nonzero_total.to_le_bytes()).map_err(io)?;
    let mut hashing = encoder.finish().map_err(io)?;
    hashing.flush().map_err(io)?;
    if hashing.bytes > COMPRESSED_LIMIT {
        return Err("attach-state memory file exceeds its compressed bound".into());
    }
    Ok((records, hex(hashing.hasher)?, hashing.bytes))
}

pub fn write_board_state(path: &Path, state: &Value) -> Result<(Sha256, u64), String> {
    let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > BOARD_STATE_LIMIT {
        return Err("board state exceeds its bound".into());
    }
    let file = File::create_new(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut encoder = GzEncoder::new(Hashing { inner: BufWriter::new(file), hasher: sha2::Sha256::new(), bytes: 0 }, Compression::fast());
    encoder.write_all(&bytes).map_err(io)?;
    let mut hashing = encoder.finish().map_err(io)?;
    hashing.flush().map_err(io)?;
    Ok((hex(hashing.hasher)?, hashing.bytes))
}

fn coprocessor(board: &Board, core: usize, crn: u32, crm: u32, opc2: u32) -> Result<Word, String> {
    let register = arm::Coprocessor { cp: 15, opc1: 0, crn, crm, opc2, secure: 0, wide: false };
    let value = board.cpu(core)?.coprocessor(register, None)?;
    u32::try_from(value).map(Word).map_err(|_| "CP15 register exceeds a word".to_owned())
}

pub fn core_context(board: &Board, core: Core) -> Result<CoreContext, String> {
    let index = core.index();
    let read = |register: arm::Register| -> Result<Word, String> {
        let value = board.read_register(index, Register::Arm32(register))?;
        u32::try_from(value).map(Word).map_err(|_| "register exceeds a word".to_owned())
    };
    let mut r = [Word(0); 13];
    for (number, slot) in r.iter_mut().enumerate() {
        *slot = read(arm::Register::R(number as u8))?;
    }
    Ok(CoreContext {
        core,
        r,
        sp: read(arm::Register::R(13))?,
        lr: read(arm::Register::R(14))?,
        pc: read(arm::Register::R(15))?,
        cpsr: read(arm::Register::Cpsr)?,
        spsr: read(arm::Register::Spsr)?,
        cpacr: read(arm::Register::Cpacr)?,
        fpexc: read(arm::Register::Fpexc)?,
        fpscr: read(arm::Register::Fpscr)?,
        sctlr: coprocessor(board, index, 1, 0, 0)?,
        tcm: [coprocessor(board, index, 9, 1, 0)?, coprocessor(board, index, 9, 1, 1)?],
    })
}

/// Captures the paused board into `output` (a fresh observation directory): the physical regions of `plan` and
/// each core's enabled TCM windows.
pub fn capture(board: &Board, plan: &CapturePlan, output: &Path) -> Result<AttachState, String> {
    let counts = board.instruction_counts();
    let instructions: [u64; 2] = counts.try_into().map_err(|_| "expected two cores")?;
    let cores = [core_context(board, Core::Primary)?, core_context(board, Core::Secondary)?];
    let mut regions = plan.regions.clone();
    for context in &cores {
        for bank in [Bank::Data, Bank::Instruction] {
            if let Some((base, end)) = context.tcm_window(bank) {
                if end - base > TCM_WINDOW_LIMIT {
                    return Err("TCM window exceeds its bound".into());
                }
                let shadowed = plan.regions.iter().flat_map(|region| &region.reads).any(|read| match read.path {
                    ReadPath::Core { core, address } => core == context.core && address.0 < end && base < address.0 + (read.end - read.start),
                    ReadPath::Bus { .. } => false,
                });
                if shadowed {
                    return Err(format!("{:?} TCM window {base:#x}..{end:#x} covers a processor read of physical memory", context.core));
                }
                let space = Space::Tcm { core: context.core, bank };
                let path = ReadPath::Core { core: context.core, address: ProcessorAddress(base) };
                regions.push(CaptureRegion { space, start: 0, end: end - base, reads: vec![RegionRead { start: 0, end: end - base, path }] });
            }
        }
    }
    regions.sort_by_key(|region| (region.space, region.start));
    let (records, memory_sha256, memory_bytes) =
        write_memory_snapshot(&output.join(MEMORY_FILE), &regions, |region, offset, buffer| read_region(board, region, offset, buffer))?;
    let (board_sha256, board_bytes) = write_board_state(&output.join(BOARD_FILE), &board.state())?;
    Ok(AttachState {
        instructions,
        cores,
        regions: records,
        unreachable: plan.unreachable.clone(),
        memory_file: FileRecord { path: MEMORY_FILE.into(), sha256: memory_sha256, compressed_bytes: memory_bytes },
        board_file: FileRecord { path: BOARD_FILE.into(), sha256: board_sha256, compressed_bytes: board_bytes },
    })
}

fn file_digest(path: &Path, limit: u64) -> Result<(Sha256, u64), String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut reader = BufReader::new(file).take(limit + 1);
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    if total > limit {
        return Err("file exceeds its bound".into());
    }
    Ok((hex(hasher)?, total))
}

pub(crate) fn bound_file(directory: &Path, record: &FileRecord, limit: u64) -> Result<std::path::PathBuf, String> {
    let relative = Path::new(&record.path);
    if relative.components().count() != 1 || relative.is_absolute() {
        return Err("file path leaves its observation directory".into());
    }
    let path = directory.join(relative);
    let (digest, bytes) = file_digest(&path, limit)?;
    if digest != record.sha256 || bytes != record.compressed_bytes {
        return Err(format!("{} differs from its receipt binding", record.path));
    }
    Ok(path)
}

/// Page records of one memory snapshot in file order.
struct PageStream {
    decoder: GzDecoder<BufReader<File>>,
    regions: Vec<(Space, u64, u64)>,
    next: Option<(usize, u64)>,
    previous: Option<(usize, u64)>,
    count: u64,
    page: Vec<u8>,
    done: bool,
}

impl PageStream {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(io)?;
        let mut decoder = GzDecoder::new(BufReader::new(file));
        let mut magic = vec![0u8; MAGIC.len()];
        decoder.read_exact(&mut magic).map_err(io)?;
        if magic != MAGIC {
            return Err("memory snapshot magic differs".into());
        }
        let count = read_u32(&mut decoder)?;
        if count > 64 {
            return Err("memory snapshot names too many regions".into());
        }
        let mut regions = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let mut header = [0u8; 3];
            decoder.read_exact(&mut header).map_err(io)?;
            let start = read_u64(&mut decoder)?;
            let length = read_u64(&mut decoder)?;
            let space = match header {
                [0, 0, 0] => Space::Physical,
                [1, core, bank @ 0..=1] => Space::Tcm {
                    core: Core::from_index(usize::from(core))?,
                    bank: if bank == 0 { Bank::Data } else { Bank::Instruction },
                },
                _ => return Err("memory snapshot region header is malformed".into()),
            };
            let end = start.checked_add(length).filter(|end| length > 0 && length % PAGE as u64 == 0 && *end <= ADDRESS_END)
                .ok_or("memory snapshot region extent is malformed")?;
            regions.push((space, start, end));
        }
        let mut stream = Self { decoder, regions, next: None, previous: None, count: 0, page: vec![0; PAGE], done: false };
        stream.advance()?;
        Ok(stream)
    }
    fn advance(&mut self) -> Result<(), String> {
        let region = read_u32(&mut self.decoder)?;
        if region == END_MARK {
            let declared = read_u64(&mut self.decoder)?;
            if declared != self.count {
                return Err("memory snapshot page count differs".into());
            }
            let mut rest = [0u8; 1];
            if self.decoder.read(&mut rest).map_err(io)? != 0 {
                return Err("memory snapshot has bytes after its terminator".into());
            }
            self.next = None;
            self.done = true;
            return Ok(());
        }
        let offset = u64::from(read_u32(&mut self.decoder)?);
        let index = region as usize;
        let (_, start, end) = *self.regions.get(index).ok_or("page names an unknown region")?;
        if offset % PAGE as u64 != 0 || offset + PAGE as u64 > end - start {
            return Err("page lies outside its region".into());
        }
        if self.previous.is_some_and(|previous| previous >= (index, offset)) {
            return Err("pages are unsorted or repeated".into());
        }
        self.count += 1;
        if self.count > NONZERO_PAGE_LIMIT {
            return Err("memory snapshot exceeds its nonzero page bound".into());
        }
        self.next = Some((index, offset));
        Ok(())
    }
    /// The bytes of page `(index, offset)`, zeros when the file has no record for it.
    fn page(&mut self, index: usize, offset: u64, into: &mut [u8]) -> Result<(), String> {
        if self.next == Some((index, offset)) {
            self.decoder.read_exact(&mut self.page).map_err(io)?;
            into.copy_from_slice(&self.page);
            self.previous = self.next;
            self.advance()?;
        } else {
            into.fill(0);
        }
        Ok(())
    }
    fn finish(self) -> Result<(), String> {
        if self.done && self.next.is_none() { Ok(()) } else { Err("memory snapshot has pages past its regions".into()) }
    }
}

fn read_u32(reader: &mut impl Read) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes).map_err(io)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64, String> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes).map_err(io)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_board(path: &Path) -> Result<Value, String> {
    let file = File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    GzDecoder::new(BufReader::new(file)).take(BOARD_STATE_LIMIT + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > BOARD_STATE_LIMIT {
        return Err("board state exceeds its bound".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let object = value.as_object().ok_or("board state is not an object")?;
    if let Some(key) = object.keys().find(|key| BoardField::parse(key).is_none()) {
        return Err(format!("board state has an unknown field {key}"));
    }
    if object.len() != BoardField::ALL.len() {
        return Err("board state lacks a field".into());
    }
    Ok(value)
}

use crate::composed_catalog::Role;

pub struct RoleCapture<'a> {
    pub role: Role,
    pub state: &'a AttachState,
    pub rows: &'a [Row],
    pub directory: &'a Path,
    pub sources: Option<&'a AttachSources>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AbsorbedMemory {
    pub label: AllowanceLabel,
    pub kind: AllowanceKindTag,
    pub differing_bytes: u64,
    pub ranges: Vec<Span>,
    pub retained: Vec<String>,
    pub composed: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AbsorbedBoard {
    pub pointer: BoardPointer,
    pub leaves: Vec<BoardLeaf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AbsorbedRegister {
    pub core: Core,
    pub register: ContextRegister,
    pub retained: Word,
    pub composed: Word,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AttachVerdict {
    pub instructions: BTreeMap<Role, [u64; 2]>,
    pub allowances: Vec<AbsorbedMemory>,
    pub registers: Vec<AbsorbedRegister>,
    pub board: Vec<AbsorbedBoard>,
    pub relations: Vec<RoleRelationSummary>,
}

/// Every attach-state difference in comparison order.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AttachDifferences {
    /// The first `ATTACH_DIFFERENCE_LIMIT`.
    pub entries: Vec<Difference>,
    pub total: u64,
    pub truncated: bool,
}

impl AttachDifferences {
    fn push(&mut self, difference: Difference) {
        self.total += 1;
        if self.entries.len() < ATTACH_DIFFERENCE_LIMIT {
            self.entries.push(difference);
        } else {
            self.truncated = true;
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachRefusal {
    /// The difference the stage order reaches first; memory differences in every space form one `AttachMemory`.
    pub first: Difference,
    pub differences: AttachDifferences,
}

#[derive(Default)]
struct Collector {
    first: Option<Difference>,
    differences: AttachDifferences,
}

impl Collector {
    fn push(&mut self, difference: Difference) {
        if self.first.is_none() {
            self.first = Some(difference.clone());
        }
        self.differences.push(difference);
    }

    /// Records `entries`, with `first` standing for them if nothing preceded.
    fn push_summarised(&mut self, first: Difference, entries: impl IntoIterator<Item = Difference>) {
        self.first.get_or_insert(first);
        for entry in entries {
            self.differences.push(entry);
        }
    }

    /// Appends `later` after everything recorded so far.
    fn append(&mut self, later: Collector) {
        if self.first.is_none() {
            self.first = later.first;
        }
        let unlisted = later.differences.total - later.differences.entries.len() as u64;
        for entry in later.differences.entries {
            self.differences.push(entry);
        }
        self.differences.total += unlisted;
        self.differences.truncated |= unlisted > 0;
    }
}

/// Physical with the `0x40000000` alias removed, or a TCM offset from the core's captured window.
fn resolve(sp: Word, space: Space, cores: &[CoreContext; 2]) -> Option<u64> {
    let sp = u64::from(sp.0);
    match space {
        Space::Physical => Some(if (ALIAS_BASE..ALIAS_END).contains(&sp) { sp - ALIAS_BASE } else { sp }),
        Space::Tcm { core, bank } => {
            let (base, end) = cores[core.index()].tcm_window(bank)?;
            (base <= sp && sp <= end).then(|| sp - base)
        }
    }
}

fn snapshot_error(role: Role, reason: impl Into<String>) -> Difference {
    Difference::AttachSnapshot { role, reason: reason.into() }
}

fn compare_context(rules: &AttachRules, attach_core: Core, retained: &AttachState, owned: &AttachState, out: &mut Collector)
    -> Vec<AbsorbedRegister> {
    let mut used = BTreeSet::new();
    let mut absorbed = Vec::new();
    for core in Core::ALL {
        let (a, b) = (&retained.cores[core.index()], &owned.cores[core.index()]);
        let compared: Vec<ContextRegister> = if core == attach_core {
            ContextRegister::GENERAL.iter().copied()
                .chain([ContextRegister::Sp, ContextRegister::Lr, ContextRegister::Pc, ContextRegister::Cpsr, ContextRegister::Spsr, ContextRegister::Fpscr])
                .chain(ContextRegister::SYSTEM).collect()
        } else {
            [ContextRegister::Cpsr, ContextRegister::Fpscr].into_iter().chain(ContextRegister::SYSTEM).collect()
        };
        for register in compared {
            let (left, right) = (a.get(register), b.get(register));
            if left == right {
                continue;
            }
            let entry = rules.registers.iter().find(|entry| entry.core == core && entry.register == register);
            if entry.is_some() {
                used.insert((core, register));
            }
            let accepted = entry.is_some_and(|entry| {
                let mask = entry.mask.0;
                left.0 & !mask == right.0 & !mask && left.0 & mask == entry.retained.0 && right.0 & mask == entry.composed.0
            });
            if accepted {
                absorbed.push(AbsorbedRegister { core, register, retained: left, composed: right });
            } else {
                out.push(Difference::AttachContext { core, register, retained: left, owned: right });
            }
        }
    }
    for unused in rules.registers.iter().filter(|entry| !used.contains(&(entry.core, entry.register))) {
        out.push(Difference::UnusedAllowance { entry: UnusedEntry::Register { core: unused.core, register: unused.register } });
    }
    absorbed
}

fn compare_peers(rules: &AttachRules, retained: &AttachState, owned: &AttachState, out: &mut Collector) {
    for peer in &rules.peers {
        for (role, state, spans) in [(Role::Retained, retained, &peer.retained_pc), (Role::Composed, owned, &peer.composed_pc)] {
            let pc = state.cores[peer.core.index()].pc;
            if !spans.iter().any(|span| span.holds(u64::from(pc.0))) {
                out.push(Difference::AttachPeer { core: peer.core, role, pc });
            }
        }
    }
}

/// Every differing leaf of `left` and `right` as an RFC 6901 pointer.
pub(crate) fn differing_leaves(prefix: &str, left: Option<&Value>, right: Option<&Value>, out: &mut Vec<String>) {
    match (left, right) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for key in keys {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                differing_leaves(&format!("{prefix}/{escaped}"), a.get(key), b.get(key), out);
            }
        }
        (Some(Value::Array(a)), Some(Value::Array(b))) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                differing_leaves(&format!("{prefix}/{index}"), Some(x), Some(y), out);
            }
        }
        (a, b) if a == b => {}
        _ => out.push(prefix.to_owned()),
    }
}

fn truncated(value: Option<&Value>) -> String {
    let mut text = value.map_or_else(|| "absent".to_owned(), Value::to_string);
    if text.len() > BOARD_VALUE_LIMIT {
        let mut cut = BOARD_VALUE_LIMIT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    text
}

/// Leaves in `fixed` are fixed per role by a relation that held in both roles and are not compared.
fn compare_board(rules: &AttachRules, retained: &Value, owned: &Value, fixed: &BTreeSet<String>, out: &mut Collector) -> Vec<AbsorbedBoard> {
    let mut leaves = Vec::new();
    for (field, name) in BoardField::ALL {
        if field.excluded() {
            continue;
        }
        differing_leaves(&format!("/{name}"), retained.get(name), owned.get(name), &mut leaves);
    }
    leaves.retain(|leaf| !fixed.contains(leaf));
    let mut absorbed: BTreeMap<&BoardPointer, Vec<BoardLeaf>> = BTreeMap::new();
    let mut refused: BTreeSet<&BoardPointer> = BTreeSet::new();
    for leaf in leaves {
        let entry = rules.board.iter().find(|entry| entry.pointer.covers(&leaf));
        let Some(entry) = entry else {
            out.push(Difference::AttachBoard {
                retained: truncated(retained.pointer(&leaf)),
                owned: truncated(owned.pointer(&leaf)),
                pointer: BoardLeaf(leaf),
            });
            continue;
        };
        if refused.contains(&entry.pointer) {
            continue;
        }
        if let BoardAllowanceKind::Exact { retained: expected_retained, composed: expected_owned } = &entry.kind {
            let pointer = entry.pointer.as_str();
            if retained.pointer(pointer) != Some(expected_retained) || owned.pointer(pointer) != Some(expected_owned) {
                out.push(Difference::AttachBoard {
                    pointer: BoardLeaf(pointer.to_owned()),
                    retained: truncated(retained.pointer(pointer)),
                    owned: truncated(owned.pointer(pointer)),
                });
                refused.insert(&entry.pointer);
                continue;
            }
        }
        absorbed.entry(&entry.pointer).or_default().push(BoardLeaf(leaf));
    }
    for unused in rules.board.iter().filter(|entry| !absorbed.contains_key(&entry.pointer) && !refused.contains(&entry.pointer)) {
        out.push(Difference::UnusedAllowance { entry: UnusedEntry::Board(unused.pointer.clone()) });
    }
    absorbed.into_iter().map(|(pointer, leaves)| AbsorbedBoard { pointer: pointer.clone(), leaves }).collect()
}

/// A span whose bytes are hashed in each role and checked against a digest.
struct Hashed {
    space: Space,
    span: Span,
    expected: [Sha256; 2],
    hashers: [sha2::Sha256; 2],
    failure: Difference,
}

struct Absorber {
    space: Space,
    span: Span,
    memory: AbsorbedMemory,
}

fn verify_stacks(rules: &AttachRules, captures: [&RoleCapture; 2], out: &mut Collector) {
    for allowance in &rules.allowances {
        for capture in captures {
            if let Err(difference) = verify_stack(allowance, capture) {
                out.push(difference);
            }
        }
    }
}

fn verify_stack(allowance: &Allowance, capture: &RoleCapture) -> Result<(), Difference> {
    let failure = |role: Role, check: StackCheck| Difference::AttachAllowance {
        label: allowance.label.clone(), role, check,
    };
    let cores = &capture.state.cores;
    match &allowance.kind {
        AllowanceKind::DeadStack { core, mode, window } => {
            let context = &cores[core.index()];
            if ArmMode::of(context.cpsr) != Some(*mode) {
                return Err(failure(capture.role, StackCheck::Mode));
            }
            let sp = resolve(context.sp, window.space, cores).ok_or_else(|| failure(capture.role, StackCheck::SpSpace))?;
            if !(window.span.start < sp && sp <= window.span.end) {
                return Err(failure(capture.role, StackCheck::SpOutsideWindow));
            }
            if allowance.span.end > sp {
                return Err(failure(capture.role, StackCheck::AboveSp));
            }
        }
        AllowanceKind::DeadStackAtBoundary { core, boundary, window } => {
            let row = capture.rows.iter().find(|row| &row.label == boundary && row.core == *core)
                .ok_or_else(|| failure(capture.role, StackCheck::BoundaryRow))?;
            let sp = resolve(row.sp, window.space, cores).ok_or_else(|| failure(capture.role, StackCheck::SpSpace))?;
            if !(window.span.start < sp && sp <= window.span.end) {
                return Err(failure(capture.role, StackCheck::SpOutsideWindow));
            }
            if allowance.span.end > sp {
                return Err(failure(capture.role, StackCheck::AboveSp));
            }
            let current = resolve(cores[core.index()].sp, window.space, cores);
            if current.is_some_and(|current| window.span.start <= current && current <= window.span.end) {
                return Err(failure(capture.role, StackCheck::StillCurrent));
            }
        }
        AllowanceKind::PeerLiveStack { core, window } => {
            let sp = resolve(cores[core.index()].sp, window.space, cores)
                .ok_or_else(|| failure(capture.role, StackCheck::SpSpace))?;
            if !window.span.holds(sp) {
                return Err(failure(capture.role, StackCheck::SpOutsideWindow));
            }
            if allowance.span.start < sp {
                return Err(failure(capture.role, StackCheck::BelowPeerSp));
            }
        }
        AllowanceKind::Copy { .. } | AllowanceKind::Declared { .. } => {}
    }
    Ok(())
}

fn hexbytes(bytes: &[u8]) -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() }

/// The relations' per-role results, cross-role agreement and the leaves and words they fix.
struct RelationOutcome {
    roles: Vec<RoleRelations>,
    failures: Vec<(Role, RelationFailure)>,
    timer_fixed: bool,
    log_words: Vec<Span>,
}

fn evaluate_relations(rules: &AttachRules, captures: [&RoleCapture; 2], boards: &[Value], out: &mut Collector) -> RelationOutcome {
    let relations = &rules.relations;
    let mut outcome = RelationOutcome { roles: Vec::new(), failures: Vec::new(), timer_fixed: false, log_words: Vec::new() };
    if !relations.any() || boards.len() != 2 {
        return outcome;
    }
    for (capture, board) in captures.into_iter().zip(boards) {
        match capture.sources {
            Some(sources) => outcome.roles.push(attach_relations::evaluate(relations, &RoleInput {
                role: capture.role, sources, directory: capture.directory, board, allowances: &rules.allowances,
            })),
            None => out.push(snapshot_error(capture.role, "receipt has no attach sources")),
        }
    }
    let [retained, owned] = outcome.roles.as_slice() else { return outcome };
    for role in [retained, owned] {
        let failures = [
            role.timer.as_ref().and_then(|result| result.as_ref().err()),
            role.lz.as_ref().and_then(|result| result.as_ref().err()),
            role.log.as_ref().and_then(|result| result.as_ref().err()),
        ];
        for failure in failures.into_iter().flatten() {
            outcome.failures.push((role.role, failure.clone()));
        }
    }
    if relations.timer.is_some() {
        let reset = captures[0].sources.map(|sources| &sources.timer_reset) == captures[1].sources.map(|sources| &sources.timer_reset);
        if !reset {
            outcome.failures.push((Role::Composed, RelationFailure::TimerResetDiffers));
        }
        let mut sequences = true;
        for core in Core::ALL {
            let (left, right) = (&retained.sequence[core.index()], &owned.sequence[core.index()]);
            if let Some(index) = (0..left.len().max(right.len())).find(|&index| left.get(index) != right.get(index)) {
                sequences = false;
                outcome.failures.push((Role::Composed, RelationFailure::TimerSequence {
                    core, index, retained: left.get(index).copied(), composed: right.get(index).copied(),
                }));
            }
        }
        outcome.timer_fixed = reset && sequences && retained.timer_device == owned.timer_device
            && matches!(retained.timer, Some(Ok(()))) && matches!(owned.timer, Some(Ok(())));
    }
    if let (Some(Ok(left)), Some(Ok(right))) = (&retained.log, &owned.log) {
        let addresses = |words: &[attach_relations::TimestampWord]| words.iter().map(|word| word.address).collect::<Vec<_>>();
        if addresses(left) == addresses(right) {
            outcome.log_words = left.iter().map(|word| Span { start: word.address, end: word.address + 4 }).collect();
        } else {
            outcome.failures.push((Role::Composed, RelationFailure::LogTimestampWords));
        }
    }
    outcome
}

/// `attach_core` is the core of the attach boundary.
pub fn compare(rules: &AttachRules, attach_core: Core, retained: &RoleCapture, owned: &RoleCapture) -> Result<AttachVerdict, AttachRefusal> {
    let captures = [retained, owned];
    let mut out = Collector::default();
    let mut memory_paths = Vec::with_capacity(2);
    let mut boards = Vec::with_capacity(2);
    for capture in captures {
        let state = capture.state;
        match bound_file(capture.directory, &state.memory_file, COMPRESSED_LIMIT) {
            Ok(path) => memory_paths.push(path),
            Err(reason) => out.push(snapshot_error(capture.role, reason)),
        }
        match bound_file(capture.directory, &state.board_file, COMPRESSED_LIMIT).and_then(|path| read_board(&path)) {
            Ok(board) => boards.push(board),
            Err(reason) => out.push(snapshot_error(capture.role, reason)),
        }
    }
    let region_shape = |state: &AttachState| {
        (state.regions.iter().map(|region| (region.space, region.start, region.end, region.reads.clone())).collect::<Vec<_>>(), state.unreachable.clone())
    };
    let shapes_agree = region_shape(retained.state) == region_shape(owned.state);
    if !shapes_agree {
        out.push(snapshot_error(Role::Composed, "captured regions, their read paths or the unreachable extents differ between roles"));
    }
    let mut relations = evaluate_relations(rules, captures, &boards, &mut out);
    let registers = compare_context(rules, attach_core, retained.state, owned.state, &mut out);
    compare_peers(rules, retained.state, owned.state, &mut out);
    let submitted = rules.relations.lz.as_ref().and_then(|lz| lz.submitted.as_ref()).and_then(|word| word.span().map(|span| (word, span)));
    let mut extract: [Vec<(Extract, Span)>; 2] = [Vec::new(), Vec::new()];
    for (slot, role) in extract.iter_mut().zip(&relations.roles) {
        if let (Some(log), Some(Ok(_))) = (&rules.relations.log, &role.log) {
            slot.push((Extract::Log, log.span));
        }
        if let Some(Ok(plan)) = &role.lz {
            if plan.end > plan.start {
                slot.push((Extract::LzSource, Span { start: plan.start, end: plan.end }));
            }
            if let Some((_, span)) = submitted {
                slot.push((Extract::LzSubmitted, span));
            }
        }
    }
    let mut memory_out = Collector::default();
    let mut held: Vec<(HeldBy, Span)> = relations.log_words.iter().map(|span| (HeldBy::Log, *span)).collect();
    held.extend(submitted.map(|(_, span)| (HeldBy::LzSubmitted, span)));
    let extras = MemoryExtras { held, extract };
    let (allowances, extracted) = if shapes_agree && memory_paths.len() == 2 {
        match compare_memory(rules, retained, owned, &memory_paths, &extras, &mut memory_out) {
            Ok(result) => result,
            Err(difference) => {
                memory_out.push(difference);
                (Vec::new(), MemoryExtracted::default())
            }
        }
    } else {
        (Vec::new(), MemoryExtracted::default())
    };
    let mut fixed: BTreeSet<String> = BTreeSet::new();
    let mut summaries = Vec::new();
    let mut log_held = !relations.log_words.is_empty();
    let mut submitted_held = 0usize;
    let mut lz_leaves: Vec<Vec<String>> = Vec::new();
    for (index, role) in relations.roles.iter_mut().enumerate() {
        let mut summary = RoleRelationSummary { role: role.role, transactions: role.rows, timestamps: Vec::new(), lz: None };
        if let (Some(log), Some(Ok(words))) = (&rules.relations.log, &role.log) {
            let bytes = extracted.bytes[index].iter().find(|(kind, _)| *kind == Extract::Log).map(|(_, bytes)| bytes);
            let word_at = |address: u64| bytes.and_then(|bytes| {
                let at = (address - log.span.start) as usize;
                bytes.get(at..at + 4).map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")))
            });
            match words.iter().find(|word| word_at(word.address) != Some(word.value.0)) {
                Some(word) => {
                    relations.failures.push((role.role, RelationFailure::LogCaptured { address: word.address }));
                    log_held = false;
                }
                None => summary.timestamps = words.clone(),
            }
        }
        if let Some(Ok(plan)) = &role.lz {
            let source = extracted.bytes[index].iter().find(|(kind, _)| *kind == Extract::LzSource).map(|(_, bytes)| bytes.as_slice());
            let bytes = if plan.end > plan.start { source } else { Some(&[][..]) };
            match bytes.map(|bytes| attach_relations::derive_lz(plan, bytes)) {
                Some(Ok((mut lz, leaves))) => {
                    let captured = extracted.bytes[index].iter().find(|(kind, _)| *kind == Extract::LzSubmitted)
                        .map(|(_, bytes)| u32::from_le_bytes(bytes.as_slice().try_into().expect("four bytes")));
                    if let (Some((word, _)), Some(captured)) = (submitted, captured) {
                        match attach_relations::check_submitted(plan, captured, word.record_length.of(role.role)) {
                            Ok(()) => {
                                lz.submitted = Some(Word(captured));
                                submitted_held += 1;
                            }
                            Err(failure) => relations.failures.push((role.role, failure)),
                        }
                    }
                    summary.lz = Some(lz);
                    lz_leaves.push(leaves);
                }
                Some(Err(failure)) => relations.failures.push((role.role, failure)),
                None => relations.failures.push((role.role, RelationFailure::LzSourceUnpinned { start: plan.start, end: plan.end })),
            }
        }
        summaries.push(summary);
    }
    if relations.timer_fixed {
        if let Some(device) = relations.roles.first().and_then(|role| role.timer_device) {
            fixed.extend(attach_relations::timer_leaves(device));
        }
    }
    if let [left, right] = lz_leaves.as_slice() {
        fixed.extend(left.iter().filter(|leaf| right.contains(leaf)).cloned());
    }
    let mut covered: Vec<String> = Vec::new();
    if rules.relations.timer.is_some() {
        covered.extend(relations.roles.first().and_then(|role| role.timer_device).map(attach_relations::timer_leaves).unwrap_or_default());
    }
    if rules.relations.lz.is_some() {
        if let Some(Ok(plan)) = relations.roles.first().and_then(|role| role.lz.as_ref()) {
            covered.extend(attach_relations::lz_leaf_prefixes(plan.device));
        }
    }
    for entry in &rules.board {
        let pointer = entry.pointer.as_str();
        if covered.iter().any(|leaf| entry.pointer.covers(leaf) || pointer.starts_with(&format!("{leaf}/"))) {
            relations.failures.push((Role::Composed, RelationFailure::RelationOverlap { pointer: entry.pointer.clone() }));
        }
    }
    for (role, failure) in relations.failures {
        out.push(Difference::AttachRelation { role, failure });
    }
    let board = match boards.as_slice() {
        [retained_board, owned_board] => compare_board(rules, retained_board, owned_board, &fixed, &mut out),
        _ => Vec::new(),
    };
    verify_stacks(rules, captures, &mut out);
    out.append(memory_out);
    let released = |by: HeldBy| match by {
        HeldBy::Log => !log_held,
        HeldBy::LzSubmitted => submitted_held < 2,
    };
    let differing: Vec<u64> = extracted.held.iter().filter(|(by, _)| released(*by)).map(|(_, address)| *address).collect();
    if let Some(&start) = differing.first() {
        let runs = differing.iter().take(DIFFERING_RUN_LIMIT).map(|&address| SpaceSpan { space: Space::Physical, start: address, end: address + 1 }).collect();
        out.push(Difference::AttachMemory { space: Space::Physical, start, end: start + 1, differing_bytes: differing.len() as u64, runs });
    }
    if let Some(first) = out.first {
        return Err(AttachRefusal { first, differences: out.differences });
    }
    let mut instructions = BTreeMap::new();
    instructions.insert(Role::Retained, retained.state.instructions);
    instructions.insert(Role::Composed, owned.state.instructions);
    Ok(AttachVerdict { instructions, allowances, registers, board, relations: summaries })
}

/// Physical spans the memory comparison holds aside (`held`) or copies out per role (`extract`).
struct MemoryExtras {
    held: Vec<(HeldBy, Span)>,
    extract: [Vec<(Extract, Span)>; 2],
}

/// The relation whose result releases a held span to the comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HeldBy {
    Log,
    LzSubmitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Extract {
    Log,
    LzSource,
    LzSubmitted,
}

#[derive(Default)]
struct MemoryExtracted {
    /// Per role, one buffer per `extract` span.
    bytes: [Vec<(Extract, Vec<u8>)>; 2],
    /// Differing bytes inside `held` spans.
    held: Vec<(HeldBy, u64)>,
}

/// Differences in the captured bytes go to `out`; an inconsistent capture is the `Err`.
fn compare_memory(rules: &AttachRules, retained: &RoleCapture, owned: &RoleCapture, paths: &[std::path::PathBuf], extras: &MemoryExtras,
    out: &mut Collector) -> Result<(Vec<AbsorbedMemory>, MemoryExtracted), Difference> {
    let regions: Vec<(Space, Span)> = retained.state.regions.iter()
        .map(|region| (region.space, Span { start: region.start, end: region.end })).collect();
    let mut hashed: Vec<Hashed> = Vec::new();
    for part in &rules.carriers {
        if let Residency::Resident { canonical_sha256, composed_sha256 } = &part.residency {
            hashed.push(Hashed {
                space: Space::Physical, span: part.physical, expected: [canonical_sha256.clone(), composed_sha256.clone()],
                hashers: [sha2::Sha256::new(), sha2::Sha256::new()],
                failure: Difference::AttachCarrier {
                    piece: part.piece.clone(), target: part.target, start: part.physical.start, end: part.physical.end, role: Role::Retained,
                },
            });
        }
    }
    let mut absorbers: Vec<Absorber> = Vec::new();
    for allowance in &rules.allowances {
        match &allowance.kind {
            AllowanceKind::Copy { canonical_sha256, composed_sha256, .. } => hashed.push(Hashed {
                space: allowance.space, span: allowance.span, expected: [canonical_sha256.clone(), composed_sha256.clone()],
                hashers: [sha2::Sha256::new(), sha2::Sha256::new()],
                failure: Difference::AttachCopy { label: allowance.label.clone(), role: Role::Retained },
            }),
            kind => {
                absorbers.push(Absorber {
                    space: allowance.space, span: allowance.span,
                    memory: AbsorbedMemory {
                        label: allowance.label.clone(), kind: kind.tag(), differing_bytes: 0, ranges: vec![], retained: vec![], composed: vec![],
                    },
                });
            }
        }
    }
    for item in hashed.iter().map(|item| (item.space, item.span)).chain(absorbers.iter().map(|item| (item.space, item.span))) {
        if !regions.iter().any(|(space, region)| *space == item.0 && region.contains(item.1)) {
            return Err(snapshot_error(Role::Retained, format!("{} {:#x}..{:#x} lies outside every captured region", space_name(item.0), item.1.start, item.1.end)));
        }
    }
    let mut streams = Vec::with_capacity(2);
    for (capture, path) in [retained, owned].into_iter().zip(paths) {
        let stream = PageStream::open(path).map_err(|reason| snapshot_error(capture.role, reason))?;
        if stream.regions != retained.state.regions.iter().map(|region| (region.space, region.start, region.end)).collect::<Vec<_>>() {
            return Err(snapshot_error(capture.role, "snapshot regions differ from the receipt"));
        }
        streams.push(stream);
    }
    let roles = [Role::Retained, Role::Composed];
    let mut extracted = MemoryExtracted::default();
    let mut filled: [Vec<u64>; 2] = [Vec::new(), Vec::new()];
    for role in 0..2 {
        extracted.bytes[role] = extras.extract[role].iter().map(|(kind, span)| (*kind, vec![0u8; span.len() as usize])).collect();
        filled[role] = vec![0; extras.extract[role].len()];
    }
    let mut pages = [vec![0u8; PAGE], vec![0u8; PAGE]];
    let mut first_difference: Option<(Space, u64, u64)> = None;
    let mut differing_unabsorbed = 0u64;
    let mut runs: Vec<SpaceSpan> = Vec::new();
    let mut by_space: Vec<(Space, u64, Vec<SpaceSpan>)> = Vec::new();
    for (index, (space, region)) in regions.iter().enumerate() {
        let slot = by_space.iter().position(|(seen, ..)| seen == space).unwrap_or_else(|| {
            by_space.push((*space, 0, Vec::new()));
            by_space.len() - 1
        });
        let mut region_hashers = [sha2::Sha256::new(), sha2::Sha256::new()];
        let mut offset = 0u64;
        while offset < region.len() {
            for (role, (stream, page)) in streams.iter_mut().zip(pages.iter_mut()).enumerate() {
                stream.page(index, offset, page).map_err(|reason| snapshot_error(roles[role], reason))?;
                region_hashers[role].update(&page[..]);
            }
            let base = region.start + offset;
            let page_span = Span { start: base, end: base + PAGE as u64 };
            for item in hashed.iter_mut().filter(|item| item.space == *space && item.span.intersects(page_span)) {
                let from = item.span.start.max(page_span.start) - base;
                let to = item.span.end.min(page_span.end) - base;
                for role in 0..2 {
                    item.hashers[role].update(&pages[role][from as usize..to as usize]);
                }
            }
            if *space == Space::Physical {
                for role in 0..2 {
                    for (slot, (_, span)) in extras.extract[role].iter().enumerate().filter(|(_, (_, span))| span.intersects(page_span)) {
                        let from = span.start.max(page_span.start);
                        let to = span.end.min(page_span.end);
                        extracted.bytes[role][slot].1[(from - span.start) as usize..(to - span.start) as usize]
                            .copy_from_slice(&pages[role][(from - base) as usize..(to - base) as usize]);
                        filled[role][slot] += to - from;
                    }
                }
            }
            if pages[0] != pages[1] {
                for byte in 0..PAGE {
                    if pages[0][byte] == pages[1][byte] {
                        continue;
                    }
                    let address = base + byte as u64;
                    if hashed.iter().any(|item| item.space == *space && item.span.holds(address)) {
                        continue;
                    }
                    if let Some((by, _)) = extras.held.iter().find(|(_, span)| *space == Space::Physical && span.holds(address)) {
                        extracted.held.push((*by, address));
                        continue;
                    }
                    if let Some(absorber) = absorbers.iter_mut().find(|item| item.space == *space && item.span.holds(address)) {
                        let memory = &mut absorber.memory;
                        memory.differing_bytes += 1;
                        if let Some(range) = memory.ranges.last_mut().filter(|range| range.end == address) {
                            range.end = address + 1;
                        } else if memory.ranges.len() < ABSORBED_RANGE_LIMIT {
                            memory.ranges.push(Span { start: address, end: address + 1 });
                        }
                        let recorded: usize = memory.retained.iter().map(|text| text.len() / 2).sum();
                        if recorded < ABSORBED_BYTE_LIMIT {
                            memory.retained.push(hexbytes(&pages[0][byte..=byte]));
                            memory.composed.push(hexbytes(&pages[1][byte..=byte]));
                        }
                        continue;
                    }
                    differing_unabsorbed += 1;
                    if first_difference.is_none() {
                        first_difference = Some((*space, address, address + 1));
                    }
                    if let Some(run) = runs.last_mut().filter(|run| run.space == *space && run.end == address) {
                        run.end = address + 1;
                    } else if runs.len() < DIFFERING_RUN_LIMIT {
                        runs.push(SpaceSpan { space: *space, start: address, end: address + 1 });
                    }
                    let (_, space_bytes, space_runs) = &mut by_space[slot];
                    *space_bytes += 1;
                    if let Some(run) = space_runs.last_mut().filter(|run| run.end == address) {
                        run.end = address + 1;
                    } else if space_runs.len() < DIFFERING_RUN_LIMIT {
                        space_runs.push(SpaceSpan { space: *space, start: address, end: address + 1 });
                    }
                }
            }
            offset += PAGE as u64;
        }
        for (role, hasher) in region_hashers.into_iter().enumerate() {
            let capture = [retained, owned][role];
            if hex(hasher).ok().as_ref() != Some(&capture.state.regions[index].sha256) {
                return Err(snapshot_error(roles[role], format!("{} region digest differs from the receipt", space_name(*space))));
            }
        }
    }
    for (stream, role) in streams.into_iter().zip(roles) {
        stream.finish().map_err(|reason| snapshot_error(role, reason))?;
    }
    for item in hashed {
        for (role, hasher) in item.hashers.into_iter().enumerate() {
            if hex(hasher).ok().as_ref() != Some(&item.expected[role]) {
                out.push(match &item.failure {
                    Difference::AttachCarrier { piece, target, start, end, .. } =>
                        Difference::AttachCarrier { piece: piece.clone(), target: *target, start: *start, end: *end, role: roles[role] },
                    Difference::AttachCopy { label, .. } => Difference::AttachCopy { label: label.clone(), role: roles[role] },
                    other => other.clone(),
                });
            }
        }
    }
    if let Some((space, start, _)) = first_difference {
        let end = runs.first().map_or(start + 1, |run| run.end);
        let per_space = by_space.into_iter().filter(|(_, bytes, _)| *bytes > 0).map(|(space, differing_bytes, runs)| {
            let (start, end) = (runs[0].start, runs[0].end);
            Difference::AttachMemory { space, start, end, differing_bytes, runs }
        });
        out.push_summarised(Difference::AttachMemory { space, start, end, differing_bytes: differing_unabsorbed, runs }, per_space);
    }
    let absorbed: Vec<AbsorbedMemory> = absorbers.into_iter().map(|absorber| absorber.memory).collect();
    for unused in absorbed.iter().filter(|memory| memory.differing_bytes == 0) {
        out.push(Difference::UnusedAllowance { entry: UnusedEntry::Memory(unused.label.clone()) });
    }
    for role in 0..2 {
        if extras.extract[role].iter().zip(&filled[role]).any(|((_, span), filled)| *filled != span.len()) {
            return Err(snapshot_error(roles[role], "a relation span lies outside the captured physical regions"));
        }
    }
    Ok((absorbed, extracted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::ValidatedUpdate;

    fn document() -> RunDocument {
        RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap()
    }

    fn merged(mut spans: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
        spans.sort_unstable();
        let mut out: Vec<(u64, u64)> = Vec::new();
        for (start, end) in spans {
            match out.last_mut() {
                Some(last) if last.1 == start => last.1 = end,
                _ => out.push((start, end)),
            }
        }
        out
    }

    /// The path that reads physical byte `address`, if the plan captures it.
    fn path_of(plan: &CapturePlan, address: u64) -> Option<ReadPath> {
        let region = plan.regions.iter().find(|region| region.start <= address && address < region.end)?;
        let at = address - region.start;
        let read = region.reads.iter().find(|read| read.start <= at && at < read.end)?;
        Some(match read.path {
            ReadPath::Bus { address } => ReadPath::Bus { address: BusAddress(address.0 + at - read.start) },
            ReadPath::Core { core, address } => ReadPath::Core { core, address: ProcessorAddress(address.0 + at - read.start) },
        })
    }

    #[test]
    fn the_plan_covers_every_backing_byte_exactly_once() {
        let document = document();
        let plan = capture_plan(&document).unwrap();
        let mut covered: Vec<(u64, u64)> = plan.regions.iter().map(|region| (region.start, region.end))
            .chain(plan.unreachable.iter().map(|span| (span.start, span.end))).collect();
        covered.sort_unstable();
        assert!(covered.windows(2).all(|pair| pair[0].1 <= pair[1].0), "{covered:#x?}");
        let backings = run::memory_backings(&document).unwrap().iter().map(|ram| (ram.base, ram.base + ram.size as u64)).collect();
        assert_eq!(merged(covered), merged(backings));
        let core = |core, address| Some(ReadPath::Core { core, address: ProcessorAddress(address) });
        assert_eq!(path_of(&plan, 0x9000_0000), core(Core::Primary, 0xffff_0000));
        assert_eq!(path_of(&plan, 0x9000_0fff), core(Core::Primary, 0xffff_0fff));
        assert_eq!(path_of(&plan, 0x9000_1000), core(Core::Primary, 0xffff_1000));
        assert_eq!(path_of(&plan, 0x9000_ffff), core(Core::Primary, 0xffff_ffff));
        assert_eq!(path_of(&plan, 0x9001_1000), core(Core::Secondary, 0xffff_1000));
        assert_eq!(path_of(&plan, 0x9001_ffff), core(Core::Secondary, 0xffff_ffff));
        assert_eq!(plan.unreachable, [Span::new(0x9001_0000, 0x9001_1000).unwrap()]);
        assert!(matches!(path_of(&plan, 0), Some(ReadPath::Bus { .. })));
        assert!(matches!(path_of(&plan, 0x8000_0000), Some(ReadPath::Bus { .. })));
    }

    #[test]
    fn every_planned_region_reads_its_backing_on_an_unexecuted_board() {
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let document = document();
        let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
        let rom: Vec<u8> = (0..0x4000u32).map(|index| (index * 7 + index / 251) as u8 | 1).collect();
        let session = run::open(&document, &update, &rom).unwrap();
        let board = &session.board;
        board.write_memory(0, 0x4000_0000, b"dram").unwrap();
        board.write_memory(0, 0xffff_0000, b"sram-0").unwrap();
        board.write_memory(0, 0xffff_1000, b"sram-0b").unwrap();
        board.write_memory(1, 0xffff_1000, b"sram-1").unwrap();
        let plan = capture_plan(&document).unwrap();
        let mut buffer = vec![0; READ_CHUNK];
        for region in &plan.regions {
            let mut offset = 0;
            while offset < region.end - region.start {
                let chunk = (region.end - region.start - offset).min(READ_CHUNK as u64) as usize;
                read_region(board, region, offset, &mut buffer[..chunk]).unwrap();
                offset += chunk as u64;
            }
        }
        let at = |address: u64, length: usize| {
            let region = plan.regions.iter().find(|region| region.start <= address && address < region.end).unwrap();
            let mut bytes = vec![0; length];
            read_region(board, region, address - region.start, &mut bytes).unwrap();
            bytes
        };
        assert_eq!(at(0, 4), b"dram");
        assert_eq!(at(0x8000_0000, 0x4000), rom);
        assert_eq!(at(0x8010_0000, 0x100), rom[..0x100]);
        assert_eq!(at(0x9000_0000, 6), b"sram-0");
        assert_eq!(at(0x9000_1000, 7), b"sram-0b");
        assert_eq!(at(0x9001_1000, 6), b"sram-1");
    }
}
