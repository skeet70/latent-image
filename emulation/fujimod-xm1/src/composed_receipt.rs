//! Composed-startup receipts and the retained/composed comparison.
use crate::attach_relations::{AttachSources, RelationFailure};
use crate::attach_state::{
    self, AllowanceLabel, AttachDifferences, AttachRefusal, AttachState, AttachVerdict, BoardLeaf, ContextRegister, RoleCapture, Space,
    SpaceSpan, StackCheck, UnusedEntry,
};
use crate::application_observation::{self, ApplicationReceipt, ApplicationRefusal, PairField};
use crate::composed_catalog::{
    BoundaryKind, Compare, Contract, Core, Declaration, Label, PiecePolicy, PieceKind, Role, Schema, Sha256, Target,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// The receipt's filename in its role's directory.
pub const RECEIPT_FILE: &str = "composed-startup.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiptKind {
    ComposedStartupReceipt,
    /// An observation with diagnostic classes; not evidence.
    ComposedStartupDiagnostic,
    /// The mutation census from reset to the attach-state boundary; not evidence.
    ComposedStartupMutationCensus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Word(pub u32);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct HexBytes(pub Vec<u8>);

impl TryFrom<String> for HexBytes {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Err("bytes are not lowercase hexadecimal".into());
        }
        (0..value.len()).step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(|error| error.to_string()))
            .collect::<Result<_, _>>().map(Self)
    }
}

impl From<HexBytes> for String {
    fn from(value: HexBytes) -> Self { value.0.iter().map(|byte| format!("{byte:02x}")).collect() }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub run: Sha256,
    pub update: Sha256,
    pub boot_rom: Sha256,
    pub observer: Sha256,
    pub contract: Sha256,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Observed,
    Refused,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadCapture {
    pub label: Label,
    pub address: u32,
    pub bytes: HexBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Register {
    R0,
    R1,
    R2,
    R3,
    Sp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub label: Label,
    pub core: Core,
    pub total_instructions: u64,
    pub r0: Word,
    pub r1: Word,
    pub r2: Word,
    pub r3: Word,
    pub r4: Word,
    pub r5: Word,
    pub r6: Word,
    pub r7: Word,
    pub r8: Word,
    pub r9: Word,
    pub r10: Word,
    pub r11: Word,
    pub r12: Word,
    pub sp: Word,
    pub lr: Word,
    pub cpsr: Word,
    pub reads: Vec<ReadCapture>,
}

impl Row {
    pub fn register(&self, register: Register) -> Word {
        match register {
            Register::R0 => self.r0,
            Register::R1 => self.r1,
            Register::R2 => self.r2,
            Register::R3 => self.r3,
            Register::Sp => self.sp,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabelCount {
    pub label: Label,
    pub core: Core,
    pub total: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuardState {
    Armed,
    Faulted,
    Ended,
    CleanupFailed,
}

/// Write rows kept verbatim per carrier.
pub const CARRIER_FIRST_ROWS: usize = 16;
/// Distinct writers counted per carrier.
pub const CARRIER_WRITER_LIMIT: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriteKind {
    CpuWrite,
    HostWrite,
    ModelBusWrite,
}

/// One classified-watch row; the watch's other row fields are not read.
#[derive(Clone, Debug, Deserialize)]
pub struct WatchRow {
    pub source: WriteKind,
    pub physical: u64,
    pub address: u64,
    pub size: u64,
    pub core: Option<Core>,
    pub pc: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Writer {
    pub kind: WriteKind,
    pub core: Option<Core>,
    pub pc: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WriterCount {
    pub writer: Writer,
    pub count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CarrierWrite {
    pub writer: Writer,
    pub address: u64,
    pub physical: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

/// Writes overlapping one watched owned-code carrier class after attach. Totals, writer counts and rows are data;
/// `coverage`, `kinds` and `owned_code_writes` are compared.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CarrierWrites {
    pub start: u64,
    pub end: u64,
    pub total: u64,
    pub writers: Vec<WriterCount>,
    pub writers_truncated: bool,
    pub first: Vec<CarrierWrite>,
    /// Physical bytes of the class written, sorted and disjoint.
    pub coverage: Vec<ByteRange>,
    pub kinds: Vec<WriteKind>,
    /// CPU writes whose PC lies in any piece's `owned_code`.
    pub owned_code_writes: u64,
    pub first_owned_code_writes: Vec<CarrierWrite>,
}

/// One summary per class in `classes` order; every row counts in each class it overlaps, and a row overlapping no
/// class is refused. `owned_code` holds owned PC spans in every spelling.
pub fn carrier_writes(classes: &[(u64, u64)], owned_code: &[(u64, u64)], rows: &[WatchRow]) -> Result<Vec<CarrierWrites>, String> {
    let mut writers: Vec<BTreeMap<Writer, u64>> = vec![BTreeMap::new(); classes.len()];
    let mut covered: Vec<Vec<ByteRange>> = vec![Vec::new(); classes.len()];
    let mut summaries: Vec<CarrierWrites> = classes.iter().map(|&(start, end)| CarrierWrites {
        start, end, total: 0, writers: vec![], writers_truncated: false, first: vec![], coverage: vec![], kinds: vec![],
        owned_code_writes: 0, first_owned_code_writes: vec![],
    }).collect();
    for row in rows {
        let row_end = row.physical.saturating_add(row.size.max(1));
        let writer = Writer { kind: row.source, core: row.core, pc: row.pc };
        let write = CarrierWrite { writer, address: row.address, physical: row.physical, size: row.size };
        let owned = row.source == WriteKind::CpuWrite
            && row.pc.is_some_and(|pc| owned_code.iter().any(|&(start, end)| start <= u64::from(pc) && u64::from(pc) < end));
        let mut matched = false;
        for (index, &(start, end)) in classes.iter().enumerate() {
            if row.physical >= end || row_end <= start {
                continue;
            }
            matched = true;
            let summary = &mut summaries[index];
            summary.total += 1;
            if summary.first.len() < CARRIER_FIRST_ROWS {
                summary.first.push(write.clone());
            }
            if !summary.kinds.contains(&row.source) {
                summary.kinds.push(row.source);
            }
            if owned {
                summary.owned_code_writes += 1;
                if summary.first_owned_code_writes.len() < CARRIER_FIRST_ROWS {
                    summary.first_owned_code_writes.push(write.clone());
                }
            }
            covered[index].push(ByteRange { start: row.physical.max(start), end: row_end.min(end) });
            let counts = &mut writers[index];
            if let Some(count) = counts.get_mut(&writer) {
                *count += 1;
            } else if counts.len() < CARRIER_WRITER_LIMIT {
                counts.insert(writer, 1);
            } else {
                summary.writers_truncated = true;
            }
        }
        if !matched {
            return Err(format!("carrier write row at {:#x} overlaps no carrier", row.physical));
        }
    }
    for ((summary, counts), mut ranges) in summaries.iter_mut().zip(writers).zip(covered) {
        summary.writers = counts.into_iter().map(|(writer, count)| WriterCount { writer, count }).collect();
        summary.kinds.sort();
        ranges.sort();
        for range in ranges {
            match summary.coverage.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => summary.coverage.push(range),
            }
        }
    }
    Ok(summaries)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub core: Core,
    /// The writer core's entered instruction count.
    pub instructions: Option<u64>,
}

/// One contract declaration of the role and where the guard admitted it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredUse {
    pub declaration: Declaration,
    pub admitted: Option<Admission>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuardObservation {
    /// The guard attaches before the first instruction; these are the counts at the attach-state boundary.
    pub boundary_instructions: u64,
    /// Watch rows recorded before the boundary.
    pub boundary_row: u64,
    /// Instruction count when the watch and guard ended, after the menu route.
    pub end_instructions: u64,
    pub status: GuardState,
    /// `Debug` form of the guard's first `GuardFault`.
    pub first_fault: Option<String>,
    pub write_checks: u64,
    /// Refused mutations.
    pub mutation_attempts: u64,
    pub mutations_admitted: u64,
    /// The role's declarations as admitted at the boundary.
    pub declared: Vec<DeclaredUse>,
    /// Carrier writes before the boundary; data, except `owned_code_writes`.
    pub carrier_writes_before_boundary: Vec<CarrierWrites>,
    pub carrier_writes: Vec<CarrierWrites>,
    pub watch_violation: Option<String>,
    pub watch_end_error: Option<String>,
}

impl GuardObservation {
    pub fn clean(&self) -> bool {
        self.status == GuardState::Ended && self.first_fault.is_none() && self.watch_violation.is_none()
            && self.watch_end_error.is_none()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MenuObservation {
    /// `(69,52,12)` at the startup deadline.
    pub erased_settings: bool,
    /// `(1,5,2)` after the route from the erased-settings menu.
    pub shooting: bool,
}

/// One piece's partition on one core with the guest writes checked under it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedPolicy {
    pub policy: PiecePolicy,
    pub checks_before_boundary: u64,
    pub checks: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: Schema,
    pub kind: ReceiptKind,
    pub role: Role,
    pub identity: Identity,
    pub policies: Vec<RecordedPolicy>,
    pub outcome: Outcome,
    pub error: Option<String>,
    pub machine_failure: Option<String>,
    pub instructions: u64,
    pub rows: Vec<Row>,
    pub counts: Vec<LabelCount>,
    /// Hits on a core outside the boundary's declared cores; they carry no row and no count.
    pub off_core: Vec<LabelCount>,
    /// Captured at the attach-state boundary.
    pub attach: Option<AttachState>,
    /// Timer and LZ accesses and log writes from reset to the attach-state boundary.
    pub attach_sources: Option<AttachSources>,
    pub guard: Option<GuardObservation>,
    pub menu: MenuObservation,
    /// Schema 5: the application phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<ApplicationReceipt>,
}

impl Receipt {
    fn count(&self, label: &Label) -> u64 {
        self.counts.iter().filter(|count| &count.label == label)
            .map(|count| count.total).sum()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityField {
    Kind,
    /// The receipt's schema is not the contract's.
    Schema,
    Role,
    Contract,
    Run,
    BootRom,
    Observer,
    Update,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Difference {
    Identity { role: Role, field: IdentityField },
    Menu { role: Role },
    Guard { role: Role },
    /// The receipt's declarations differ from the contract's for the role.
    DeclarationList { role: Role },
    /// A declaration the guard had not admitted by the boundary.
    DeclarationUnused { role: Role, index: usize },
    CarrierWrites { start: u64, end: u64, field: CarrierField },
    /// A composed-role CPU write from owned code landed in a carrier.
    OwnedCodeWrite { start: u64, end: u64, first: Option<CarrierWrite> },
    OffCore { label: Label, core: Core, role: Role },
    /// Occurrences beyond the recorded rows leave the boundary's order unobserved.
    Unrecorded { label: Label, role: Role, recorded: u64, total: u64 },
    Sequence { core: Core, index: usize, retained: Option<Label>, owned: Option<Label> },
    Register { label: Label, core: Core, occurrence: usize, register: Register, retained: Word, owned: Word },
    Presence { label: Label, role: Role },
    Read { label: Label, occurrence: usize },
    /// A capture file or its receipt binding is missing, malformed or inconsistent.
    AttachSnapshot { role: Role, reason: String },
    AttachContext { core: Core, register: ContextRegister, retained: Word, owned: Word },
    /// A row-valued register expectation with no earlier row of that label on the core in the role.
    AttachRegisterRow { core: Core, register: ContextRegister, role: Role, label: Label },
    AttachPeer { core: Core, role: Role, pc: Word },
    /// Values are JSON truncated to `attach_state::BOARD_VALUE_LIMIT` bytes.
    AttachBoard { pointer: BoardLeaf, retained: String, owned: String },
    AttachCarrier { piece: PieceKind, target: Target, start: u64, end: u64, role: Role },
    AttachCopy { label: AllowanceLabel, role: Role },
    AttachAllowance { label: AllowanceLabel, role: Role, check: StackCheck },
    /// `runs` holds up to the first 64 maximal differing ranges.
    AttachMemory { space: Space, start: u64, end: u64, differing_bytes: u64, runs: Vec<SpaceSpan> },
    UnusedAllowance { entry: UnusedEntry },
    AttachRelation { role: Role, failure: RelationFailure },
    /// A schema-5 application check refused the role's evidence.
    Application { role: Role, refusal: ApplicationRefusal },
    /// The roles' application evidence differs.
    ApplicationPair { field: PairField },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerdictKind {
    Equal,
    Different,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Verdict {
    pub schema: Schema,
    pub verdict: VerdictKind,
    pub first_difference: Option<Difference>,
    /// From the composed receipt.
    pub policies: Vec<RecordedPolicy>,
    pub declared: RoleDeclarations,
    pub attach: Option<AttachVerdict>,
    /// Empty unless the attach-state comparison ran and refused.
    pub attach_differences: AttachDifferences,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RoleDeclarations {
    pub retained: Vec<DeclaredUse>,
    pub composed: Vec<DeclaredUse>,
}

/// The recorded declarations must be the contract's for the role, each admitted by the boundary.
fn declarations(contract: &Contract, role: Role, guard: &GuardObservation) -> Result<(), Difference> {
    let recorded: Vec<&Declaration> = guard.declared.iter().map(|used| &used.declaration).collect();
    if !recorded.iter().copied().eq(contract.declared(role)) {
        return Err(Difference::DeclarationList { role });
    }
    match guard.declared.iter().position(|used| used.admitted.is_none()) {
        Some(index) => Err(Difference::DeclarationUnused { role, index }),
        None => Ok(()),
    }
}

fn identity(contract: &Contract, digest: &Sha256, receipt: &Receipt, role: Role) -> Result<(), Difference> {
    let field = if receipt.kind != ReceiptKind::ComposedStartupReceipt {
        IdentityField::Kind
    } else if receipt.schema != contract.schema() {
        IdentityField::Schema
    } else if receipt.role != role {
        IdentityField::Role
    } else if &receipt.identity.contract != digest {
        IdentityField::Contract
    } else if receipt.identity.update != contract.identities(role).dat_sha256 {
        IdentityField::Update
    } else {
        return Ok(());
    };
    Err(Difference::Identity { role, field })
}

fn reached(receipt: &Receipt) -> bool {
    receipt.outcome == Outcome::Observed && receipt.menu.erased_settings && receipt.menu.shooting
}

fn compare_rows(contract: &Contract, retained: &Receipt, owned: &Receipt) -> Result<(), Difference> {
    let sequenced = || contract.boundaries().filter(|boundary| boundary.compare == Compare::Sequence && boundary.retained_pc.is_some());
    for boundary in sequenced() {
        for (role, receipt) in [(Role::Retained, retained), (Role::Composed, owned)] {
            let recorded = receipt.rows.iter().filter(|row| row.label == boundary.label).count() as u64;
            let total = receipt.count(&boundary.label);
            if total > recorded {
                return Err(Difference::Unrecorded { label: boundary.label.clone(), role, recorded, total });
            }
        }
    }
    let compared = |row: &&Row| contract.boundary(&row.label)
        .is_some_and(|boundary| boundary.compare == Compare::Sequence && boundary.retained_pc.is_some());
    for core in Core::ALL {
        let on = |receipt: &'_ Receipt| receipt.rows.iter().filter(compared).filter(|row| row.core == core).cloned().collect::<Vec<_>>();
        let (left, right) = (on(retained), on(owned));
        for index in 0..left.len().max(right.len()) {
            let (a, b) = (left.get(index).map(|row| &row.label), right.get(index).map(|row| &row.label));
            if a != b {
                return Err(Difference::Sequence { core, index, retained: a.cloned(), owned: b.cloned() });
            }
        }
        let mut occurrences: BTreeMap<&Label, usize> = BTreeMap::new();
        for (a, b) in left.iter().zip(&right) {
            let occurrence = occurrences.entry(&a.label).or_default();
            let registers: &[Register] = match contract.boundary(&a.label).map(|boundary| boundary.kind) {
                Some(BoundaryKind::Entry) => &[Register::R0, Register::R1, Register::R2, Register::R3],
                Some(BoundaryKind::Return) => &[Register::R0, Register::Sp],
                _ => &[],
            };
            if let Some(&register) = registers.iter().find(|&&register| a.register(register) != b.register(register)) {
                return Err(Difference::Register {
                    label: a.label.clone(), core, occurrence: *occurrence, register,
                    retained: a.register(register), owned: b.register(register),
                });
            }
            *occurrence += 1;
        }
    }
    Ok(())
}

fn compare_presence(contract: &Contract, retained: &Receipt, owned: &Receipt) -> Result<(), Difference> {
    for boundary in contract.boundaries().filter(|boundary| boundary.compare == Compare::Presence && boundary.retained_pc.is_some()) {
        for (role, receipt) in [(Role::Retained, retained), (Role::Composed, owned)] {
            if receipt.count(&boundary.label) == 0 {
                return Err(Difference::Presence { label: boundary.label.clone(), role });
            }
        }
    }
    Ok(())
}

fn compare_reads(contract: &Contract, retained: &Receipt, owned: &Receipt) -> Result<(), Difference> {
    for read in contract.reads().filter(|read| read.differs.is_none()) {
        let captures = |receipt: &'_ Receipt| receipt.rows.iter().flat_map(|row| &row.reads)
            .filter(|capture| capture.label == read.label).map(|capture| capture.bytes.clone()).collect::<Vec<_>>();
        let (a, b) = (captures(retained), captures(owned));
        if let Some(occurrence) = (0..a.len().max(b.len())).find(|&index| a.get(index) != b.get(index)) {
            return Err(Difference::Read { label: read.label.clone(), occurrence });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CarrierField {
    Classes,
    Coverage,
    Kinds,
}

fn compare_carriers(retained: &[CarrierWrites], owned: &[CarrierWrites]) -> Result<(), Difference> {
    if let Some(carrier) = owned.iter().find(|carrier| carrier.owned_code_writes > 0) {
        return Err(Difference::OwnedCodeWrite {
            start: carrier.start, end: carrier.end, first: carrier.first_owned_code_writes.first().cloned(),
        });
    }
    if retained.len() != owned.len() {
        let (start, end) = retained.first().or(owned.first()).map_or((0, 0), |carrier| (carrier.start, carrier.end));
        return Err(Difference::CarrierWrites { start, end, field: CarrierField::Classes });
    }
    for (a, b) in retained.iter().zip(owned) {
        let field = if (a.start, a.end) != (b.start, b.end) {
            CarrierField::Classes
        } else if a.coverage != b.coverage {
            CarrierField::Coverage
        } else if a.kinds != b.kinds {
            CarrierField::Kinds
        } else {
            continue;
        };
        return Err(Difference::CarrierWrites { start: a.start, end: a.end, field });
    }
    Ok(())
}

pub struct CaptureDirectories<'a> {
    pub retained: &'a Path,
    pub owned: &'a Path,
}

struct Refusal {
    first: Difference,
    attach: AttachDifferences,
}

impl From<Difference> for Refusal {
    fn from(first: Difference) -> Self { Self { first, attach: AttachDifferences::default() } }
}

impl From<AttachRefusal> for Refusal {
    fn from(refusal: AttachRefusal) -> Self { Self { first: refusal.first, attach: refusal.differences } }
}

fn differences(contract: &Contract, digest: &Sha256, retained: &Receipt, owned: &Receipt, directories: &CaptureDirectories)
    -> Result<AttachVerdict, Refusal> {
    identity(contract, digest, retained, Role::Retained)?;
    identity(contract, digest, owned, Role::Composed)?;
    for (field, same) in [
        (IdentityField::Run, retained.identity.run == owned.identity.run),
        (IdentityField::BootRom, retained.identity.boot_rom == owned.identity.boot_rom),
        (IdentityField::Observer, retained.identity.observer == owned.identity.observer),
    ] {
        if !same {
            return Err(Difference::Identity { role: Role::Composed, field }.into());
        }
    }
    for (role, receipt) in [(Role::Retained, retained), (Role::Composed, owned)] {
        if !reached(receipt) {
            return Err(Difference::Menu { role }.into());
        }
    }
    let clean = |role, receipt: &Receipt| receipt.guard.clone().filter(GuardObservation::clean).ok_or(Difference::Guard { role });
    let (retained_guard, owned_guard) = (clean(Role::Retained, retained)?, clean(Role::Composed, owned)?);
    declarations(contract, Role::Retained, &retained_guard)?;
    declarations(contract, Role::Composed, &owned_guard)?;
    for (role, receipt) in [(Role::Retained, retained), (Role::Composed, owned)] {
        if let Some(hit) = receipt.off_core.first() {
            return Err(Difference::OffCore { label: hit.label.clone(), core: hit.core, role }.into());
        }
    }
    fn captured(role: Role, receipt: &Receipt) -> Result<&AttachState, Difference> {
        receipt.attach.as_ref().ok_or_else(|| Difference::AttachSnapshot { role, reason: "receipt has no attach capture".into() })
    }
    let (retained_state, owned_state) = (captured(Role::Retained, retained)?, captured(Role::Composed, owned)?);
    let attach = attach_state::compare(
        contract.attach_rules(),
        contract.attach_core(),
        &RoleCapture { role: Role::Retained, state: retained_state, rows: &retained.rows, directory: directories.retained, sources: retained.attach_sources.as_ref() },
        &RoleCapture { role: Role::Composed, state: owned_state, rows: &owned.rows, directory: directories.owned, sources: owned.attach_sources.as_ref() },
    )?;
    if let Some(carrier) = owned_guard.carrier_writes_before_boundary.iter().find(|carrier| carrier.owned_code_writes > 0) {
        return Err(Difference::OwnedCodeWrite {
            start: carrier.start, end: carrier.end, first: carrier.first_owned_code_writes.first().cloned(),
        }.into());
    }
    compare_carriers(&retained_guard.carrier_writes, &owned_guard.carrier_writes)?;
    compare_rows(contract, retained, owned)?;
    compare_presence(contract, retained, owned)?;
    compare_reads(contract, retained, owned)?;
    if let Some(application) = contract.application() {
        let mut sections = [None, None];
        for (slot, (role, receipt, directory)) in sections.iter_mut()
            .zip([(Role::Retained, retained, directories.retained), (Role::Composed, owned, directories.owned)])
        {
            let section = receipt.application.as_ref().ok_or_else(|| Difference::Application {
                role, refusal: ApplicationRefusal::new(application_observation::ApplicationCheck::CaptureBound, "the receipt has no application section"),
            })?;
            application_observation::check_role(application, role, section, directory)
                .map_err(|refusal| Difference::Application { role, refusal })?;
            *slot = Some(section);
        }
        if let [Some(retained), Some(owned)] = sections {
            application_observation::check_pair(retained, owned).map_err(|field| Difference::ApplicationPair { field })?;
        }
    }
    Ok(attach)
}

/// `digest` is the SHA256 of the contract file both receipts must name.
pub fn compare(contract: &Contract, digest: &Sha256, retained: &Receipt, owned: &Receipt, directories: &CaptureDirectories) -> Verdict {
    let result = differences(contract, digest, retained, owned, directories);
    let (attach, first_difference, attach_differences) = match result {
        Ok(attach) => (Some(attach), None, AttachDifferences::default()),
        Err(refusal) => (None, Some(refusal.first), refusal.attach),
    };
    Verdict {
        schema: contract.schema(),
        verdict: if first_difference.is_none() { VerdictKind::Equal } else { VerdictKind::Different },
        first_difference,
        policies: owned.policies.clone(),
        declared: RoleDeclarations {
            retained: retained.guard.as_ref().map_or_else(Vec::new, |guard| guard.declared.clone()),
            composed: owned.guard.as_ref().map_or_else(Vec::new, |guard| guard.declared.clone()),
        },
        attach,
        attach_differences,
    }
}
