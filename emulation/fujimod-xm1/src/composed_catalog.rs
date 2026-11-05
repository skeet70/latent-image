//! Strict parse of `composed-contract.json` (schema 4, or 5 with the `application` section) and the observation plan
//! derived from it.
use crate::attach_state::{AttachRuleRefusal, AttachRules, RawAttachRules, RuleContext, Span};
use fujimod_emulator::board::observation::{
    CompositeGuardConfig, DeclaredMutation, DecoderChoice, DecoderIndex, DestinationPolicy, GuardCore, GuardRange,
    GuardSpan, GuardTarget, MAX_GUARD_DECLARED_MUTATIONS, MAX_GUARD_PARTITIONS, MAX_GUARD_PC_SPANS, MAX_GUARD_RANGES,
    PcPartition, SourcePolicy, TcmBank, TcmRegister, WatchKinds, WatchRange, WriteSource, WriterPc,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CORES: usize = 2;
/// Receipt rows across every boundary of one run.
pub const ROW_LIMIT: usize = 65_536;
/// Bytes captured by every read at its boundary's full `record_limit`.
pub const READ_BYTE_LIMIT: u64 = 64 << 20;
/// Armed boundary PCs per declared core in one role.
pub const BOUNDARY_PC_LIMIT: usize = 4096;
const WATCH_CLASS_LIMIT: usize = 256;
const ADDRESS_END: u64 = 1 << 32;
const ALIAS_BASE: u64 = 0x4000_0000;
const ALIAS_END: u64 = 0x6000_0000;
const DRAM_END: u64 = ALIAS_END - ALIAS_BASE;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Retained,
    Composed,
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "retained" => Ok(Self::Retained),
            "composed" => Ok(Self::Composed),
            _ => Err("role must be retained or composed".into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256(String);

impl TryFrom<String> for Sha256 {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            Ok(Self(value))
        } else {
            Err("digest is not lowercase SHA256".into())
        }
    }
}

impl From<Sha256> for String {
    fn from(value: Sha256) -> Self { value.0 }
}

impl Sha256 {
    pub fn of(bytes: &[u8]) -> Self { Self(crate::bridge::sha256(bytes)) }
    pub fn as_str(&self) -> &str { &self.0 }
}

fn kebab(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && !value.ends_with('-')
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

macro_rules! kebab_name {
    ($name:ident, $what:literal) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if kebab(&value) { Ok(Self(value)) } else { Err(concat!($what, " is not a kebab-case name").into()) }
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

kebab_name!(PieceKind, "piece kind");
kebab_name!(AreaName, "area");
kebab_name!(JoinKind, "join kind");

impl PieceKind {
    /// The piece whose owned code may hold the pump sites.
    pub fn application_pump() -> Self { Self("application-pump".into()) }
    pub fn task1_actor() -> Self { Self("task1-actor".into()) }
}

macro_rules! text {
    ($name:ident, $what:literal) => {
        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if value.trim().is_empty() { Err(concat!($what, " is empty").into()) } else { Ok(Self(value)) }
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self { value.0 }
        }
    };
}

text!(Citation, "citation");
text!(Premise, "premise");
text!(Reason, "reason");

/// `<piece kind>/<name>`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Label(String);

impl TryFrom<String> for Label {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.split_once('/') {
            Some((piece, name)) if kebab(piece) && kebab(name) => Ok(Self(value)),
            _ => Err("label is not <piece>/<name>".into()),
        }
    }
}

impl From<Label> for String {
    fn from(value: Label) -> Self { value.0 }
}

impl Label {
    pub fn as_str(&self) -> &str { &self.0 }
    fn piece(&self) -> &str { self.0.split_once('/').map_or("", |(piece, _)| piece) }
}

/// A guest instruction address as fetched.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Pc(pub u32);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Core {
    Primary,
    Secondary,
}

impl TryFrom<u8> for Core {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Primary),
            1 => Ok(Self::Secondary),
            _ => Err("core is not 0 or 1".into()),
        }
    }
}

impl From<Core> for u8 {
    fn from(value: Core) -> Self { value.index() as u8 }
}

impl Core {
    pub const ALL: [Self; CORES] = [Self::Primary, Self::Secondary];
    pub fn index(self) -> usize {
        match self {
            Self::Primary => 0,
            Self::Secondary => 1,
        }
    }
    pub fn from_index(index: usize) -> Result<Self, String> {
        u8::try_from(index).map_err(|_| "core index exceeds u8".to_owned()).and_then(Self::try_from)
    }
    fn guard(self) -> GuardCore {
        GuardCore::new(self.index()).expect("two cores are within the guard core bound")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Record1,
    Record2,
    LoaderA,
    LoaderB,
    Image4,
    Image5,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryKind {
    Entry,
    Return,
    Site,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Compare {
    Sequence,
    Presence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Qualification {
    HostComposed,
    Pending,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JoinBasis {
    HostProof,
    ObservationQualified,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FootprintBasis {
    AdmissionRead,
    RuntimePremise,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WritesBasis {
    Proved,
    Premised,
}

/// The guard's `PartitionId` of one piece on one core: the partition's index in that core's `SourcePolicy`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PartitionIndex(pub u16);

impl PartitionIndex {
    /// Position in `GuardStatusToken::partition_checks`.
    pub fn position(self) -> usize { usize::from(self.0) }
}

/// One piece's guard partition on one core.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PiecePolicy {
    pub piece: PieceKind,
    pub core: Core,
    pub partition: PartitionIndex,
    pub basis: WritesBasis,
    pub premise: Option<Premise>,
    pub spans: Vec<PlanSpan>,
    pub allow: Vec<RangePlan>,
}

/// One mutation the guard admits once in a role, with the instruction that requests it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Declaration {
    TcmRegion { core: Core, bank: Bank, previous: u32, next: u32, writer: u32, citation: Citation },
    DecoderCommit { decoder: u16, previous: u16, next: u16, core: Core, writer: u32, citation: Citation },
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mutated {
    Tcm(Core, Bank),
    Decoder(u16),
}

impl Declaration {
    pub fn guard(&self) -> DeclaredMutation {
        match *self {
            Self::TcmRegion { core, bank, previous, next, writer, .. } => DeclaredMutation::TcmRegion {
                core: core.guard(),
                bank: bank.guard(),
                previous: TcmRegister(previous),
                next: TcmRegister(next),
                writer: WriterPc(writer),
            },
            Self::DecoderCommit { decoder, previous, next, core, writer, .. } => DeclaredMutation::DecoderCommit {
                decoder: DecoderIndex(decoder),
                previous: DecoderChoice(previous),
                next: DecoderChoice(next),
                core: core.guard(),
                writer: WriterPc(writer),
            },
        }
    }
    fn link(&self) -> (Mutated, u32, u32) {
        match *self {
            Self::TcmRegion { core, bank, previous, next, .. } => (Mutated::Tcm(core, bank), previous, next),
            Self::DecoderCommit { decoder, previous, next, .. } => (Mutated::Decoder(decoder), previous.into(), next.into()),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredLists {
    pub retained: Vec<Declaration>,
    pub composed: Vec<Declaration>,
}

impl DeclaredLists {
    pub fn role(&self, role: Role) -> &[Declaration] {
        match role {
            Role::Retained => &self.retained,
            Role::Composed => &self.composed,
        }
    }
}

/// Each names the declaration's index in its role's list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationProblem {
    Unchanged(usize),
    UnalignedWriter(usize),
    /// Another declaration of the same register or decoder leaves the same value.
    SharedPrevious(usize),
}

/// A nonempty, strictly ascending list of cores.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "Vec<Core>")]
pub struct CoreSet([bool; CORES]);

impl TryFrom<Vec<Core>> for CoreSet {
    type Error = String;
    fn try_from(cores: Vec<Core>) -> Result<Self, Self::Error> {
        if cores.is_empty() || cores.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("cores is not a nonempty ascending list of distinct cores".into());
        }
        let mut set = [false; CORES];
        for core in cores { set[core.index()] = true; }
        Ok(Self(set))
    }
}

impl CoreSet {
    pub fn contains(self, core: Core) -> bool { self.0[core.index()] }
    pub fn iter(self) -> impl Iterator<Item = Core> { Core::ALL.into_iter().filter(move |core| self.contains(*core)) }
    pub fn single(self) -> Option<Core> {
        let mut cores = self.iter();
        cores.next().filter(|_| cores.next().is_none())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bank {
    Data,
    Instruction,
}

impl Bank {
    fn guard(self) -> TcmBank {
        match self {
            Self::Data => TcmBank::Data,
            Self::Instruction => TcmBank::Instruction,
        }
    }
}

/// Half-open, nonempty and within the 32-bit address space.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Interval {
    pub start: u64,
    pub end: u64,
}

impl Interval {
    fn new(start: u64, end: u64) -> Result<Self, String> {
        if start < end && end <= ADDRESS_END { Ok(Self { start, end }) } else { Err("span is empty or exceeds 32 bits".into()) }
    }
    fn holds(self, address: u64) -> bool { self.start <= address && address < self.end }
    /// Physical addresses of a span written through the `0x40000000` DRAM alias; spans that contain or avoid the
    /// whole alias window are already physical.
    fn physical(self) -> Option<Self> {
        if self.start >= ALIAS_BASE && self.end <= ALIAS_END {
            Some(Self { start: self.start - ALIAS_BASE, end: self.end - ALIAS_BASE })
        } else if self.end <= ALIAS_BASE || self.start >= ALIAS_END || (self.start <= ALIAS_BASE && self.end >= ALIAS_END) {
            Some(self)
        } else {
            None
        }
    }
    fn guard(self) -> GuardSpan { GuardSpan::new(self.start, self.end).expect("interval is a valid guard span") }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInterval {
    start: u64,
    end: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "RawTargetSpan")]
pub struct TargetSpan {
    pub target: Target,
    pub span: Interval,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTargetSpan {
    target: Target,
    start: u64,
    end: u64,
}

impl TryFrom<RawTargetSpan> for TargetSpan {
    type Error = String;
    fn try_from(raw: RawTargetSpan) -> Result<Self, Self::Error> {
        Ok(Self { target: raw.target, span: Interval::new(raw.start, raw.end)? })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "RawInterval")]
pub struct CodeSpan(pub Interval);

impl TryFrom<RawInterval> for CodeSpan {
    type Error = String;
    fn try_from(raw: RawInterval) -> Result<Self, Self::Error> { Interval::new(raw.start, raw.end).map(Self) }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RawEdit {
    target: Target,
    start: u64,
    end: u64,
    sha256: Sha256,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edit {
    pub span: TargetSpan,
    pub sha256: Sha256,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RawFootprint {
    target: Target,
    start: u64,
    end: u64,
    basis: FootprintBasis,
    citation: Citation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Footprint {
    pub span: TargetSpan,
    pub basis: FootprintBasis,
    pub citation: Citation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
enum RawWriteTarget {
    Physical,
    Tcm,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWriteRange {
    target: RawWriteTarget,
    #[serde(default)]
    core: Option<Core>,
    #[serde(default)]
    bank: Option<Bank>,
    start: u64,
    end: u64,
}

/// A TCM span is an offset range within that core's bank. A physical range with a core binds only that core's
/// partition.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum WriteSpace {
    Physical { core: Option<Core> },
    Tcm { core: Core, bank: Bank },
}

impl WriteSpace {
    fn guard(self) -> GuardTarget {
        match self {
            Self::Physical { .. } => GuardTarget::Physical,
            Self::Tcm { core, bank } => GuardTarget::Tcm { core: core.guard(), bank: bank.guard() },
        }
    }
    fn binds(self, core: Core) -> bool {
        match self {
            Self::Physical { core: owner } => owner.is_none_or(|owner| owner == core),
            Self::Tcm { core: owner, .. } => owner == core,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteRange {
    pub space: WriteSpace,
    pub span: Interval,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Writes {
    pub allow: Vec<WriteRange>,
    pub basis: WritesBasis,
    pub premise: Option<Premise>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWrites {
    allow: Vec<RawWriteRange>,
    basis: WritesBasis,
    premise: Option<Premise>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub label: Label,
    pub kind: BoundaryKind,
    pub cores: CoreSet,
    pub composed_pc: Pc,
    pub retained_pc: Option<Pc>,
    pub stop: bool,
    pub compare: Compare,
    pub record_limit: u32,
}

impl Boundary {
    pub fn pc(&self, role: Role) -> Option<Pc> {
        match role {
            Role::Composed => Some(self.composed_pc),
            Role::Retained => self.retained_pc,
        }
    }
    /// Entry and site boundaries lie in owned code in the composed role.
    pub fn owned(&self) -> bool { self.kind != BoundaryKind::Return }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub label: Label,
    pub boundary: Label,
    pub address: u32,
    pub length: u32,
    pub differs: Option<Reason>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    boundaries: Vec<Boundary>,
    owned_code: Vec<CodeSpan>,
    writes: RawWrites,
    reads: Vec<Read>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Catalog {
    pub boundaries: Vec<Boundary>,
    pub owned_code: Vec<Interval>,
    pub writes: Writes,
    pub reads: Vec<Read>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPiece {
    kind: PieceKind,
    areas: Vec<AreaName>,
    carriers: Vec<TargetSpan>,
    edits: Vec<RawEdit>,
    footprint: Vec<RawFootprint>,
    premises: Vec<Premise>,
    catalog: RawCatalog,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Piece {
    pub kind: PieceKind,
    pub areas: Vec<AreaName>,
    pub carriers: Vec<TargetSpan>,
    pub edits: Vec<Edit>,
    pub footprint: Vec<Footprint>,
    pub premises: Vec<Premise>,
    pub catalog: Catalog,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Join {
    pub kind: JoinKind,
    pub members: Vec<PieceKind>,
    pub spans: Vec<TargetSpan>,
    pub basis: JoinBasis,
    pub reason: Option<Reason>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    pub record1: Sha256,
    pub record2: Sha256,
    pub loader_a: Sha256,
    pub loader_b: Sha256,
    pub image4: Sha256,
    pub image5: Sha256,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Identities {
    pub dat_sha256: Sha256,
    pub targets: Targets,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuardAttach {
    pub attach: Label,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGuard {
    attach: Label,
    attach_state: RawAttachRules,
    declared: DeclaredLists,
}

/// Contract, receipt and verdict schema; 5 carries the `application` section.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "u32", into = "u32")]
pub enum Schema {
    Four,
    Five,
}

impl TryFrom<u32> for Schema {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            4 => Ok(Self::Four),
            5 => Ok(Self::Five),
            _ => Err("schema must be 4 or 5".into()),
        }
    }
}

impl From<Schema> for u32 {
    fn from(value: Schema) -> Self {
        match value {
            Schema::Four => 4,
            Schema::Five => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ContractKind {
    ComposedStartupContract,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContract {
    schema: Schema,
    #[allow(dead_code)]
    kind: ContractKind,
    qualification: Qualification,
    pending_joins: Vec<JoinKind>,
    manifest_sha256: Sha256,
    canonical: Identities,
    composed: Identities,
    guard: RawGuard,
    joins: Vec<Join>,
    pieces: Vec<RawPiece>,
    #[serde(default)]
    application: Option<RawApplication>,
    pool_allocation: Option<PoolAllocation>,
}

/// Where a normalized word points in an allocation: from its base (inclusive) or up to its end (inclusive).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AllocationPosition {
    Base,
    Top,
}

/// Words of a read that hold pool addresses: every `stride` bytes from `offset`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AllocationWords {
    pub read: Label,
    pub offset: u32,
    pub stride: u32,
    pub position: AllocationPosition,
}

/// Physical `[start, end)` of the task reservation pool.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PoolSpan {
    pub start: u32,
    pub end: u32,
}

/// A read whose base word, every `stride` bytes from `offset`, names an allocation reserved by `core`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AllocationOwner {
    pub core: Core,
    pub read: Label,
    pub offset: u32,
    pub stride: u32,
}

/// Each role's allocations come from its storage-record read and its cursor read; listed read words
/// and the SP of each listed return boundary compare as (task, offset) within that role's own allocations,
/// and each core's tasks, ordered by base, must match between the roles.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PoolAllocation {
    pub pool: PoolSpan,
    pub records: Label,
    pub cursor: Label,
    pub words: Vec<AllocationWords>,
    pub stack_tops: Vec<Label>,
    pub owners: Vec<AllocationOwner>,
    pub citation: Citation,
}

/// Storage records are `{base, size, reserved}` words (replacement/task_storage/src.rs).
pub const ALLOCATION_RECORD_BYTES: u32 = 12;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PoolAllocationProblem {
    Pool,
    UnknownRead(Label),
    RecordLayout(Label),
    CursorLength(Label),
    WordLayout(Label),
    StackTop(Label),
    DuplicateWords(Label),
    Differs(Label),
    OwnerLayout(Label),
}

/// One word every `stride` bytes from `offset` fits each item of a `length`-byte read.
fn word_layout(offset: u32, stride: u32, length: u32) -> bool {
    stride != 0 && stride % 4 == 0 && offset % 4 == 0 && offset.checked_add(4).is_some_and(|end| end <= stride) && length % stride == 0
}

/// The relation's reads exist, are compared, and have whole records, a one-word cursor and words inside
/// their stride; each read has at most one `words` entry, and each stack top names a return boundary.
fn check_pool_allocation<'a>(relation: &PoolAllocation, boundaries: &BTreeMap<Label, Boundary>, reads: impl Iterator<Item = &'a Read>)
    -> Result<(), CatalogRefusal> {
    let refuse = |problem| Err(CatalogRefusal::PoolAllocation(problem));
    let reads: BTreeMap<&Label, &Read> = reads.map(|read| (&read.label, read)).collect();
    let read = |label: &Label| {
        let read = reads.get(label).copied().ok_or_else(|| CatalogRefusal::PoolAllocation(PoolAllocationProblem::UnknownRead(label.clone())))?;
        if read.differs.is_some() {
            return Err(CatalogRefusal::PoolAllocation(PoolAllocationProblem::Differs(label.clone())));
        }
        Ok(read)
    };
    if relation.pool.start >= relation.pool.end || relation.pool.start % 4 != 0 || relation.pool.end % 4 != 0 {
        return refuse(PoolAllocationProblem::Pool);
    }
    if read(&relation.records)?.length % ALLOCATION_RECORD_BYTES != 0 {
        return refuse(PoolAllocationProblem::RecordLayout(relation.records.clone()));
    }
    if read(&relation.cursor)?.length != 4 {
        return refuse(PoolAllocationProblem::CursorLength(relation.cursor.clone()));
    }
    let mut listed = BTreeSet::new();
    for words in &relation.words {
        if !word_layout(words.offset, words.stride, read(&words.read)?.length) {
            return refuse(PoolAllocationProblem::WordLayout(words.read.clone()));
        }
        if !listed.insert(&words.read) {
            return refuse(PoolAllocationProblem::DuplicateWords(words.read.clone()));
        }
    }
    for owner in &relation.owners {
        if !word_layout(owner.offset, owner.stride, read(&owner.read)?.length) {
            return refuse(PoolAllocationProblem::OwnerLayout(owner.read.clone()));
        }
    }
    for label in &relation.stack_tops {
        if boundaries.get(label).is_none_or(|boundary| boundary.kind != BoundaryKind::Return) {
            return refuse(PoolAllocationProblem::StackTop(label.clone()));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpanList {
    Carriers(PieceKind),
    Edits(PieceKind),
    Footprint(PieceKind),
    OwnedCode(PieceKind),
    WriteAllow(PieceKind),
    JoinSpans(JoinKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuardBound {
    Ranges(usize),
    PcSpans(usize),
    WatchClasses(usize),
    Partitions(usize),
    Declared(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogRefusal {
    /// JSON that does not have the schema-4 shape, including unknown keys and invalid scalars.
    Shape(String),
    Qualification,
    NoPieces,
    UnsortedSpans(SpanList),
    OverlappingSpans(SpanList),
    UnqualifiedLabel(Label),
    DuplicateLabel(Label),
    DuplicatePc { role: Role, pc: Pc },
    RecordLimit(Label),
    RowCap(u64),
    OwnedPcOutsideCode(Label),
    AttachUnknown(Label),
    AttachNotStop(Label),
    AttachNotRetained(Label),
    AttachCores(Label),
    Declaration { role: Role, problem: DeclarationProblem },
    AttachState(AttachRuleRefusal),
    WriteRangeShape(PieceKind),
    WritesPremise(PieceKind),
    AliasStraddle(PieceKind),
    ForeignTcm { piece: PieceKind, core: Core },
    GuardBound(GuardBound),
    DuplicateRead(Label),
    ReadBoundary(Label),
    ReadExtent(Label),
    ReadBudget(u64),
    JoinReason(JoinKind),
    OwnedCodeOverlap { first: PieceKind, second: PieceKind },
    BoundaryBound { role: Role, core: Core, count: usize },
    ReadNotRetained(Label),
    IdenticalDats,
    Application(ApplicationProblem),
    PoolAllocation(PoolAllocationProblem),
}

impl std::fmt::Display for CatalogRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "composed contract refused: {self:?}") }
}

impl std::error::Error for CatalogRefusal {}

#[derive(Clone, Debug)]
pub struct Contract {
    pub qualification: Qualification,
    pub pending_joins: Vec<JoinKind>,
    pub manifest_sha256: Sha256,
    pub canonical: Identities,
    pub composed: Identities,
    pub guard: GuardAttach,
    attach_core: Core,
    attach_rules: AttachRules,
    pub joins: Vec<Join>,
    pub pieces: Vec<Piece>,
    boundaries: BTreeMap<Label, Boundary>,
    guards: [CompositeGuardConfig; 2],
    declared: DeclaredLists,
    policies: Vec<PiecePolicy>,
    classes: Vec<WatchRange>,
    schema: Schema,
    application: Option<Application>,
    pool_allocation: Option<PoolAllocation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SitePlan {
    pub sites: BTreeMap<Pc, Label>,
    pub stops: BTreeSet<Label>,
}

impl SitePlan {
    pub fn trace_sites(&self) -> BTreeMap<u32, String> {
        self.sites.iter().map(|(pc, label)| (pc.0, label.0.clone())).collect()
    }
    pub fn stop_labels(&self) -> impl Iterator<Item = String> + '_ { self.stops.iter().map(|label| label.0.clone()) }
}

fn sorted_disjoint<T, K: Ord + Copy>(items: &[T], key: impl Fn(&T) -> (K, Interval), list: impl Fn() -> SpanList)
    -> Result<(), CatalogRefusal> {
    for pair in items.windows(2) {
        let ((space, previous), (next_space, next)) = (key(&pair[0]), key(&pair[1]));
        if (next_space, next.start) < (space, previous.start) {
            return Err(CatalogRefusal::UnsortedSpans(list()));
        }
        if space == next_space && next.start < previous.end {
            return Err(CatalogRefusal::OverlappingSpans(list()));
        }
    }
    Ok(())
}

/// Sorted union per space, merging overlapping and abutting ranges.
fn merge(mut ranges: Vec<WriteRange>) -> Vec<WriteRange> {
    ranges.sort_by_key(|range| (range.space, range.span.start));
    let mut merged: Vec<WriteRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if last.space == range.space && range.span.start <= last.span.end => {
                last.span.end = last.span.end.max(range.span.end);
            }
            _ => merged.push(range),
        }
    }
    merged
}

fn writes(kind: &PieceKind, raw: RawWrites, cores: &BTreeSet<Core>) -> Result<Writes, CatalogRefusal> {
    match (raw.basis, &raw.premise) {
        (WritesBasis::Premised, Some(_)) | (WritesBasis::Proved, None) => {}
        _ => return Err(CatalogRefusal::WritesPremise(kind.clone())),
    }
    let mut allow = Vec::with_capacity(raw.allow.len());
    for range in raw.allow {
        let span = Interval::new(range.start, range.end).map_err(CatalogRefusal::Shape)?;
        let space = match (range.target, range.core, range.bank) {
            (RawWriteTarget::Physical, core, None) => WriteSpace::Physical { core },
            (RawWriteTarget::Tcm, Some(core), Some(bank)) => WriteSpace::Tcm { core, bank },
            _ => return Err(CatalogRefusal::WriteRangeShape(kind.clone())),
        };
        allow.push(WriteRange { space, span });
    }
    sorted_disjoint(&allow, |range| (range.space, range.span), || SpanList::WriteAllow(kind.clone()))?;
    let mut translated = Vec::with_capacity(allow.len());
    for range in &allow {
        let span = match range.space {
            WriteSpace::Physical { core } => {
                if let Some(core) = core.filter(|core| !cores.contains(core)) {
                    return Err(CatalogRefusal::ForeignTcm { piece: kind.clone(), core });
                }
                range.span.physical().ok_or_else(|| CatalogRefusal::AliasStraddle(kind.clone()))?
            }
            WriteSpace::Tcm { core, .. } => {
                if !cores.contains(&core) {
                    return Err(CatalogRefusal::ForeignTcm { piece: kind.clone(), core });
                }
                range.span
            }
        };
        translated.push(WriteRange { space: range.space, span });
    }
    translated.sort_by_key(|range| (range.space, range.span.start));
    for core in Core::ALL {
        let mut bound: Vec<(GuardTarget, Interval)> = translated.iter().filter(|range| range.space.binds(core))
            .map(|range| (range.space.guard(), range.span)).collect();
        bound.sort_by_key(|(target, span)| (*target, span.start));
        if bound.windows(2).any(|pair| pair[0].0 == pair[1].0 && pair[1].1.start < pair[0].1.end) {
            return Err(CatalogRefusal::OverlappingSpans(SpanList::WriteAllow(kind.clone())));
        }
    }
    Ok(Writes { allow: translated, basis: raw.basis, premise: raw.premise })
}

fn piece(raw: RawPiece) -> Result<Piece, CatalogRefusal> {
    let kind = raw.kind;
    sorted_disjoint(&raw.carriers, |span| (span.target, span.span), || SpanList::Carriers(kind.clone()))?;
    let edits = raw.edits.into_iter().map(|edit| {
        Ok(Edit { span: TargetSpan { target: edit.target, span: Interval::new(edit.start, edit.end).map_err(CatalogRefusal::Shape)? }, sha256: edit.sha256 })
    }).collect::<Result<Vec<_>, CatalogRefusal>>()?;
    sorted_disjoint(&edits, |edit| (edit.span.target, edit.span.span), || SpanList::Edits(kind.clone()))?;
    let footprint = raw.footprint.into_iter().map(|item| {
        Ok(Footprint { span: TargetSpan { target: item.target, span: Interval::new(item.start, item.end).map_err(CatalogRefusal::Shape)? }, basis: item.basis, citation: item.citation })
    }).collect::<Result<Vec<_>, CatalogRefusal>>()?;
    sorted_disjoint(&footprint, |item| (item.span.target, item.span.span), || SpanList::Footprint(kind.clone()))?;
    let owned_code: Vec<Interval> = raw.catalog.owned_code.into_iter().map(|span| span.0).collect();
    sorted_disjoint(&owned_code, |span| ((), *span), || SpanList::OwnedCode(kind.clone()))?;
    if owned_code.iter().any(|span| span.physical().is_none()) {
        return Err(CatalogRefusal::AliasStraddle(kind.clone()));
    }
    for boundary in &raw.catalog.boundaries {
        if boundary.label.piece() != kind.as_str() {
            return Err(CatalogRefusal::UnqualifiedLabel(boundary.label.clone()));
        }
        if boundary.record_limit == 0 {
            return Err(CatalogRefusal::RecordLimit(boundary.label.clone()));
        }
        if boundary.owned() && !owned_code.iter().any(|span| span.holds(u64::from(boundary.composed_pc.0))) {
            return Err(CatalogRefusal::OwnedPcOutsideCode(boundary.label.clone()));
        }
    }
    for read in &raw.catalog.reads {
        if read.label.piece() != kind.as_str() {
            return Err(CatalogRefusal::UnqualifiedLabel(read.label.clone()));
        }
        if read.length == 0 || u64::from(read.address) + u64::from(read.length) > ADDRESS_END {
            return Err(CatalogRefusal::ReadExtent(read.label.clone()));
        }
    }
    let cores = raw.catalog.boundaries.iter().flat_map(|boundary| boundary.cores.iter()).collect();
    let writes = writes(&kind, raw.catalog.writes, &cores)?;
    Ok(Piece {
        kind,
        areas: raw.areas,
        carriers: raw.carriers,
        edits,
        footprint,
        premises: raw.premises,
        catalog: Catalog { boundaries: raw.catalog.boundaries, owned_code, writes, reads: raw.catalog.reads },
    })
}

fn stock(core: Core) -> DestinationPolicy {
    let everything = Interval { start: 0, end: ADDRESS_END };
    DestinationPolicy {
        allow: [WriteSpace::Physical { core: None }, WriteSpace::Tcm { core, bank: Bank::Data }, WriteSpace::Tcm { core, bank: Bank::Instruction }]
            .into_iter().map(|space| GuardRange { target: space.guard(), span: everything.guard() }).collect(),
        forbid: vec![],
    }
}

/// Both spellings of every owned span of `pieces` in DRAM: physical and `0x40000000` alias.
fn owned_spans<'a>(pieces: impl Iterator<Item = &'a Piece>) -> Vec<Interval> {
    let mut spans: Vec<Interval> = pieces.flat_map(|piece| &piece.catalog.owned_code).flat_map(|span| {
        let physical = span.physical().unwrap_or(*span);
        let alias = (physical.end <= DRAM_END)
            .then(|| Interval { start: physical.start + ALIAS_BASE, end: physical.end + ALIAS_BASE });
        [Some(physical), alias].into_iter().flatten()
    }).collect();
    spans.sort();
    let mut merged: Vec<Interval> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }
    merged
}

fn range_plan(range: &GuardRange) -> RangePlan {
    let (start, end) = (range.span.start(), range.span.end());
    match range.target {
        GuardTarget::Physical => RangePlan::Physical { start, end },
        GuardTarget::Tcm { core, bank } => RangePlan::Tcm {
            core: core.get(), bank: match bank { TcmBank::Data => Bank::Data, TcmBank::Instruction => Bank::Instruction }, start, end,
        },
    }
}

/// One policy per source in the order Cpu cores, CoreHost cores, PhysicalHost, ModelBus; in the composed role each
/// Cpu core gets one partition per piece.
fn guard_config(pieces: &[Piece], declared: &[Declaration], role: Role) -> (CompositeGuardConfig, Vec<PiecePolicy>) {
    let physical_only = DestinationPolicy {
        allow: vec![GuardRange { target: GuardTarget::Physical, span: Interval { start: 0, end: ADDRESS_END }.guard() }],
        forbid: vec![],
    };
    let mut policies = Vec::new();
    let cpu: Vec<SourcePolicy> = Core::ALL.into_iter().map(|core| {
        let partitions: Vec<PcPartition> = match role {
            Role::Retained => vec![],
            Role::Composed => pieces.iter().map(|piece| {
                let destinations = DestinationPolicy {
                    allow: piece.catalog.writes.allow.iter().filter(|range| range.space.binds(core))
                        .map(|range| GuardRange { target: range.space.guard(), span: range.span.guard() }).collect(),
                    forbid: vec![],
                };
                PcPartition { spans: owned_spans(std::iter::once(piece)).iter().map(|span| span.guard()).collect(), destinations }
            }).collect(),
        };
        for (index, (piece, partition)) in pieces.iter().zip(&partitions).enumerate() {
            policies.push(PiecePolicy {
                piece: piece.kind.clone(),
                core,
                partition: PartitionIndex(u16::try_from(index).expect("partitions per core are within MAX_GUARD_PARTITIONS")),
                basis: piece.catalog.writes.basis,
                premise: piece.catalog.writes.premise.clone(),
                spans: partition.spans.iter().map(|span| PlanSpan { start: span.start(), end: span.end() }).collect(),
                allow: partition.destinations.allow.iter().map(range_plan).collect(),
            });
        }
        SourcePolicy { source: WriteSource::Cpu(core.guard()), destinations: stock(core), partitions }
    }).collect();
    let host = Core::ALL.into_iter().map(|core| SourcePolicy { source: WriteSource::CoreHost(core.guard()), destinations: stock(core), partitions: vec![] });
    let bus = [WriteSource::PhysicalHost, WriteSource::ModelBus].into_iter()
        .map(|source| SourcePolicy { source, destinations: physical_only.clone(), partitions: vec![] });
    let declared = declared.iter().map(Declaration::guard).collect();
    (CompositeGuardConfig { sources: cpu.into_iter().chain(host).chain(bus).collect(), declared }, policies)
}

/// Per role: at most `MAX_GUARD_DECLARED_MUTATIONS`, each changing its register or decoder, TCM writers word aligned,
/// and no two declarations of one register or decoder leaving the same value.
fn check_declared(lists: &DeclaredLists) -> Result<(), CatalogRefusal> {
    for role in [Role::Retained, Role::Composed] {
        let list = lists.role(role);
        if list.len() > MAX_GUARD_DECLARED_MUTATIONS {
            return Err(CatalogRefusal::GuardBound(GuardBound::Declared(list.len())));
        }
        let refuse = |problem| Err(CatalogRefusal::Declaration { role, problem });
        for (index, declaration) in list.iter().enumerate() {
            let (mutated, previous, next) = declaration.link();
            if previous == next {
                return refuse(DeclarationProblem::Unchanged(index));
            }
            if matches!(declaration, Declaration::TcmRegion { writer, .. } if writer % 4 != 0) {
                return refuse(DeclarationProblem::UnalignedWriter(index));
            }
            if list[..index].iter().map(Declaration::link).any(|(other, from, _)| other == mutated && from == previous) {
                return refuse(DeclarationProblem::SharedPrevious(index));
            }
        }
    }
    Ok(())
}

fn check_guard(config: &CompositeGuardConfig) -> Result<(), CatalogRefusal> {
    let lists = |policy: &SourcePolicy| {
        std::iter::once(&policy.destinations).chain(policy.partitions.iter().map(|partition| &partition.destinations))
            .map(|destinations| destinations.allow.len() + destinations.forbid.len()).sum::<usize>()
    };
    let ranges: usize = config.sources.iter().map(lists).sum();
    let spans: usize = config.sources.iter().flat_map(|policy| &policy.partitions).map(|partition| partition.spans.len()).sum();
    let partitions = config.sources.iter().map(|policy| policy.partitions.len()).max().unwrap_or(0);
    if ranges > MAX_GUARD_RANGES {
        return Err(CatalogRefusal::GuardBound(GuardBound::Ranges(ranges)));
    }
    if spans > MAX_GUARD_PC_SPANS {
        return Err(CatalogRefusal::GuardBound(GuardBound::PcSpans(spans)));
    }
    if partitions > MAX_GUARD_PARTITIONS {
        return Err(CatalogRefusal::GuardBound(GuardBound::Partitions(partitions)));
    }
    Ok(())
}

impl Contract {
    pub fn parse(bytes: &[u8]) -> Result<Self, CatalogRefusal> {
        let raw: RawContract = serde_json::from_slice(bytes).map_err(|error| CatalogRefusal::Shape(error.to_string()))?;
        match (raw.qualification, raw.pending_joins.is_empty()) {
            (Qualification::Pending, false) | (Qualification::HostComposed, true) => {}
            _ => return Err(CatalogRefusal::Qualification),
        }
        for join in &raw.joins {
            sorted_disjoint(&join.spans, |span| (span.target, span.span), || SpanList::JoinSpans(join.kind.clone()))?;
            if join.basis == JoinBasis::ObservationQualified && join.reason.is_none() {
                return Err(CatalogRefusal::JoinReason(join.kind.clone()));
            }
        }
        if raw.pieces.is_empty() {
            return Err(CatalogRefusal::NoPieces);
        }
        let pieces = raw.pieces.into_iter().map(piece).collect::<Result<Vec<_>, _>>()?;
        if raw.canonical.dat_sha256 == raw.composed.dat_sha256 {
            return Err(CatalogRefusal::IdenticalDats);
        }
        let owned: Vec<(Interval, &PieceKind)> = pieces.iter()
            .flat_map(|piece| piece.catalog.owned_code.iter().map(move |span| (span.physical().unwrap_or(*span), &piece.kind)))
            .collect();
        for (index, (span, kind)) in owned.iter().enumerate() {
            if let Some((_, other)) = owned[..index].iter()
                .find(|(other, other_kind)| other_kind != kind && other.start < span.end && span.start < other.end)
            {
                return Err(CatalogRefusal::OwnedCodeOverlap { first: (*other).clone(), second: (*kind).clone() });
            }
        }
        let mut boundaries = BTreeMap::new();
        let mut pcs: [BTreeSet<Pc>; 2] = [BTreeSet::new(), BTreeSet::new()];
        let mut rows = 0u64;
        for boundary in pieces.iter().flat_map(|piece| &piece.catalog.boundaries) {
            if boundaries.insert(boundary.label.clone(), boundary.clone()).is_some() {
                return Err(CatalogRefusal::DuplicateLabel(boundary.label.clone()));
            }
            for (index, role) in [Role::Retained, Role::Composed].into_iter().enumerate() {
                if let Some(pc) = boundary.pc(role)
                    && !pcs[index].insert(pc)
                {
                    return Err(CatalogRefusal::DuplicatePc { role, pc });
                }
            }
            rows += u64::from(boundary.record_limit);
        }
        if rows > ROW_LIMIT as u64 {
            return Err(CatalogRefusal::RowCap(rows));
        }
        for role in [Role::Retained, Role::Composed] {
            for core in Core::ALL {
                let count = boundaries.values().filter(|boundary| boundary.cores.contains(core) && boundary.pc(role).is_some()).count();
                if count > BOUNDARY_PC_LIMIT {
                    return Err(CatalogRefusal::BoundaryBound { role, core, count });
                }
            }
        }
        let attach = boundaries.get(&raw.guard.attach).ok_or_else(|| CatalogRefusal::AttachUnknown(raw.guard.attach.clone()))?;
        if !attach.stop {
            return Err(CatalogRefusal::AttachNotStop(attach.label.clone()));
        }
        if attach.retained_pc.is_none() {
            return Err(CatalogRefusal::AttachNotRetained(attach.label.clone()));
        }
        let attach_core = attach.cores.single().ok_or_else(|| CatalogRefusal::AttachCores(attach.label.clone()))?;
        let mut reads = BTreeSet::new();
        let mut read_bytes = 0u64;
        for read in pieces.iter().flat_map(|piece| &piece.catalog.reads) {
            if !reads.insert(read.label.clone()) {
                return Err(CatalogRefusal::DuplicateRead(read.label.clone()));
            }
            let boundary = boundaries.get(&read.boundary).filter(|boundary| boundary.stop)
                .ok_or_else(|| CatalogRefusal::ReadBoundary(read.label.clone()))?;
            read_bytes += u64::from(read.length) * u64::from(boundary.record_limit);
            if boundary.retained_pc.is_none() && read.differs.is_none() {
                return Err(CatalogRefusal::ReadNotRetained(read.label.clone()));
            }
        }
        if read_bytes > READ_BYTE_LIMIT {
            return Err(CatalogRefusal::ReadBudget(read_bytes));
        }
        if let Some(relation) = &raw.pool_allocation {
            check_pool_allocation(relation, &boundaries, pieces.iter().flat_map(|piece| &piece.catalog.reads))?;
        }
        let mut carriers = Vec::new();
        let mut carrier_spans = Vec::new();
        for piece in &pieces {
            for carrier in &piece.carriers {
                let span = carrier.span.physical().ok_or_else(|| CatalogRefusal::AliasStraddle(piece.kind.clone()))?;
                carriers.push(WriteRange { space: WriteSpace::Physical { core: None }, span });
                carrier_spans.push((piece.kind.clone(), *carrier, Span { start: span.start, end: span.end }));
            }
        }
        let merged = merge(carriers);
        check_declared(&raw.guard.declared)?;
        let (retained_guard, _) = guard_config(&pieces, &raw.guard.declared.retained, Role::Retained);
        let (composed_guard, policies) = guard_config(&pieces, &raw.guard.declared.composed, Role::Composed);
        let guards = [retained_guard, composed_guard];
        guards.iter().try_for_each(check_guard)?;
        let classes: Vec<WatchRange> = merged.into_iter()
            .map(|range| WatchRange { start: range.span.start, end: range.span.end, kinds: WatchKinds::WRITES }).collect();
        let application = match (raw.schema, raw.application) {
            (Schema::Four, None) => None,
            (Schema::Five, Some(raw)) => Some(application(raw, &pieces).map_err(CatalogRefusal::Application)?),
            _ => return Err(CatalogRefusal::Application(ApplicationProblem::Section)),
        };
        let application_classes = application.as_ref().map_or(0, |application| application.code.len());
        if classes.is_empty() || classes.len() + application_classes > WATCH_CLASS_LIMIT {
            return Err(CatalogRefusal::GuardBound(GuardBound::WatchClasses(classes.len() + application_classes)));
        }
        let cores = boundaries.values().flat_map(|boundary| boundary.cores.iter()).collect();
        let context = RuleContext { attach_core, cores, boundaries: &boundaries, carriers: carrier_spans };
        let attach_rules = AttachRules::parse(raw.guard.attach_state, &context).map_err(CatalogRefusal::AttachState)?;
        Ok(Self {
            qualification: raw.qualification,
            pending_joins: raw.pending_joins,
            manifest_sha256: raw.manifest_sha256,
            canonical: raw.canonical,
            composed: raw.composed,
            guard: GuardAttach { attach: raw.guard.attach },
            declared: raw.guard.declared,
            attach_core,
            attach_rules,
            joins: raw.joins,
            pieces,
            boundaries,
            guards,
            policies,
            classes,
            schema: raw.schema,
            application,
            pool_allocation: raw.pool_allocation,
        })
    }

    pub fn schema(&self) -> Schema { self.schema }

    /// The schema-5 `application` section.
    pub fn application(&self) -> Option<&Application> { self.application.as_ref() }

    pub fn identities(&self, role: Role) -> &Identities {
        match role {
            Role::Retained => &self.canonical,
            Role::Composed => &self.composed,
        }
    }

    pub fn boundary(&self, label: &Label) -> Option<&Boundary> { self.boundaries.get(label) }

    pub fn boundaries(&self) -> impl Iterator<Item = &Boundary> { self.boundaries.values() }

    pub fn reads(&self) -> impl Iterator<Item = &Read> { self.pieces.iter().flat_map(|piece| &piece.catalog.reads) }

    pub fn pool_allocation(&self) -> Option<&PoolAllocation> { self.pool_allocation.as_ref() }

    pub fn sites(&self, role: Role) -> SitePlan {
        let armed = || self.boundaries.values().filter_map(|boundary| boundary.pc(role).map(|pc| (pc, boundary)));
        SitePlan {
            sites: armed().map(|(pc, boundary)| (pc, boundary.label.clone())).collect(),
            stops: armed().filter(|(_, boundary)| boundary.stop).map(|(_, boundary)| boundary.label.clone()).collect(),
        }
    }

    pub fn guard_config(&self, role: Role) -> CompositeGuardConfig {
        self.guards[match role { Role::Retained => 0, Role::Composed => 1 }].clone()
    }

    /// Each piece's partition per core in the composed role.
    pub fn policies(&self, role: Role) -> Vec<PiecePolicy> {
        match role {
            Role::Retained => vec![],
            Role::Composed => self.policies.clone(),
        }
    }

    pub fn attach_rules(&self) -> &AttachRules { &self.attach_rules }

    pub fn attach_core(&self) -> Core { self.attach_core }

    pub fn declared(&self, role: Role) -> &[Declaration] { self.declared.role(role) }

    /// Write classes over every owned code carrier, in physical addresses.
    pub fn watch_classes(&self) -> Vec<WatchRange> { self.classes.clone() }

    /// Every piece's owned code PC spans in both spellings.
    pub fn owned_code_spellings(&self) -> Vec<(u64, u64)> {
        owned_spans(self.pieces.iter()).into_iter().map(|span| (span.start, span.end)).collect()
    }

    pub fn plan(&self) -> Plan {
        Plan {
            roles: [Role::Retained, Role::Composed].into_iter().map(|role| self.role_plan(role)).collect(),
            relations: self.attach_rules.relations.clone(),
            application: self.application.clone(),
        }
    }

    fn role_plan(&self, role: Role) -> RolePlan {
        let sites = self.sites(role);
        RolePlan {
            role,
            armed: Core::ALL.into_iter().map(|core| ArmedCore {
                core,
                sites: sites.sites.iter()
                    .filter_map(|(pc, label)| self.boundaries.get(label).filter(|boundary| boundary.cores.contains(core))
                        .map(|boundary| ArmedSite { pc: *pc, label: label.clone(), stop: boundary.stop }))
                    .collect(),
            }).collect(),
            stop_labels: sites.stops.into_iter().collect(),
            watch_classes: self.classes.iter().map(|class| PlanSpan { start: class.start, end: class.end }).collect(),
            guard: self.guard_config(role).sources.iter().map(PolicyPlan::of).collect(),
            policies: self.policies(role),
            declared: self.declared(role).to_vec(),
        }
    }
}

/// The host-derived observation plan for both roles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Plan {
    pub roles: Vec<RolePlan>,
    pub relations: crate::attach_relations::Relations,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<Application>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RolePlan {
    pub role: Role,
    pub armed: Vec<ArmedCore>,
    pub stop_labels: Vec<Label>,
    pub watch_classes: Vec<PlanSpan>,
    pub guard: Vec<PolicyPlan>,
    pub policies: Vec<PiecePolicy>,
    pub declared: Vec<Declaration>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArmedCore {
    pub core: Core,
    pub sites: Vec<ArmedSite>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArmedSite {
    pub pc: Pc,
    pub label: Label,
    pub stop: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct PlanSpan {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SourcePlan {
    Cpu { core: usize },
    CoreHost { core: usize },
    PhysicalHost,
    ModelBus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "target", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RangePlan {
    Physical { start: u64, end: u64 },
    Tcm { core: usize, bank: Bank, start: u64, end: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PartitionPlan {
    pub spans: Vec<PlanSpan>,
    pub allow: Vec<RangePlan>,
    pub forbid: Vec<RangePlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PolicyPlan {
    pub source: SourcePlan,
    pub allow: Vec<RangePlan>,
    pub forbid: Vec<RangePlan>,
    pub partitions: Vec<PartitionPlan>,
}

impl PolicyPlan {
    fn of(policy: &SourcePolicy) -> Self {
        let ranges = |ranges: &[GuardRange]| ranges.iter().map(range_plan).collect::<Vec<_>>();
        Self {
            source: match policy.source {
                WriteSource::Cpu(core) => SourcePlan::Cpu { core: core.get() },
                WriteSource::CoreHost(core) => SourcePlan::CoreHost { core: core.get() },
                WriteSource::PhysicalHost => SourcePlan::PhysicalHost,
                WriteSource::ModelBus => SourcePlan::ModelBus,
            },
            allow: ranges(&policy.destinations.allow),
            forbid: ranges(&policy.destinations.forbid),
            partitions: policy.partitions.iter().map(|partition| PartitionPlan {
                spans: partition.spans.iter().map(|span| PlanSpan { start: span.start(), end: span.end() }).collect(),
                allow: ranges(&partition.destinations.allow),
                forbid: ranges(&partition.destinations.forbid),
            }).collect(),
        }
    }
}

/// The physical address of a DRAM address in either spelling.
pub fn physical_address(address: u32) -> u32 {
    if (ALIAS_BASE..ALIAS_END).contains(&u64::from(address)) { address - ALIAS_BASE as u32 } else { address }
}

/// A code word as a role's decoded image holds it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CodeWord(pub u32);

/// Instructions entered across both cores.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Instructions(pub u64);

macro_rules! address_label {
    ($name:ident, $prefix:literal, $what:literal) => {
        /// The prefix and the stock PC in lowercase hexadecimal.
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let parsed = value.strip_prefix($prefix)
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok().filter(|pc| format!("{pc:x}") == hex));
                match parsed {
                    Some(_) => Ok(Self(value)),
                    None => Err(concat!($what, " is not ", $prefix, "<lowercase hex>").into()),
                }
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self { value.0 }
        }
        impl $name {
            pub fn of(pc: Pc) -> Self { Self(format!(concat!($prefix, "{:x}"), pc.0)) }
            pub fn as_str(&self) -> &str { &self.0 }
        }
    };
}

address_label!(SiteLabel, "app-", "application site label");
address_label!(QueueLabel, "queue-", "queue site label");

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SiteKind {
    Task1,
    Pump,
    HookTarget,
    Adapter,
    Uilib,
    Shared,
    Publication,
    Initializer,
}

impl SiteKind {
    /// Sites only task1 executes; its committed context is checked at each.
    pub fn task1(self) -> bool { matches!(self, Self::Task1 | Self::Pump | Self::HookTarget | Self::Adapter) }
}

/// Every site a schema-5 contract lists, by stock PC.
pub const APPLICATION_SITES: [(u32, SiteKind); 56] = {
    use SiteKind::{Adapter, HookTarget, Initializer, Publication, Pump, Shared, Task1, Uilib};
    [
        (0xc22f60, Task1), (0xc22f64, Task1), (0xc22f6c, Task1), (0xc22f70, Task1), (0xc22f74, Task1), (0xc22f84, Task1),
        (0xbfb124, Task1), (0xc22bd0, Task1), (0xc22fb4, Task1), (0xc22fd4, Task1), (0xc22fd8, Task1), (0xc23094, Task1),
        (0xc230a0, Task1), (0xc230b4, Task1), (0xc230c0, Task1), (0xc230d8, Task1), (0xc230f4, Task1), (0xc23104, Task1),
        (0xc23114, Task1), (0xc2311c, Task1), (0xc2312c, Task1), (0xc2314c, Task1), (0xc23150, Task1), (0xc231ec, Task1),
        (0xc231fc, Task1), (0xc23274, Task1), (0xc23278, Task1),
        (0xbfbd20, Pump), (0xbfbd24, Pump), (0xbfbd28, Pump), (0xbfbd2c, Pump), (0xbfbd30, Pump), (0xbfbd34, Pump),
        (0xbfbd38, Pump), (0xbfbd3c, Pump),
        (0xbfb1b8, HookTarget), (0xbfb20c, HookTarget), (0xbfb1e0, HookTarget),
        (0xdb5124, Adapter), (0xdb515c, Adapter), (0xdb5170, Adapter), (0xdb5174, Adapter), (0xdb5178, Adapter),
        (0xdb51d0, Adapter),
        (0x725214, Shared),
        (0x11090a4, Uilib), (0x1109130, Uilib), (0x1108a2c, Uilib), (0x110f9d8, Uilib),
        (0x73b4b0, Publication), (0x73b558, Publication),
        (0x69d2e0, Initializer), (0x69d2e4, Initializer), (0xd985a4, Initializer), (0xd985c8, Initializer),
        (0xd985d8, Initializer),
    ]
};

/// The shared queue code's scope points on task1's stack.
pub const QUEUE_SITES: [u32; 9] = [0x725318, 0x725378, 0x72537c, 0x7253c0, 0x7253f8, 0x7253fc, 0x725414, 0x725418, 0x72541c];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationSite {
    pub label: SiteLabel,
    pub kind: SiteKind,
    pub retained_pc: Pc,
    pub composed_pc: Pc,
    pub retained_word: CodeWord,
    pub composed_word: CodeWord,
}

impl ApplicationSite {
    pub fn pc(&self, role: Role) -> Pc {
        match role {
            Role::Retained => self.retained_pc,
            Role::Composed => self.composed_pc,
        }
    }
    pub fn word(&self, role: Role) -> CodeWord {
        match role {
            Role::Retained => self.retained_word,
            Role::Composed => self.composed_word,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueueSite {
    pub label: QueueLabel,
    pub pc: Pc,
    pub word: CodeWord,
}

/// Kernel domain A (`0`) or B (`1`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Domain {
    A,
    B,
}

impl TryFrom<u8> for Domain {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::A),
            1 => Ok(Self::B),
            _ => Err("domain is not 0 or 1".into()),
        }
    }
}

impl From<Domain> for u8 {
    fn from(value: Domain) -> Self { value.index() as u8 }
}

impl Domain {
    pub const ALL: [Self; 2] = [Self::A, Self::B];
    pub fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }
    /// The domain of a core reading CP14 identity `identity` in a recorded model.
    pub fn of_identity(identity: u32) -> Option<Self> {
        crate::run::CoreIdentities::domain(identity).map(|index| [Self::A, Self::B][index])
    }
}

/// A kernel cell's guest address.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Cell(pub u32);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskLayout {
    pub domain: Domain,
    pub creation: Cell,
    pub task: Cell,
    pub ids: Cell,
    pub domains: Cell,
    pub current: Cell,
    pub selected: Cell,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodeLabel {
    Task1Body,
    Task1Setup,
    Pump,
}

/// Physical `[start, end)` of stock application code with each role's image digest of it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodeRange {
    pub label: CodeLabel,
    pub start: u64,
    pub end: u64,
    pub retained_sha256: Sha256,
    pub composed_sha256: Sha256,
}

impl CodeRange {
    pub fn sha256(&self, role: Role) -> &Sha256 {
        match role {
            Role::Retained => &self.retained_sha256,
            Role::Composed => &self.composed_sha256,
        }
    }
    pub fn overlaps(&self, start: u64, end: u64) -> bool { start < self.end && self.start < end }
}

/// The settle interval after a first-ready candidate and the instruction count the search must end by.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub settle: Instructions,
    pub end: Instructions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct HookCall {
    pub site: Pc,
    pub target: Pc,
    pub continuation: Pc,
}

/// The pump's hook calls in order (prehook, normalizer, posthook) and its return instruction, decoded from the
/// contract's code words.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PumpProfile {
    pub entry: Pc,
    pub calls: [HookCall; 3],
    pub returned: Pc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApplication {
    sites: Vec<ApplicationSite>,
    queue_sites: Vec<QueueSite>,
    task_layouts: Vec<TaskLayout>,
    code: Vec<CodeRange>,
    readiness: Readiness,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Application {
    pub sites: Vec<ApplicationSite>,
    pub queue_sites: Vec<QueueSite>,
    /// Domain A, then B.
    pub task_layouts: [TaskLayout; 2],
    pub code: Vec<CodeRange>,
    pub readiness: Readiness,
    pub pump: PumpProfile,
}

impl Application {
    pub fn site(&self, label: &SiteLabel) -> Option<&ApplicationSite> { self.sites.iter().find(|site| &site.label == label) }
    pub fn role_pc(&self, retained: Pc, role: Role) -> Option<Pc> {
        self.sites.iter().find(|site| site.retained_pc == retained).map(|site| site.pc(role))
    }
    pub fn queue(&self, label: &QueueLabel) -> Option<&QueueSite> { self.queue_sites.iter().find(|site| &site.label == label) }
    pub fn layout(&self, domain: Domain) -> &TaskLayout { &self.task_layouts[domain.index()] }
    pub fn code_range(&self, label: CodeLabel) -> Option<&CodeRange> { self.code.iter().find(|range| range.label == label) }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationProblem {
    /// Schema 5 without an `application` section, or schema 4 with one.
    Section,
    SiteLabel(Pc),
    /// The sites' stock PCs and kinds differ from `APPLICATION_SITES`, or a composed PC is unaligned.
    Sites,
    QueueLabel(Pc),
    QueueSites,
    /// A composed-role site inside a piece's owned code, other than a pump site in the `application-pump` piece.
    SiteInPiece(Pc),
    TaskLayouts,
    CodeRanges,
    Readiness,
    /// A role's pump words do not decode to three calls of the hook targets and a return, or the roles' decodings
    /// differ.
    Pump,
}

fn branch_target(pc: u32, word: u32) -> Option<u32> {
    (word >> 24 == 0xeb).then(|| (i64::from(pc) + 8 + i64::from(((word << 8) as i32) >> 6)) as u32)
}

/// `bx lr` or `pop {..., pc}`.
fn returns(word: u32) -> bool { word == 0xe12f_ff1e || (word & 0xffff_0000 == 0xe8bd_0000 && word & (1 << 15) != 0) }

fn pump_profile(sites: &[ApplicationSite], role: Role) -> Option<PumpProfile> {
    let mut pump: Vec<&ApplicationSite> = sites.iter().filter(|site| site.kind == SiteKind::Pump).collect();
    pump.sort_by_key(|site| site.pc(role));
    let mut calls = Vec::with_capacity(3);
    let mut returned = None;
    for site in &pump {
        let pc = site.pc(role);
        if let Some(target) = branch_target(pc.0, site.word(role).0) {
            calls.push(HookCall { site: pc, target: Pc(target), continuation: Pc(pc.0 + 4) });
        } else if returns(site.word(role).0) {
            returned = Some(pc);
            break;
        }
    }
    let hooks: BTreeSet<Pc> = sites.iter().filter(|site| site.kind == SiteKind::HookTarget).map(|site| site.pc(role)).collect();
    let calls: [HookCall; 3] = calls.try_into().ok()?;
    let targets: BTreeSet<Pc> = calls.iter().map(|call| call.target).collect();
    let continued = calls.iter().all(|call| pump.iter().any(|site| site.pc(role) == call.continuation));
    let entry = pump.first()?.pc(role);
    (targets == hooks && continued).then_some(PumpProfile { entry, calls, returned: returned? })
}

fn application(raw: RawApplication, pieces: &[Piece]) -> Result<Application, ApplicationProblem> {
    if let Some(site) = raw.sites.iter().find(|site| site.label != SiteLabel::of(site.retained_pc)) {
        return Err(ApplicationProblem::SiteLabel(site.retained_pc));
    }
    let listed: BTreeMap<u32, SiteKind> = raw.sites.iter().map(|site| (site.retained_pc.0, site.kind)).collect();
    let required: BTreeMap<u32, SiteKind> = APPLICATION_SITES.into_iter().collect();
    if listed.len() != raw.sites.len() || listed != required || raw.sites.iter().any(|site| site.composed_pc.0 % 4 != 0) {
        return Err(ApplicationProblem::Sites);
    }
    if let Some(site) = raw.queue_sites.iter().find(|site| site.label != QueueLabel::of(site.pc)) {
        return Err(ApplicationProblem::QueueLabel(site.pc));
    }
    let queued: BTreeSet<u32> = raw.queue_sites.iter().map(|site| site.pc.0).collect();
    if queued.len() != raw.queue_sites.len() || queued != QUEUE_SITES.into_iter().collect() {
        return Err(ApplicationProblem::QueueSites);
    }
    let owned: Vec<(Interval, &PieceKind)> = pieces.iter()
        .flat_map(|piece| piece.catalog.owned_code.iter().map(move |span| (span.physical().unwrap_or(*span), &piece.kind)))
        .collect();
    let owner = |pc: Pc| owned.iter().find(|(span, _)| span.holds(u64::from(physical_address(pc.0)))).map(|&(_, kind)| kind);
    let pump_piece = PieceKind::application_pump();
    let actor_piece = PieceKind::task1_actor();
    let exempt = |site: &ApplicationSite| {
        (site.kind == SiteKind::Pump && owner(site.composed_pc) == Some(&pump_piece))
            || (site.kind == SiteKind::Task1 && owner(site.composed_pc) == Some(&actor_piece))
    };
    let sites = raw.sites.iter().filter(|site| !exempt(site)).map(|site| site.composed_pc);
    if let Some(pc) = sites.chain(raw.queue_sites.iter().map(|site| site.pc)).find(|&pc| owner(pc).is_some()) {
        return Err(ApplicationProblem::SiteInPiece(pc));
    }
    let task_layouts = match raw.task_layouts[..] {
        [a, b] if a.domain == Domain::A && b.domain == Domain::B => [a, b],
        _ => return Err(ApplicationProblem::TaskLayouts),
    };
    let cells = task_layouts.iter().flat_map(|layout| [layout.creation, layout.task, layout.ids, layout.domains, layout.current, layout.selected]);
    if cells.into_iter().any(|cell| cell.0 == 0 || cell.0 % 4 != 0) {
        return Err(ApplicationProblem::TaskLayouts);
    }
    let labels: BTreeSet<CodeLabel> = raw.code.iter().map(|range| range.label).collect();
    let malformed = raw.code.iter().any(|range| range.start >= range.end || range.end > ADDRESS_END);
    let overlapping = raw.code.iter().enumerate().any(|(index, range)| raw.code[..index].iter().any(|other| range.overlaps(other.start, other.end)));
    if raw.code.len() != 3 || labels.len() != 3 || malformed || overlapping {
        return Err(ApplicationProblem::CodeRanges);
    }
    if raw.readiness.settle.0 == 0 || raw.readiness.settle >= raw.readiness.end {
        return Err(ApplicationProblem::Readiness);
    }
    let pump = match (pump_profile(&raw.sites, Role::Retained), pump_profile(&raw.sites, Role::Composed)) {
        (Some(retained), Some(composed)) if retained == composed => retained,
        _ => return Err(ApplicationProblem::Pump),
    };
    Ok(Application { sites: raw.sites, queue_sites: raw.queue_sites, task_layouts, code: raw.code, readiness: raw.readiness, pump })
}
