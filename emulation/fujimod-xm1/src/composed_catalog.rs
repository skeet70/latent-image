//! Strict parse of `composed-contract.json` (schema 1) and the observation plan derived from it.
use fujimod_emulator::board::observation::{
    CompositeGuardConfig, DestinationPolicy, GuardCore, GuardRange, GuardSpan, GuardTarget, MAX_GUARD_PC_SPANS,
    MAX_GUARD_RANGES, OwnedPcPolicy, SourcePolicy, TcmBank, WatchKinds, WatchRange, WriteSource,
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
    /// No enumerated destinations; the piece's owned code is left out of the owned-PC policy.
    Unenumerated,
}

/// The composite guard's write policy for code at owned PCs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confinement {
    /// No owned-PC policy; every CPU may write all physical memory and its own TCM.
    Stock,
    /// Owned code of every enumerated piece shares the union of those pieces' `writes.allow`.
    UnionOfEnumeratedAllows,
}

/// Owned code of an unenumerated piece, which runs under the stock policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnconfinedCode {
    pub piece: PieceKind,
    pub start: u64,
    pub end: u64,
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

/// A TCM span is an offset range within that core's bank.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum WriteSpace {
    Physical,
    Tcm { core: Core, bank: Bank },
}

impl WriteSpace {
    fn guard(self) -> GuardTarget {
        match self {
            Self::Physical => GuardTarget::Physical,
            Self::Tcm { core, bank } => GuardTarget::Tcm { core: core.guard(), bank: bank.guard() },
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

impl Piece {
    pub fn enumerated(&self) -> bool { self.catalog.writes.basis != WritesBasis::Unenumerated }
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GuardAttach {
    pub attach: Label,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "u32")]
struct SchemaOne;

impl TryFrom<u32> for SchemaOne {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 1 { Ok(Self) } else { Err("contract schema must be 1".into()) }
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
    #[allow(dead_code)]
    schema: SchemaOne,
    #[allow(dead_code)]
    kind: ContractKind,
    qualification: Qualification,
    pending_joins: Vec<JoinKind>,
    manifest_sha256: Sha256,
    canonical: Identities,
    composed: Identities,
    guard: GuardAttach,
    joins: Vec<Join>,
    pieces: Vec<RawPiece>,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogRefusal {
    /// JSON that does not have the schema-1 shape, including unknown keys and invalid scalars.
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
    UnenumeratedAllow(PieceKind),
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
    pub joins: Vec<Join>,
    pub pieces: Vec<Piece>,
    boundaries: BTreeMap<Label, Boundary>,
    guards: [CompositeGuardConfig; 2],
    classes: Vec<WatchRange>,
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
        (WritesBasis::Premised | WritesBasis::Unenumerated, Some(_)) | (WritesBasis::Proved, None) => {}
        _ => return Err(CatalogRefusal::WritesPremise(kind.clone())),
    }
    if raw.basis == WritesBasis::Unenumerated && !raw.allow.is_empty() {
        return Err(CatalogRefusal::UnenumeratedAllow(kind.clone()));
    }
    let mut allow = Vec::with_capacity(raw.allow.len());
    for range in raw.allow {
        let span = Interval::new(range.start, range.end).map_err(CatalogRefusal::Shape)?;
        let space = match (range.target, range.core, range.bank) {
            (RawWriteTarget::Physical, None, None) => WriteSpace::Physical,
            (RawWriteTarget::Tcm, Some(core), Some(bank)) => WriteSpace::Tcm { core, bank },
            _ => return Err(CatalogRefusal::WriteRangeShape(kind.clone())),
        };
        allow.push(WriteRange { space, span });
    }
    sorted_disjoint(&allow, |range| (range.space, range.span), || SpanList::WriteAllow(kind.clone()))?;
    let mut translated = Vec::with_capacity(allow.len());
    for range in &allow {
        let span = match range.space {
            WriteSpace::Physical => range.span.physical().ok_or_else(|| CatalogRefusal::AliasStraddle(kind.clone()))?,
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
    if translated.windows(2).any(|pair| pair[0].space == pair[1].space && pair[1].span.start < pair[0].span.end) {
        return Err(CatalogRefusal::OverlappingSpans(SpanList::WriteAllow(kind.clone())));
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
        allow: [WriteSpace::Physical, WriteSpace::Tcm { core, bank: Bank::Data }, WriteSpace::Tcm { core, bank: Bank::Instruction }]
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

/// One policy per source in the order Cpu cores, CoreHost cores, PhysicalHost, ModelBus.
fn guard_config(pieces: &[Piece], role: Role) -> CompositeGuardConfig {
    let spans = match role {
        Role::Retained => vec![],
        Role::Composed => owned_spans(pieces.iter().filter(|piece| piece.enumerated())),
    };
    let allow = merge(pieces.iter().filter(|piece| piece.enumerated()).flat_map(|piece| piece.catalog.writes.allow.iter().copied()).collect());
    let physical_only = DestinationPolicy {
        allow: vec![GuardRange { target: GuardTarget::Physical, span: Interval { start: 0, end: ADDRESS_END }.guard() }],
        forbid: vec![],
    };
    let cpu = Core::ALL.into_iter().map(|core| SourcePolicy {
        source: WriteSource::Cpu(core.guard()),
        destinations: stock(core),
        owned_pc: (!spans.is_empty()).then(|| OwnedPcPolicy {
            spans: spans.iter().map(|span| span.guard()).collect(),
            destinations: DestinationPolicy {
                allow: allow.iter()
                    .filter(|range| match range.space { WriteSpace::Physical => true, WriteSpace::Tcm { core: owner, .. } => owner == core })
                    .map(|range| GuardRange { target: range.space.guard(), span: range.span.guard() })
                    .collect(),
                forbid: vec![],
            },
        }),
    });
    let host = Core::ALL.into_iter().map(|core| SourcePolicy { source: WriteSource::CoreHost(core.guard()), destinations: stock(core), owned_pc: None });
    let bus = [WriteSource::PhysicalHost, WriteSource::ModelBus].into_iter()
        .map(|source| SourcePolicy { source, destinations: physical_only.clone(), owned_pc: None });
    CompositeGuardConfig { sources: cpu.chain(host).chain(bus).collect() }
}

fn check_guard(config: &CompositeGuardConfig) -> Result<(), CatalogRefusal> {
    let ranges: usize = config.sources.iter().map(|policy| {
        policy.destinations.allow.len() + policy.destinations.forbid.len()
            + policy.owned_pc.as_ref().map_or(0, |owned| owned.destinations.allow.len() + owned.destinations.forbid.len())
    }).sum();
    let spans: usize = config.sources.iter().map(|policy| policy.owned_pc.as_ref().map_or(0, |owned| owned.spans.len())).sum();
    if ranges > MAX_GUARD_RANGES {
        return Err(CatalogRefusal::GuardBound(GuardBound::Ranges(ranges)));
    }
    if spans > MAX_GUARD_PC_SPANS {
        return Err(CatalogRefusal::GuardBound(GuardBound::PcSpans(spans)));
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
        if attach.cores.single().is_none() {
            return Err(CatalogRefusal::AttachCores(attach.label.clone()));
        }
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
        let guards = [guard_config(&pieces, Role::Retained), guard_config(&pieces, Role::Composed)];
        guards.iter().try_for_each(check_guard)?;
        let mut carriers = Vec::new();
        for piece in &pieces {
            for carrier in &piece.carriers {
                let span = carrier.span.physical().ok_or_else(|| CatalogRefusal::AliasStraddle(piece.kind.clone()))?;
                carriers.push(WriteRange { space: WriteSpace::Physical, span });
            }
        }
        let classes: Vec<WatchRange> = merge(carriers).into_iter()
            .map(|range| WatchRange { start: range.span.start, end: range.span.end, kinds: WatchKinds::WRITES }).collect();
        if classes.is_empty() || classes.len() > WATCH_CLASS_LIMIT {
            return Err(CatalogRefusal::GuardBound(GuardBound::WatchClasses(classes.len())));
        }
        Ok(Self {
            qualification: raw.qualification,
            pending_joins: raw.pending_joins,
            manifest_sha256: raw.manifest_sha256,
            canonical: raw.canonical,
            composed: raw.composed,
            guard: raw.guard,
            joins: raw.joins,
            pieces,
            boundaries,
            guards,
            classes,
        })
    }

    pub fn identities(&self, role: Role) -> &Identities {
        match role {
            Role::Retained => &self.canonical,
            Role::Composed => &self.composed,
        }
    }

    pub fn boundary(&self, label: &Label) -> Option<&Boundary> { self.boundaries.get(label) }

    pub fn boundaries(&self) -> impl Iterator<Item = &Boundary> { self.boundaries.values() }

    pub fn reads(&self) -> impl Iterator<Item = &Read> { self.pieces.iter().flat_map(|piece| &piece.catalog.reads) }

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

    pub fn confinement(&self, role: Role) -> Confinement {
        match self.guards[1].sources.iter().any(|policy| policy.owned_pc.is_some()) {
            true if role == Role::Composed => Confinement::UnionOfEnumeratedAllows,
            _ => Confinement::Stock,
        }
    }

    pub fn unconfined(&self, role: Role) -> Vec<UnconfinedCode> {
        if role == Role::Retained {
            return vec![];
        }
        self.pieces.iter().filter(|piece| !piece.enumerated())
            .flat_map(|piece| piece.catalog.owned_code.iter()
                .map(|span| UnconfinedCode { piece: piece.kind.clone(), start: span.start, end: span.end }))
            .collect()
    }

    /// Write classes over every owned code carrier, in physical addresses.
    pub fn watch_classes(&self) -> Vec<WatchRange> { self.classes.clone() }

    /// Every piece's owned code PC spans in both spellings.
    pub fn owned_code_spellings(&self) -> Vec<(u64, u64)> {
        owned_spans(self.pieces.iter()).into_iter().map(|span| (span.start, span.end)).collect()
    }

    pub fn plan(&self) -> Plan {
        Plan { roles: [Role::Retained, Role::Composed].into_iter().map(|role| self.role_plan(role)).collect() }
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
            confinement: self.confinement(role),
            unconfined: self.unconfined(role),
        }
    }
}

/// The host-derived observation plan for both roles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Plan {
    pub roles: Vec<RolePlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RolePlan {
    pub role: Role,
    pub armed: Vec<ArmedCore>,
    pub stop_labels: Vec<Label>,
    pub watch_classes: Vec<PlanSpan>,
    pub guard: Vec<PolicyPlan>,
    pub confinement: Confinement,
    pub unconfined: Vec<UnconfinedCode>,
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "target", rename_all = "kebab-case")]
pub enum RangePlan {
    Physical { start: u64, end: u64 },
    Tcm { core: usize, bank: Bank, start: u64, end: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnedPcPlan {
    pub spans: Vec<PlanSpan>,
    pub allow: Vec<RangePlan>,
    pub forbid: Vec<RangePlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PolicyPlan {
    pub source: SourcePlan,
    pub allow: Vec<RangePlan>,
    pub forbid: Vec<RangePlan>,
    pub owned_pc: Option<OwnedPcPlan>,
}

impl PolicyPlan {
    fn of(policy: &SourcePolicy) -> Self {
        let ranges = |ranges: &[GuardRange]| ranges.iter().map(|range| {
            let (start, end) = (range.span.start(), range.span.end());
            match range.target {
                GuardTarget::Physical => RangePlan::Physical { start, end },
                GuardTarget::Tcm { core, bank } => RangePlan::Tcm {
                    core: core.get(), bank: match bank { TcmBank::Data => Bank::Data, TcmBank::Instruction => Bank::Instruction }, start, end,
                },
            }
        }).collect::<Vec<_>>();
        Self {
            source: match policy.source {
                WriteSource::Cpu(core) => SourcePlan::Cpu { core: core.get() },
                WriteSource::CoreHost(core) => SourcePlan::CoreHost { core: core.get() },
                WriteSource::PhysicalHost => SourcePlan::PhysicalHost,
                WriteSource::ModelBus => SourcePlan::ModelBus,
            },
            allow: ranges(&policy.destinations.allow),
            forbid: ranges(&policy.destinations.forbid),
            owned_pc: policy.owned_pc.as_ref().map(|owned| OwnedPcPlan {
                spans: owned.spans.iter().map(|span| PlanSpan { start: span.start(), end: span.end() }).collect(),
                allow: ranges(&owned.destinations.allow),
                forbid: ranges(&owned.destinations.forbid),
            }),
        }
    }
}
