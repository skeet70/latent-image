use crate::{modal_permit::{CoreId, Cpsr}, modal_task::{PhysicalExtent, SourceLayout, TaskId, VfpOwner, source_layouts}, provenance::Guest};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DRAM_END: u32 = 0x2000_0000;
const STR_PUBLICATION: u32 = 0xe581_3000;
const SETTINGS_START: u32 = 0x0096_cc64;
const SETTINGS_BYTES: usize = 450;
const NODE_BYTES: u32 = 56;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LifetimeFailure {
    InvalidRequest, InvalidExtent, InvalidSource, InitialResident, InitialReadonly, Memory,
    InvalidCore, MalformedRow, NonCpuWrite, InvalidWrite, OutsideWatch, ImmutableWrite,
    ResourceChanged, IdentityWrite, MissingPermit, PendingStore, WrongPhase, WrongEntry,
    WrongWriter, WrongOrder, WrongCore, WrongBefore, WrongValue, WrongAddress, WrongWidth,
    InvalidContext, ActiveFrames, IncompleteRollback, RetiredFetch,
}
impl std::fmt::Display for LifetimeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for LifetimeFailure {}

type Result<T, E = LifetimeFailure> = std::result::Result<T, E>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifetimeRequest {
    runtime: [u32; 2],
    resident: [u32; 2],
    readonly: Vec<ReadonlyRequest>,
    install: ServiceRequest,
    rollback: ServiceRequest,
    publications: Vec<PublicationRequest>,
    #[serde(default)]
    retired: Vec<[u32; 2]>,
    #[serde(default)]
    settings_writers: Vec<[u32; 2]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadonlyRequest { start: u32, bytes: Vec<u8> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceRequest { entry: SiteRequest, writer: SiteRequest }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteRequest { pc: u32, word: u32 }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicationRequest { address: u32, old: u32, new: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeSite { pub pc: Guest, pub word: Word }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeSiteRole { Entry, PublicationStore, ContextStore(CoreId), CurrentStore(CoreId), VfpOwnerStore(CoreId) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSite { pub instruction: CodeSite, pub role: CodeSiteRole }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Publication { pub address: Guest, pub old: Word, pub new: Word }
struct ReadonlyImage { extent: PhysicalExtent, bytes: Vec<u8> }
#[derive(Clone, Copy)]
struct Service { entry: CodeSite, writer: CodeSite }

pub struct LifetimeConfig {
    runtime: PhysicalExtent,
    resident: PhysicalExtent,
    readonly: Vec<ReadonlyImage>,
    sources: Vec<PhysicalExtent>,
    install: Service,
    rollback: Service,
    publications: Vec<Publication>,
    publication_addresses: Vec<Guest>,
    watched: Vec<PhysicalExtent>,
    /// Stock code no CPU may fetch through any alias from the install entry
    /// through the last reverse publication write.
    retired: Vec<PhysicalExtent>,
    /// Code whose value-changing stores to the live-settings block are admitted and counted.
    settings_writers: Vec<PhysicalExtent>,
    sites: Vec<CodeSite>,
}

fn extent(start: u32, end: u32) -> Result<PhysicalExtent> {
    if start >= end || end > DRAM_END { return Err(LifetimeFailure::InvalidExtent); }
    PhysicalExtent::new(Guest(start), end - start).map_err(|_| LifetimeFailure::InvalidExtent)
}
fn contains(outer: PhysicalExtent, inner: PhysicalExtent) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}
fn word_extent(address: Guest) -> Result<PhysicalExtent> {
    extent(address.0, address.0.checked_add(4).ok_or(LifetimeFailure::InvalidExtent)?)
}
fn fixed_resources() -> [PhysicalExtent; 8] {
    [
        (0x00cf_59e4, 0x00cf_59e8), (0x00cf_e3d4, 0x00cf_e3d8),
        (0x00d0_0268, 0x00d0_026c), (0x00cd_5f0e, 0x00cd_5f10),
        (0x00cd_1ae4, 0x00cd_3f8c), (0x00d0_4080, 0x00d0_452c),
        (0x00d0_1370, 0x00d0_1378), (0x0146_7588, 0x0146_7840),
    ].map(|(start, end)| PhysicalExtent::new(Guest(start), end - start).expect("fixed physical extent"))
}
fn settings_extent() -> PhysicalExtent {
    PhysicalExtent::new(Guest(SETTINGS_START), SETTINGS_BYTES as u32).expect("fixed settings extent")
}

const MAX_RUNTIME_BYTES: usize = 1 << 20;
const MAX_RETIRED_EXTENTS: usize = 64;
const MAX_SETTINGS_WRITERS: usize = 8;

impl LifetimeRequest {
    pub fn into_config(self, immutable_sources: Vec<PhysicalExtent>) -> Result<LifetimeConfig> {
        let runtime = extent(self.runtime[0], self.runtime[1])?;
        let resident = extent(self.resident[0], self.resident[1])?;
        let readonly_bytes = self.readonly.iter().try_fold(0usize,
            |total, image| total.checked_add(image.bytes.len())).ok_or(LifetimeFailure::InvalidRequest)?;
        if resident.bytes() != 1024 || !contains(runtime, resident)
            || !matches!(self.publications.len(), 223 | 224) || immutable_sources.is_empty()
            || runtime.bytes() as usize > MAX_RUNTIME_BYTES || readonly_bytes > MAX_RUNTIME_BYTES
            || self.readonly.len() > 64 {
            return Err(LifetimeFailure::InvalidRequest);
        }
        let site = |request: SiteRequest| -> Result<CodeSite> {
            let result = CodeSite { pc: Guest(request.pc), word: Word(request.word) };
            if request.pc % 4 != 0 || !contains(runtime, word_extent(result.pc)?) {
                return Err(LifetimeFailure::InvalidSource);
            }
            Ok(result)
        };
        let install = Service { entry: site(self.install.entry)?, writer: site(self.install.writer)? };
        let rollback = Service { entry: site(self.rollback.entry)?, writer: site(self.rollback.writer)? };
        if install.writer.word != Word(STR_PUBLICATION) || rollback.writer.word != Word(STR_PUBLICATION) {
            return Err(LifetimeFailure::InvalidSource);
        }
        let mut sites = vec![install.entry, install.writer, rollback.entry, rollback.writer];
        sites.sort_by_key(|site| site.pc);
        if sites.windows(2).any(|pair| pair[0].pc == pair[1].pc) {
            return Err(LifetimeFailure::InvalidSource);
        }
        let mut readonly = self.readonly.into_iter().map(|image| {
            let size = u32::try_from(image.bytes.len()).map_err(|_| LifetimeFailure::InvalidExtent)?;
            let image_extent = extent(image.start, image.start.checked_add(size).ok_or(LifetimeFailure::InvalidExtent)?)?;
            if !contains(runtime, image_extent) || resident.overlaps(image_extent) {
                return Err(LifetimeFailure::InvalidSource);
            }
            Ok(ReadonlyImage { extent: image_extent, bytes: image.bytes })
        }).collect::<Result<Vec<_>>>()?;
        readonly.sort_by_key(|image| image.extent.start());
        if readonly.windows(2).any(|pair| pair[0].extent.overlaps(pair[1].extent)) {
            return Err(LifetimeFailure::InvalidSource);
        }
        if !sites.iter().all(|site| readonly.iter().any(|image| contains(image.extent, word_extent(site.pc).expect("validated site")))
            || immutable_sources.iter().any(|source| contains(*source, word_extent(site.pc).expect("validated site")))) {
            return Err(LifetimeFailure::InvalidSource);
        }
        for image in &readonly {
            for site in &sites {
                if contains(image.extent, word_extent(site.pc)?) {
                    let offset = (site.pc.0 - image.extent.start().0) as usize;
                    if image.bytes[offset..offset + 4] != site.word.0.to_le_bytes() {
                        return Err(LifetimeFailure::InvalidSource);
                    }
                }
            }
        }
        let layouts = source_layouts();
        let publications = self.publications.into_iter().map(|row| {
            let address = Guest(row.address);
            let target = word_extent(address)?;
            if row.address % 4 != 0 || row.old == row.new || target.overlaps(resident)
                || target.overlaps(settings_extent())
                || layouts.iter().any(|layout| [layout.nodes, layout.creations, layout.current, layout.context, layout.vfp_owner].iter().any(|range| range.overlaps(target))) {
                return Err(LifetimeFailure::InvalidRequest);
            }
            Ok(Publication { address, old: Word(row.old), new: Word(row.new) })
        }).collect::<Result<Vec<_>>>()?;
        let mut addresses: Vec<_> = publications.iter().map(|row| row.address).collect();
        addresses.sort();
        if addresses.windows(2).any(|pair| pair[0] == pair[1]) { return Err(LifetimeFailure::InvalidRequest); }
        for spec in CONTEXT_SITES {
            let code_site = spec.site();
            if !immutable_sources.iter().any(|source| contains(*source, word_extent(code_site.pc).expect("fixed code site"))) {
                return Err(LifetimeFailure::InvalidSource);
            }
            sites.push(code_site);
        }
        sites.sort_by_key(|site| site.pc);
        let mut watched = vec![resident, settings_extent()];
        watched.extend(fixed_resources());
        watched.extend(immutable_sources.iter().copied());
        watched.extend(readonly.iter().map(|image| image.extent));
        watched.extend(publications.iter().map(|row| word_extent(row.address).expect("validated publication")));
        for layout in layouts { watched.extend([layout.context, layout.current, layout.nodes, layout.creations, layout.vfp_owner]); }
        watched.sort_by_key(|range| range.start());
        let mut merged: Vec<PhysicalExtent> = Vec::with_capacity(watched.len());
        for range in watched {
            if let Some(previous) = merged.last_mut() {
                if range.start() <= previous.end() {
                    *previous = extent(previous.start().0, previous.end().0.max(range.end().0))?;
                    continue;
                }
            }
            merged.push(range);
        }
        let mut retired = self.retired.into_iter().map(|[start, end]| {
            if start % 4 != 0 || end % 4 != 0 { return Err(LifetimeFailure::InvalidExtent); }
            let candidate = extent(start, end)?;
            if merged.iter().any(|range| range.overlaps(candidate)) || candidate.overlaps(runtime) {
                return Err(LifetimeFailure::InvalidSource);
            }
            Ok(candidate)
        }).collect::<Result<Vec<_>>>()?;
        retired.sort_by_key(|range| range.start());
        if retired.len() > MAX_RETIRED_EXTENTS || retired.windows(2).any(|pair| pair[0].overlaps(pair[1])) {
            return Err(LifetimeFailure::InvalidRequest);
        }
        let mut settings_writers = self.settings_writers.into_iter().map(|[start, end]| {
            if start % 4 != 0 || end % 4 != 0 { return Err(LifetimeFailure::InvalidExtent); }
            extent(start, end)
        }).collect::<Result<Vec<_>>>()?;
        settings_writers.sort_by_key(|range| range.start());
        if settings_writers.len() > MAX_SETTINGS_WRITERS || settings_writers.windows(2).any(|pair| pair[0].overlaps(pair[1])) {
            return Err(LifetimeFailure::InvalidRequest);
        }
        Ok(LifetimeConfig { runtime, resident, readonly, sources: immutable_sources, install, rollback, publications, publication_addresses: addresses, watched: merged, retired, settings_writers, sites })
    }
}

impl LifetimeConfig {
    pub fn runtime(&self) -> PhysicalExtent { self.runtime }
    pub fn resident(&self) -> PhysicalExtent { self.resident }
    pub fn watched_extents(&self) -> &[PhysicalExtent] { &self.watched }
    pub fn source_site(&self, pc: Guest) -> Option<SourceSite> {
        let index = self.sites.binary_search_by_key(&pc, |site| site.pc).ok()?;
        let instruction = self.sites[index];
        let role = if pc == self.install.entry.pc || pc == self.rollback.entry.pc {
            CodeSiteRole::Entry
        } else if let Some(site) = CONTEXT_SITES.iter().find(|site| site.pc == pc.0) {
            match site.kind {
                ContextKind::Current => CodeSiteRole::CurrentStore(CoreId(site.core)),
                ContextKind::VfpOwner => CodeSiteRole::VfpOwnerStore(CoreId(site.core)),
                _ => CodeSiteRole::ContextStore(CoreId(site.core)),
            }
        } else {
            CodeSiteRole::PublicationStore
        };
        Some(SourceSite { instruction, role })
    }
    pub fn code_sites(&self) -> &[CodeSite] { &self.sites }
    pub fn publications(&self) -> &[Publication] { &self.publications }
    pub fn readonly_images(&self) -> impl Iterator<Item = (PhysicalExtent, &[u8])> {
        self.readonly.iter().map(|image| (image.extent, image.bytes.as_slice()))
    }
    pub fn retired_extents(&self) -> &[PhysicalExtent] { &self.retired }
    pub fn settings_writers(&self) -> &[PhysicalExtent] { &self.settings_writers }
}

pub trait LifetimeMemory {
    fn read(&self, physical: Guest, output: &mut [u8]) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifetimePhase { Prepared, Publishing, Installed, RollingBack, RollbackStored, RollbackVerified, Ended }
#[derive(Clone, Copy, Debug, Serialize)]
pub struct LifetimeReceipt {
    pub phase: LifetimePhase,
    pub publication_count: usize,
    pub forward_writes: u64,
    pub reverse_writes: u64,
    pub pending_stores: usize,
    pub install_core: Option<usize>,
    pub rollback_core: Option<usize>,
    pub checked_resource_writes: u64,
    pub checked_identity_writes: u64,
    pub checked_scheduler_writes: u64,
    pub checked_context_writes: u64,
    pub checked_vfp_owner_writes: u64,
    pub audited_writes: u64,
    /// Value-changing live-settings stores from a settings writer.
    pub admitted_settings_writes: u64,
    pub failure: Option<LifetimeFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteSource { Cpu, Host, Model }
#[derive(Clone, Copy)]
enum Before<'a> { Bytes(&'a [u8]), Decoded { bytes: [u8; 8], len: usize } }
impl Before<'_> {
    fn len(self) -> usize { match self { Self::Bytes(bytes) => bytes.len(), Self::Decoded { len, .. } => len } }
    fn byte(self, index: usize) -> Option<u8> {
        match self {
            Self::Bytes(bytes) => bytes.get(index).copied(),
            Self::Decoded { bytes, len } => (index < len).then(|| bytes[index]),
        }
    }
    fn word(self) -> Option<Word> {
        if self.len() != 4 { return None; }
        Some(Word(u32::from_le_bytes([self.byte(0)?, self.byte(1)?, self.byte(2)?, self.byte(3)?])))
    }
}
#[derive(Clone, Copy)]
enum WriteWidth { Byte, Halfword, Word, Doubleword, Unsupported }
impl WriteWidth {
    fn from_bytes(bytes: usize) -> Self {
        match bytes { 1 => Self::Byte, 2 => Self::Halfword, 4 => Self::Word, 8 => Self::Doubleword, _ => Self::Unsupported }
    }
    fn bytes(self) -> Result<usize> {
        match self { Self::Byte => Ok(1), Self::Halfword => Ok(2), Self::Word => Ok(4), Self::Doubleword => Ok(8), Self::Unsupported => Err(LifetimeFailure::InvalidWrite) }
    }
}
#[derive(Clone, Copy)]
struct StoreValue(u64);
#[derive(Clone, Copy)]
pub struct WriteAttempt<'a> {
    source: WriteSource,
    core: CoreId,
    pc: Guest,
    address: Guest,
    physical: Guest,
    width: WriteWidth,
    before: Before<'a>,
    value: StoreValue,
}
impl<'a> WriteAttempt<'a> {
    pub fn cpu(core: CoreId, pc: Guest, address: Guest, physical: Guest, before: &'a [u8], value: u64) -> Self {
        Self { source: WriteSource::Cpu, core, pc, address, physical, width: WriteWidth::from_bytes(before.len()), before: Before::Bytes(before), value: StoreValue(value) }
    }
    pub fn with_source(mut self, source: WriteSource) -> Self { self.source = source; self }
    pub fn from_row(row: &'a Value) -> Result<Self> {
        let row = row.as_object().ok_or(LifetimeFailure::MalformedRow)?;
        let source = match row.get("source").and_then(Value::as_str) {
            Some("cpu-write") => WriteSource::Cpu,
            Some("host-write") => return Err(LifetimeFailure::NonCpuWrite),
            Some("model-bus-write") => return Err(LifetimeFailure::NonCpuWrite),
            _ => return Err(LifetimeFailure::MalformedRow),
        };
        const KEYS: [&str; 12] = ["source", "address", "physical", "size", "value", "before", "byte_order", "core", "pc", "instructions", "tick", "tcm"];
        if row.keys().any(|key| !KEYS.contains(&key.as_str())) || row.contains_key("tcm")
            || row.get("byte_order").and_then(Value::as_str) != Some("little") {
            return Err(LifetimeFailure::MalformedRow);
        }
        let number = |key| row.get(key).and_then(Value::as_u64).ok_or(LifetimeFailure::MalformedRow);
        let address = |key| u32::try_from(number(key)?).map(Guest).map_err(|_| LifetimeFailure::MalformedRow);
        number("instructions")?;
        number("tick")?;
        let core = usize::try_from(number("core")?).map_err(|_| LifetimeFailure::MalformedRow)?;
        let width = usize::try_from(number("size")?).map_err(|_| LifetimeFailure::MalformedRow)?;
        let before = row.get("before").and_then(Value::as_array).ok_or(LifetimeFailure::MalformedRow)?;
        if before.len() > 8 { return Err(LifetimeFailure::InvalidWrite); }
        let mut decoded = [0; 8];
        for (target, byte) in decoded.iter_mut().zip(before) {
            *target = byte.as_u64().and_then(|value| u8::try_from(value).ok()).ok_or(LifetimeFailure::MalformedRow)?;
        }
        Ok(Self { source, core: CoreId(core), pc: address("pc")?, address: address("address")?, physical: address("physical")?, width: WriteWidth::from_bytes(width),
            before: Before::Decoded { bytes: decoded, len: before.len() }, value: StoreValue(number("value")?) })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermitKind { Forward, Reverse, Context, VfpOwner }
#[derive(Clone, Copy)]
struct StorePermit { pc: Guest, address: Guest, physical: Guest, before: Word, value: Word, kind: PermitKind }
#[derive(Clone, Copy)]
enum ContextKind { Increment, Decrement, Current, Clear, Set, Initialize, VfpOwner }
#[derive(Clone, Copy)]
struct ContextSite { pc: u32, core: usize, kind: ContextKind }
impl ContextSite {
    fn site(self) -> CodeSite {
        let word = match self.kind {
            ContextKind::Increment => 0xe582_0000,
            ContextKind::Decrement => 0xe582_3000,
            ContextKind::Current => 0xe582_1000,
            ContextKind::Clear | ContextKind::Set => 0xe586_3000,
            ContextKind::Initialize => 0xe583_2000,
            ContextKind::VfpOwner => 0xe583_5000,
        };
        CodeSite { pc: Guest(self.pc), word: Word(word) }
    }
}
const CONTEXT_SITES: [ContextSite; 16] = [
    ContextSite { pc: 0x40240, core: 0, kind: ContextKind::Increment },
    ContextSite { pc: 0x4031c, core: 0, kind: ContextKind::Decrement },
    ContextSite { pc: 0x436b8, core: 0, kind: ContextKind::Current },
    ContextSite { pc: 0x43690, core: 0, kind: ContextKind::Clear },
    ContextSite { pc: 0x436f0, core: 0, kind: ContextKind::Set },
    ContextSite { pc: 0x43720, core: 0, kind: ContextKind::Clear },
    ContextSite { pc: 0x43848, core: 0, kind: ContextKind::Initialize },
    ContextSite { pc: 0x42440, core: 0, kind: ContextKind::VfpOwner },
    ContextSite { pc: 0xd0240, core: 1, kind: ContextKind::Increment },
    ContextSite { pc: 0xd031c, core: 1, kind: ContextKind::Decrement },
    ContextSite { pc: 0xd36b8, core: 1, kind: ContextKind::Current },
    ContextSite { pc: 0xd3690, core: 1, kind: ContextKind::Clear },
    ContextSite { pc: 0xd36f0, core: 1, kind: ContextKind::Set },
    ContextSite { pc: 0xd3720, core: 1, kind: ContextKind::Clear },
    ContextSite { pc: 0xd3848, core: 1, kind: ContextKind::Initialize },
    ContextSite { pc: 0xd2440, core: 1, kind: ContextKind::VfpOwner },
];

#[derive(Clone, Copy)]
pub struct CurrentState { pub selected: Word, pub context_depth: Word }
/// The trapping core's current task and the owner word it replaces, both validated against task memory.
#[derive(Clone, Copy)]
pub struct VfpOwnerState { pub before: VfpOwner, pub current: TaskId }
#[derive(Clone, Copy)]
enum StoreState { Plain, Current(CurrentState), VfpOwner(VfpOwnerState) }

pub struct LifetimeMonitor {
    config: LifetimeConfig,
    settings: [u8; SETTINGS_BYTES],
    receipt: LifetimeReceipt,
    pending: [Option<StorePermit>; 2],
}
impl LifetimeMonitor {
    pub fn new(config: LifetimeConfig, memory: &impl LifetimeMemory) -> Result<Self> {
        let mut resident = [0; 1024];
        memory.read(config.resident.start(), &mut resident)?;
        if resident.iter().any(|byte| *byte != 0) { return Err(LifetimeFailure::InitialResident); }
        let mut settings = [0; SETTINGS_BYTES];
        memory.read(Guest(SETTINGS_START), &mut settings)?;
        let mut buffer = [0; 256];
        for image in &config.readonly {
            for (index, expected) in image.bytes.chunks(buffer.len()).enumerate() {
                memory.read(Guest(image.extent.start().0 + (index * buffer.len()) as u32), &mut buffer[..expected.len()])?;
                if buffer[..expected.len()] != *expected { return Err(LifetimeFailure::InitialReadonly); }
            }
        }
        for row in &config.publications {
            let mut bytes = [0; 4];
            memory.read(row.address, &mut bytes)?;
            if Word(u32::from_le_bytes(bytes)) != row.old { return Err(LifetimeFailure::WrongBefore); }
        }
        let publication_count = config.publications.len();
        Ok(Self { config, settings, pending: [None; 2], receipt: LifetimeReceipt {
            phase: LifetimePhase::Prepared, publication_count, forward_writes: 0, reverse_writes: 0,
            pending_stores: 0, install_core: None, rollback_core: None, checked_resource_writes: 0,
            checked_identity_writes: 0, checked_scheduler_writes: 0, checked_context_writes: 0, checked_vfp_owner_writes: 0,
            audited_writes: 0, admitted_settings_writes: 0, failure: None,
        } })
    }
    pub fn config(&self) -> &LifetimeConfig { &self.config }
    pub fn receipt(&self) -> LifetimeReceipt {
        LifetimeReceipt { pending_stores: self.pending.iter().filter(|permit| permit.is_some()).count(), ..self.receipt }
    }
    fn healthy(&self) -> Result<()> { self.receipt.failure.map_or(Ok(()), Err) }
    fn record<T>(&mut self, result: Result<T>) -> Result<T> {
        if let Err(failure) = result { self.receipt.failure.get_or_insert(failure); }
        result
    }
    pub fn refuse(&mut self, failure: LifetimeFailure) -> LifetimeFailure {
        *self.receipt.failure.get_or_insert(failure)
    }
    pub fn observe_entry(&mut self, core: CoreId, pc: Guest, word: Word) -> Result<()> {
        self.healthy()?;
        let result = self.entry(core, pc, word);
        self.record(result)
    }
    fn entry(&mut self, core: CoreId, pc: Guest, word: Word) -> Result<()> {
        if core.0 >= 2 { return Err(LifetimeFailure::InvalidCore); }
        if self.pending.iter().any(Option::is_some) { return Err(LifetimeFailure::PendingStore); }
        if pc == self.config.install.entry.pc && word == self.config.install.entry.word {
            if self.receipt.phase != LifetimePhase::Prepared { return Err(LifetimeFailure::WrongPhase); }
            self.receipt.install_core = Some(core.0);
            self.receipt.phase = LifetimePhase::Publishing;
        } else if pc == self.config.rollback.entry.pc && word == self.config.rollback.entry.word {
            if self.receipt.phase != LifetimePhase::Installed { return Err(LifetimeFailure::WrongPhase); }
            self.receipt.rollback_core = Some(core.0);
            self.receipt.phase = LifetimePhase::RollingBack;
        } else { return Err(LifetimeFailure::WrongEntry); }
        Ok(())
    }
    pub fn prepare_store(&mut self, core: CoreId, pc: Guest, word: Word, registers: &[u32; 13], before: Word, cpsr: Cpsr) -> Result<()> {
        self.healthy()?;
        let result = self.prepare(core, pc, word, registers, before, cpsr, StoreState::Plain);
        self.record(result)
    }
    pub fn prepare_current_store(&mut self, core: CoreId, pc: Guest, word: Word, registers: &[u32; 13], before: Word, cpsr: Cpsr, current: CurrentState) -> Result<()> {
        self.healthy()?;
        if !CONTEXT_SITES.iter().any(|spec| spec.pc == pc.0 && matches!(spec.kind, ContextKind::Current)) {
            return Err(self.refuse(LifetimeFailure::WrongWriter));
        }
        let result = self.prepare(core, pc, word, registers, before, cpsr, StoreState::Current(current));
        self.record(result)
    }
    pub fn prepare_vfp_owner_store(&mut self, core: CoreId, pc: Guest, word: Word, registers: &[u32; 13], before: Word, cpsr: Cpsr, state: VfpOwnerState) -> Result<()> {
        self.healthy()?;
        if !CONTEXT_SITES.iter().any(|spec| spec.pc == pc.0 && matches!(spec.kind, ContextKind::VfpOwner)) {
            return Err(self.refuse(LifetimeFailure::WrongWriter));
        }
        let result = self.prepare(core, pc, word, registers, before, cpsr, StoreState::VfpOwner(state));
        self.record(result)
    }
    fn prepare(&mut self, core: CoreId, pc: Guest, word: Word, registers: &[u32; 13], before: Word, cpsr: Cpsr, state: StoreState) -> Result<()> {
        if core.0 >= 2 { return Err(LifetimeFailure::InvalidCore); }
        if self.receipt.phase == LifetimePhase::Ended { return Err(LifetimeFailure::WrongPhase); }
        if self.pending[core.0].is_some() { return Err(LifetimeFailure::PendingStore); }
        let permit = if pc == self.config.install.writer.pc || pc == self.config.rollback.writer.pc {
            let (service, expected_core, index, kind) = match self.receipt.phase {
                LifetimePhase::Publishing => (self.config.install, self.receipt.install_core, self.receipt.forward_writes as usize, PermitKind::Forward),
                LifetimePhase::RollingBack => (self.config.rollback, self.receipt.rollback_core, self.config.publications.len() - 1 - self.receipt.reverse_writes as usize, PermitKind::Reverse),
                _ => return Err(LifetimeFailure::WrongPhase),
            };
            if pc != service.writer.pc || word != service.writer.word { return Err(LifetimeFailure::WrongWriter); }
            if expected_core != Some(core.0) { return Err(LifetimeFailure::WrongCore); }
            let row = self.config.publications.get(index).ok_or(LifetimeFailure::WrongOrder)?;
            let (old, new) = if kind == PermitKind::Forward { (row.old, row.new) } else { (row.new, row.old) };
            if registers[1] != row.address.0 { return Err(LifetimeFailure::WrongOrder); }
            if Word(registers[3]) != new { return Err(LifetimeFailure::WrongValue); }
            if before != old { return Err(LifetimeFailure::WrongBefore); }
            StorePermit { pc, address: row.address, physical: row.address, before: old, value: new, kind }
        } else {
            let spec = CONTEXT_SITES.iter().find(|spec| spec.pc == pc.0).ok_or(LifetimeFailure::WrongWriter)?;
            if spec.core != core.0 { return Err(LifetimeFailure::WrongCore); }
            if spec.site().word != word { return Err(LifetimeFailure::WrongWriter); }
            // Kernel stores run in SVC; the lazy-VFP store runs in the undefined-instruction handler. Both mask IRQ.
            let mode = if matches!(spec.kind, ContextKind::VfpOwner) { 0x9b } else { 0x93 };
            if cpsr.0 & ((1 << 24) | (1 << 9) | (1 << 7) | (1 << 5) | 0x1f) != mode {
                return Err(LifetimeFailure::InvalidContext);
            }
            let layout = source_layouts()[core.0];
            let (address, value, target) = match spec.kind {
                ContextKind::Current => (Guest(registers[2]), Word(registers[1]), layout.current),
                ContextKind::Increment => (Guest(registers[2]), Word(registers[0]), layout.context),
                ContextKind::Decrement => (Guest(registers[2]), Word(registers[3]), layout.context),
                ContextKind::Clear | ContextKind::Set => (Guest(registers[6]), Word(registers[3]), layout.context),
                ContextKind::VfpOwner => (Guest(registers[3]), Word(registers[5]), layout.vfp_owner),
                ContextKind::Initialize => return Err(LifetimeFailure::InvalidContext),
            };
            let canonical = if matches!(spec.kind, ContextKind::VfpOwner) { target.start().0 } else { target.start().0 + 0x4000_0000 };
            if address.0 != canonical { return Err(LifetimeFailure::WrongAddress); }
            let valid_node = |value: Word| value.0 == 0 || (value.0 >= layout.nodes.start().0 + 0x4000_0000
                && value.0 < layout.nodes.end().0 + 0x4000_0000
                && (value.0 - layout.nodes.start().0 - 0x4000_0000) % NODE_BYTES == 0);
            let valid = match (spec.kind, state) {
                (ContextKind::Increment, StoreState::Plain) => before.0.checked_add(1) == Some(value.0),
                (ContextKind::Decrement, StoreState::Plain) => before.0.checked_sub(1) == Some(value.0),
                (ContextKind::Current, StoreState::Current(current)) => current.context_depth == Word(0) && current.selected == value
                    && valid_node(value) && valid_node(before),
                (ContextKind::Clear, StoreState::Plain) => value == Word(0) && before == Word(if spec.pc == 0x43690 || spec.pc == 0xd3690 { 0 } else { 1 }),
                (ContextKind::Set, StoreState::Plain) => value == Word(1) && before == Word(0),
                (ContextKind::VfpOwner, StoreState::VfpOwner(owner)) => before == Word(owner.before.word())
                    && value == Word(u32::from(owner.current.get())),
                _ => false,
            };
            if !valid { return Err(LifetimeFailure::InvalidContext); }
            let kind = if matches!(spec.kind, ContextKind::VfpOwner) { PermitKind::VfpOwner } else { PermitKind::Context };
            StorePermit { pc, address, physical: target.start(), before, value, kind }
        };
        self.pending[core.0] = Some(permit);
        Ok(())
    }
    /// A retired fetch refuses from the install entry until the last reverse
    /// publication write.
    pub fn observe_row(&mut self, row: &Value) -> Result<()> {
        self.healthy()?;
        if row.get("source").and_then(Value::as_str) == Some("cpu-fetch") {
            let retired = row.get("physical").and_then(Value::as_u64).and_then(|physical| u32::try_from(physical).ok())
                .is_some_and(|physical| self.config.retired.iter().any(|extent| extent.start().0 <= physical && physical < extent.end().0));
            let published = matches!(self.receipt.phase, LifetimePhase::Publishing | LifetimePhase::Installed | LifetimePhase::RollingBack);
            if retired && !published { return Ok(()); }
            return Err(self.refuse(if retired { LifetimeFailure::RetiredFetch } else { LifetimeFailure::MalformedRow }));
        }
        let attempt = WriteAttempt::from_row(row);
        match attempt { Ok(attempt) => self.observe_write(attempt), Err(failure) => Err(self.refuse(failure)) }
    }
    pub fn observe_write(&mut self, attempt: WriteAttempt<'_>) -> Result<()> {
        self.healthy()?;
        let result = self.write(attempt);
        self.record(result)
    }
    fn write(&mut self, attempt: WriteAttempt<'_>) -> Result<()> {
        if attempt.source != WriteSource::Cpu { return Err(LifetimeFailure::NonCpuWrite); }
        if self.receipt.phase == LifetimePhase::Ended { return Err(LifetimeFailure::WrongPhase); }
        if attempt.core.0 >= 2 { return Err(LifetimeFailure::InvalidCore); }
        let width = attempt.width.bytes()?;
        let value = attempt.value.0;
        if attempt.before.len() != width
            || (width < 8 && value >> (width * 8) != 0)
            || (0..width).any(|index| attempt.before.byte(index).is_none()) {
            return Err(LifetimeFailure::InvalidWrite);
        }
        let target = extent(attempt.physical.0, attempt.physical.0.checked_add(width as u32).ok_or(LifetimeFailure::InvalidExtent)?)?;
        if attempt.address != attempt.physical && attempt.address.0 != attempt.physical.0 + 0x4000_0000 {
            return Err(LifetimeFailure::WrongAddress);
        }
        if !self.config.watched.iter().any(|range| contains(*range, target)) { return Err(LifetimeFailure::OutsideWatch); }
        if let Some(permit) = self.pending[attempt.core.0] {
            if attempt.pc != permit.pc { return Err(LifetimeFailure::WrongWriter); }
            if attempt.address != permit.address || attempt.physical != permit.physical { return Err(LifetimeFailure::WrongAddress); }
            if width != 4 { return Err(LifetimeFailure::WrongWidth); }
            if attempt.before.word() != Some(permit.before) { return Err(LifetimeFailure::WrongBefore); }
            if value != u64::from(permit.value.0) { return Err(LifetimeFailure::WrongValue); }
            self.pending[attempt.core.0] = None;
            match permit.kind {
                PermitKind::Forward => {
                    self.receipt.forward_writes += 1;
                    if self.receipt.forward_writes as usize == self.config.publications.len() { self.receipt.phase = LifetimePhase::Installed; }
                }
                PermitKind::Reverse => {
                    self.receipt.reverse_writes += 1;
                    if self.receipt.reverse_writes as usize == self.config.publications.len() { self.receipt.phase = LifetimePhase::RollbackStored; }
                }
                PermitKind::Context => self.receipt.checked_context_writes += 1,
                PermitKind::VfpOwner => self.receipt.checked_vfp_owner_writes += 1,
            }
            return Ok(());
        }
        let next_publication = self.config.publication_addresses.partition_point(|address| address.0 + 4 <= target.start().0);
        if self.config.publication_addresses.get(next_publication).is_some_and(|address| address.0 < target.end().0) {
            return Err(LifetimeFailure::MissingPermit);
        }
        for layout in source_layouts() {
            if layout.creations.overlaps(target) || backlink_overlap(layout, target) {
                self.receipt.checked_identity_writes += 1;
                return Err(LifetimeFailure::IdentityWrite);
            }
            if layout.current.overlaps(target) || layout.context.overlaps(target) || layout.vfp_owner.overlaps(target) { return Err(LifetimeFailure::MissingPermit); }
        }
        if self.config.resident.overlaps(target) || settings_extent().overlaps(target) {
            self.receipt.checked_resource_writes += 1;
            let admitted = contains(settings_extent(), target)
                && self.config.settings_writers.iter().any(|writer| writer.start() <= attempt.pc && attempt.pc < writer.end());
            let mut changed = false;
            for index in 0..width {
                let address = attempt.physical.0 + index as u32;
                let new = ((value >> (index * 8)) & 255) as u8;
                let expected = if self.config.resident.start().0 <= address && address < self.config.resident.end().0 { Some(0) }
                    else if SETTINGS_START <= address && address < SETTINGS_START + SETTINGS_BYTES as u32 { Some(self.settings[(address - SETTINGS_START) as usize]) }
                    else { None };
                if let Some(expected) = expected {
                    if attempt.before.byte(index) != Some(expected) { return Err(LifetimeFailure::ResourceChanged); }
                    if new != expected {
                        if !admitted { return Err(LifetimeFailure::ResourceChanged); }
                        changed = true;
                    }
                }
            }
            if changed {
                for index in 0..width {
                    self.settings[(attempt.physical.0 - SETTINGS_START) as usize + index] = ((value >> (index * 8)) & 255) as u8;
                }
                self.receipt.admitted_settings_writes += 1;
            }
        }
        let resources = fixed_resources();
        if resources[..7].iter().any(|range| range.overlaps(target))
            || self.config.sources.iter().any(|range| range.overlaps(target))
            || self.config.readonly.iter().any(|image| image.extent.overlaps(target)) {
            self.receipt.checked_resource_writes += 1;
            return Err(LifetimeFailure::ImmutableWrite);
        }
        if source_layouts().iter().any(|layout| layout.nodes.overlaps(target)) {
            self.receipt.checked_scheduler_writes += 1;
            return Ok(());
        }
        if resources[7].overlaps(target) { self.receipt.audited_writes += 1; return Ok(()); }
        if contains(self.config.resident, target) || contains(settings_extent(), target) { return Ok(()); }
        Err(LifetimeFailure::OutsideWatch)
    }
    pub fn verify_rollback(&mut self, memory: &impl LifetimeMemory, active_frames: usize) -> Result<()> {
        self.healthy()?;
        let result = self.verify(memory, active_frames);
        self.record(result)
    }
    fn verify(&mut self, memory: &impl LifetimeMemory, active_frames: usize) -> Result<()> {
        if self.receipt.phase != LifetimePhase::RollbackStored { return Err(LifetimeFailure::WrongPhase); }
        if self.pending.iter().any(Option::is_some) { return Err(LifetimeFailure::PendingStore); }
        if active_frames != 0 { return Err(LifetimeFailure::ActiveFrames); }
        for row in &self.config.publications {
            let mut bytes = [0; 4];
            memory.read(row.address, &mut bytes)?;
            if Word(u32::from_le_bytes(bytes)) != row.old { return Err(LifetimeFailure::IncompleteRollback); }
        }
        self.receipt.phase = LifetimePhase::RollbackVerified;
        Ok(())
    }
    pub fn end(&mut self) -> Result<()> {
        self.healthy()?;
        let result = if self.receipt.phase != LifetimePhase::RollbackVerified { Err(LifetimeFailure::WrongPhase) }
            else if self.pending.iter().any(Option::is_some) { Err(LifetimeFailure::PendingStore) }
            else { self.receipt.phase = LifetimePhase::Ended; Ok(()) };
        self.record(result)
    }
}
fn backlink_overlap(layout: SourceLayout, target: PhysicalExtent) -> bool {
    if !layout.nodes.overlaps(target) { return false; }
    let first = target.start().0.saturating_sub(layout.nodes.start().0) / NODE_BYTES;
    let last = (target.end().0 - 1).saturating_sub(layout.nodes.start().0) / NODE_BYTES;
    (first..=last.min(119)).any(|index| {
        let start = layout.nodes.start().0 + index * NODE_BYTES + 8;
        target.start().0 < start + 4 && start < target.end().0
    })
}
