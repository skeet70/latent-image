use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fmt};

const EVENT_LIMIT: usize = 32768;
const LIVE_LIMIT: usize = 4096;
const PENDING_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Address(pub u32);
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TaskId(pub u32);
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Core(pub usize);
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Counter(pub u64);
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AllocationId(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ContextKey {
    pub domain: Core,
    pub task: TaskId,
    pub task_record: Address,
    pub mode: u8,
    pub exception_depth: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation { Malloc, Free, Realloc, Calloc, Trim, Sbrk, Lock, Unlock }

impl Operation {
    fn lifetime(self) -> bool {
        matches!(self, Self::Malloc | Self::Free | Self::Realloc | Self::Calloc)
    }
}

pub fn entry(address: Address) -> Option<Operation> {
    match address.0 {
        0x89cde4 => Some(Operation::Malloc),
        0x89e560 => Some(Operation::Free),
        0x89da64 => Some(Operation::Realloc),
        0xbf1c44 => Some(Operation::Calloc),
        0x89e464 => Some(Operation::Trim),
        0x72b110 => Some(Operation::Sbrk),
        0x72b164 => Some(Operation::Lock),
        0x72b1cc => Some(Operation::Unlock),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    pub pc: Address,
    pub core: Core,
    pub context: ContextKey,
    pub registers: [u32; 16],
    pub cpsr: u32,
    pub counts: [u64; 2],
    pub errno: Option<u32>,
    pub chunk: Option<u32>,
    pub heap_break: u32,
    pub owner: u32,
    pub depth: u32,
}

pub const IMAGE5_LOAD_POINTS: [u32; 6] = [0x6a96c0, 0x6ae360, 0x6a97bc, 0x6a9840, 0x69d2e0, 0xd985a4];
pub const IMAGE5_HEAP_CODE: (u32, u32) = (0xbf1c1c, 0xbf1cd4);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Image5Phase { Unloaded, Installing, Admitted }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Image5CodeStatus { NotSampled, Canonical }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Image5Decoder { NotConfigured, Active, Drained }
#[derive(Clone, Copy, Debug)]
enum Image5Installation {
    Unloaded,
    Installing { context: ContextKey, entry_sp: Address, decoder: Image5Decoder },
    Admitted,
}
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Image5Failure { Order, Context, Frame, DecoderArguments, ProducerStatus, CodeIdentity, PrepublicationFetch }
pub struct Image5Lifecycle { state: Image5Installation }
impl Default for Image5Lifecycle {
    fn default() -> Self { Self { state: Image5Installation::Unloaded } }
}
impl Image5Lifecycle {
    pub fn phase(&self) -> Image5Phase {
        match self.state {
            Image5Installation::Unloaded => Image5Phase::Unloaded,
            Image5Installation::Installing { .. } => Image5Phase::Installing,
            Image5Installation::Admitted => Image5Phase::Admitted,
        }
    }
    pub fn check_fetch(&self, address: Address) -> Result<(), Image5Failure> {
        let pc = address.0 & !0x40000000;
        if ((IMAGE5_HEAP_CODE.0..IMAGE5_HEAP_CODE.1).contains(&pc) || pc == 0xd985a4) && self.phase() != Image5Phase::Admitted {
            return Err(Image5Failure::PrepublicationFetch);
        }
        Ok(())
    }
    pub fn observe(&mut self, snapshot: &Snapshot, code: Image5CodeStatus) -> Result<(), Image5Failure> {
        let pc = snapshot.pc.0 & !0x40000000;
        self.check_fetch(snapshot.pc)?;
        match pc {
            0x6a96c0 => {
                if self.phase() != Image5Phase::Unloaded { return Err(Image5Failure::Order); }
                if snapshot.registers[14] != 0x73b4b0 { return Err(Image5Failure::Frame); }
                self.state = Image5Installation::Installing { context: snapshot.context, entry_sp: Address(snapshot.registers[13]), decoder: Image5Decoder::NotConfigured };
            }
            0x6ae360 | 0x6a97bc | 0x6a9840 => {
                let Image5Installation::Installing { context, entry_sp, decoder } = &mut self.state else { return Err(Image5Failure::Order); };
                if snapshot.context != *context { return Err(Image5Failure::Context); }
                match pc {
                    0x6ae360 => {
                        if *decoder != Image5Decoder::NotConfigured { return Err(Image5Failure::Order); }
                        if snapshot.registers[0] != 0xbf1a00 || snapshot.registers[1] != 0x43e40000 || snapshot.registers[14] != 0x6a977c {
                            return Err(Image5Failure::DecoderArguments);
                        }
                        *decoder = Image5Decoder::Active;
                    }
                    0x6a97bc => {
                        if *decoder != Image5Decoder::Active { return Err(Image5Failure::Order); }
                        *decoder = Image5Decoder::Drained;
                    }
                    _ => {
                        if *decoder != Image5Decoder::Drained { return Err(Image5Failure::Order); }
                        if entry_sp.0.checked_sub(28) != Some(snapshot.registers[13]) { return Err(Image5Failure::Frame); }
                        if snapshot.registers[0] != 0 { return Err(Image5Failure::ProducerStatus); }
                        if code != Image5CodeStatus::Canonical { return Err(Image5Failure::CodeIdentity); }
                        self.state = Image5Installation::Admitted;
                    }
                }
            }
            0x69d2e0 => {
                if self.phase() != Image5Phase::Admitted { return Err(Image5Failure::Order); }
                if snapshot.registers[3] != 0xd985a4 { return Err(Image5Failure::Frame); }
                if code != Image5CodeStatus::Canonical { return Err(Image5Failure::CodeIdentity); }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FreeKind { Client, Nested, Internal, Null }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Classification {
    Allocation { logical_invocation: u64, logical: bool },
    Free { pointer: Address, free_kind: FreeKind },
    Helper,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Boundary {
    Enter { invocation: u64, operation: Operation, parent: Option<u64> },
    Return { invocation: u64, operation: Operation, entry: Snapshot },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    pub sequence: Counter,
    pub snapshot: Snapshot,
    pub boundary: Boundary,
    pub classification: Classification,
    pub active_context: [Option<ContextKey>; 2],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Failure {
    EventLimit { limit: usize },
    LiveAllocationLimit { limit: usize },
    PendingLimit { limit: usize },
    InvalidCore { core: Core },
    ContextMode { context: ContextKey, cpsr: u32 },
    BackwardsClock { core: Core, previous: Counter, observed: Counter },
    StalledClock { counts: [u64; 2] },
    ClockOverflow { counts: [u64; 2] },
    UnexpectedPc { pc: Address, context: ContextKey },
    NonTopReturn { invocation: u64, top: u64, context: ContextKey },
    ForeignFree { pointer: Address },
    ForeignRealloc { pointer: Address },
    AllocationChanged { pointer: Address },
    DuplicateAllocation { pointer: Address },
    MissingChunk { pointer: Address, operation: Operation },
    InvalidChunk { pointer: Address, requested: Counter, usable: Counter },
    InvalidPointer { pointer: Address },
    RequestOverflow { count: u32, element_size: u32 },
    ReallocLostOld { pointer: Address },
    Incomplete { pending: usize },
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Failure {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Allocation {
    pub identity: AllocationId,
    pub address: Address,
    pub requested: Counter,
    pub usable: Counter,
    pub logical_invocation: u64,
    pub context: ContextKey,
    pub core: Core,
    pub created: Counter,
    pub updated: Counter,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct TaskKey {
    pub domain: Core,
    pub task: TaskId,
    pub task_record: Address,
}

impl From<ContextKey> for TaskKey {
    fn from(context: ContextKey) -> Self {
        Self { domain: context.domain, task: context.task, task_record: context.task_record }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Mix {
    pub calls: BTreeMap<Operation, Counter>,
    pub successful_calls: Counter,
    pub failed_calls: Counter,
    pub requested_bytes: Counter,
    pub live_allocations: Counter,
    pub requested_live: Counter,
    pub usable_live: Counter,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskMix { pub task: TaskKey, pub mix: Mix }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoreMix { pub core: Core, pub mix: Mix }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PendingInvocation {
    pub invocation: u64,
    pub operation: Operation,
    pub parent: Option<u64>,
    pub entry: Snapshot,
    pub classification: Classification,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Summary {
    pub live_allocations: Vec<Allocation>,
    pub requested_live: Counter,
    pub usable_live: Counter,
    pub requested_peak: Counter,
    pub usable_peak: Counter,
    pub backing_initial_break: Option<Address>,
    pub backing_highwater_break: Option<Address>,
    pub backing_highwater: Counter,
    pub logical_calls: BTreeMap<Operation, Counter>,
    pub per_task: Vec<TaskMix>,
    pub per_core: Vec<CoreMix>,
    pub pending: Vec<PendingInvocation>,
    pub partial: bool,
    pub failure: Option<Failure>,
    pub events: Counter,
    pub counts: Option<[u64; 2]>,
}

#[derive(Clone, Copy)]
struct Origin {
    invocation: u64,
    identity: AllocationId,
    context: ContextKey,
    core: Core,
    created: Counter,
}

struct Frame {
    invocation: u64,
    operation: Operation,
    parent: Option<u64>,
    entry: Snapshot,
    classification: Classification,
    logical: bool,
    origin: Origin,
    requested: u64,
    old: Option<Allocation>,
    old_released: bool,
    freed_identity: Option<AllocationId>,
}

impl Frame {
    fn matches(&self, pc: Address, sp: Address) -> bool {
        self.entry.registers[14] == pc.0 && self.entry.registers[13] == sp.0
    }

    fn pending(&self) -> PendingInvocation {
        PendingInvocation {
            invocation: self.invocation, operation: self.operation, parent: self.parent,
            entry: self.entry.clone(), classification: self.classification,
        }
    }
}

pub struct Recorder {
    events: Vec<Event>,
    pending: BTreeMap<ContextKey, Vec<Frame>>,
    pending_count: usize,
    active_context: [Option<ContextKey>; 2],
    live: BTreeMap<Address, Allocation>,
    requested_live: u64,
    usable_live: u64,
    requested_peak: u64,
    usable_peak: u64,
    initial_break: Option<u32>,
    high_break: Option<u32>,
    counts: Option<[u64; 2]>,
    next_invocation: u64,
    logical_calls: BTreeMap<Operation, Counter>,
    per_task: BTreeMap<TaskKey, Mix>,
    per_core: [Mix; 2],
    failure: Option<Failure>,
}

impl Default for Recorder {
    fn default() -> Self { Self::new() }
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            events: Vec::new(), pending: BTreeMap::new(), pending_count: 0,
            active_context: [None; 2], live: BTreeMap::new(),
            requested_live: 0, usable_live: 0, requested_peak: 0, usable_peak: 0,
            initial_break: None, high_break: None, counts: None, next_invocation: 1,
            logical_calls: BTreeMap::new(), per_task: BTreeMap::new(),
            per_core: std::array::from_fn(|_| Mix::default()), failure: None,
        }
    }

    pub fn observe(&mut self, snapshot: Snapshot) -> Result<(), Failure> {
        if let Some(failure) = &self.failure { return Err(failure.clone()); }
        let result = self.observe_boundary(snapshot);
        if let Err(failure) = &result { self.failure = Some(failure.clone()); }
        result
    }

    pub fn records(&self) -> &[Event] { &self.events }
    pub fn pending_count(&self) -> usize { self.pending_count }

    pub fn returns(&self) -> BTreeSet<Address> {
        self.pending.values().flatten().map(|frame| Address(frame.entry.registers[14])).collect()
    }

    pub fn live_headers(&self) -> Vec<Address> {
        self.live.keys().map(|pointer| Address(pointer.0 - 4)).collect()
    }

    pub fn return_operation(&self, context: ContextKey, pc: Address, sp: Address) -> Option<Operation> {
        self.return_frame(context, pc, sp).map(|frame| frame.operation)
    }

    pub fn return_context(&self, context: ContextKey, pc: Address, sp: Address) -> Option<Address> {
        self.return_frame(context, pc, sp).map(|frame| Address(frame.entry.registers[0]))
    }

    fn return_frame(&self, context: ContextKey, pc: Address, sp: Address) -> Option<&Frame> {
        self.pending.get(&context)?.last().filter(|frame| frame.matches(pc, sp))
    }

    pub fn finish(&self) -> Result<Summary, Failure> {
        if let Some(failure) = &self.failure { return Err(failure.clone()); }
        if self.pending_count != 0 { return Err(Failure::Incomplete { pending: self.pending_count }); }
        Ok(self.summary())
    }

    pub fn summary(&self) -> Summary {
        let mut tasks = self.per_task.clone();
        let mut cores = self.per_core.clone();
        for allocation in self.live.values() {
            let task_mix = tasks.entry(allocation.context.into()).or_default();
            for mix in [task_mix, &mut cores[allocation.core.0]] {
                mix.live_allocations.0 += 1;
                mix.requested_live.0 += allocation.requested.0;
                mix.usable_live.0 += allocation.usable.0;
            }
        }
        let mut pending: Vec<_> = self.pending.values().flatten().map(Frame::pending).collect();
        pending.sort_by_key(|frame| frame.invocation);
        Summary {
            live_allocations: self.live.values().cloned().collect(),
            requested_live: Counter(self.requested_live), usable_live: Counter(self.usable_live),
            requested_peak: Counter(self.requested_peak), usable_peak: Counter(self.usable_peak),
            backing_initial_break: self.initial_break.map(Address),
            backing_highwater_break: self.high_break.map(Address),
            backing_highwater: Counter(match (self.initial_break, self.high_break) {
                (Some(initial), Some(high)) => u64::from(high - initial),
                _ => 0,
            }),
            logical_calls: self.logical_calls.clone(),
            per_task: tasks.into_iter().map(|(task, mix)| TaskMix { task, mix }).collect(),
            per_core: cores.into_iter().enumerate().map(|(core, mix)| CoreMix { core: Core(core), mix }).collect(),
            pending, partial: self.pending_count != 0 || self.failure.is_some(),
            failure: self.failure.clone(), events: Counter(self.events.len() as u64), counts: self.counts,
        }
    }

    fn observe_boundary(&mut self, snapshot: Snapshot) -> Result<(), Failure> {
        self.validate_snapshot(&snapshot)?;
        self.counts = Some(snapshot.counts);
        self.initial_break.get_or_insert(snapshot.heap_break);
        self.high_break = Some(self.high_break.map_or(snapshot.heap_break, |high| high.max(snapshot.heap_break)));
        self.active_context[snapshot.core.0] = Some(snapshot.context);
        let sp = Address(snapshot.registers[13]);
        if self.return_frame(snapshot.context, snapshot.pc, sp).is_some() {
            return self.leave(snapshot);
        }
        if let Some(stack) = self.pending.get(&snapshot.context) {
            if let Some(older) = stack.iter().find(|frame| frame.matches(snapshot.pc, sp)) {
                return Err(Failure::NonTopReturn {
                    invocation: older.invocation, top: stack.last().map_or(older.invocation, |top| top.invocation),
                    context: snapshot.context,
                });
            }
        }
        if let Some(operation) = entry(snapshot.pc) { return self.enter(snapshot, operation); }
        Ok(())
    }

    fn validate_snapshot(&self, snapshot: &Snapshot) -> Result<(), Failure> {
        if snapshot.core.0 >= 2 { return Err(Failure::InvalidCore { core: snapshot.core }); }
        if u32::from(snapshot.context.mode) != snapshot.cpsr & 31 {
            return Err(Failure::ContextMode { context: snapshot.context, cpsr: snapshot.cpsr });
        }
        let observed = snapshot.counts[0].checked_add(snapshot.counts[1])
            .ok_or(Failure::ClockOverflow { counts: snapshot.counts })?;
        if let Some(previous) = self.counts {
            for (core, (before, after)) in previous.into_iter().zip(snapshot.counts).enumerate() {
                if after < before {
                    return Err(Failure::BackwardsClock { core: Core(core), previous: Counter(before), observed: Counter(after) });
                }
            }
            if observed == previous[0] + previous[1] {
                return Err(Failure::StalledClock { counts: snapshot.counts });
            }
        }
        Ok(())
    }

    fn record(&mut self, snapshot: Snapshot, boundary: Boundary, classification: Classification) {
        self.events.push(Event {
            sequence: Counter(self.events.len() as u64 + 1), snapshot, boundary, classification,
            active_context: self.active_context,
        });
    }

    fn enter(&mut self, snapshot: Snapshot, operation: Operation) -> Result<(), Failure> {
        if self.events.len() == EVENT_LIMIT { return Err(Failure::EventLimit { limit: EVENT_LIMIT }); }
        if self.pending_count == PENDING_LIMIT { return Err(Failure::PendingLimit { limit: PENDING_LIMIT }); }
        let invocation = self.next_invocation;
        let stack = self.pending.get(&snapshot.context);
        let parent = stack.and_then(|frames| frames.last()).map(|frame| frame.invocation);
        let ancestor = stack.and_then(|frames| frames.iter().rev().find(|frame| frame.operation.lifetime()));
        let logical = operation.lifetime() && ancestor.is_none();
        let pointer = Address(snapshot.registers[1]);
        let old = if operation == Operation::Realloc && pointer.0 != 0 {
            self.live.get(&pointer).cloned()
        } else { None };
        let mut origin = ancestor.map_or(Origin {
            invocation, identity: AllocationId(invocation), context: snapshot.context,
            core: snapshot.core, created: Counter(self.events.len() as u64 + 1),
        }, |frame| frame.origin);
        if let Some(allocation) = &old {
            origin.identity = allocation.identity;
            origin.context = allocation.context;
            origin.core = allocation.core;
            origin.created = allocation.created;
        }
        let requested = match operation {
            Operation::Malloc => u64::from(snapshot.registers[1]),
            Operation::Realloc => u64::from(snapshot.registers[2]),
            Operation::Calloc => u64::from(snapshot.registers[1]) * u64::from(snapshot.registers[2]),
            _ => 0,
        };
        let internal_free = stack.is_some_and(|frames| frames.iter()
            .any(|frame| matches!(frame.operation, Operation::Malloc | Operation::Realloc)));
        let freed_identity = if operation == Operation::Free {
            self.live.get(&pointer).map(|allocation| allocation.identity)
        } else { None };
        let classification = match operation {
            Operation::Free => Classification::Free { pointer, free_kind: if pointer.0 == 0 { FreeKind::Null }
                else if freed_identity.is_none() && internal_free { FreeKind::Internal }
                else if logical { FreeKind::Client } else { FreeKind::Nested } },
            Operation::Malloc | Operation::Realloc | Operation::Calloc => Classification::Allocation {
                logical_invocation: origin.invocation, logical,
            },
            _ => Classification::Helper,
        };
        let validation = if requested > u64::from(u32::MAX) {
            Err(Failure::RequestOverflow { count: snapshot.registers[1], element_size: snapshot.registers[2] })
        } else if operation == Operation::Free && pointer.0 != 0 && freed_identity.is_none() && !internal_free {
            Err(Failure::ForeignFree { pointer })
        } else if operation == Operation::Realloc && pointer.0 != 0 && old.is_none() {
            Err(Failure::ForeignRealloc { pointer })
        } else { Ok(()) };
        let frame = Frame {
            invocation, operation, parent, entry: snapshot.clone(), classification, logical, origin,
            requested, old, old_released: false, freed_identity,
        };
        if logical {
            self.logical_calls.entry(operation).or_default().0 += 1;
            self.per_task.entry(snapshot.context.into()).or_default().calls.entry(operation).or_default().0 += 1;
            self.per_core[snapshot.core.0].calls.entry(operation).or_default().0 += 1;
        }
        self.pending.entry(snapshot.context).or_default().push(frame);
        self.pending_count += 1;
        self.next_invocation += 1;
        self.record(snapshot, Boundary::Enter { invocation, operation, parent }, classification);
        validation
    }

    fn leave(&mut self, snapshot: Snapshot) -> Result<(), Failure> {
        if self.events.len() == EVENT_LIMIT { return Err(Failure::EventLimit { limit: EVENT_LIMIT }); }
        let context = snapshot.context;
        let Some(stack) = self.pending.get_mut(&context) else {
            return Err(Failure::UnexpectedPc { pc: snapshot.pc, context });
        };
        let Some(frame) = stack.pop() else {
            return Err(Failure::UnexpectedPc { pc: snapshot.pc, context });
        };
        if stack.is_empty() { self.pending.remove(&context); }
        self.pending_count -= 1;
        let result = self.apply_return(&frame, &snapshot);
        if result.is_ok() && frame.logical && matches!(frame.operation, Operation::Malloc | Operation::Realloc | Operation::Calloc) {
            let task_mix = self.per_task.entry(frame.entry.context.into()).or_default();
            for mix in [task_mix, &mut self.per_core[frame.entry.core.0]] {
                if snapshot.registers[0] == 0 { mix.failed_calls.0 += 1; }
                else {
                    mix.successful_calls.0 += 1;
                    mix.requested_bytes.0 += frame.requested;
                }
            }
        }
        self.record(snapshot, Boundary::Return {
            invocation: frame.invocation, operation: frame.operation, entry: frame.entry,
        }, frame.classification);
        result
    }

    fn apply_return(&mut self, frame: &Frame, snapshot: &Snapshot) -> Result<(), Failure> {
        match frame.operation {
            Operation::Malloc | Operation::Calloc => {
                if snapshot.registers[0] != 0 { self.allocate(frame, snapshot, None)?; }
            }
            Operation::Realloc => {
                let old_pointer = Address(frame.entry.registers[1]);
                if old_pointer.0 != 0 {
                    let present = self.live.get(&old_pointer).zip(frame.old.as_ref())
                        .is_some_and(|(current, old)| current.identity == old.identity);
                    if (!present && !frame.old_released) || (snapshot.registers[0] == 0 && !present) {
                        return Err(Failure::ReallocLostOld { pointer: old_pointer });
                    }
                }
                if snapshot.registers[0] != 0 {
                    self.allocate(frame, snapshot, (old_pointer.0 != 0).then_some(old_pointer))?;
                }
            }
            Operation::Free => {
                if let Some(identity) = frame.freed_identity {
                    let pointer = Address(frame.entry.registers[1]);
                    if self.live.get(&pointer).is_none_or(|allocation| allocation.identity != identity) {
                        return Err(Failure::AllocationChanged { pointer });
                    }
                    self.remove(pointer);
                    if let Some(stack) = self.pending.get_mut(&frame.entry.context) {
                        for ancestor in stack.iter_mut().filter(|ancestor| ancestor.operation == Operation::Realloc) {
                            if ancestor.old.as_ref().is_some_and(|allocation| allocation.address == pointer && allocation.identity == identity) {
                                ancestor.old_released = true;
                            }
                        }
                    }
                }
            }
            Operation::Trim | Operation::Sbrk | Operation::Lock | Operation::Unlock => {}
        }
        Ok(())
    }

    fn allocate(&mut self, frame: &Frame, snapshot: &Snapshot, old: Option<Address>) -> Result<(), Failure> {
        let pointer = Address(snapshot.registers[0]);
        if pointer.0 < 4 || pointer.0 & 7 != 0 { return Err(Failure::InvalidPointer { pointer }); }
        let usable = snapshot.chunk.ok_or(Failure::MissingChunk { pointer, operation: frame.operation })?;
        if u64::from(usable) < frame.requested {
            return Err(Failure::InvalidChunk { pointer, requested: Counter(frame.requested), usable: Counter(u64::from(usable)) });
        }
        let replacing = if let Some(existing) = self.live.get(&pointer) {
            let original = old == Some(pointer) && frame.old.as_ref().is_some_and(|allocation| allocation.identity == existing.identity);
            if !original && existing.logical_invocation != frame.origin.invocation {
                return Err(Failure::DuplicateAllocation { pointer });
            }
            true
        } else { false };
        let removing = old.is_some_and(|old| old != pointer && self.live.contains_key(&old));
        if !replacing && self.live.len() - usize::from(removing) == LIVE_LIMIT {
            return Err(Failure::LiveAllocationLimit { limit: LIVE_LIMIT });
        }
        if let Some(old) = old.filter(|old| *old != pointer) { self.remove(old); }
        let allocation = Allocation {
            identity: frame.origin.identity, address: pointer, requested: Counter(frame.requested),
            usable: Counter(u64::from(usable)), logical_invocation: frame.origin.invocation,
            context: frame.origin.context, core: frame.origin.core, created: frame.origin.created,
            updated: Counter(self.events.len() as u64 + 1),
        };
        self.requested_live += allocation.requested.0;
        self.usable_live += allocation.usable.0;
        if let Some(previous) = self.live.insert(pointer, allocation) {
            self.requested_live -= previous.requested.0;
            self.usable_live -= previous.usable.0;
        }
        self.requested_peak = self.requested_peak.max(self.requested_live);
        self.usable_peak = self.usable_peak.max(self.usable_live);
        Ok(())
    }

    fn remove(&mut self, pointer: Address) {
        if let Some(allocation) = self.live.remove(&pointer) {
            self.requested_live -= allocation.requested.0;
            self.usable_live -= allocation.usable.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Address, Boundary, Classification, ContextKey, Core, Counter, Failure, FreeKind, Operation, Recorder, Snapshot, TaskId, entry};

    fn context(task: u32) -> ContextKey {
        ContextKey { domain: Core(0), task: TaskId(task), task_record: Address(0x40010000 + task * 0x100), mode: 0x13, exception_depth: 0 }
    }

    fn sample(pc: u32, task: u32, tick: u64) -> Snapshot {
        let mut registers = [0; 16];
        registers[0] = 0x40020000;
        registers[13] = 0x40030000 + task * 0x1000;
        registers[14] = 0x1000 + task * 0x100;
        registers[15] = pc;
        Snapshot {
            pc: Address(pc), core: Core(0), context: context(task), registers, cpsr: 0x13,
            counts: [tick, 0], errno: Some(0), chunk: None, heap_break: 0xa71a00, owner: 0, depth: 0,
        }
    }

    fn call(recorder: &mut Recorder, pc: u32, task: u32, tick: u64, first: u32, second: u32) -> Snapshot {
        let mut snapshot = sample(pc, task, tick);
        snapshot.registers[1] = first;
        snapshot.registers[2] = second;
        recorder.observe(snapshot.clone()).unwrap();
        snapshot
    }

    fn returned(entry: &Snapshot, tick: u64, result: u32, chunk: Option<u32>) -> Snapshot {
        let mut snapshot = entry.clone();
        snapshot.pc = Address(entry.registers[14]);
        snapshot.registers[15] = snapshot.pc.0;
        snapshot.registers[0] = result;
        snapshot.counts = [tick, 0];
        snapshot.chunk = chunk;
        snapshot
    }

    fn malloc(recorder: &mut Recorder, task: u32, tick: u64, requested: u32, pointer: u32, usable: u32) {
        let entry = call(recorder, 0x89cde4, task, tick, requested, 0);
        recorder.observe(returned(&entry, tick + 1, pointer, Some(usable))).unwrap();
    }

    #[test]
    fn maps_only_reentrant_boundaries() {
        for (address, operation) in [(0x89cde4, Operation::Malloc), (0x89e560, Operation::Free),
            (0x89da64, Operation::Realloc), (0xbf1c44, Operation::Calloc), (0x89e464, Operation::Trim),
            (0x72b110, Operation::Sbrk), (0x72b164, Operation::Lock), (0x72b1cc, Operation::Unlock)] {
            assert_eq!(entry(Address(address)), Some(operation));
        }
        assert_eq!(entry(Address(0x89cda4)), None);
        assert_eq!(entry(Address(0x89da3c)), None);
    }

    #[test]
    fn two_tasks_return_in_non_lifo_global_order_on_one_core() {
        let mut recorder = Recorder::new();
        let first = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        let second = call(&mut recorder, 0x89cde4, 2, 2, 40, 0);
        recorder.observe(returned(&first, 3, 0xa72000, Some(28))).unwrap();
        recorder.observe(returned(&second, 4, 0xa72100, Some(44))).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.live_allocations.len(), 2);
        assert_eq!(summary.requested_peak, Counter(60));
        assert_eq!(summary.usable_peak, Counter(72));
        assert_eq!(summary.per_task.len(), 2);
        assert_eq!(recorder.records()[2].active_context[0], Some(context(1)));
        assert_eq!(recorder.records()[3].active_context[0], Some(context(2)));
    }

    #[test]
    fn unrelated_task_lr_hit_leaves_pending_and_lifetimes_unchanged() {
        let mut recorder = Recorder::new();
        let first = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        let mut wrong = returned(&first, 2, 0xa72000, Some(28));
        wrong.context = context(2);
        recorder.observe(wrong).unwrap();
        assert_eq!(recorder.pending_count(), 1);
        assert_eq!(recorder.records().len(), 1);
        assert!(recorder.summary().live_allocations.is_empty());
        assert_eq!(recorder.summary().pending[0].entry, first);
        recorder.observe(returned(&first, 3, 0xa72000, Some(28))).unwrap();
        assert_eq!(recorder.finish().unwrap().live_allocations.len(), 1);
    }

    #[test]
    fn exception_context_cannot_return_task_frame() {
        let mut recorder = Recorder::new();
        let first = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        let mut wrong = returned(&first, 2, 0xa72000, Some(28));
        wrong.context.exception_depth = 1;
        recorder.observe(wrong).unwrap();
        assert_eq!(recorder.pending_count(), 1);
        assert!(recorder.summary().live_allocations.is_empty());
        recorder.observe(returned(&first, 3, 0xa72000, Some(28))).unwrap();
        assert_eq!(recorder.finish().unwrap().live_allocations.len(), 1);
    }

    #[test]
    fn nested_same_task_helpers_pair_with_parent_and_saved_reentrant_context() {
        let mut recorder = Recorder::new();
        let outer = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        let mut inner = sample(0x72b164, 1, 2);
        inner.registers[13] -= 32;
        inner.registers[14] = 0x89ce20;
        recorder.observe(inner.clone()).unwrap();
        assert_eq!(recorder.return_operation(inner.context, Address(0x89ce20), Address(inner.registers[13])), Some(Operation::Lock));
        assert_eq!(recorder.return_context(inner.context, Address(0x89ce20), Address(inner.registers[13])), Some(Address(0x40020000)));
        recorder.observe(returned(&inner, 3, 0, None)).unwrap();
        recorder.observe(returned(&outer, 4, 0xa72000, Some(28))).unwrap();
        assert_eq!(recorder.finish().unwrap().logical_calls.get(&Operation::Malloc), Some(&Counter(1)));
        assert!(matches!(recorder.records()[1].boundary, Boundary::Enter { parent: Some(1), .. }));
        assert!(matches!(&recorder.records()[2].boundary, Boundary::Return { entry, operation: Operation::Lock, .. } if entry == &inner));
    }

    #[test]
    fn older_frame_return_refuses_even_when_its_pc_is_an_entry() {
        let mut recorder = Recorder::new();
        let mut outer = sample(0x89cde4, 1, 1);
        outer.registers[14] = 0x89e560;
        recorder.observe(outer.clone()).unwrap();
        let mut inner = sample(0x72b164, 1, 2);
        inner.registers[13] -= 32;
        inner.registers[14] = 0x89ce20;
        recorder.observe(inner).unwrap();
        assert!(matches!(recorder.observe(returned(&outer, 3, 0xa72000, Some(12))), Err(Failure::NonTopReturn { invocation: 1, top: 2, .. })));
        assert_eq!(recorder.pending_count(), 2);
    }

    #[test]
    fn wrong_stack_pointer_cannot_return_a_frame() {
        let mut recorder = Recorder::new();
        let first = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        let mut wrong = returned(&first, 2, 0xa72000, Some(28));
        wrong.registers[13] += 4;
        recorder.observe(wrong).unwrap();
        assert_eq!(recorder.pending_count(), 1);
        assert!(recorder.summary().live_allocations.is_empty());
        recorder.observe(returned(&first, 3, 0xa72000, Some(28))).unwrap();
        assert_eq!(recorder.finish().unwrap().live_allocations.len(), 1);
    }

    #[test]
    fn failed_realloc_keeps_old_identity_and_size() {
        let mut recorder = Recorder::new();
        malloc(&mut recorder, 1, 1, 20, 0xa72000, 28);
        let before = recorder.finish().unwrap().live_allocations[0].clone();
        let resize = call(&mut recorder, 0x89da64, 1, 3, 0xa72000, 100);
        let mut failure = returned(&resize, 4, 0, None);
        failure.errno = Some(12);
        recorder.observe(failure).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.live_allocations.len(), 1);
        assert_eq!(summary.live_allocations[0].identity, before.identity);
        assert_eq!(summary.live_allocations[0].requested, Counter(20));
        assert_eq!(summary.requested_peak, Counter(20));
        assert_eq!(summary.per_task[0].mix.failed_calls, Counter(1));
    }

    #[test]
    fn moving_realloc_without_malloc_preserves_identity() {
        let mut recorder = Recorder::new();
        malloc(&mut recorder, 1, 1, 20, 0xa72000, 28);
        let identity = recorder.finish().unwrap().live_allocations[0].identity;
        let resize = call(&mut recorder, 0x89da64, 1, 3, 0xa72000, 60);
        recorder.observe(returned(&resize, 4, 0xa71f00, Some(68))).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.live_allocations.len(), 1);
        assert_eq!(summary.live_allocations[0].identity, identity);
        assert_eq!(summary.live_allocations[0].address, Address(0xa71f00));
        assert_eq!(summary.requested_live, Counter(60));
        assert_eq!(recorder.live_headers(), vec![Address(0xa71efc)]);
        assert!(matches!(&recorder.records()[3].boundary, Boundary::Return { entry, .. } if entry.registers[1] == 0xa72000));
    }

    #[test]
    fn moving_realloc_reconciles_nested_malloc_and_free() {
        let mut recorder = Recorder::new();
        malloc(&mut recorder, 1, 1, 20, 0xa72000, 28);
        let identity = recorder.finish().unwrap().live_allocations[0].identity;
        let resize = call(&mut recorder, 0x89da64, 1, 3, 0xa72000, 60);
        let mut allocate = sample(0x89cde4, 1, 4);
        allocate.registers[1] = 60;
        allocate.registers[13] -= 32;
        allocate.registers[14] = 0x89dc00;
        recorder.observe(allocate.clone()).unwrap();
        recorder.observe(returned(&allocate, 5, 0xa72100, Some(68))).unwrap();
        let mut free = sample(0x89e560, 1, 6);
        free.registers[1] = 0xa72000;
        free.registers[13] -= 32;
        free.registers[14] = 0x89dc20;
        recorder.observe(free.clone()).unwrap();
        recorder.observe(returned(&free, 7, 0, None)).unwrap();
        recorder.observe(returned(&resize, 8, 0xa72100, Some(68))).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.live_allocations.len(), 1);
        assert_eq!(summary.live_allocations[0].identity, identity);
        assert_eq!(summary.requested_live, Counter(60));
        assert_eq!(summary.requested_peak, Counter(80));
        assert_eq!(summary.usable_peak, Counter(96));
        assert_eq!(summary.logical_calls.get(&Operation::Malloc), Some(&Counter(1)));
        assert_eq!(summary.logical_calls.get(&Operation::Realloc), Some(&Counter(1)));
        assert_eq!(summary.logical_calls.get(&Operation::Free), None);
    }

    #[test]
    fn foreign_client_free_is_refused_and_preserved() {
        let mut recorder = Recorder::new();
        let mut free = sample(0x89e560, 1, 1);
        free.registers[1] = 0xa72000;
        assert_eq!(recorder.observe(free.clone()), Err(Failure::ForeignFree { pointer: Address(0xa72000) }));
        assert_eq!(recorder.records()[0].snapshot, free);
        assert!(matches!(recorder.finish(), Err(Failure::ForeignFree { .. })));
        assert!(recorder.summary().partial);
    }

    #[test]
    fn split_remainder_free_is_internal_only_under_malloc_or_realloc() {
        let mut recorder = Recorder::new();
        let outer = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        let mut free = sample(0x89e560, 1, 2);
        free.registers[1] = 0xa72100;
        free.registers[13] -= 32;
        free.registers[14] = 0x89cf00;
        recorder.observe(free.clone()).unwrap();
        recorder.observe(returned(&free, 3, 0, None)).unwrap();
        recorder.observe(returned(&outer, 4, 0xa72000, Some(28))).unwrap();
        assert!(matches!(recorder.records()[1].classification, Classification::Free { free_kind: FreeKind::Internal, .. }));
        assert_eq!(recorder.finish().unwrap().live_allocations.len(), 1);
        let mut other = Recorder::new();
        call(&mut other, 0xbf1c44, 1, 1, 1, 20);
        free.counts = [2, 0];
        assert!(matches!(other.observe(free), Err(Failure::ForeignFree { .. })));
    }

    #[test]
    fn calloc_nested_malloc_is_one_logical_allocation() {
        let mut recorder = Recorder::new();
        let outer = call(&mut recorder, 0xbf1c44, 1, 1, 3, 8);
        let mut inner = sample(0x89cde4, 1, 2);
        inner.registers[1] = 24;
        inner.registers[13] -= 16;
        inner.registers[14] = 0xbf1c50;
        recorder.observe(inner.clone()).unwrap();
        recorder.observe(returned(&inner, 3, 0xa72000, Some(28))).unwrap();
        recorder.observe(returned(&outer, 4, 0xa72000, Some(28))).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.live_allocations.len(), 1);
        assert_eq!(summary.requested_peak, Counter(24));
        assert_eq!(summary.usable_peak, Counter(28));
        assert_eq!(summary.logical_calls.get(&Operation::Calloc), Some(&Counter(1)));
        assert_eq!(summary.logical_calls.get(&Operation::Malloc), None);
        assert_eq!(summary.per_task[0].mix.successful_calls, Counter(1));
    }

    #[test]
    fn calloc_product_overflow_is_a_typed_refusal() {
        let mut recorder = Recorder::new();
        let mut snapshot = sample(0xbf1c44, 1, 1);
        snapshot.registers[1] = 0x80000000;
        snapshot.registers[2] = 2;
        assert_eq!(recorder.observe(snapshot), Err(Failure::RequestOverflow { count: 0x80000000, element_size: 2 }));
        assert_eq!(recorder.records().len(), 1);
    }

    #[test]
    fn zero_size_allocations_and_null_free_keep_stock_minimum_chunk() {
        let mut recorder = Recorder::new();
        malloc(&mut recorder, 1, 1, 0, 0xa72000, 12);
        let free = call(&mut recorder, 0x89e560, 1, 3, 0, 0);
        recorder.observe(returned(&free, 4, 0, None)).unwrap();
        let resize = call(&mut recorder, 0x89da64, 1, 5, 0xa72000, 0);
        recorder.observe(returned(&resize, 6, 0xa72000, Some(12))).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.live_allocations.len(), 1);
        assert_eq!(summary.requested_live, Counter(0));
        assert_eq!(summary.usable_live, Counter(12));
    }

    #[test]
    fn clocks_are_global_and_componentwise_monotonic() {
        let mut recorder = Recorder::new();
        let first = call(&mut recorder, 0x89cde4, 1, 10, 20, 0);
        let mut other = sample(0x89cde4, 2, 10);
        other.core = Core(1);
        other.counts = [10, 2];
        other.registers[1] = 40;
        recorder.observe(other.clone()).unwrap();
        let mut first_return = returned(&first, 11, 0xa72000, Some(28));
        first_return.counts = [11, 2];
        recorder.observe(first_return).unwrap();
        let mut other_return = returned(&other, 11, 0xa72100, Some(44));
        other_return.counts = [11, 3];
        recorder.observe(other_return).unwrap();
        assert_eq!(recorder.finish().unwrap().requested_peak, Counter(60));
        assert_eq!(recorder.records()[3].sequence, Counter(4));
        assert_eq!(recorder.records()[3].active_context, [Some(context(1)), Some(context(2))]);
    }

    #[test]
    fn backwards_stalled_and_overflowing_clocks_refuse() {
        let mut backwards = Recorder::new();
        call(&mut backwards, 0x89cde4, 1, 10, 20, 0);
        let mut snapshot = sample(0x89cde4, 2, 9);
        snapshot.counts = [9, 10];
        assert!(matches!(backwards.observe(snapshot), Err(Failure::BackwardsClock { core: Core(0), .. })));
        let mut stalled = Recorder::new();
        call(&mut stalled, 0x89cde4, 1, 10, 20, 0);
        assert_eq!(stalled.observe(sample(0x89cde4, 2, 10)), Err(Failure::StalledClock { counts: [10, 0] }));
        let mut overflowing = Recorder::new();
        let mut snapshot = sample(0x89cde4, 1, u64::MAX);
        snapshot.counts[1] = 1;
        assert!(matches!(overflowing.observe(snapshot), Err(Failure::ClockOverflow { .. })));
    }

    #[test]
    fn dropped_allocation_entry_is_detected_when_the_pointer_is_freed() {
        let mut recorder = Recorder::new();
        let snapshot = sample(0x89cde4, 1, 1);
        recorder.observe(returned(&snapshot, 2, 0xa72000, Some(28))).unwrap();
        let mut free = sample(0x89e560, 1, 3);
        free.registers[1] = 0xa72000;
        assert!(matches!(recorder.observe(free), Err(Failure::ForeignFree { pointer: Address(0xa72000) })));
    }

    #[test]
    fn unfinished_invocations_are_explicitly_partial() {
        let mut recorder = Recorder::new();
        let snapshot = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        assert!(matches!(recorder.finish(), Err(Failure::Incomplete { pending: 1 })));
        let summary = recorder.summary();
        assert!(summary.partial);
        assert_eq!(summary.pending[0].entry, snapshot);
        assert_eq!(recorder.returns().into_iter().collect::<Vec<_>>(), vec![Address(snapshot.registers[14])]);
    }

    #[test]
    fn pending_limit_refuses_without_dropping_suspended_frames() {
        let mut recorder = Recorder::new();
        for task in 0..256 {
            call(&mut recorder, 0x89cde4, task, u64::from(task) + 1, 20, 0);
        }
        assert_eq!(recorder.observe(sample(0x89cde4, 256, 257)), Err(Failure::PendingLimit { limit: 256 }));
        assert_eq!(recorder.pending_count(), 256);
        assert_eq!(recorder.summary().pending.len(), 256);
    }

    #[test]
    fn live_allocation_limit_retains_all_known_lifetimes() {
        let mut recorder = Recorder::new();
        for index in 0..4096 {
            malloc(&mut recorder, 1, u64::from(index) * 2 + 1, 8, 0xa72000 + index * 16, 12);
        }
        let last = call(&mut recorder, 0x89cde4, 1, 8193, 8, 0);
        assert_eq!(recorder.observe(returned(&last, 8194, 0xa82000, Some(12))), Err(Failure::LiveAllocationLimit { limit: 4096 }));
        let summary = recorder.summary();
        assert_eq!(summary.live_allocations.len(), 4096);
        assert_eq!(summary.requested_live, Counter(32768));
        assert!(summary.partial);
    }

    #[test]
    fn event_limit_refuses_without_replacing_old_records() {
        let mut recorder = Recorder::new();
        for index in 0..16384 {
            let snapshot = call(&mut recorder, 0x72b164, 1, index * 2 + 1, 0, 0);
            recorder.observe(returned(&snapshot, index * 2 + 2, 0, None)).unwrap();
        }
        assert_eq!(recorder.observe(sample(0x72b164, 1, 32769)), Err(Failure::EventLimit { limit: 32768 }));
        assert_eq!(recorder.records().len(), 32768);
        assert_eq!(recorder.records()[0].sequence, Counter(1));
        assert_eq!(recorder.records()[32767].sequence, Counter(32768));
    }

    #[test]
    fn historical_return_points_are_not_new_returns() {
        let mut recorder = Recorder::new();
        let first = call(&mut recorder, 0x89cde4, 1, 1, 20, 0);
        recorder.observe(returned(&first, 2, 0xa72000, Some(28))).unwrap();
        let mut historical = returned(&first, 3, 0, None);
        historical.context = context(2);
        recorder.observe(historical).unwrap();
        assert_eq!(recorder.records().len(), 2);
        assert_eq!(recorder.pending_count(), 0);
        assert_eq!(recorder.finish().unwrap().requested_live, Counter(20));
    }

    #[test]
    fn selected_root_domain_is_independent_of_execution_core() {
        let mut recorder = Recorder::new();
        let mut first = sample(0x89cde4, 1, 1);
        first.context.domain = Core(1);
        first.registers[1] = 20;
        recorder.observe(first.clone()).unwrap();
        recorder.observe(returned(&first, 2, 0xa72000, Some(28))).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.per_task[0].task.domain, Core(1));
        assert_eq!(summary.per_core[0].core, Core(0));
        assert_eq!(summary.per_core[0].mix.requested_live, Counter(20));
    }

    #[test]
    fn client_free_releases_requested_and_usable_lifetimes() {
        let mut recorder = Recorder::new();
        malloc(&mut recorder, 1, 1, 20, 0xa72000, 28);
        let free = call(&mut recorder, 0x89e560, 1, 3, 0xa72000, 0);
        recorder.observe(returned(&free, 4, 0, None)).unwrap();
        let summary = recorder.finish().unwrap();
        assert!(summary.live_allocations.is_empty());
        assert_eq!(summary.requested_live, Counter(0));
        assert_eq!(summary.usable_live, Counter(0));
        assert_eq!(summary.requested_peak, Counter(20));
        assert_eq!(summary.usable_peak, Counter(28));
        assert_eq!(summary.logical_calls.get(&Operation::Free), Some(&Counter(1)));
    }

    #[test]
    fn successful_in_place_resize_updates_size_without_replacing_identity() {
        let mut recorder = Recorder::new();
        malloc(&mut recorder, 1, 1, 20, 0xa72000, 28);
        let identity = recorder.finish().unwrap().live_allocations[0].identity;
        let resize = call(&mut recorder, 0x89da64, 1, 3, 0xa72000, 40);
        recorder.observe(returned(&resize, 4, 0xa72000, Some(44))).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.live_allocations.len(), 1);
        assert_eq!(summary.live_allocations[0].identity, identity);
        assert_eq!(summary.requested_live, Counter(40));
        assert_eq!(summary.usable_live, Counter(44));
    }

    #[test]
    fn backing_highwater_is_separate_from_live_peaks_and_survives_trim() {
        let mut recorder = Recorder::new();
        let initial = call(&mut recorder, 0x72b110, 1, 1, 4096, 0);
        let mut grown = returned(&initial, 2, 0xa71a00, None);
        grown.heap_break = 0xa72a00;
        recorder.observe(grown).unwrap();
        let trim = call(&mut recorder, 0x89e464, 1, 3, 0, 0);
        recorder.observe(returned(&trim, 4, 1, None)).unwrap();
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.backing_highwater, Counter(4096));
        assert_eq!(summary.backing_highwater_break, Some(Address(0xa72a00)));
        assert_eq!(summary.requested_peak, Counter(0));
        assert_eq!(summary.usable_peak, Counter(0));
    }

    fn image5_load() -> [Snapshot; 4] {
        let mut entered = sample(0x6a96c0, 60, 1);
        entered.registers[14] = 0x73b4b0;
        let mut configured = sample(0x6ae360, 60, 2);
        configured.registers[0] = 0xbf1a00;
        configured.registers[1] = 0x43e40000;
        configured.registers[14] = 0x6a977c;
        let drained = sample(0x6a97bc, 60, 3);
        let mut produced = sample(0x6a9840, 60, 4);
        produced.registers[0] = 0;
        produced.registers[13] = entered.registers[13] - 28;
        [entered, configured, drained, produced]
    }

    #[test]
    fn image5_heap_fetches_require_completed_code_admission_in_both_aliases() {
        let loader = Image5Lifecycle::default();
        for address in [0xbf1c1c, 0xbf1c1e, 0xbf1c44, 0x40bf1c1c, 0x40bf1c1e, 0x40bf1c44, 0xd985a4] {
            assert_eq!(loader.check_fetch(Address(address)), Err(Image5Failure::PrepublicationFetch));
        }
    }

    #[test]
    fn image5_successful_producer_and_code_identity_admit_heap_calls() {
        let mut loader = Image5Lifecycle::default();
        let load = image5_load();
        for boundary in &load[..3] { loader.observe(boundary, Image5CodeStatus::NotSampled).unwrap(); }
        loader.observe(&load[3], Image5CodeStatus::Canonical).unwrap();
        assert_eq!(loader.phase(), Image5Phase::Admitted);
        loader.check_fetch(Address(0xbf1c44)).unwrap();
        loader.check_fetch(Address(0x40bf1c44)).unwrap();
        let mut transfer = sample(0x69d2e0, 95, 5);
        transfer.registers[3] = 0xd985a4;
        loader.observe(&transfer, Image5CodeStatus::Canonical).unwrap();
    }

    #[test]
    fn image5_success_status_without_code_identity_does_not_admit_execution() {
        let mut loader = Image5Lifecycle::default();
        let load = image5_load();
        for boundary in &load[..3] { loader.observe(boundary, Image5CodeStatus::NotSampled).unwrap(); }
        assert_eq!(loader.observe(&load[3], Image5CodeStatus::NotSampled), Err(Image5Failure::CodeIdentity));
        assert_eq!(loader.check_fetch(Address(0xbf1c44)), Err(Image5Failure::PrepublicationFetch));
    }

    #[test]
    fn image5_producer_completion_from_another_task_refuses() {
        let mut loader = Image5Lifecycle::default();
        let mut load = image5_load();
        for boundary in &load[..3] { loader.observe(boundary, Image5CodeStatus::NotSampled).unwrap(); }
        load[3].context = context(59);
        assert_eq!(loader.observe(&load[3], Image5CodeStatus::Canonical), Err(Image5Failure::Context));
    }

    #[test]
    fn image5_storage_failure_refuses_even_with_canonical_code() {
        let mut loader = Image5Lifecycle::default();
        let mut load = image5_load();
        for boundary in &load[..3] { loader.observe(boundary, Image5CodeStatus::NotSampled).unwrap(); }
        load[3].registers[0] = u32::MAX;
        assert_eq!(loader.observe(&load[3], Image5CodeStatus::Canonical), Err(Image5Failure::ProducerStatus));
    }

    #[test]
    fn image5_producer_cannot_skip_decoder_configuration_or_drain() {
        for skip in [1, 2] {
            let mut loader = Image5Lifecycle::default();
            let load = image5_load();
            for (index, boundary) in load.iter().enumerate().take(3) {
                if index != skip {
                    let result = loader.observe(boundary, Image5CodeStatus::NotSampled);
                    if index == 2 && skip == 1 { assert_eq!(result, Err(Image5Failure::Order)); }
                    else { result.unwrap(); }
                }
            }
            assert_eq!(loader.observe(&load[3], Image5CodeStatus::Canonical), Err(Image5Failure::Order));
        }
    }

    #[test]
    fn image5_producer_return_requires_its_saved_frame() {
        let mut loader = Image5Lifecycle::default();
        let mut load = image5_load();
        for boundary in &load[..3] { loader.observe(boundary, Image5CodeStatus::NotSampled).unwrap(); }
        load[3].registers[13] += 4;
        assert_eq!(loader.observe(&load[3], Image5CodeStatus::Canonical), Err(Image5Failure::Frame));
    }

    #[test]
    fn image5_decoder_requires_the_original_destination_staging_and_callsite() {
        for register in [0, 1, 14] {
            let mut loader = Image5Lifecycle::default();
            let mut load = image5_load();
            loader.observe(&load[0], Image5CodeStatus::NotSampled).unwrap();
            load[1].registers[register] ^= 4;
            assert_eq!(loader.observe(&load[1], Image5CodeStatus::NotSampled), Err(Image5Failure::DecoderArguments));
        }
    }

    #[test]
    fn image5_reinstallation_cannot_reopen_a_frozen_code_guard() {
        let mut loader = Image5Lifecycle::default();
        let load = image5_load();
        for boundary in &load[..3] { loader.observe(boundary, Image5CodeStatus::NotSampled).unwrap(); }
        loader.observe(&load[3], Image5CodeStatus::Canonical).unwrap();
        assert_eq!(loader.observe(&load[0], Image5CodeStatus::NotSampled), Err(Image5Failure::Order));
    }
}
