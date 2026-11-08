//! Diagnostic-only stack demand inside source-bound render invocations.
use crate::{modal_permit::CoreId, modal_task::{self, MemoryError, PhysicalExtent, StackExtent, TaskIdentity, ValidatedTaskMemory},
    render_stack_context::{Attributor, Sample as ContextSample, StackDescriptor},
    memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window}};
use fujimod_emulator::{board::{factory::{Graph, MemoryMap}, observation::{InstructionPoints, ObservationContext}}, session::Session};
use fujimod_emulator_cpu::{Hook, HookId, HookRange, InstructionEntry, InstructionEntryId, Model, Register, arm};
use serde::{Deserialize, Serialize};
use std::{cell::{Cell, RefCell}, ffi::c_void, io::Read, path::PathBuf, rc::Rc, time::Instant};
use crate::{provenance::Guest, render_provenance::Boundary};

#[path = "render_stack_mapping.rs"]
mod mapping;
use mapping::A32Mappings;

const RECORD_LIMIT: usize = 4096;
const TRANSITION_LIMIT: usize = 65536;
const NESTING_LIMIT: usize = 32;
const SOURCE_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role { Stock, Owned }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind { Tree, Picker, Pane }
impl EntryKind {
    fn address(self) -> u32 { match self { Self::Tree => 0x1164e1c, Self::Picker => 0x1163f2c, Self::Pane => 0x11662a0 } }
}
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind { Owned, Retained }
#[derive(Clone, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
struct Digest(String);
impl TryFrom<String> for Digest {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Err("stack diagnostic requires lowercase SHA256".into());
        }
        Ok(Self(value))
    }
}
impl From<Digest> for String { fn from(value: Digest) -> Self { value.0 } }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source { start: u32, path: PathBuf, sha256: Digest, kind: SourceKind }
struct Image { start: u32, bytes: Vec<u8>, kind: SourceKind }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry { kind: EntryKind, address: u32, frame_depth: u32 }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginRequest {
    version: u32, role: Role, sources: Vec<Source>, entries: Vec<Entry>, returns: Vec<u32>, retained_services: Vec<u32>,
}
struct Config { request: BeginRequest, images: Vec<Image>, access: ObservationContext, fetch_mappings: A32Mappings }
struct BoundInstruction { kind: SourceKind, word: u32 }
struct Refusal { message: String, observed_word: Option<u32> }
impl Refusal {
    fn with_word(message: impl Into<String>, observed_word: u32) -> Self {
        Self { message: message.into(), observed_word: Some(observed_word) }
    }
}
impl From<String> for Refusal {
    fn from(message: String) -> Self { Self { message, observed_word: None } }
}
impl From<&str> for Refusal {
    fn from(message: &str) -> Self { message.to_owned().into() }
}
impl Config {
    fn source(&self, address: u32) -> Result<(&Image, u32), String> {
        let address = code_address(address);
        self.images.iter().find_map(|image| {
            let offset = address.checked_sub(image.start)? as usize;
            let word = image.bytes.get(offset..offset.checked_add(4)?)?;
            Some((image, u32::from_le_bytes(word.try_into().ok()?)))
        }).ok_or_else(|| format!("stack diagnostic instruction lacks bound source at {address:#x}"))
    }
    fn check(&self, core: usize, pc: u32, entry: InstructionEntry) -> Result<BoundInstruction, Refusal> {
        let (source, expected) = self.source(pc)?;
        let mut bytes = [0; 4];
        let canonical = code_address(pc);
        let cpu = self.access.cpu(core)?;
        if entry.address != u64::from(pc) || entry.bytes != 4 { return Err("stack diagnostic fetch entry identity unavailable".into()); }
        let mut tcm_source = false;
        for (bank, observed) in entry.tcm.into_iter().enumerate() {
            let register = cpu.coprocessor(arm::Coprocessor {
                cp:15, opc1:0, crn:9, crm:1, opc2:bank as u32, secure:0, wide:false,
            }, None)?;
            let window = tcm_window(TcmRegister(u32::try_from(register).map_err(|_| "stack diagnostic TCM register exceeds32bits")?))?;
            if window != observed.map(|span| (span.address, span.address + span.bytes as u64)) {
                return Err("stack diagnostic fetch TCM context changed".into());
            }
            if let Some((start, end)) = window {
                if start < u64::from(pc) + 4 && u64::from(pc) < end {
                    if tcm_source || u64::from(pc) < start || u64::from(pc) + 4 > end {
                        return Err("stack diagnostic fetch has ambiguous TCM backing".into());
                    }
                    tcm_source = true;
                }
            }
        }
        if tcm_source {
            // InstructionEntry.physical describes declared maps, not TCM overlays.
            cpu.read_memory(u64::from(pc), &mut bytes)?;
        } else {
            if entry.physical != Some(u64::from(canonical))
                || !self.fetch_mappings.admits(CoreId(core), Guest(pc), Guest(canonical)) {
                return Err(format!("stack diagnostic code alias is not source-mapped: core {core}, pc {pc:#010x}, canonical {canonical:#010x}, entry physical {:?}, TCM {:?}, pstate {:#010x}", entry.physical, entry.tcm, entry.pstate).into());
            }
            cpu.read_memory(u64::from(pc), &mut bytes)?;
            let mut physical = [0; 4];
            self.access.read_backing(u64::from(canonical), &mut physical)
                .map_err(|error| Refusal::with_word(error, u32::from_le_bytes(bytes)))?;
            if physical != bytes {
                return Err(Refusal::with_word(format!("stack diagnostic guest/physical source bytes differ: core {core}, pc {pc:#010x}, guest {:#010x}, physical {:#010x}, entry physical {:?}, TCM {:?}", u32::from_le_bytes(bytes), u32::from_le_bytes(physical), entry.physical, entry.tcm), u32::from_le_bytes(bytes)));
            }
        }
        if u32::from_le_bytes(bytes) != expected {
            return Err(Refusal::with_word(format!("stack diagnostic source differs: core {core}, pc {pc:#010x}, expected {expected:#010x}, observed {:#010x}, entry physical {:?}, TCM {:?}", u32::from_le_bytes(bytes), entry.physical, entry.tcm), u32::from_le_bytes(bytes)));
        }
        Ok(BoundInstruction { kind: source.kind, word: expected })
    }
    fn new(mut request: BeginRequest, access: ObservationContext, fetch_mappings: A32Mappings) -> Result<Self, String> {
        if request.version != 1 || request.entries.len() != 3 || request.returns.is_empty() || request.returns.len() > 64
            || request.retained_services.len() > 128 || request.sources.is_empty() || request.sources.len() > 128 {
            return Err("invalid stack diagnostic configuration bounds".into());
        }
        let mut total = 0usize;
        request.sources.sort_by_key(|image| image.start);
        let mut previous_end = 0u64;
        let mut images = Vec::with_capacity(request.sources.len());
        for source in &request.sources {
            let file = std::fs::File::open(&source.path).map_err(|error| error.to_string())?;
            let size = usize::try_from(file.metadata().map_err(|error| error.to_string())?.len())
                .map_err(|_| "stack diagnostic source length overflow")?;
            let end = u64::from(source.start) + size as u64;
            if source.start % 4 != 0 || size == 0 || size % 4 != 0 || size > SOURCE_LIMIT - total
                || end > 1 << 32 || u64::from(source.start) < previous_end {
                return Err("invalid stack diagnostic source images".into());
            }
            let mut bytes = Vec::with_capacity(size);
            file.take((SOURCE_LIMIT - total + 1) as u64).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            if bytes.len() != size || crate::bridge::sha256(&bytes) != source.sha256.0 {
                return Err("stack diagnostic source identity differs".into());
            }
            total += size;
            previous_end = end;
            images.push(Image { start: source.start, bytes, kind: source.kind });
        }
        for kind in [EntryKind::Tree, EntryKind::Picker, EntryKind::Pane] {
            if request.entries.iter().filter(|entry| entry.kind == kind && entry.address == kind.address()
                && entry.frame_depth <= 4096).count() != 1 { return Err("stack diagnostic requires all canonical render entries".into()); }
        }
        request.returns.sort_unstable();
        request.returns.dedup();
        request.retained_services.sort_unstable();
        request.retained_services.dedup();
        let config = Self { request, images, access, fetch_mappings };
        for entry in &config.request.entries {
            let (_, word) = config.source(entry.address)?;
            match config.request.role {
                Role::Stock if word == 0xe92d4ff0 => {},
                Role::Owned if word & 0xff000000 == 0xea000000 => {
                    let displacement = ((word << 8) as i32 >> 6) as u32;
                    let target = entry.address.wrapping_add(8).wrapping_add(displacement);
                    let (image, first) = config.source(target)?;
                    if image.kind != SourceKind::Owned || first != 0xe92d5fff {
                        return Err("stack diagnostic entry does not branch to the bound adapter push".into());
                    }
                }
                _ => return Err("stack diagnostic entry differs from the selected role".into()),
            }
            config.source(entry.address + 4)?;
        }
        for &pc in config.request.returns.iter().chain(&config.request.retained_services) {
            if pc % 4 != 0 { return Err("stack diagnostic boundary is not A32 aligned".into()); }
            config.source(pc)?;
        }
        Ok(config)
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Identity { kernel: Kernel, task: u16, node: u32, creation: u32, descriptor: [u32; 10] }
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Kernel { Primary, Secondary }
impl Identity {
    fn new(identity: TaskIdentity, descriptor: [u32; 10]) -> Self {
        Self { kernel: match identity.kernel { modal_task::KernelId::Primary => Kernel::Primary, modal_task::KernelId::Secondary => Kernel::Secondary },
            task: identity.id.get(), node: identity.node.address().0, creation: identity.creation.address().0, descriptor }
    }
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Terminal { OwnedReturn, FallbackHandoff, StockReturn }
#[derive(Serialize)]
struct Transition { pc: u32, cpsr: u32, context_depth: u32, current_node: u32, owner_node: Option<u32>, scheduler: bool,
    mode_source: Option<ModeSource> }
#[derive(Clone, Copy, Serialize)]
struct ModeSource { pc: u32, word: u32 }
#[derive(Serialize)]
#[serde(transparent)]
struct ObservationOrdinal(u64);
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum MinimumCoordinate { EntryBoundary { boundary: Boundary }, Observation { ordinal: ObservationOrdinal } }
#[derive(Serialize)]
struct MinimumContext {
    coordinate: MinimumCoordinate, core: usize, cpsr: u32, context_depth: u32,
    current: Identity, owner: Option<Identity>, scheduler: Option<bool>, mode_source: Option<ModeSource>,
}
impl MinimumContext {
    fn new(sample: &InstructionSample, owner: Option<&Identity>, scheduler: Option<bool>, coordinate: MinimumCoordinate) -> Self {
        Self { coordinate, core: sample.core, cpsr: sample.cpsr, context_depth: sample.context_depth,
            current: Identity::new(sample.identity, sample.descriptor), owner: owner.cloned(), scheduler, mode_source: sample.mode_source }
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct InvocationId(u64);
#[derive(Clone, Copy)]
struct OwnedObservation { invocation: InvocationId, identity: TaskIdentity, pc: Guest, word: u32, sp: u32, cpsr: u32 }
#[derive(Clone, Copy)]
enum ObservedSourceTransfer { Other, AdjacentOwnedToRetained(OwnedObservation) }
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum ServiceCoverage { PartialObservedSourceTransitions }
#[derive(Serialize)]
struct ServiceSpan { observed_owned_pc: u32, observed_owned_word: u32, entry_pc: u32, entry_sp: u32, return_pc: u32, exit_sp: Option<u32>, min_sp: u32, deepest_pc: u32, deepest_lr: u32, minimum_context: MinimumContext, within_allowance: bool }
#[derive(Serialize)]
struct Invocation {
    id: u64, parent: Option<u64>, kind: EntryKind, core: usize, task_identity: Identity,
    entry_pc: u32, entry_sp: u32, entry_lr: u32, entry_mode: u32,
    entry_boundary: Boundary, terminal_boundary: Option<Boundary>,
    exit_sp: Option<u32>, min_sp: u32, deepest_pc: u32, deepest_lr: u32,
    minimum_context: MinimumContext,
    terminal: Option<Terminal>, frame_depth: u32, allowance: u32, within_bound: bool,
    transitions: Vec<Transition>, retained_spans: Vec<ServiceSpan>,
    #[serde(skip)] identity: TaskIdentity,
    #[serde(skip)] last_context: Option<(u32, u32, u32, Option<u32>, bool)>,
    #[serde(skip)] open_services: Vec<usize>,
}
#[derive(Default, Serialize)]
struct Performance { active_instructions: u64, invocation_count: u64, hook_transitions: u64, host_nanoseconds: u64 }
struct State { invocations: Vec<Invocation>, open: Vec<usize>, failure: Option<String>, performance: Performance, transitions: usize, service_spans: usize }
impl State {
    fn fail(&mut self, error: String) { self.failure.get_or_insert(error); }
    fn active(&self, core: usize) -> bool { self.open.iter().any(|&index| self.invocations[index].core == core) }
    fn observe(&mut self, config: &Config, sample: &InstructionSample, owner: Option<TaskIdentity>, scheduler: bool, transfer: ObservedSourceTransfer) -> Result<(), String> {
        self.performance.active_instructions = self.performance.active_instructions.checked_add(1).ok_or("stack diagnostic instruction counter overflow")?;
        let current_node = sample.identity.node.address().0;
        let owner_node = owner.map(|task| task.node.address().0);
        let context = (sample.cpsr & 0x1ff, sample.context_depth, current_node, owner_node, scheduler);
        for &index in &self.open {
            let invocation = &mut self.invocations[index];
            if invocation.core != sample.core { continue; }
            if invocation.last_context != Some(context) {
                if invocation.transitions.len() == RECORD_LIMIT || self.transitions == TRANSITION_LIMIT {
                    return Err("stack diagnostic transition storage bound exceeded".into());
                }
                self.transitions += 1;
                invocation.transitions.push(Transition { pc: sample.pc, cpsr: sample.cpsr, context_depth: sample.context_depth,
                    current_node, owner_node, scheduler, mode_source: sample.mode_source });
                invocation.last_context = Some(context);
            }
            if owner != Some(invocation.identity) { continue; }
            if sample.identity == invocation.identity && invocation.task_identity != Identity::new(sample.identity, sample.descriptor) {
                return Err("stack diagnostic task descriptor changed".into());
            }
            if physical(sample.sp) < physical(invocation.min_sp) {
                invocation.min_sp = sample.sp; invocation.deepest_pc = sample.pc; invocation.deepest_lr = sample.lr;
                invocation.minimum_context = MinimumContext::new(sample, Some(&invocation.task_identity), Some(scheduler),
                    MinimumCoordinate::Observation { ordinal: ObservationOrdinal(self.performance.active_instructions) });
            }
            let base = physical(invocation.task_identity.descriptor[5]);
            let end = base.checked_add(invocation.task_identity.descriptor[4]).ok_or("stack diagnostic descriptor overflow")?;
            if sample.sp % 4 != 0 || physical(sample.sp) < base || physical(sample.sp) > end {
                return Err("stack diagnostic owner SP leaves descriptor".into());
            }
            if let Some(&span_index) = invocation.open_services.last() {
                let span = &mut invocation.retained_spans[span_index];
                if sample.pc == span.return_pc && sample.sp == span.entry_sp {
                    span.exit_sp = Some(sample.sp);
                    span.within_allowance = physical(span.entry_sp).saturating_sub(physical(span.min_sp)) <= 1024;
                    invocation.open_services.pop();
                }
            }
            for &span_index in &invocation.open_services {
                let span = &mut invocation.retained_spans[span_index];
                if physical(sample.sp) < physical(span.min_sp) {
                    span.min_sp = sample.sp; span.deepest_pc = sample.pc; span.deepest_lr = sample.lr;
                    span.minimum_context = MinimumContext::new(sample, Some(&invocation.task_identity), Some(scheduler),
                        MinimumCoordinate::Observation { ordinal: ObservationOrdinal(self.performance.active_instructions) });
                }
            }
            let observed_owned = match transfer {
                ObservedSourceTransfer::AdjacentOwnedToRetained(observed) if observed.invocation == InvocationId(invocation.id) => Some(observed),
                _ => None,
            };
            if let Some(observed) = observed_owned.filter(|_| config.request.retained_services.binary_search(&code_address(sample.pc)).is_ok()) {
                if self.service_spans == RECORD_LIMIT || invocation.open_services.len() == NESTING_LIMIT {
                    return Err("stack diagnostic retained service bound exceeded".into());
                }
                self.service_spans += 1;
                invocation.open_services.push(invocation.retained_spans.len());
                invocation.retained_spans.push(ServiceSpan { observed_owned_pc: observed.pc.0, observed_owned_word: observed.word,
                    entry_pc: sample.pc, entry_sp: sample.sp, return_pc: sample.lr,
                    exit_sp: None, min_sp: sample.sp, deepest_pc: sample.pc, deepest_lr: sample.lr,
                    minimum_context: MinimumContext::new(sample, Some(&invocation.task_identity), Some(scheduler),
                        MinimumCoordinate::Observation { ordinal: ObservationOrdinal(self.performance.active_instructions) }),
                    within_allowance: false });
            }
        }
        Ok(())
    }
    fn boundary(&mut self, config: &Config, sample: &InstructionSample, boundary: Boundary) -> Result<(), String> {
        if let Some(entry) = config.request.entries.iter().find(|entry| entry.address == code_address(sample.pc)) {
            if sample.core != 0 || sample.identity.kernel != modal_task::KernelId::Primary || sample.identity.id.get() != 33
                || sample.identity.node.address().0 != 0x4006060c || sample.identity.creation.address().0 != 0x4005c3ac
                || sample.descriptor[2] != 0x1109130 || sample.descriptor[4] != 4096 || physical(sample.descriptor[5]) != 0xa4ce00 {
                return Err("stack diagnostic entry is not the admitted primary task33".into());
            }
            if sample.cpsr & 0x0100023f != 0x13 || config.request.returns.binary_search(&code_address(sample.lr)).is_err() {
                return Err("stack diagnostic entry mode or continuation is not admitted".into());
            }
            let caller = code_address(sample.lr).checked_sub(4).ok_or("stack diagnostic entry caller underflow")?;
            let (_, word) = config.source(caller)?;
            let target = direct_target(caller, word).ok_or("stack diagnostic entry caller is not a direct call")?;
            if code_address(target) != entry.address {
                return Err("stack diagnostic entry caller is not the applicable direct call".into());
            }
            if self.invocations.len() == RECORD_LIMIT || self.open.len() == NESTING_LIMIT {
                return Err("stack diagnostic invocation bound exceeded".into());
            }
            if self.open.iter().any(|&index| self.invocations[index].identity == sample.identity
                && self.invocations[index].core != sample.core) { return Err("stack diagnostic invocation migrated core".into()); }
            let parent = self.open.iter().rev().find_map(|&index| {
                let invocation = &self.invocations[index];
                (invocation.identity == sample.identity).then_some(invocation.id)
            });
            let id = self.invocations.len() as u64 + 1;
            self.open.push(self.invocations.len());
            self.invocations.push(Invocation { id, parent, kind: entry.kind, core: sample.core,
                task_identity: Identity::new(sample.identity, sample.descriptor), identity: sample.identity,
                entry_pc: sample.pc, entry_sp: sample.sp, entry_lr: sample.lr, entry_mode: sample.cpsr & 0x1f,
                entry_boundary: boundary, terminal_boundary: None,
                exit_sp: None, min_sp: sample.sp, deepest_pc: sample.pc, deepest_lr: sample.lr, terminal: None,
                minimum_context: MinimumContext::new(sample, None, None, MinimumCoordinate::EntryBoundary { boundary }),
                frame_depth: entry.frame_depth, allowance: 1024, within_bound: false, transitions: Vec::new(),
                retained_spans: Vec::new(), open_services: Vec::new(), last_context: None });
            self.performance.invocation_count += 1;
            return Ok(());
        }
        let terminal = self.open.iter().rev().find_map(|&index| {
            let invocation = &self.invocations[index];
            (invocation.identity == sample.identity && invocation.core == sample.core).then_some(index)
        });
        let Some(index) = terminal else { return Ok(()) };
        let invocation = &mut self.invocations[index];
        let fallback = config.request.role == Role::Owned && sample.pc == invocation.entry_pc + 4;
        if sample.pc != invocation.entry_lr && !fallback { return Ok(()) }
        let expected_sp = if fallback { invocation.entry_sp.checked_sub(36).ok_or("stack diagnostic fallback SP underflow")? } else { invocation.entry_sp };
        if sample.sp != expected_sp || sample.cpsr & 0x1f != invocation.entry_mode
            || Identity::new(sample.identity, sample.descriptor) != invocation.task_identity || !invocation.open_services.is_empty() {
            return Err("stack diagnostic return is ambiguous or unrestored".into());
        }
        invocation.exit_sp = Some(sample.sp);
        invocation.terminal_boundary = Some(boundary);
        invocation.terminal = Some(if fallback { Terminal::FallbackHandoff } else if config.request.role == Role::Owned { Terminal::OwnedReturn } else { Terminal::StockReturn });
        let floor = physical(invocation.entry_sp).checked_sub(invocation.frame_depth + invocation.allowance)
            .ok_or("stack diagnostic floor underflow")?;
        invocation.within_bound = physical(invocation.min_sp) >= floor;
        self.open.retain(|&open| open != index);
        Ok(())
    }
}
fn physical(address: u32) -> u32 { address & !0x40000000 }
fn code_address(address: u32) -> u32 { if address < 0x60000000 { physical(address) } else { address } }
fn link_branch(word: u32) -> bool { word & 0x0f000000 == 0x0b000000 || word & 0x0ffffff0 == 0x012fff30 }
fn direct_target(pc: u32, word: u32) -> Option<u32> {
    if word & 0x0f000000 != 0x0b000000 { return None; }
    let displacement = (((word & 0xffffff) << 8) as i32 >> 6) as u32;
    Some(pc.wrapping_add(8).wrapping_add(displacement))
}
fn mode_transition(previous: Option<(u32, u32, u32)>, sample: &InstructionSample) -> Result<Option<ModeSource>, String> {
    let mode = sample.cpsr & 0x1f;
    let Some((old_mode, previous_pc, previous_word)) = previous else { return Ok(None) };
    if old_mode == mode { return Ok(None) }
    let pc = code_address(sample.pc);
    let vector = match mode { 0x1b => Some(4), 0x17 => Some(if pc & 0xffff == 12 { 12 } else { 16 }), 0x12 => Some(24), 0x11 => Some(28), _ => None };
    if vector.is_some_and(|offset| pc == offset || pc == 0xffff0000 + offset) && sample.word == 0xe59ff018 {
        return Ok(Some(ModeSource { pc: sample.pc, word: sample.word }));
    }
    let offset = if sample.core == 0 { 0 } else { 0x90000 };
    let prior = code_address(previous_pc).checked_sub(offset);
    let next = pc.checked_sub(offset);
    let source_switch = matches!((prior, previous_word, mode, next),
        (Some(0x40208), 0xe129f00d, 0x13, Some(0x4020c)) |
        (Some(0x40214), 0xe129f000, 0x12, Some(0x40218)) |
        (Some(0x40228), 0xe129f002, 0x13, Some(0x4022c)));
    let source_return = matches!((prior, previous_word),
        (Some(0x401b0 | 0x40200), 0xe8fd9000) | (Some(0x40364 | 0x437d8), 0xe8fdd00f));
    if source_switch || source_return { return Ok(Some(ModeSource { pc: previous_pc, word: previous_word })) }
    Err("stack diagnostic stack-bank transition lacks a bound source".into())
}
struct InstructionSample { core: usize, pc: u32, sp: u32, lr: u32, cpsr: u32, r1: u32, r2: u32,
    identity: TaskIdentity, descriptor: [u32; 10], context_depth: u32, word: u32, source: SourceKind, mode_source: Option<ModeSource> }
struct ContextMemory<'a> { access: &'a ObservationContext, maps: &'a [MemoryMap] }
impl ContextMemory<'_> {
    fn validate(&self, extent: PhysicalExtent) -> Result<(), MemoryError> {
        if self.access.core_count() != 2 { return Err(MemoryError::Translation); }
        for core in 0..2 {
            let cpu = self.access.cpu(core).map_err(|_| MemoryError::Read)?;
            let mut tcm = [None; 2];
            for (bank, window) in tcm.iter_mut().enumerate() {
                let value = cpu.coprocessor(arm::Coprocessor {
                    cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32,
                    secure: 0, wide: false,
                }, None).map_err(|_| MemoryError::Read)?;
                let value = u32::try_from(value).map_err(|_| MemoryError::ActiveTcm)?;
                *window = tcm_window(TcmRegister(value)).map_err(|_| MemoryError::ActiveTcm)?;
            }
            for alias in [0, 0x4000_0000] {
                let start = u64::from(extent.start().0 | alias);
                let end = start + u64::from(extent.bytes());
                if tcm.iter().flatten().any(|&(low, high)| low < end && start < high) {
                    return Err(MemoryError::ActiveTcm);
                }
                let span = static_span(self.maps, MemoryCore(core), MemoryAddress(start)..MemoryAddress(end))
                    .map_err(|_| MemoryError::Translation)?;
                if span.physical.0 != u64::from(extent.start().0) { return Err(MemoryError::Translation); }
            }
        }
        Ok(())
    }
}
impl ValidatedTaskMemory for ContextMemory<'_> {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        let bytes = u32::try_from(output.len()).map_err(|_| MemoryError::Unmapped)?;
        let extent = PhysicalExtent::new(address, bytes).map_err(|_| MemoryError::Unmapped)?;
        self.validate(extent)?;
        self.access.read_physical(u64::from(extent.start().0), output).map_err(|_| MemoryError::Read)?;
        let mut observed = [0; 256];
        for core in 0..2 {
            for alias in [0, 0x4000_0000] {
                for (index, expected) in output.chunks(observed.len()).enumerate() {
                    let start = u64::from(extent.start().0 | alias) + (index * observed.len()) as u64;
                    let bytes = &mut observed[..expected.len()];
                    self.access.read_memory(core, start, bytes).map_err(|_| MemoryError::Read)?;
                    if bytes != expected { return Err(MemoryError::PhysicalMismatch); }
                }
            }
        }
        Ok(())
    }
    fn validate_stack(&self, extent: StackExtent) -> Result<(), MemoryError> { self.validate(extent.physical()) }
}

fn sample(config: &Config, core: usize, pc: u32, entry: InstructionEntry) -> Result<InstructionSample, Refusal> {
    let source = config.check(core, pc, entry)?;
    let word = source.word;
    sample_bound(config, core, pc, source).map_err(|error| Refusal::with_word(error, word))
}
fn sample_bound(config: &Config, core: usize, pc: u32, source: BoundInstruction) -> Result<InstructionSample, String> {
    let register = |register| config.access.read_register(core, Register::Arm32(register))
        .and_then(|value| u32::try_from(value).map_err(|_| "stack diagnostic register exceeds32bits".into()));
    let memory = ContextMemory { access: &config.access, maps: config.access.mappings() };
    let sampled = modal_task::sample_context(&memory, CoreId(core))
        .map_err(|error| format!("stack diagnostic context: {error:?}"))?;
    let (identity, descriptor, context_depth) = (sampled.identity, sampled.descriptor, sampled.context_depth);
    let cpsr = register(arm::Register::Cpsr)?;
    if cpsr & ((1 << 24) | (1 << 9) | (1 << 5)) != 0 { return Err("stack diagnostic requires A32 little-endian execution".into()); }
    Ok(InstructionSample { core, pc, sp: register(arm::Register::R(13))?, lr: register(arm::Register::R(14))?,
        cpsr, r1: register(arm::Register::R(1))?, r2: register(arm::Register::R(2))?,
        identity, descriptor, context_depth, word: source.word, source: source.kind, mode_source: None })
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct ReplayKey { pc: u32, sp: u32, lr: u32, mode: u32, r1: u32, r2: u32, identity: TaskIdentity, descriptor: [u32; 10], word: u32 }
impl From<&InstructionSample> for ReplayKey {
    fn from(sample: &InstructionSample) -> Self {
        Self { pc: sample.pc, sp: sample.sp, lr: sample.lr, mode: sample.cpsr & 0x1f, r1: sample.r1, r2: sample.r2,
            identity: sample.identity, descriptor: sample.descriptor, word: sample.word }
    }
}
#[derive(Clone, Copy)]
struct OwnedFrame { identity: TaskIdentity, invocation: Option<InvocationId>, continuation: u32, entry_sp: u32 }
struct Callback {
    core: usize, leased: Cell<bool>, config: Rc<Config>, state: Rc<RefCell<State>>,
    attribution: RefCell<Attributor>, last: Cell<Option<(InstructionEntryId, u64, u32)>>,
    previous_source: Cell<Option<SourceKind>>,
    previous_owned: Cell<Option<OwnedObservation>>,
    previous_bank: Cell<Option<(u32, u32, u32)>>,
    previous_instruction: Cell<Option<(u32, u32)>>,
    last_sample: Cell<Option<ReplayKey>>,
    pending_replays: RefCell<Vec<(InstructionEntryId, ReplayKey)>>,
    owned_frames: RefCell<Vec<OwnedFrame>>,
    deferred_mode: Cell<Option<ModeSource>>,
}
impl Callback {
    fn active_invocation(&self, identity: TaskIdentity) -> Option<InvocationId> {
        let state = self.state.borrow();
        state.open.iter().rev().find_map(|&index| {
            let invocation = &state.invocations[index];
            (invocation.core == self.core && invocation.identity == identity).then_some(InvocationId(invocation.id))
        })
    }
    fn observe(&self, pc: u32) -> Result<(), Refusal> {
        let cpu = self.config.access.cpu(self.core)?;
        let entry = cpu.instruction_entry().ok_or("stack diagnostic entry identity unavailable")?;
        if self.last.get().is_some_and(|(id, _, _)| id == entry.id) { return Ok(()) }
        self.last.set(Some((entry.id, entry.progress, pc)));
        let sampled = sample(&self.config, self.core, pc, entry)?;
        let word = sampled.word;
        self.observe_sample(pc, sampled).map_err(|error| Refusal::with_word(error, word))
    }
    fn observe_sample(&self, pc: u32, mut sampled: InstructionSample) -> Result<(), String> {
        sampled.mode_source = mode_transition(self.previous_bank.get(), &sampled)?;
        self.previous_bank.set(Some((sampled.cpsr & 0x1f, pc, sampled.word)));
        let key = ReplayKey::from(&sampled);
        self.last_sample.set(Some(key));
        let mut replays = self.pending_replays.borrow_mut();
        if let Some(index) = replays.iter().position(|(_, pending)| pending.pc == key.pc && pending.sp == key.sp
            && pending.identity == key.identity && pending.mode == key.mode) {
            if replays[index].1 != key { return Err("stack diagnostic unentered callback context changed".into()); }
            replays.remove(index);
            if let Some(source) = sampled.mode_source { self.deferred_mode.set(Some(source)); }
            self.previous_source.set(Some(sampled.source));
            self.previous_instruction.set(Some((pc, sampled.word)));
            self.previous_owned.set(None);
            return Ok(());
        }
        drop(replays);
        sampled.mode_source = sampled.mode_source.or(self.deferred_mode.take());
        let attribution = self.attribution.borrow_mut().observe(ContextSample { core: self.core, pc: code_address(pc), word: sampled.word,
            sp: sampled.sp, cpsr: sampled.cpsr, r1: sampled.r1, r2: sampled.r2,
            current: sampled.identity, stack: StackDescriptor { bytes: sampled.descriptor[4], base: sampled.descriptor[5] },
            context_depth: sampled.context_depth })?;
        let source = sampled.source;
        let previous_source = self.previous_source.get();
        let previous = self.previous_instruction.get();
        if previous_source == Some(SourceKind::Retained) && source == SourceKind::Owned
            && previous.is_some_and(|(previous_pc, word)| link_branch(word) && sampled.lr == previous_pc.wrapping_add(4)) {
            let invocation = self.active_invocation(sampled.identity);
            let mut frames = self.owned_frames.borrow_mut();
            if frames.len() == NESTING_LIMIT { return Err("stack diagnostic owned call frame bound exceeded".into()); }
            frames.push(OwnedFrame { identity: sampled.identity, invocation,
                continuation: code_address(sampled.lr), entry_sp: sampled.sp });
        }
        if previous_source == Some(SourceKind::Owned) && source == SourceKind::Retained
            && attribution.owner.is_some() && !attribution.scheduler_transition {
            let canonical = code_address(pc);
            let active = self.active_invocation(sampled.identity);
            let terminal = self.state.borrow().open.iter().any(|&index| {
                let state = self.state.borrow();
                let invocation = &state.invocations[index];
                invocation.identity == sampled.identity && (pc == invocation.entry_lr || pc == invocation.entry_pc + 4)
            });
            let entered = terminal || self.config.request.entries.iter().any(|entry| entry.address == canonical)
                || self.config.request.retained_services.binary_search(&canonical).is_ok();
            let mut frames = self.owned_frames.borrow_mut();
            if entered {
                if previous.is_some_and(|(_, word)| !link_branch(word)) {
                    if let Some(index) = frames.iter().rposition(|frame| frame.identity == sampled.identity && frame.invocation == active) {
                        frames.remove(index);
                    }
                }
            } else {
                let matched = frames.iter().rposition(|frame| frame.identity == sampled.identity && frame.invocation == active
                    && frame.continuation == canonical && frame.entry_sp == sampled.sp);
                let returned = previous.is_some_and(|(_, word)| !link_branch(word)) && matched.is_some_and(|index| {
                    frames[index + 1..].iter().all(|frame| frame.identity != sampled.identity || frame.invocation != active)
                });
                if returned {
                    frames.remove(matched.unwrap());
                } else {
                    return Err("stack diagnostic owned-to-retained transfer lacks a bound service".into());
                }
            }
        }
        let previous_owned = self.previous_owned.take();
        let invocation = if attribution.owner == Some(sampled.identity) && !attribution.scheduler_transition
            && (source == SourceKind::Owned || previous_owned.is_some()) {
            let state = self.state.borrow();
            state.open.iter().rev().find_map(|&index| {
                let invocation = &state.invocations[index];
                (invocation.core == self.core && invocation.identity == sampled.identity).then_some(InvocationId(invocation.id))
            })
        } else { None };
        let transfer = match (previous_owned, invocation) {
            (Some(previous), Some(invocation)) if source == SourceKind::Retained && previous.invocation == invocation
                && previous.identity == sampled.identity && previous.sp == sampled.sp && previous.cpsr & 0x1f == sampled.cpsr & 0x1f =>
                ObservedSourceTransfer::AdjacentOwnedToRetained(previous),
            _ => ObservedSourceTransfer::Other,
        };
        if source == SourceKind::Owned {
            self.previous_owned.set(invocation.map(|invocation| OwnedObservation { invocation, identity: sampled.identity,
                pc: Guest(pc), word: sampled.word, sp: sampled.sp, cpsr: sampled.cpsr }));
        }
        self.previous_source.set(Some(source));
        self.previous_instruction.set(Some((pc, sampled.word)));
        self.state.borrow_mut().observe(&self.config, &sampled, attribution.owner, attribution.scheduler_transition, transfer)
    }
    fn retain_unentered(&self, session: &Session) -> Result<(), String> {
        let Some((id, progress, pc)) = self.last.get() else { return Ok(()) };
        let cpu = session.board.cpu(self.core)?;
        let unentered = cpu.instruction_entry().is_some_and(|entry| entry.id == id)
            && cpu.instruction_progress().and_then(|count| count.checked_add(1)) == Some(progress)
            && session.board.read_register(self.core, Register::Pc)? == u64::from(pc);
        if unentered {
            let key = self.last_sample.get().ok_or("stack diagnostic unentered callback lacks its sample")?;
            let mut pending = self.pending_replays.borrow_mut();
            if !pending.iter().any(|&(previous, _)| previous == id) {
                if pending.len() == NESTING_LIMIT { return Err("stack diagnostic replay bound exceeded".into()); }
                pending.push((id, key));
            }
        }
        Ok(())
    }
    fn refusal_context(&self, pc: u64, error: Refusal) -> String {
        let access = &self.config.access;
        let cpu = access.cpu(self.core);
        let entry = cpu.as_ref().map(|cpu| cpu.instruction_entry()).map_err(Clone::clone);
        let sp = access.read_register(self.core, Register::Arm32(arm::Register::R(13)));
        let lr = access.read_register(self.core, Register::Arm32(arm::Register::R(14)));
        let cpsr = access.read_register(self.core, Register::Arm32(arm::Register::Cpsr));
        let tcm_registers: [Result<u64, String>; 2] = std::array::from_fn(|bank| {
            cpu.as_ref().map_err(Clone::clone).and_then(|cpu| cpu.coprocessor(arm::Coprocessor {
                cp:15, opc1:0, crn:9, crm:1, opc2:bank as u32, secure:0, wide:false,
            }, None))
        });
        let tcm_windows = tcm_registers.each_ref().map(|register| register.as_ref().map_err(Clone::clone)
            .and_then(|register| u32::try_from(*register).map_err(|_| "TCM register exceeds32bits".to_owned()))
            .and_then(|register| tcm_window(TcmRegister(register))));
        let canonical = u32::try_from(pc).map(code_address);
        let expected = u32::try_from(pc).map_err(|_| "PC exceeds32bits".to_owned())
            .and_then(|pc| self.config.source(pc).map(|(_, word)| word));
        let observed = match error.observed_word {
            Some(word) => Ok(word),
            None => cpu.as_ref().map_err(Clone::clone).and_then(|cpu| {
                let mut bytes = [0; 4];
                cpu.read_memory(pc, &mut bytes)?;
                Ok(u32::from_le_bytes(bytes))
            }),
        };
        let physical = pc.checked_add(4).ok_or_else(|| "PC span overflows".to_owned()).and_then(|end| {
            static_span(access.mappings(), MemoryCore(self.core), MemoryAddress(pc)..MemoryAddress(end))
                .map(|span| span.physical.0)
        });
        format!("{}; refusal context core {}, pc {pc:#010x}, canonical {canonical:x?}, static physical {physical:x?}, entry {entry:x?}, SP {sp:x?}, LR {lr:x?}, CPSR {cpsr:x?}, live TCM registers {tcm_registers:x?}, live TCM windows {tcm_windows:x?}, expected word {expected:x?}, observed word {observed:x?}, previous bank {:x?}", error.message, self.core, self.previous_bank.get())
    }
    fn request_stop(&self) {
        self.config.access.request_pause();
        if let Ok(cpu) = self.config.access.cpu(self.core) { let _ = cpu.stop_before_instruction(); }
    }
}
unsafe fn instruction(pc: u64, _: u32, data: *mut c_void) {
    // The collector retains this callback until its hook is removed.
    let callback = unsafe { &*data.cast::<Callback>() };
    if callback.state.borrow().failure.is_some() {
        callback.request_stop();
        return;
    }
    if !callback.leased.get() {
        callback.state.borrow_mut().fail(callback.refusal_context(pc, "stack diagnostic callback outside execution window".into()));
        callback.request_stop();
        return;
    }
    let started = Instant::now();
    let result = u32::try_from(pc).map_err(|_| Refusal::from("stack diagnostic PC exceeds32bits"))
        .and_then(|pc| callback.observe(pc))
        .map_err(|error| callback.refusal_context(pc, error));
    let mut state = callback.state.borrow_mut();
    state.performance.host_nanoseconds = state.performance.host_nanoseconds.saturating_add(started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
    if let Err(error) = result {
        state.fail(error);
        callback.request_stop();
    }
}
struct Core { points: Box<InstructionPoints>, callback: Box<Callback>, hook: Option<HookId>, resume: Option<(u32, u64)>, processed: Option<InstructionEntryId> }
pub struct Collector { config: Rc<Config>, state: Rc<RefCell<State>>, cores: Vec<Core>, finished: bool,
    start: Boundary, end: Option<Boundary> }
impl Collector {
    pub fn begin(session: &Session, graph: &Graph, request: BeginRequest) -> Result<Self, String> {
        let access = session.board.observation_context();
        let fetch_mappings = A32Mappings::new(graph, &access)?;
        let config = Rc::new(Config::new(request, access, fetch_mappings)?);
        let state = Rc::new(RefCell::new(State { invocations: Vec::new(), open: Vec::new(), failure: None,
            performance: Performance::default(), transitions: 0, service_spans: 0 }));
        let mut collector = Self { config, state, cores: Vec::new(), finished: false,
            start: Boundary::sample(session)?, end: None };
        let addresses: Vec<u64> = collector.config.request.entries.iter().flat_map(|entry| [entry.address, entry.address + 4])
            .chain(collector.config.request.returns.iter().copied())
            .flat_map(|address| [u64::from(address), u64::from(address | 0x40000000)]).collect();
        for core in 0..collector.config.access.core_count() {
            let cpu = collector.config.access.cpu(core)?;
            if cpu.model() != Model::CortexR5F || !cpu.features().instruction_progress || !cpu.features().architectural_exits {
                collector.disarm(session)?;
                return Err("stack diagnostic requires exact counted R-profile observation".into());
            }
            match session.board.instruction_points_begin(core, addresses.clone()) {
                Ok(points) => collector.cores.push(Core { points, hook: None, resume: None, processed: None,
                    callback: Box::new(Callback { core, leased: Cell::new(false), config: collector.config.clone(), state: collector.state.clone(),
                        attribution: RefCell::new(Attributor::default()), last: Cell::new(None),
                        previous_source: Cell::new(None), previous_owned: Cell::new(None), previous_bank: Cell::new(None),
                        previous_instruction: Cell::new(None), last_sample: Cell::new(None), pending_replays: RefCell::new(Vec::new()),
                        owned_frames: RefCell::new(Vec::new()), deferred_mode: Cell::new(None) }) }),
                Err(error) => { collector.disarm(session)?; return Err(error) }
            }
        }
        Ok(collector)
    }
    pub(crate) fn before_advance(&self, session: &Session) -> Result<(), String> {
        if self.finished { return Ok(()) }
        if self.state.borrow().failure.is_some() { return Err("stack diagnostic already refused".into()); }
        if self.config.access.core_count() != 2 || session.board.instruction_counts().len() != 2 {
            self.refuse("stack diagnostic topology changed");
            return Err("stack diagnostic topology changed".into());
        }
        for core in 0..2 {
            let bound = self.config.access.cpu(core).map_err(|error| {
                self.refuse(&error);
                error
            })?;
            let live = session.board.cpu(core).map_err(|error| {
                self.refuse(&error);
                error
            })?;
            if !std::ptr::eq(bound, live) {
                self.refuse("stack diagnostic board changed");
                return Err("stack diagnostic board changed".into());
            }
        }
        if session.board.mappings() != self.config.access.mappings() {
            self.refuse("stack diagnostic admitted mapping snapshot changed");
            return Err("stack diagnostic admitted mapping snapshot changed".into());
        }
        let counts = session.board.instruction_counts();
        for (core, observer) in self.cores.iter().enumerate() {
            let pc = session.board.read_register(core, Register::Pc)?;
            let skip = observer.resume.filter(|&(address, count)| u64::from(address) == pc && count == counts[core]);
            observer.points.resume(pc, skip.map_or(!pc, |(address, _)| u64::from(address)));
            observer.callback.leased.set(true);
        }
        Ok(())
    }
    pub(crate) fn after_advance(&mut self, session: &Session) -> Result<(), String> {
        if self.finished { return Ok(()) }
        for core in &self.cores { core.callback.leased.set(false); }
        if let Some(error) = &self.state.borrow().failure { return Err(error.clone()); }
        let started = Instant::now();
        let result = self.collect(session).and_then(|()| {
            self.cores.iter().filter(|core| core.hook.is_some())
                .try_for_each(|core| core.callback.retain_unentered(session))
        });
        let mut state = self.state.borrow_mut();
        state.performance.host_nanoseconds = state.performance.host_nanoseconds.saturating_add(started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
        if let Err(error) = &result { state.fail(error.clone()); }
        result
    }
    fn collect(&mut self, session: &Session) -> Result<(), String> {
        let boundary = Boundary::sample(session)?;
        let counts = session.board.instruction_counts();
        for core in 0..self.cores.len() {
            let Some(pc) = self.cores[core].points.event() else { continue };
            let pc = u32::try_from(pc).map_err(|_| "stack diagnostic boundary exceeds32bits")?;
            let event = self.cores[core].points.event_context().ok_or("stack diagnostic boundary identity missing")?;
            if self.cores[core].processed != Some(event.entry.id) {
                if event.core.0 != core { return Err("stack diagnostic boundary core differs".into()); }
                let sampled = sample(&self.config, core, pc, event.entry)
                    .map_err(|error| self.cores[core].callback.refusal_context(u64::from(pc), error))?;
                let was_active = self.state.borrow().active(core);
                if was_active {
                    self.cores[core].callback.observe(pc)
                        .map_err(|error| self.cores[core].callback.refusal_context(u64::from(pc), error))?;
                }
                let result = self.state.borrow_mut().boundary(&self.config, &sampled, boundary);
                result.map_err(|error| self.cores[core].callback.refusal_context(u64::from(pc), Refusal::with_word(error, sampled.word)))?;
                self.cores[core].processed = Some(event.entry.id);
                let active = self.state.borrow().active(core);
                if active && self.cores[core].hook.is_none() {
                    let callback = (&*self.cores[core].callback as *const Callback).cast_mut().cast();
                    // Callback storage remains boxed until this hook is removed.
                    let hook = unsafe { session.board.cpu(core)?.install_hook(Hook::Instruction { callback: instruction, data: callback, range: HookRange::All })? };
                    self.cores[core].hook = Some(hook);
                    self.state.borrow_mut().performance.hook_transitions += 1;
                } else if !active {
                    if let Some(hook) = self.cores[core].hook {
                        session.board.cpu(core)?.remove_hook(hook)?;
                        self.cores[core].hook = None;
                        self.cores[core].callback.last.set(None);
                        self.cores[core].callback.previous_source.set(None);
                        self.cores[core].callback.previous_owned.set(None);
                        self.cores[core].callback.previous_bank.set(None);
                        self.cores[core].callback.previous_instruction.set(None);
                        self.cores[core].callback.last_sample.set(None);
                        self.cores[core].callback.pending_replays.borrow_mut().clear();
                        self.cores[core].callback.owned_frames.borrow_mut().clear();
                        self.cores[core].callback.deferred_mode.set(None);
                        *self.cores[core].callback.attribution.borrow_mut() = Attributor::default();
                        self.state.borrow_mut().performance.hook_transitions += 1;
                    }
                }
            }
            self.cores[core].points.resume(u64::from(pc), u64::from(pc));
            self.cores[core].resume = Some((pc, counts[core]));
        }
        Ok(())
    }
    pub fn refuse(&self, error: &str) { self.state.borrow_mut().fail(error.into()); }
    pub fn finished(&self) -> bool { self.finished }
    pub(crate) fn disarm(&mut self, session: &Session) -> Result<(), String> {
        if !self.finished { self.refuse("stack diagnostic instrumentation removed before end"); }
        self.remove_hooks(session)
    }
    pub fn finish(&mut self, session: &Session) -> Result<serde_json::Value, String> {
        if self.finished { return self.report(); }
        if !self.state.borrow().open.is_empty() { self.refuse("stack diagnostic ended with incomplete invocations"); }
        if let Err(error) = self.remove_hooks(session) { self.refuse(&error); return Err(error); }
        self.end = Some(Boundary::sample(session)?);
        self.finished = true;
        self.report()
    }
    fn remove_hooks(&mut self, session: &Session) -> Result<(), String> {
        for core in &self.cores { core.callback.leased.set(false); }
        while let Some(index) = self.cores.len().checked_sub(1) {
            if let Some(hook) = self.cores[index].hook {
                session.board.cpu(index)?.remove_hook(hook)?;
                self.cores[index].hook = None;
                self.state.borrow_mut().performance.hook_transitions += 1;
            }
            session.board.instruction_points_end(index, &mut self.cores[index].points)?;
            self.cores.pop();
        }
        Ok(())
    }
    pub fn report(&self) -> Result<serde_json::Value, String> {
        let state = self.state.borrow();
        Ok(serde_json::json!({"version":1,"diagnostic_only":true,
            "coverage":if state.failure.is_none() && state.open.is_empty() { "continuous" } else { "refused" },
            "failure":state.failure,"incomplete_intervals":!state.open.is_empty(),"active_invocations":state.open.len(),
            "retained_service_coverage":ServiceCoverage::PartialObservedSourceTransitions,"retained_service_spans_informational":true,
            "invocations":state.invocations,"performance":state.performance,"sources":self.config.request.sources,
            "whole_route_stack_minimum_proven":false,"scope":"installed-before-unload","finished":self.finished,
            "role":self.config.request.role,"start":self.start,"end":self.end}))
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        for core in self.cores.drain(..) {
            core.callback.leased.set(false);
            if let Some(hook) = core.hook {
                let removed = self.config.access.cpu(core.callback.core).and_then(|cpu| cpu.remove_hook(hook));
                if removed.is_err() {
                    core.callback.state.borrow_mut().fail("stack diagnostic dropped with an unremoved instruction hook".into());
                    std::mem::forget(core.callback);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BeginRequest, Collector, Role, SourceKind};
    use crate::{machine::{Machine, RunOutcome}, run::{RunDocument, ValidatedUpdate}};
    use fujimod_emulator_cpu::{Register, arm};
    use serde_json::{Value, json};
    use std::{path::PathBuf, sync::atomic::{AtomicU64, Ordering}};

    const TREE: u32 = 0x1164e1c;
    const PICKER: u32 = 0x1163f2c;
    const PANE: u32 = 0x11662a0;
    const ADAPTER: u32 = 0x1200000;
    const RETURN: u32 = 0x1180000;
    const SP: u32 = 0xa4dc60;
    static SERIAL: AtomicU64 = AtomicU64::new(0);

    struct Fixture { machine: Machine, root: PathBuf, sources: Vec<Value>, services: Vec<u32> }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); } }
    impl Fixture {
        fn new() -> Self {
            fujimod_emulator_exr2_board::register_board_builder();
            fujimod_emulator_models::register();
            let document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
            let mut rom = vec![0; 16384];
            rom[..4].copy_from_slice(&0xeafffffeu32.to_le_bytes());
            let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
            let mut session = crate::run::open(&document, &update, &rom).unwrap();
            session.board.set_execution_policy(document.execution_policy.into()).unwrap();
            let machine = Machine::new(document, session).unwrap();
            let root = std::env::temp_dir().join(format!("render-stack-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&root).unwrap();
            let fixture = Self { machine, root, sources: Vec::new(), services: Vec::new() };
            let word = |address, value: u32| fixture.machine.session.board.write_physical(address, &value.to_le_bytes()).unwrap();
            word(0x54760, 0x4006060c);
            word(0x60614, 0x4005c3ac);
            for (index, value) in [0, 0, 0x1109130, 8, 4096, 0xa4ce00, 0, 0, 33, 1].into_iter().enumerate() {
                word(0x5c3ac + index as u64 * 4, value);
            }
            word(0x545a4, 0);
            fixture
        }
        fn image(&mut self, start: u32, words: &[u32], kind: SourceKind) {
            let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
            let path = self.root.join(format!("{start:x}.bin"));
            std::fs::write(&path, &bytes).unwrap();
            self.machine.session.board.write_physical(u64::from(start), &bytes).unwrap();
            self.sources.push(json!({"start":start,"path":path,"sha256":crate::bridge::sha256(&bytes),"kind":kind}));
        }
        fn request(&mut self, role: Role, returns: &[u32]) -> BeginRequest {
            self.image(RETURN, &[0xeafffffe], SourceKind::Retained);
            self.image(RETURN - 4, &[branch(RETURN - 4, TREE, true)], SourceKind::Retained);
            serde_json::from_value(json!({"version":1,"role":role,
                "sources":self.sources,"entries":[
                    {"kind":"tree","address":TREE,"frame_depth":56},
                    {"kind":"picker","address":PICKER,"frame_depth":56},
                    {"kind":"pane","address":PANE,"frame_depth":56}],
                "returns":returns,"retained_services":self.services})).unwrap()
        }
        fn begin(&mut self, role: Role, returns: &[u32]) {
            let request = self.request(role, returns);
            self.machine.begin_render_stack_diagnostic(request).unwrap();
            self.enter();
        }
        fn enter(&self) {
            for (register, value) in [(Register::Pc, TREE), (Register::Arm32(arm::Register::R(13)), SP),
                (Register::Arm32(arm::Register::R(14)), RETURN)] {
                self.machine.session.board.write_register(0, register, u64::from(value)).unwrap();
            }
        }
        fn report(&self) -> Value { self.machine.render_stack_diagnostic.as_ref().unwrap().report().unwrap() }
    }
    fn branch(from: u32, to: u32, link: bool) -> u32 {
        (if link { 0xeb000000 } else { 0xea000000 }) | ((to.wrapping_sub(from + 8) >> 2) & 0x00ffffff)
    }
    fn roots(fixture: &mut Fixture, role: Role) {
        for entry in [TREE, PICKER, PANE] {
            let code = if role == Role::Owned { vec![branch(entry, ADAPTER, false), 0xe8bd8ff0] }
                else { vec![0xe92d4ff0, 0xe24dd020, 0xe28dd020, 0xe8bd8ff0] };
            fixture.image(entry, &code, SourceKind::Retained);
        }
    }
    #[test]
    fn pane_entry_with_a_tree_caller_continuation_refuses() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Stock);
        fixture.begin(Role::Stock, &[RETURN]);
        fixture.machine.session.board.write_register(0, Register::Pc, u64::from(PANE)).unwrap();
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.report()["coverage"], "refused");
    }

    #[test]
    fn stock_interval_reports_actual_stack_demand_and_stops_sampling_after_return() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Stock);
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed);
        let before = fixture.report();
        assert_eq!(before["coverage"], "continuous");
        assert_eq!(before["invocations"][0]["min_sp"], SP - 68);
        assert_eq!(before["invocations"][0]["terminal"], "stock-return");
        assert_eq!(before["invocations"][0]["exit_sp"], SP);
        assert_eq!(before["whole_route_stack_minimum_proven"], false);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed);
        assert_eq!(fixture.report()["performance"]["active_instructions"], before["performance"]["active_instructions"]);
    }
    #[test]
    fn owned_fallback_prefix_includes_adapter_frame_and_recreated_stock_prologue() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        fixture.image(ADAPTER, &[0xe92d5fff, 0xe24dd010, 0xe28dd010, 0xe8bd5fff, 0xe92d4ff0,
            branch(ADAPTER + 20, TREE + 4, false)], SourceKind::Owned);
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"][0]["min_sp"], SP - 72);
        assert_eq!(report["invocations"][0]["exit_sp"], SP - 36);
        assert_eq!(report["invocations"][0]["terminal"], "fallback-handoff");
    }
    #[test]
    fn nested_owned_calls_update_enclosing_minimum_until_the_outer_return() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        fixture.sources.retain(|source| source["start"] != PICKER);
        fixture.image(PICKER, &[branch(PICKER, ADAPTER + 0x100, false), 0xe8bd8ff0], SourceKind::Retained);
        fixture.image(ADAPTER, &[0xe92d5fff, branch(ADAPTER + 4, PICKER, true), 0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
        fixture.image(ADAPTER + 0x100, &[0xe92d5fff, 0xe24dd020, 0xe28dd020, 0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Owned, &[RETURN, ADAPTER + 8]);
        assert_eq!(fixture.machine.run(2000), RunOutcome::Completed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"].as_array().unwrap().len(), 2);
        assert_eq!(report["invocations"][1]["parent"], 1);
        assert_eq!(report["invocations"][0]["min_sp"], SP - 144);
        assert_eq!(report["invocations"][1]["min_sp"], SP - 144);
        assert_eq!(report["invocations"][0]["exit_sp"], SP);
        assert_eq!(report["invocations"][1]["exit_sp"], SP - 56);
        assert_eq!(report["performance"]["hook_transitions"], 2);
    }
    #[test]
    fn missing_instruction_source_and_unfinished_intervals_refuse_the_diagnostic() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        fixture.image(ADAPTER, &[0xe92d5fff, branch(ADAPTER + 4, ADAPTER + 0x100, false)], SourceKind::Owned);
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "refused");
        assert_eq!(report["incomplete_intervals"], true);
        assert!(report["failure"].is_string());
        let machine = &mut fixture.machine;
        let ended = machine.render_stack_diagnostic.as_mut().unwrap().finish(&machine.session).unwrap();
        assert_eq!(ended["coverage"], "refused");
        assert_eq!(ended["finished"], true);
    }
    #[test]
    fn a_wrongly_restored_return_is_not_accepted() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        fixture.image(ADAPTER, &[0xe92d5fff, 0xe8bd5fff, 0xe24dd004, 0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.report()["coverage"], "refused");
        assert!(fixture.report()["invocations"][0]["terminal"].is_null());
    }

    #[test]
    fn irq_mask_changes_do_not_omit_task_stack_demand() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        fixture.image(ADAPTER, &[0xe92d5fff, 0xe10f0000, 0xe3c00080, 0xe121f000,
            0xe24dd010, 0xe3800080, 0xe121f000, 0xe28dd010, 0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Owned, &[RETURN]);
        fixture.machine.traces.arm(&fixture.machine.session, [(ADAPTER + 24, "before-mask".into())].into()).unwrap();
        fixture.machine.traces.stop_labels.insert("before-mask".into());
        assert_eq!(fixture.machine.run(1000), RunOutcome::Stopped);
        let paused = fixture.report();
        assert_eq!(paused["invocations"][0]["deepest_pc"], ADAPTER + 20);
        assert_eq!(paused["invocations"][0]["minimum_context"]["cpsr"].as_u64().unwrap(),
            fixture.machine.session.board.read_register(0, Register::Arm32(arm::Register::Cpsr)).unwrap());
        assert_eq!(paused["invocations"][0]["min_sp"].as_u64().unwrap(),
            fixture.machine.session.board.read_register(0, Register::Arm32(arm::Register::R(13))).unwrap());
        assert_eq!(paused["invocations"][0]["deepest_lr"].as_u64().unwrap(),
            fixture.machine.session.board.read_register(0, Register::Arm32(arm::Register::R(14))).unwrap());
        fixture.machine.traces.stop_labels.clear();
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"][0]["min_sp"], SP - 72);
        let minimum = &report["invocations"][0]["minimum_context"];
        assert_eq!(minimum, &paused["invocations"][0]["minimum_context"]);
        assert_eq!(minimum["cpsr"].as_u64().unwrap() & 128, 0);
        assert_eq!(minimum["cpsr"].as_u64().unwrap() & 31, 0x13);
        assert_eq!(minimum["context_depth"], 0);
        assert_eq!(minimum["core"], 0);
        assert_eq!(minimum["current"]["task"], 33);
        assert_eq!(minimum["owner"], minimum["current"]);
        assert_eq!(minimum["scheduler"], false);
        assert_eq!(minimum["coordinate"]["kind"], "observation");
        let ordinal = minimum["coordinate"]["ordinal"].as_u64().unwrap();
        assert!(ordinal > 0 && ordinal < report["performance"]["active_instructions"].as_u64().unwrap());
        let transitions = report["invocations"][0]["transitions"].as_array().unwrap();
        assert!(transitions.iter().any(|row| row["cpsr"].as_u64().unwrap() & 128 == 0));
        assert!(transitions.iter().any(|row| row["cpsr"].as_u64().unwrap() & 128 != 0));
    }

    #[test]
    fn bound_core_private_tcm_code_does_not_use_underlying_dram_bytes() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        let words = [0xe92d5fffu32, 0xe8bd5fff, 0xe12fff1e];
        fixture.image(ADAPTER, &words, SourceKind::Owned);
        fixture.machine.session.board.cpu(0).unwrap().coprocessor(arm::Coprocessor {
            cp:15, opc1:0, crn:9, crm:1, opc2:1, secure:0, wide:false,
        }, Some(u64::from(ADAPTER | 0xd))).unwrap();
        let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        fixture.machine.session.board.write_memory(0, u64::from(ADAPTER), &bytes).unwrap();
        fixture.machine.session.board.write_physical(u64::from(ADAPTER), &[0xaa; 12]).unwrap();
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed);
        assert_eq!(fixture.report()["coverage"], "continuous");
        assert_eq!(fixture.report()["invocations"][0]["min_sp"], SP - 56);
    }

    #[test]
    fn nesting_bound_refuses_without_discarding_the_covered_prefix() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        fixture.image(ADAPTER, &[0xe92d5fff, branch(ADAPTER + 4, TREE, true), 0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Owned, &[RETURN, ADAPTER + 8]);
        assert_eq!(fixture.machine.run(10000), RunOutcome::Failed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "refused");
        assert_eq!(report["active_invocations"], 32);
        assert_eq!(report["invocations"].as_array().unwrap().len(), 32);
        assert!(report["failure"].is_string());
    }

    #[test]
    fn stopping_and_resuming_an_unentered_instruction_preserves_interval_evidence() {
        let prepare = || {
            let mut fixture = Fixture::new();
            roots(&mut fixture, Role::Owned);
            fixture.image(ADAPTER, &[0xe92d5fff, 0xe24dd010, 0xe28dd010, 0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
            fixture.begin(Role::Owned, &[RETURN]);
            fixture
        };
        let mut uninterrupted = prepare();
        assert_eq!(uninterrupted.machine.run(2000), RunOutcome::Completed);
        let mut stopped = prepare();
        stopped.machine.traces.arm(&stopped.machine.session, [(ADAPTER + 4, "pause".into())].into()).unwrap();
        stopped.machine.traces.stop_labels.insert("pause".into());
        assert_eq!(stopped.machine.run(1000), RunOutcome::Stopped);
        assert_eq!(stopped.report()["coverage"], "refused");
        stopped.machine.traces.stop_labels.clear();
        assert_eq!(stopped.machine.run(2000), RunOutcome::Completed);
        assert_eq!(stopped.report()["invocations"], uninterrupted.report()["invocations"]);
        assert_eq!(stopped.report()["performance"]["active_instructions"], uninterrupted.report()["performance"]["active_instructions"]);
    }

    #[test]
    fn ending_the_diagnostic_removes_sparse_points_before_source_changes() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Stock);
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed);
        let machine = &mut fixture.machine;
        let final_report = machine.render_stack_diagnostic.as_mut().unwrap().finish(&machine.session).unwrap();
        assert_eq!(final_report["coverage"], "continuous");
        assert_eq!(final_report["finished"], true);
        machine.session.board.write_physical(u64::from(TREE), &0xeafffffeu32.to_le_bytes()).unwrap();
        machine.session.board.write_register(0, Register::Pc, u64::from(TREE)).unwrap();
        assert_eq!(machine.run(1000), RunOutcome::Completed);
        let repeated = machine.render_stack_diagnostic.as_mut().unwrap().finish(&machine.session).unwrap();
        assert_eq!(repeated["end"], final_report["end"]);
        assert_eq!(fixture.report()["invocations"], final_report["invocations"]);
    }

    #[test]
    fn diagnostic_evidence_cannot_be_reused_on_another_board() {
        let mut original = Fixture::new();
        roots(&mut original, Role::Stock);
        original.begin(Role::Stock, &[RETURN]);
        let mut other = Fixture::new();
        other.machine.render_stack_diagnostic = original.machine.render_stack_diagnostic.take();
        assert_eq!(other.machine.run(1000), RunOutcome::Failed);
        assert_eq!(other.report()["coverage"], "refused");
        assert_eq!(other.report()["performance"]["active_instructions"], 0);
    }

    #[test]
    fn interval_markers_match_trace_hit_global_instruction_counts() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Stock);
        fixture.begin(Role::Stock, &[RETURN]);
        fixture.machine.traces.arm(&fixture.machine.session, [(TREE, "root".into()), (RETURN, "caller".into())].into()).unwrap();
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed);
        let report = fixture.report();
        let hits = &fixture.machine.traces.hits;
        assert_eq!(report["role"], "stock");
        assert_eq!(report["invocations"][0]["entry_boundary"]["instructions"], hits.iter().find(|hit| hit.site == "root").unwrap().instructions);
        assert_eq!(report["invocations"][0]["terminal_boundary"]["instructions"], hits.iter().find(|hit| hit.site == "caller").unwrap().instructions);
        assert!(report["end"].is_null());
        let machine = &mut fixture.machine;
        let ended = machine.render_stack_diagnostic.as_mut().unwrap().finish(&machine.session).unwrap();
        assert_eq!(ended["end"]["instructions"], machine.instructions());
        assert!(ended["start"]["instructions"].as_u64().unwrap() <= ended["end"]["instructions"].as_u64().unwrap());
    }
    fn vector_call(fixture: &mut Fixture, switch: bool) {
        roots(fixture, Role::Stock);
        fixture.sources.retain(|source| source["start"] != TREE);
        fixture.image(TREE, &[0xe92d4ff0, branch(TREE + 4, 0x18, true), 0xe8bd8ff0], SourceKind::Retained);
        fixture.image(0x18, &[0xe12fff1e], SourceKind::Retained);
        fixture.machine.session.board.write_physical(0x80000018, &0xe12fff1eu32.to_le_bytes()).unwrap();
        fixture.machine.session.board.write_physical(0x40000018, &0xe12fff1eu32.to_le_bytes()).unwrap();
        if switch {
            fixture.image(ADAPTER, &[0xe3081008, 0xe34f1ff9, 0xe3a00001, 0xe5810000,
                branch(ADAPTER + 16, TREE, false)], SourceKind::Retained);
        }
    }
    fn enter_decoder_setup(fixture: &Fixture) {
        fixture.machine.session.board.write_register(0, Register::Pc, u64::from(ADAPTER)).unwrap();
    }

    #[test]
    fn decoder_selected_canonical_backing_preserves_complete_invocation_evidence() {
        let mut fixture = Fixture::new();
        vector_call(&mut fixture, true);
        fixture.machine.session.board.write_physical(0x80000018, &0xeafffffeu32.to_le_bytes()).unwrap();
        fixture.begin(Role::Stock, &[RETURN]);
        enter_decoder_setup(&fixture);
        let outcome = fixture.machine.run(1000);
        assert_eq!(outcome, RunOutcome::Completed, "machine {:?}; diagnostic {}", fixture.machine.failure, fixture.report());
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"][0]["terminal"], "stock-return");
        assert_eq!(report["invocations"][0]["exit_sp"], SP);
    }

    #[test]
    fn identical_rom_bytes_do_not_admit_noncanonical_backing() {
        let mut fixture = Fixture::new();
        vector_call(&mut fixture, false);
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.machine.session.board.read_register(0, Register::Pc).unwrap(), 0x18);
        assert_eq!(fixture.report()["coverage"], "refused");
    }

    #[test]
    fn canonical_backing_without_admitted_decoder_mapping_refuses() {
        let mut fixture = Fixture::new();
        vector_call(&mut fixture, true);
        let (_, mut graph) = crate::run::graph(&fixture.machine.document).unwrap();
        graph.decoders.clear();
        let request = fixture.request(Role::Stock, &[RETURN]);
        fixture.machine.render_stack_diagnostic = Some(Collector::begin(&fixture.machine.session, &graph, request).unwrap());
        fixture.enter();
        enter_decoder_setup(&fixture);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.machine.session.board.read_register(0, Register::Pc).unwrap(), 0x18);
        assert_eq!(fixture.report()["coverage"], "refused");
    }

    #[test]
    fn canonical_decoder_backing_must_match_committed_instruction_bytes() {
        let mut fixture = Fixture::new();
        vector_call(&mut fixture, true);
        fixture.begin(Role::Stock, &[RETURN]);
        fixture.machine.session.board.write_physical(0x40000018, &0xeafffffeu32.to_le_bytes()).unwrap();
        enter_decoder_setup(&fixture);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.machine.session.board.read_register(0, Register::Pc).unwrap(), 0x18);
        assert_eq!(fixture.report()["coverage"], "refused");
    }
    #[test]
    fn ambiguous_complete_decoder_mapping_refuses_before_guest_progress() {
        let mut fixture = Fixture::new();
        vector_call(&mut fixture, true);
        let (_, mut graph) = crate::run::graph(&fixture.machine.document).unwrap();
        let choice = graph.decoders.iter_mut().flat_map(|decoder| &mut decoder.choices)
            .find(|choice| !choice.processors.is_empty()).unwrap();
        let duplicate = choice.processors[0].clone();
        choice.processors.push(duplicate);
        let request = fixture.request(Role::Stock, &[RETURN]);
        assert!(Collector::begin(&fixture.machine.session, &graph, request).is_err());
        assert_eq!(fixture.machine.instructions(), 0);
    }
    #[test]
    fn canonical_backing_reads_bypass_host_aliases_and_refuse_overflow_without_copy() {
        let fixture = Fixture::new();
        fixture.machine.session.board.write_physical(0x80000018, &0xe12fff1eu32.to_le_bytes()).unwrap();
        fixture.machine.session.board.write_physical(0x40000018, &0xeafffffeu32.to_le_bytes()).unwrap();
        let context = fixture.machine.session.board.observation_context();
        let mut word = [0; 4];
        context.read_physical(0x18, &mut word).unwrap();
        assert_eq!(u32::from_le_bytes(word), 0xe12fff1e);
        context.read_backing(0x18, &mut word).unwrap();
        assert_eq!(u32::from_le_bytes(word), 0xeafffffe);
        let before = word;
        assert!(context.read_backing(u64::MAX - 1, &mut word).is_err());
        assert_eq!(word, before);
        assert!(context.read_backing(0x40000018, &mut word).is_err());
        assert_eq!(word, before);
        let (_, graph) = crate::run::graph(&fixture.machine.document).unwrap();
        let region = graph.ram.iter().find(|region| region.base == 0).unwrap();
        assert!(context.read_backing(region.size as u64 - 2, &mut word).is_err());
        assert_eq!(word, before);
    }
    const SERVICE: u32 = 0x1190000;
    const INNER_SERVICE: u32 = SERVICE + 0x100;
    enum NativeTransfer { Call, Tail }
    fn service_bodies(fixture: &mut Fixture, transfer: NativeTransfer) {
        let outer = match transfer {
            NativeTransfer::Call => vec![0xe92d4010, branch(SERVICE + 4, INNER_SERVICE, true), 0xe8bd8010],
            NativeTransfer::Tail => vec![branch(SERVICE, INNER_SERVICE, false)],
        };
        fixture.image(SERVICE, &outer, SourceKind::Retained);
        fixture.image(INNER_SERVICE, &[0xe92d4010, 0xe24dd020, 0xe28dd020, 0xe8bd8010], SourceKind::Retained);
        fixture.services = vec![SERVICE, INNER_SERVICE];
    }
    fn owned_service(transfer: NativeTransfer) -> Fixture {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        service_bodies(&mut fixture, transfer);
        fixture.image(ADAPTER, &[0xe92d5fff, branch(ADAPTER + 4, SERVICE, true), 0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
        fixture
    }

    #[test]
    fn stock_native_calls_do_not_spend_owned_transition_metadata_capacity() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Stock);
        service_bodies(&mut fixture, NativeTransfer::Call);
        fixture.sources.retain(|source| source["start"] != TREE);
        fixture.image(TREE, &[0xe92d4ff0, 0xe3014004, branch(TREE + 8, SERVICE, true), 0xe2544001,
            (branch(TREE + 16, TREE + 8, false) & 0x0fffffff) | 0x10000000, 0xe8bd8ff0], SourceKind::Retained);
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(300000), RunOutcome::Completed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["retained_service_coverage"], "partial-observed-source-transitions");
        assert_eq!(report["retained_service_spans_informational"], true);
        assert_eq!(report["invocations"][0]["retained_spans"], json!([]));
        assert_eq!(report["invocations"][0]["min_sp"], SP - 84);
    }

    #[test]
    fn observed_owned_transition_includes_nested_native_stack_demand_without_extra_spans() {
        let mut fixture = owned_service(NativeTransfer::Call);
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(2000), RunOutcome::Completed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"][0]["min_sp"], SP - 104);
        let spans = report["invocations"][0]["retained_spans"].as_array().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0]["observed_owned_pc"], ADAPTER + 4);
        assert_eq!(spans[0]["entry_pc"], SERVICE);
        assert_eq!(spans[0]["min_sp"], SP - 104);
        assert_eq!(spans[0]["exit_sp"], SP - 56);
        assert_eq!(spans[0]["minimum_context"], report["invocations"][0]["minimum_context"]);
        assert_eq!(spans[0]["minimum_context"]["owner"], report["invocations"][0]["task_identity"]);
    }

    #[test]
    fn retained_tail_call_preserving_owned_lr_does_not_create_another_observed_transition() {
        let mut fixture = owned_service(NativeTransfer::Tail);
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(2000), RunOutcome::Completed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"][0]["min_sp"], SP - 96);
        let spans = report["invocations"][0]["retained_spans"].as_array().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0]["entry_pc"], SERVICE);
        assert_eq!(spans[0]["return_pc"], ADAPTER + 8);
        assert_eq!(spans[0]["exit_sp"], SP - 56);
    }

    #[test]
    fn a_recorded_unfinished_span_still_refuses_an_otherwise_restored_invocation_return() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Owned);
        fixture.image(ADAPTER, &[0xe92d5fff, branch(ADAPTER + 4, SERVICE, true), 0xe320f000, 0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
        fixture.image(SERVICE, &[branch(SERVICE, ADAPTER + 12, false)], SourceKind::Retained);
        fixture.services = vec![SERVICE];
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(2000), RunOutcome::Failed);
        assert_eq!(fixture.machine.session.board.read_register(0, Register::Arm32(arm::Register::R(13))).unwrap(), u64::from(SP));
        let report = fixture.report();
        assert_eq!(report["coverage"], "refused");
        assert!(report["invocations"][0]["retained_spans"][0]["exit_sp"].is_null());
    }

    #[test]
    fn actual_owned_transition_overflow_preserves_the_fixed_record_bound() {
        let mut fixture = owned_service(NativeTransfer::Call);
        fixture.sources.retain(|source| source["start"] != ADAPTER);
        fixture.image(ADAPTER, &[0xe92d5fff, 0xe3014004, branch(ADAPTER + 8, SERVICE, true), 0xe2544001,
            (branch(ADAPTER + 16, ADAPTER + 8, false) & 0x0fffffff) | 0x10000000,
            0xe8bd5fff, 0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Owned, &[RETURN]);
        assert_eq!(fixture.machine.run(300000), RunOutcome::Failed);
        let report = fixture.report();
        assert_eq!(report["coverage"], "refused");
        assert_eq!(report["invocations"][0]["retained_spans"].as_array().unwrap().len(), 4096);
    }

    #[test]
    fn unadmitted_owned_transfer_and_unknown_current_source_still_refuse() {
        let mut unadmitted = owned_service(NativeTransfer::Call);
        unadmitted.services.clear();
        unadmitted.begin(Role::Owned, &[RETURN]);
        assert_eq!(unadmitted.machine.run(2000), RunOutcome::Failed);
        assert_eq!(unadmitted.report()["coverage"], "refused");
        let mut unknown = owned_service(NativeTransfer::Call);
        unknown.sources.retain(|source| source["start"] != INNER_SERVICE);
        unknown.image(INNER_SERVICE, &[0xe92d4010], SourceKind::Retained);
        unknown.begin(Role::Owned, &[RETURN]);
        assert_eq!(unknown.machine.run(2000), RunOutcome::Failed);
        assert_eq!(unknown.report()["coverage"], "refused");
    }

    fn retained_caller(fixture: &mut Fixture) {
        roots(fixture, Role::Stock);
        fixture.sources.retain(|source| source["start"] != TREE);
        fixture.image(TREE, &[0xe92d4ff0, branch(TREE + 4, ADAPTER, true), 0xe8bd8ff0], SourceKind::Retained);
    }

    #[test]
    fn owned_direct_return_to_the_retained_call_continuation_is_bound() {
        let mut fixture = Fixture::new();
        retained_caller(&mut fixture);
        fixture.image(ADAPTER, &[0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed, "{}", fixture.report());
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"][0]["terminal"], "stock-return");
    }

    #[test]
    fn owned_return_after_a_nested_retained_service_keeps_the_caller_continuation() {
        let mut fixture = Fixture::new();
        retained_caller(&mut fixture);
        fixture.image(SERVICE, &[0xe12fff1e], SourceKind::Retained);
        fixture.image(ADAPTER, &[0xe92d4010, branch(ADAPTER + 4, SERVICE, true), 0xe8bd8010], SourceKind::Owned);
        fixture.services = vec![SERVICE];
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Completed, "{}", fixture.report());
        let report = fixture.report();
        assert_eq!(report["coverage"], "continuous");
        assert_eq!(report["invocations"][0]["terminal"], "stock-return");
        assert_eq!(report["invocations"][0]["retained_spans"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn ended_owned_call_continuation_is_not_reused_by_a_later_tail_entry() {
        let mut fixture = Fixture::new();
        retained_caller(&mut fixture);
        fixture.sources.retain(|source| source["start"] != TREE);
        fixture.image(TREE, &[0xe92d4ff0, branch(TREE + 4, ADAPTER, true),
            branch(TREE + 8, ADAPTER + 0x100, false)], SourceKind::Retained);
        fixture.image(ADAPTER, &[0xe12fff1e], SourceKind::Owned);
        fixture.image(ADAPTER + 0x100, &[0xe304ee24, 0xe340e116, 0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.report()["coverage"], "refused");
    }

    #[test]
    fn nested_owned_return_to_the_outer_continuation_still_refuses() {
        let mut fixture = Fixture::new();
        retained_caller(&mut fixture);
        fixture.image(SERVICE, &[branch(SERVICE, INNER_SERVICE, true), 0xe12fff1e], SourceKind::Retained);
        fixture.image(INNER_SERVICE, &[0xe304ee24, 0xe340e116, 0xe12fff1e], SourceKind::Owned);
        fixture.image(ADAPTER, &[0xe92d4010, branch(ADAPTER + 4, SERVICE, true), 0xe8bd8010], SourceKind::Owned);
        fixture.services = vec![SERVICE];
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.report()["coverage"], "refused");
    }

    #[test]
    fn owned_transfer_to_a_retained_address_loaded_into_lr_still_refuses() {
        let mut fixture = Fixture::new();
        retained_caller(&mut fixture);
        fixture.image(ADAPTER, &[0xe306e2a4, 0xe340e116, 0xe12fff1e], SourceKind::Owned);
        fixture.begin(Role::Stock, &[RETURN]);
        assert_eq!(fixture.machine.run(1000), RunOutcome::Failed);
        assert_eq!(fixture.report()["coverage"], "refused");
    }
    #[test]
    fn initial_floor_context_retains_its_entry_boundary_without_inventing_an_owner() {
        let mut fixture = Fixture::new();
        roots(&mut fixture, Role::Stock);
        fixture.begin(Role::Stock, &[RETURN]);
        fixture.machine.traces.arm(&fixture.machine.session, [(TREE, "entry".into())].into()).unwrap();
        fixture.machine.traces.stop_labels.insert("entry".into());
        assert_eq!(fixture.machine.run(1000), RunOutcome::Stopped);
        let report = fixture.report();
        let invocation = &report["invocations"][0];
        assert_eq!(invocation["min_sp"], SP);
        assert_eq!(invocation["minimum_context"]["coordinate"]["kind"], "entry-boundary");
        assert_eq!(invocation["minimum_context"]["coordinate"]["boundary"], invocation["entry_boundary"]);
        assert!(invocation["minimum_context"]["owner"].is_null());
        assert!(invocation["minimum_context"]["scheduler"].is_null());
        assert_eq!(invocation["minimum_context"]["current"], invocation["task_identity"]);
        assert_eq!(invocation["minimum_context"]["cpsr"].as_u64().unwrap(),
            fixture.machine.session.board.read_register(0, Register::Arm32(arm::Register::Cpsr)).unwrap());
    }


}
