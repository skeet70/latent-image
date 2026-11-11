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

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContextCatalogue { Stock, OwnedUnion }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "u16")]
pub struct PropertyDeltaWords(u16);
impl TryFrom<u16> for PropertyDeltaWords {
    type Error = LifetimeFailure;
    fn try_from(value: u16) -> Result<Self> {
        if value == 0 || value > 3767 { return Err(LifetimeFailure::InvalidRequest); }
        Ok(Self(value))
    }
}

/// The wire request requires `context_catalogue`: `stock` or `owned_union`.
/// Property plans supply `property_delta_words`; ordinary plans leave it absent or null.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifetimeRequest {
    runtime: [u32; 2],
    resident: [u32; 2],
    readonly: Vec<ReadonlyRequest>,
    install: ServiceRequest,
    rollback: ServiceRequest,
    publications: Vec<PublicationRequest>,
    context_catalogue: ContextCatalogue,
    property_delta_words: Option<PropertyDeltaWords>,
    #[serde(default)]
    bank_variant: Option<String>,
    #[serde(default)]
    retired: Vec<[u32; 2]>,
    #[serde(default)]
    settings_writers: Vec<[u32; 2]>,
    #[serde(default)]
    settings_reloads: Vec<ReloadRequest>,
    #[serde(default)]
    task33_stack_guard: Option<Task33StackGuardRequest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReloadRequest { call: u32, #[serde(rename = "return")] resume: u32, copy_store: u32, target: [u32; 2] }
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task33StackGuardRequest {
    node: u32, creation: u32, entry: u32, base: u32, size: u32,
    guard_bytes: u32, baseline: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StackGuardScope { DiagnosticOnlyBottomGuardWriteAttempts }

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Task33StackGuardReceipt {
    pub scope: StackGuardScope,
    pub node: u32,
    pub creation: u32,
    pub entry: u32,
    pub base: u32,
    pub size: u32,
    pub guard_bytes: u32,
    #[serde(serialize_with = "serialize_guard_baseline")]
    pub baseline: [u8; 64],
    pub priority: u32,
    pub task_id: u32,
    pub created: u32,
}

struct Task33StackGuard {
    receipt: Task33StackGuardReceipt,
    immutable: [PhysicalExtent; 3],
}
impl Task33StackGuardRequest {
    fn into_guard(self) -> Result<Task33StackGuard> {
        if self.node != 0x4006_060c || self.creation != 0x4005_c3ac
            || self.entry != 0x0110_9130 || self.base != 0x00a4_ce00
            || self.size != 4096 || self.guard_bytes != 64 {
            return Err(LifetimeFailure::InvalidRequest);
        }
        let baseline = self.baseline.try_into().map_err(|_| LifetimeFailure::InvalidRequest)?;
        Ok(Task33StackGuard {
            receipt: Task33StackGuardReceipt {
                scope: StackGuardScope::DiagnosticOnlyBottomGuardWriteAttempts,
                node: self.node, creation: self.creation, entry: self.entry,
                base: self.base, size: self.size, guard_bytes: self.guard_bytes,
                baseline, priority: 8, task_id: 33, created: 1,
            },
            immutable: [
                extent(self.base, self.base + self.guard_bytes)?,
                extent((self.node & !0x4000_0000) + 8, (self.node & !0x4000_0000) + 12)?,
                extent(self.creation & !0x4000_0000, (self.creation & !0x4000_0000) + 40)?,
            ],
        })
    }
}
impl Task33StackGuard {
    fn validate(&self, memory: &impl LifetimeMemory) -> Result<()> {
        let mut backlink = [0; 4];
        memory.read(self.immutable[1].start(), &mut backlink)?;
        if u32::from_le_bytes(backlink) != self.receipt.creation {
            return Err(LifetimeFailure::InvalidContext);
        }
        let mut descriptor = [0; 40];
        memory.read(self.immutable[2].start(), &mut descriptor)?;
        for (offset, expected) in [
            (8, self.receipt.entry), (12, self.receipt.priority),
            (16, self.receipt.size), (20, self.receipt.base),
            (32, self.receipt.task_id), (36, self.receipt.created),
        ] {
            if descriptor[offset..offset + 4] != expected.to_le_bytes() {
                return Err(LifetimeFailure::InvalidContext);
            }
        }
        let mut baseline = [0; 64];
        memory.read(self.immutable[0].start(), &mut baseline)?;
        if baseline != self.receipt.baseline { return Err(LifetimeFailure::WrongBefore); }
        Ok(())
    }
}

fn serialize_guard_baseline<S: serde::Serializer>(bytes: &[u8; 64], serializer: S) -> std::result::Result<S::Ok, S::Error> {
    bytes.as_slice().serialize(serializer)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeSite { pub pc: Guest, pub word: Word }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeSiteRole { Entry, PublicationStore, ContextStore(CoreId), CurrentStore(CoreId), SelectedRelativeCurrentStore(CoreId), VfpOwnerStore(CoreId) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSite { pub instruction: CodeSite, pub role: CodeSiteRole }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Publication { pub address: Guest, pub old: Word, pub new: Word }
struct ReadonlyImage { extent: PhysicalExtent, bytes: Vec<u8> }
#[derive(Clone, Copy)]
struct Service { entry: CodeSite, writer: CodeSite }
/// A settings-image reload: the call instruction, the instruction the call returns
/// to, the copy routine's store instruction, and the live-settings bytes it may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsReload { pub call: Guest, pub resume: Guest, pub copy_store: Guest, pub target: PhysicalExtent }
/// The admitted writer of a value-changing live-settings store.
#[derive(Clone, Copy)]
enum SettingsWriter { Setter, Reload }

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
    /// Reloads whose sequential halfword copy stores are admitted between a core's call and return fetches.
    settings_reloads: Vec<SettingsReload>,
    sites: Vec<CodeSite>,
    task33_stack_guard: Option<Task33StackGuard>,
    context_catalogue: ContextCatalogue,
    property_delta_words: Option<PropertyDeltaWords>,
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
const MAX_SETTINGS_RELOADS: usize = 4;

/// The bank variants the guard binds (RamPushLayout `BankVariant`).
#[derive(Clone, Copy)]
enum GuardedVariant { Stock, Modal75, Modal75MenuEngine, Modal75MenuEngineRender }
/// The groups a variant adds to the stock publication vector.
#[derive(Clone, Copy)]
enum PublicationGroup { Modal75, MenuEngine, Render }
const STOCK_PUBLICATIONS: usize = 223;
impl PublicationGroup {
    fn publications(self) -> usize {
        match self { PublicationGroup::Modal75 | PublicationGroup::MenuEngine => 1, PublicationGroup::Render => 3 }
    }
}
impl GuardedVariant {
    const ALL: [GuardedVariant; 4] = [GuardedVariant::Stock, GuardedVariant::Modal75, GuardedVariant::Modal75MenuEngine, GuardedVariant::Modal75MenuEngineRender];
    fn groups(self) -> &'static [PublicationGroup] {
        match self {
            GuardedVariant::Stock => &[],
            GuardedVariant::Modal75 => &[PublicationGroup::Modal75],
            GuardedVariant::Modal75MenuEngine => &[PublicationGroup::Modal75, PublicationGroup::MenuEngine],
            GuardedVariant::Modal75MenuEngineRender => &[PublicationGroup::Modal75, PublicationGroup::MenuEngine, PublicationGroup::Render],
        }
    }
    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "standard" => Some(GuardedVariant::Stock),
            "modal75" => Some(GuardedVariant::Modal75),
            "modal75,menu-engine" => Some(GuardedVariant::Modal75MenuEngine),
            "modal75,menu-engine,render" => Some(GuardedVariant::Modal75MenuEngineRender),
            _ => None,
        }
    }
    fn publications(self) -> usize {
        STOCK_PUBLICATIONS + self.groups().iter().map(|group| group.publications()).sum::<usize>()
    }
}

impl LifetimeRequest {
    pub fn into_config(self, immutable_sources: Vec<PhysicalExtent>) -> Result<LifetimeConfig> {
        let runtime = extent(self.runtime[0], self.runtime[1])?;
        let resident = extent(self.resident[0], self.resident[1])?;
        let readonly_bytes = self.readonly.iter().try_fold(0usize,
            |total, image| total.checked_add(image.bytes.len())).ok_or(LifetimeFailure::InvalidRequest)?;
        let variant = match self.bank_variant.as_deref() {
            Some(tag) => GuardedVariant::from_tag(tag).ok_or(LifetimeFailure::InvalidRequest)?,
            None => GuardedVariant::ALL.iter().copied().find(|variant| variant.publications() == self.publications.len())
                .ok_or(LifetimeFailure::InvalidRequest)?,
        };
        let expected_publications = if let Some(delta) = self.property_delta_words {
            let carrier = 0x0062_c120..0x0062_fbfc;
            if !matches!(variant, GuardedVariant::Modal75MenuEngineRender) || runtime.bytes() != 0x3dc80
                || self.publications.iter().filter(|row| carrier.contains(&row.address)).count() != usize::from(delta.0)
                || self.publications.iter().any(|row| (0x0061_eac4..0x0061_eae8).contains(&row.address)
                    || (0x0062_5064..0x0062_5088).contains(&row.address)) {
                return Err(LifetimeFailure::InvalidRequest);
            }
            210 + usize::from(delta.0)
        } else {
            variant.publications()
        };
        if resident.bytes() != 1024 || !contains(runtime, resident)
            || expected_publications != self.publications.len() || immutable_sources.is_empty()
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
        let task33_stack_guard = self.task33_stack_guard.map(Task33StackGuardRequest::into_guard).transpose()?;
        if let Some(guard) = &task33_stack_guard {
            let bottom = guard.immutable[0];
            let publication_overlap = self.publications.iter().try_fold(false, |hit, row| {
                Ok::<bool, LifetimeFailure>(hit || word_extent(Guest(row.address))?.overlaps(bottom))
            })?;
            if bottom.overlaps(runtime) || settings_extent().overlaps(bottom)
                || immutable_sources.iter().chain(fixed_resources().iter()).any(|range| range.overlaps(bottom))
                || publication_overlap {
                return Err(LifetimeFailure::InvalidSource);
            }
        }
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
                || layouts.iter().any(|layout| [layout.nodes, layout.creations, layout.current, layout.selected, layout.context, layout.vfp_owner].iter().any(|range| range.overlaps(target))) {
                return Err(LifetimeFailure::InvalidRequest);
            }
            Ok(Publication { address, old: Word(row.old), new: Word(row.new) })
        }).collect::<Result<Vec<_>>>()?;
        let mut addresses: Vec<_> = publications.iter().map(|row| row.address).collect();
        addresses.sort();
        if addresses.windows(2).any(|pair| pair[0] == pair[1]) { return Err(LifetimeFailure::InvalidRequest); }
        for spec in self.context_catalogue.sites() {
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
        for layout in layouts { watched.extend([layout.context, layout.current, layout.selected, layout.nodes, layout.creations, layout.vfp_owner]); }
        if let Some(guard) = &task33_stack_guard { watched.extend(guard.immutable); }
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
        if self.settings_reloads.len() > MAX_SETTINGS_RELOADS { return Err(LifetimeFailure::InvalidRequest); }
        let settings_reloads = self.settings_reloads.into_iter().map(|request| {
            if [request.call, request.resume, request.copy_store].iter().any(|pc| pc % 4 != 0) || request.call == request.resume {
                return Err(LifetimeFailure::InvalidExtent);
            }
            let target = extent(request.target[0], request.target[1])?;
            if !contains(settings_extent(), target) { return Err(LifetimeFailure::InvalidRequest); }
            for site in [request.call, request.resume] {
                let word = word_extent(Guest(site))?;
                if merged.iter().chain(&retired).any(|range| range.overlaps(word)) || word.overlaps(runtime) {
                    return Err(LifetimeFailure::InvalidSource);
                }
            }
            let copy_store = Guest(request.copy_store);
            word_extent(copy_store)?;
            if settings_writers.iter().any(|writer| writer.start() <= copy_store && copy_store < writer.end()) {
                return Err(LifetimeFailure::InvalidSource);
            }
            Ok(SettingsReload { call: Guest(request.call), resume: Guest(request.resume), copy_store, target })
        }).collect::<Result<Vec<_>>>()?;
        let mut reload_sites: Vec<_> = settings_reloads.iter().flat_map(|reload| [reload.call, reload.resume]).collect();
        reload_sites.sort();
        if reload_sites.windows(2).any(|pair| pair[0] == pair[1]) { return Err(LifetimeFailure::InvalidRequest); }
        Ok(LifetimeConfig { runtime, resident, readonly, sources: immutable_sources, install, rollback, publications, publication_addresses: addresses, watched: merged, retired, settings_writers, settings_reloads, sites, task33_stack_guard, context_catalogue: self.context_catalogue, property_delta_words: self.property_delta_words })
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
        } else if let Some(site) = self.context_catalogue.sites().iter().find(|site| site.pc == pc.0) {
            match site.kind {
                ContextKind::Current => CodeSiteRole::CurrentStore(CoreId(site.core)),
                ContextKind::CurrentSelectedRelative => CodeSiteRole::SelectedRelativeCurrentStore(CoreId(site.core)),
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
    pub fn settings_reloads(&self) -> &[SettingsReload] { &self.settings_reloads }
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
    /// Value-changing live-settings stores from a settings reload's copy store.
    pub admitted_reload_writes: u64,
    pub failure: Option<LifetimeFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task33_stack_guard: Option<Task33StackGuardReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_delta_words: Option<PropertyDeltaWords>,
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
#[derive(Clone, Copy)]
pub(crate) enum RowProjection<'a> { Canonical, AfAttribution(&'a [fujimod_emulator::board::factory::MemoryMap]) }
impl RowProjection<'_> {
    pub(crate) fn retains(self, key: &str) -> bool {
        match self {
            Self::Canonical => true,
            Self::AfAttribution(_) => !matches!(key, "latest_fetch" | "core_instructions"),
        }
    }
    pub(crate) fn validates(self, row: &Value) -> bool {
        match self {
            Self::Canonical => true,
            Self::AfAttribution(maps) => crate::af_ownership_receipt::validates_projection_metadata(row,maps),
        }
    }
}
impl<'a> WriteAttempt<'a> {
    pub fn cpu(core: CoreId, pc: Guest, address: Guest, physical: Guest, before: &'a [u8], value: u64) -> Self {
        Self { source: WriteSource::Cpu, core, pc, address, physical, width: WriteWidth::from_bytes(before.len()), before: Before::Bytes(before), value: StoreValue(value) }
    }
    pub fn with_source(mut self, source: WriteSource) -> Self { self.source = source; self }
    pub fn from_row(row: &'a Value) -> Result<Self> {
        Self::from_projected_row(row, RowProjection::Canonical)
    }
    fn from_projected_row(row: &'a Value, projection: RowProjection<'_>) -> Result<Self> {
        let row = row.as_object().ok_or(LifetimeFailure::MalformedRow)?;
        let source = match row.get("source").and_then(Value::as_str) {
            Some("cpu-write") => WriteSource::Cpu,
            Some("host-write") => return Err(LifetimeFailure::NonCpuWrite),
            Some("model-bus-write") => return Err(LifetimeFailure::NonCpuWrite),
            _ => return Err(LifetimeFailure::MalformedRow),
        };
        const KEYS: [&str; 12] = ["source", "address", "physical", "size", "value", "before", "byte_order", "core", "pc", "instructions", "tick", "tcm"];
        if row.keys().filter(|key| projection.retains(key)).any(|key| !KEYS.contains(&key.as_str())) || row.contains_key("tcm")
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
enum ContextKind { Increment, Decrement, Current, CurrentSelectedRelative, NoSaveClear, Clear, Set, Initialize, VfpOwner }
#[derive(Clone, Copy)]
struct ContextSite { pc: u32, core: usize, kind: ContextKind }
impl ContextSite {
    fn site(self) -> CodeSite {
        let word = match self.kind {
            ContextKind::Increment => 0xe582_0000,
            ContextKind::Decrement => 0xe582_3000,
            ContextKind::Current => 0xe582_1000,
            ContextKind::CurrentSelectedRelative => 0xe500_1008,
            ContextKind::NoSaveClear | ContextKind::Clear | ContextKind::Set => 0xe586_3000,
            ContextKind::Initialize => 0xe583_2000,
            ContextKind::VfpOwner => 0xe583_5000,
        };
        CodeSite { pc: Guest(self.pc), word: Word(word) }
    }
}
const STOCK_CONTEXT_SITES: [ContextSite; 16] = [
    ContextSite { pc: 0x40240, core: 0, kind: ContextKind::Increment },
    ContextSite { pc: 0x4031c, core: 0, kind: ContextKind::Decrement },
    ContextSite { pc: 0x436b8, core: 0, kind: ContextKind::Current },
    ContextSite { pc: 0x43690, core: 0, kind: ContextKind::NoSaveClear },
    ContextSite { pc: 0x436f0, core: 0, kind: ContextKind::Set },
    ContextSite { pc: 0x43720, core: 0, kind: ContextKind::Clear },
    ContextSite { pc: 0x43848, core: 0, kind: ContextKind::Initialize },
    ContextSite { pc: 0x42440, core: 0, kind: ContextKind::VfpOwner },
    ContextSite { pc: 0xd0240, core: 1, kind: ContextKind::Increment },
    ContextSite { pc: 0xd031c, core: 1, kind: ContextKind::Decrement },
    ContextSite { pc: 0xd36b8, core: 1, kind: ContextKind::Current },
    ContextSite { pc: 0xd3690, core: 1, kind: ContextKind::NoSaveClear },
    ContextSite { pc: 0xd36f0, core: 1, kind: ContextKind::Set },
    ContextSite { pc: 0xd3720, core: 1, kind: ContextKind::Clear },
    ContextSite { pc: 0xd3848, core: 1, kind: ContextKind::Initialize },
    ContextSite { pc: 0xd2440, core: 1, kind: ContextKind::VfpOwner },
];
const OWNED_UNION_CONTEXT_SITES: [ContextSite; 16] = [
    ContextSite { pc: 0x40240, core: 0, kind: ContextKind::Increment },
    ContextSite { pc: 0x4031c, core: 0, kind: ContextKind::Decrement },
    ContextSite { pc: 0x43d48, core: 0, kind: ContextKind::CurrentSelectedRelative },
    ContextSite { pc: 0x43d2c, core: 0, kind: ContextKind::NoSaveClear },
    ContextSite { pc: 0x43d84, core: 0, kind: ContextKind::Set },
    ContextSite { pc: 0x43db8, core: 0, kind: ContextKind::Clear },
    ContextSite { pc: 0x43848, core: 0, kind: ContextKind::Initialize },
    ContextSite { pc: 0x42440, core: 0, kind: ContextKind::VfpOwner },
    ContextSite { pc: 0xd0240, core: 1, kind: ContextKind::Increment },
    ContextSite { pc: 0xd031c, core: 1, kind: ContextKind::Decrement },
    ContextSite { pc: 0xd3d04, core: 1, kind: ContextKind::CurrentSelectedRelative },
    ContextSite { pc: 0xd3ce8, core: 1, kind: ContextKind::NoSaveClear },
    ContextSite { pc: 0xd3d40, core: 1, kind: ContextKind::Set },
    ContextSite { pc: 0xd3d74, core: 1, kind: ContextKind::Clear },
    ContextSite { pc: 0xd3848, core: 1, kind: ContextKind::Initialize },
    ContextSite { pc: 0xd2440, core: 1, kind: ContextKind::VfpOwner },
];
impl ContextCatalogue {
    fn sites(self) -> &'static [ContextSite; 16] {
        match self { Self::Stock => &STOCK_CONTEXT_SITES, Self::OwnedUnion => &OWNED_UNION_CONTEXT_SITES }
    }
    fn selected_store(self, core: CoreId) -> Guest {
        match self {
            Self::Stock => [Guest(0x43d08), Guest(0xd3cc4)][core.0],
            Self::OwnedUnion => [Guest(0x43d0c), Guest(0xd3cc8)][core.0],
        }
    }
}

#[derive(Clone, Copy)]
pub struct CurrentState { pub selected: Word, pub context_depth: Word }
/// The trapping core's current task and the owner word it replaces, both validated against task memory.
#[derive(Clone, Copy)]
pub struct VfpOwnerState { pub before: VfpOwner, pub current: TaskId }
#[derive(Clone, Copy)]
enum StoreState { Plain, Current(CurrentState), VfpOwner(VfpOwnerState) }

/// Each replacement has strictly higher priority; a kernel has 16 ready-queue levels, plus null.
const SUPERSEDED_LIMIT: usize = 17;
/// Values a kernel's selected cell held before the other core's make-ready replaced them, since that
/// kernel's core last wrote the cell or stored its current task.
#[derive(Clone, Copy)]
struct Superseded { values: [u32; SUPERSEDED_LIMIT], len: usize }
impl Superseded {
    const EMPTY: Self = Self { values: [0; SUPERSEDED_LIMIT], len: 0 };
    fn push(&mut self, value: Word) -> Result<()> {
        let slot = self.values.get_mut(self.len).ok_or(LifetimeFailure::InvalidContext)?;
        *slot = value.0;
        self.len += 1;
        Ok(())
    }
    fn contains(&self, value: Word) -> bool { self.values[..self.len].contains(&value.0) }
    fn clear(&mut self) { self.len = 0; }
}

pub struct LifetimeMonitor {
    config: LifetimeConfig,
    settings: [u8; SETTINGS_BYTES],
    receipt: LifetimeReceipt,
    pending: [Option<StorePermit>; 2],
    /// Each core's reload whose call fetch was observed after that core's last return fetch.
    reloads: [Option<OpenReload>; 2],
    superseded: [Superseded; 2],
}
/// An open reload bracket and the address its next copy store must write.
#[derive(Clone, Copy)]
struct OpenReload { reload: SettingsReload, cursor: Guest }
impl LifetimeMonitor {
    pub fn new(config: LifetimeConfig, memory: &impl LifetimeMemory) -> Result<Self> {
        if let Some(guard) = &config.task33_stack_guard { guard.validate(memory)?; }
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
        let task33_stack_guard = config.task33_stack_guard.as_ref().map(|guard| guard.receipt);
        let property_delta_words = config.property_delta_words;
        Ok(Self { config, settings, pending: [None; 2], reloads: [None; 2], superseded: [Superseded::EMPTY; 2], receipt: LifetimeReceipt {
            phase: LifetimePhase::Prepared, publication_count, forward_writes: 0, reverse_writes: 0,
            pending_stores: 0, install_core: None, rollback_core: None, checked_resource_writes: 0,
            checked_identity_writes: 0, checked_scheduler_writes: 0, checked_context_writes: 0, checked_vfp_owner_writes: 0,
            audited_writes: 0, admitted_settings_writes: 0, admitted_reload_writes: 0, failure: None, task33_stack_guard,
            property_delta_words,
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
        if !self.config.context_catalogue.sites().iter().any(|spec| spec.pc == pc.0 && matches!(spec.kind, ContextKind::Current | ContextKind::CurrentSelectedRelative)) {
            return Err(self.refuse(LifetimeFailure::WrongWriter));
        }
        let result = self.prepare(core, pc, word, registers, before, cpsr, StoreState::Current(current));
        self.record(result)
    }
    pub fn prepare_vfp_owner_store(&mut self, core: CoreId, pc: Guest, word: Word, registers: &[u32; 13], before: Word, cpsr: Cpsr, state: VfpOwnerState) -> Result<()> {
        self.healthy()?;
        if !self.config.context_catalogue.sites().iter().any(|spec| spec.pc == pc.0 && matches!(spec.kind, ContextKind::VfpOwner)) {
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
            let spec = self.config.context_catalogue.sites().iter().find(|spec| spec.pc == pc.0).ok_or(LifetimeFailure::WrongWriter)?;
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
                ContextKind::CurrentSelectedRelative => (Guest(registers[0].checked_sub(8).ok_or(LifetimeFailure::WrongAddress)?), Word(registers[1]), layout.current),
                ContextKind::Increment => (Guest(registers[2]), Word(registers[0]), layout.context),
                ContextKind::Decrement => (Guest(registers[2]), Word(registers[3]), layout.context),
                ContextKind::NoSaveClear | ContextKind::Clear | ContextKind::Set => (Guest(registers[6]), Word(registers[3]), layout.context),
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
                (ContextKind::Current | ContextKind::CurrentSelectedRelative, StoreState::Current(current)) => current.context_depth == Word(0)
                    && (current.selected == value || self.superseded[spec.core].contains(value))
                    && valid_node(value) && valid_node(before),
                (ContextKind::NoSaveClear, StoreState::Plain) => value == Word(0) && before == Word(0),
                (ContextKind::Clear, StoreState::Plain) => value == Word(0) && before == Word(1),
                (ContextKind::Set, StoreState::Plain) => value == Word(1) && before == Word(0),
                (ContextKind::VfpOwner, StoreState::VfpOwner(owner)) => before == Word(owner.before.word())
                    && value == Word(u32::from(owner.current.get())),
                _ => false,
            };
            if !valid { return Err(LifetimeFailure::InvalidContext); }
            if matches!(spec.kind, ContextKind::Current | ContextKind::CurrentSelectedRelative) { self.superseded[spec.core].clear(); }
            let kind = if matches!(spec.kind, ContextKind::VfpOwner) { PermitKind::VfpOwner } else { PermitKind::Context };
            StorePermit { pc, address, physical: target.start(), before, value, kind }
        };
        self.pending[core.0] = Some(permit);
        Ok(())
    }
    /// A retired fetch refuses from the install entry until the last reverse
    /// publication write. A reload call fetch opens its core's reload bracket and a
    /// reload return fetch closes it, in any phase.
    pub fn observe_row(&mut self, row: &Value) -> Result<()> {
        self.observe_projected_row(row, RowProjection::Canonical)
    }
    pub fn observe_attributed_row(&mut self, row: &Value, maps: &[fujimod_emulator::board::factory::MemoryMap]) -> Result<()> {
        self.observe_projected_row(row, RowProjection::AfAttribution(maps))
    }
    fn observe_projected_row(&mut self, row: &Value, projection: RowProjection<'_>) -> Result<()> {
        if matches!(row.get("source").and_then(Value::as_str), Some("host-write" | "model-bus-write")) {
            let target = row.get("physical").and_then(Value::as_u64)
                .and_then(|physical| u32::try_from(physical).ok())
                .zip(row.get("size").and_then(Value::as_u64).and_then(|size| u32::try_from(size).ok()))
                .and_then(|(start, size)| start.checked_add(size).and_then(|end| extent(start, end).ok()));
            if target.is_some_and(|target| self.config.task33_stack_guard.as_ref()
                .is_some_and(|guard| guard.immutable.iter().any(|range| range.overlaps(target)))) {
                return Err(self.refuse(LifetimeFailure::ImmutableWrite));
            }
        }
        self.healthy()?;
        if !projection.validates(row) {
            return Err(self.refuse(LifetimeFailure::MalformedRow));
        }
        if row.get("source").and_then(Value::as_str) == Some("cpu-fetch") {
            let physical = row.get("physical").and_then(Value::as_u64).and_then(|physical| u32::try_from(physical).ok());
            if let Some(site) = physical.map(Guest) {
                let call = self.config.settings_reloads.iter().find(|reload| reload.call == site).copied();
                if call.is_some() || self.config.settings_reloads.iter().any(|reload| reload.resume == site) {
                    let core = row.get("core").and_then(Value::as_u64).ok_or(LifetimeFailure::MalformedRow)
                        .and_then(|core| usize::try_from(core).ok().filter(|core| *core < 2).ok_or(LifetimeFailure::InvalidCore))
                        .and_then(|core| if row.get("size").and_then(Value::as_u64) == Some(4) { Ok(core) } else { Err(LifetimeFailure::MalformedRow) });
                    return match core {
                        Ok(core) => { self.reloads[core] = call.map(|reload| OpenReload { reload, cursor: reload.target.start() }); Ok(()) }
                        Err(failure) => Err(self.refuse(failure)),
                    };
                }
            }
            let retired = physical.is_some_and(|physical| self.config.retired.iter().any(|extent| extent.start().0 <= physical && physical < extent.end().0));
            let published = matches!(self.receipt.phase, LifetimePhase::Publishing | LifetimePhase::Installed | LifetimePhase::RollingBack);
            if retired && !published { return Ok(()); }
            return Err(self.refuse(if retired { LifetimeFailure::RetiredFetch } else { LifetimeFailure::MalformedRow }));
        }
        let attempt = WriteAttempt::from_projected_row(row, projection);
        match attempt { Ok(attempt) => self.observe_write(attempt), Err(failure) => Err(self.refuse(failure)) }
    }
    pub fn observe_write(&mut self, attempt: WriteAttempt<'_>) -> Result<()> {
        self.healthy()?;
        let result = self.write(attempt);
        self.record(result)
    }
    fn write(&mut self, attempt: WriteAttempt<'_>) -> Result<()> {
        if let Some(guard) = &self.config.task33_stack_guard {
            let width = attempt.width.bytes()?;
            let target = extent(attempt.physical.0, attempt.physical.0.checked_add(width as u32).ok_or(LifetimeFailure::InvalidExtent)?)?;
            if guard.immutable.iter().any(|range| range.overlaps(target)) {
                return Err(LifetimeFailure::ImmutableWrite);
            }
        }
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
        if let Some(kernel) = source_layouts().iter().position(|layout| layout.selected.overlaps(target)) {
            if target != source_layouts()[kernel].selected { return Err(LifetimeFailure::WrongWidth); }
            let before = attempt.before.word().ok_or(LifetimeFailure::InvalidWrite)?;
            self.receipt.checked_scheduler_writes += 1;
            if attempt.core.0 == kernel { self.superseded[kernel].clear(); }
            else if attempt.pc == self.config.context_catalogue.selected_store(attempt.core) { self.superseded[kernel].push(before)?; }
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
            let reload_store = match self.reloads[attempt.core.0] {
                Some(open) if open.reload.copy_store == attempt.pc && open.reload.target.overlaps(target) => {
                    let sequential = width == 2 && attempt.physical == open.cursor && contains(open.reload.target, target);
                    self.reloads[attempt.core.0] = sequential.then(|| OpenReload { cursor: Guest(open.cursor.0 + 2), ..open });
                    sequential
                }
                _ => false,
            };
            let writer = if !contains(settings_extent(), target) { None }
                else if self.config.settings_writers.iter().any(|writer| writer.start() <= attempt.pc && attempt.pc < writer.end()) { Some(SettingsWriter::Setter) }
                else if reload_store { Some(SettingsWriter::Reload) }
                else { None };
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
                        if writer.is_none() { return Err(LifetimeFailure::ResourceChanged); }
                        changed = true;
                    }
                }
            }
            if let (true, Some(writer)) = (changed, writer) {
                for index in 0..width {
                    self.settings[(attempt.physical.0 - SETTINGS_START) as usize + index] = ((value >> (index * 8)) & 255) as u8;
                }
                match writer {
                    SettingsWriter::Setter => self.receipt.admitted_settings_writes += 1,
                    SettingsWriter::Reload => self.receipt.admitted_reload_writes += 1,
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_publication_extent_error_is_not_a_panic() {
        let mut publications = (0..228).map(|index| PublicationRequest {
            address: 0x00c4_2000 + index * 4, old: 0, new: 0,
        }).collect::<Vec<_>>();
        publications[0].address = 0x2000_0000;
        let request = LifetimeRequest {
            runtime: [0x00c4_0000, 0x00c5_0000],
            resident: [0x00c4_0000, 0x00c4_0400],
            readonly: vec![],
            install: ServiceRequest {
                entry: SiteRequest { pc: 0x00c4_1000, word: 0 },
                writer: SiteRequest { pc: 0x00c4_1004, word: STR_PUBLICATION },
            },
            rollback: ServiceRequest {
                entry: SiteRequest { pc: 0x00c4_1008, word: 0 },
                writer: SiteRequest { pc: 0x00c4_100c, word: STR_PUBLICATION },
            },
            publications,
            bank_variant: Some("modal75,menu-engine,render".into()),
            context_catalogue: ContextCatalogue::Stock,
            property_delta_words: None,
            retired: vec![],
            settings_writers: vec![],
            settings_reloads: vec![],
            task33_stack_guard: Some(Task33StackGuardRequest {
                node: 0x4006_060c, creation: 0x4005_c3ac, entry: 0x0110_9130,
                base: 0x00a4_ce00, size: 4096, guard_bytes: 64, baseline: vec![0; 64],
            }),
        };
        let sources = vec![extent(0x00c4_1000, 0x00c4_1010).unwrap()];
        assert!(matches!(request.into_config(sources), Err(LifetimeFailure::InvalidExtent)));
    }
}
