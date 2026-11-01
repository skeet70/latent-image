use crate::heap_state::{Address, AllocationId, Boundary, Classification, ContextKey, Core, Counter, Event, FreeKind, Operation, Snapshot, Summary};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Bytes(pub u32);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct InvocationId(pub u64);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Layout { pub size: Bytes, pub alignment: Alignment }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Alignment { Eight }
impl Alignment { pub fn bytes(self) -> u32 { 8 } }
impl Layout {
    fn parse(size: u32) -> Result<Self, Failure> {
        if size > 0x7fff_fff4 { return Err(Failure::InvalidSize); }
        Ok(Self { size: Bytes(size), alignment: Alignment::Eight })
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AllocationOutcome { Success { identity: AllocationId }, Failure }
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Request {
    Malloc { layout: Layout, outcome: AllocationOutcome },
    Calloc { count: Bytes, element: Bytes, layout: Layout, outcome: AllocationOutcome },
    Free { identity: Option<AllocationId>, layout: Option<Layout> },
    Realloc { previous: Option<AllocationId>, previous_layout: Option<Layout>, layout: Layout, outcome: AllocationOutcome },
}
impl Request {
    pub fn operation(&self) -> Operation {
        match self { Self::Malloc { .. } => Operation::Malloc, Self::Calloc { .. } => Operation::Calloc,
            Self::Free { .. } => Operation::Free, Self::Realloc { .. } => Operation::Realloc }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Step {
    pub sequence: Counter,
    pub invocation: InvocationId,
    pub context: ContextKey,
    pub core: Core,
    pub observed_instructions: [Counter; 2],
    pub request: Request,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Failure {
    PartialTrace, ConcurrentCalls, UnmatchedReturn, ArgumentBinding, UnsupportedOperation,
    InvalidSize, ArithmeticOverflow, UnknownFree { pointer: Address }, DuplicatePointer { pointer: Address },
    SummaryMismatch, NonTaskContext, Clock,
}
#[derive(Clone, Debug, Serialize)]
pub struct Tape {
    steps: Vec<Step>,
    final_live: BTreeMap<AllocationId, Layout>,
    requested_live: Bytes,
    requested_peak: Bytes,
    maximum_request: Bytes,
    source_live: BTreeMap<Address, Layout>,
}
#[derive(Clone, Copy)]
struct Live { identity: AllocationId, layout: Layout }
struct Population {
    live: BTreeMap<Address, Live>,
    next: AllocationId,
    requested: Bytes,
}
impl Population {
    fn allocate(&mut self, pointer: Address, layout: Layout, previous: Option<Live>) -> Result<AllocationOutcome, Failure> {
        if pointer.0 == 0 { return Ok(AllocationOutcome::Failure); }
        let identity = if let Some(previous) = previous { previous.identity } else {
            let identity = self.next;
            self.next.0 = self.next.0.checked_add(1).ok_or(Failure::ArithmeticOverflow)?;
            identity
        };
        if self.live.contains_key(&pointer) { return Err(Failure::DuplicatePointer { pointer }); }
        self.live.insert(pointer, Live { identity, layout });
        self.requested.0 = self.requested.0.checked_add(layout.size.0).ok_or(Failure::ArithmeticOverflow)?;
        Ok(AllocationOutcome::Success { identity })
    }
    fn remove(&mut self, pointer: Address) -> Result<Option<Live>, Failure> {
        if pointer.0 == 0 { return Ok(None); }
        let allocation = self.live.remove(&pointer).ok_or(Failure::UnknownFree { pointer })?;
        self.requested.0 = self.requested.0.checked_sub(allocation.layout.size.0).ok_or(Failure::ArithmeticOverflow)?;
        Ok(Some(allocation))
    }
}
struct Pending<'a> { invocation: InvocationId, operation: Operation, entry: &'a Snapshot }
impl Tape {
    pub fn steps(&self) -> &[Step] { &self.steps }
    pub fn final_live(&self) -> &BTreeMap<AllocationId, Layout> { &self.final_live }
    pub fn requested_peak(&self) -> Bytes { self.requested_peak }
    pub fn requested_live(&self) -> Bytes { self.requested_live }
    pub fn maximum_request(&self) -> Bytes { self.maximum_request }
    pub fn from_events(events: &[Event]) -> Result<Self, Failure> {
        let mut steps = Vec::new();
        let mut population = Population { live: BTreeMap::new(), next: AllocationId(1), requested: Bytes(0) };
        let mut pending: Option<Pending> = None;
        let mut requested_peak = 0u32;
        let mut maximum_request = 0u32;
        let mut last_sequence = 0u64;
        for event in events {
            if event.sequence.0 <= last_sequence { return Err(Failure::ArgumentBinding); }
            last_sequence = event.sequence.0;
            let logical = match event.classification {
                Classification::Allocation { logical, .. } => logical,
                Classification::Free { free_kind, .. } => matches!(free_kind, FreeKind::Client | FreeKind::Null),
                Classification::Helper => false,
            };
            if !logical { continue; }
            let context = event.snapshot.context;
            if event.snapshot.core != Core(0) || context.domain != Core(0) || context.mode != 0x13
                || context.exception_depth != 0 || event.snapshot.cpsr & 0x80 != 0 {
                return Err(Failure::NonTaskContext);
            }
            match &event.boundary {
                Boundary::Enter { invocation, operation, .. } => {
                    if pending.is_some() { return Err(Failure::ConcurrentCalls); }
                    pending = Some(Pending { invocation: InvocationId(*invocation), operation: *operation, entry: &event.snapshot });
                }
                Boundary::Return { invocation, operation, entry } => {
                    let active = pending.take().ok_or(Failure::UnmatchedReturn)?;
                    if active.invocation != InvocationId(*invocation) || active.operation != *operation
                        || active.entry != entry || entry.context != event.snapshot.context {
                        return Err(Failure::ArgumentBinding);
                    }
                    let mut elapsed = [Counter(0); 2];
                    for (index, value) in elapsed.iter_mut().enumerate() {
                        value.0 = event.snapshot.counts[index].checked_sub(entry.counts[index]).ok_or(Failure::Clock)?;
                    }
                    let pointer = Address(event.snapshot.registers[0]);
                    let request = match operation {
                        Operation::Malloc => {
                            let layout = Layout::parse(entry.registers[1])?;
                            maximum_request = maximum_request.max(layout.size.0);
                            let outcome = population.allocate(pointer, layout, None)?;
                            Request::Malloc { layout, outcome }
                        }
                        Operation::Calloc => {
                            let count = entry.registers[1];
                            let element = entry.registers[2];
                            let layout = Layout::parse(count.checked_mul(element).ok_or(Failure::ArithmeticOverflow)?)?;
                            maximum_request = maximum_request.max(layout.size.0);
                            let outcome = population.allocate(pointer, layout, None)?;
                            Request::Calloc { count: Bytes(count), element: Bytes(element), layout, outcome }
                        }
                        Operation::Free => {
                            let old = Address(entry.registers[1]);
                            let allocation = population.remove(old)?;
                            Request::Free { identity: allocation.map(|value| value.identity), layout: allocation.map(|value| value.layout) }
                        }
                        Operation::Realloc => {
                            let old = Address(entry.registers[1]);
                            let layout = Layout::parse(entry.registers[2])?;
                            let previous = if old.0 == 0 { None } else { Some(*population.live.get(&old).ok_or(Failure::UnknownFree { pointer: old })?) };
                            let outcome = if pointer.0 == 0 {
                                maximum_request = maximum_request.max(layout.size.0);
                                AllocationOutcome::Failure
                            } else {
                                if previous.is_some() { population.remove(old)?; }
                                maximum_request = maximum_request.max(layout.size.0);
                                population.allocate(pointer, layout, previous)?
                            };
                            Request::Realloc { previous: previous.map(|value| value.identity), previous_layout: previous.map(|value| value.layout), layout, outcome }
                        }
                        _ => return Err(Failure::UnsupportedOperation),
                    };
                    requested_peak = requested_peak.max(population.requested.0);
                    steps.push(Step { sequence: event.sequence, invocation: active.invocation, context,
                        core: event.snapshot.core, observed_instructions: elapsed, request });
                }
            }
        }
        if pending.is_some() { return Err(Failure::PartialTrace); }
        Ok(Self { steps, final_live: population.live.values().map(|value| (value.identity, value.layout)).collect(),
            requested_live: population.requested, requested_peak: Bytes(requested_peak), maximum_request: Bytes(maximum_request),
            source_live: population.live.into_iter().map(|(pointer, value)| (pointer, value.layout)).collect() })
    }
    pub fn bind_summary(&self, summary: &Summary) -> Result<(), Failure> {
        if summary.partial || summary.failure.is_some() || !summary.pending.is_empty() { return Err(Failure::PartialTrace); }
        let source_live: BTreeMap<_, _> = summary.live_allocations.iter().map(|allocation| {
            Ok((allocation.address, Layout::parse(u32::try_from(allocation.requested.0).map_err(|_| Failure::ArithmeticOverflow)?)?))
        }).collect::<Result<_, Failure>>()?;
        if source_live != self.source_live || summary.requested_live.0 != u64::from(self.requested_live.0)
            || summary.requested_peak.0 != u64::from(self.requested_peak.0) {
            return Err(Failure::SummaryMismatch);
        }
        let mut calls = BTreeMap::<Operation, u64>::new();
        for step in &self.steps { *calls.entry(step.request.operation()).or_default() += 1; }
        if calls != summary.logical_calls.iter().map(|(operation, count)| (*operation, count.0)).collect() {
            return Err(Failure::SummaryMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap_state::TaskId;
    fn pair(invocation: u64, operation: Operation, pointer: u32, first: u32, second: u32, result: u32, sequence: u64) -> [Event; 2] {
        let context = ContextKey { domain: Core(0), task: TaskId(1), task_record: Address(0x4005ff0c), mode: 0x13, exception_depth: 0 };
        let mut registers = [0; 16];
        registers[1] = first;
        registers[2] = second;
        let entry = Snapshot { pc: Address(0x89cde4), core: Core(0), context, registers, cpsr: 0x13,
            counts: [sequence, 0], errno: Some(0), chunk: None, heap_break: 0xa71a00, owner: 0, depth: 0 };
        let classification = if operation == Operation::Free {
            Classification::Free { pointer: Address(pointer), free_kind: if pointer == 0 { FreeKind::Null } else { FreeKind::Client } }
        } else { Classification::Allocation { logical_invocation: invocation, logical: true } };
        let mut returned = entry.clone();
        returned.registers[0] = result;
        returned.counts[0] += 1;
        [Event { sequence: Counter(sequence), snapshot: entry.clone(), boundary: Boundary::Enter { invocation, operation, parent: None }, classification, active_context: [Some(context), None] },
            Event { sequence: Counter(sequence + 1), snapshot: returned, boundary: Boundary::Return { invocation, operation, entry }, classification, active_context: [Some(context), None] }]
    }
    #[test]
    fn pointer_reuse_gets_a_distinct_lifetime() {
        let events: Vec<_> = [pair(1, Operation::Malloc, 0, 12, 0, 0xa71a08, 1),
            pair(2, Operation::Free, 0xa71a08, 0xa71a08, 0, 0, 3), pair(3, Operation::Malloc, 0, 20, 0, 0xa71a08, 5)].into_iter().flatten().collect();
        let tape = Tape::from_events(&events).unwrap();
        assert_eq!(tape.final_live().keys().copied().collect::<Vec<_>>(), [AllocationId(2)]);
        assert_eq!(tape.requested_peak(), Bytes(20));
    }
    #[test]
    fn failed_resize_keeps_the_old_layout_and_lifetime() {
        let events: Vec<_> = [pair(1, Operation::Malloc, 0, 40, 0, 0xa71a08, 1),
            pair(2, Operation::Realloc, 0, 0xa71a08, 80, 0, 3)].into_iter().flatten().collect();
        let tape = Tape::from_events(&events).unwrap();
        assert_eq!(tape.requested_live(), Bytes(40));
        assert_eq!(tape.final_live().get(&AllocationId(1)).unwrap().size, Bytes(40));
        assert!(matches!(tape.steps()[1].request, Request::Realloc { outcome: AllocationOutcome::Failure, .. }));
    }
    #[test]
    fn moved_and_zero_size_resize_keep_the_logical_object() {
        let events: Vec<_> = [pair(1, Operation::Malloc, 0, 40, 0, 0xa71a08, 1),
            pair(2, Operation::Realloc, 0, 0xa71a08, 80, 0xa71b08, 3),
            pair(3, Operation::Realloc, 0, 0xa71b08, 0, 0xa71c08, 5)].into_iter().flatten().collect();
        let tape = Tape::from_events(&events).unwrap();
        assert_eq!(tape.final_live().keys().copied().collect::<Vec<_>>(), [AllocationId(1)]);
        assert_eq!(tape.requested_live(), Bytes(0));
        assert_eq!(tape.requested_peak(), Bytes(80));
    }
    #[test]
    fn calloc_product_and_overflow_are_explicit() {
        let tape = Tape::from_events(&pair(1, Operation::Calloc, 0, 3, 16, 0xa71a08, 1)).unwrap();
        assert_eq!(tape.requested_live(), Bytes(48));
        assert!(matches!(Tape::from_events(&pair(1, Operation::Calloc, 0, u32::MAX, 2, 0, 1)), Err(Failure::ArithmeticOverflow)));
    }
    #[test]
    fn incomplete_foreign_and_overlapping_calls_refuse() {
        let call = pair(1, Operation::Malloc, 0, 12, 0, 0xa71a08, 1);
        assert!(matches!(Tape::from_events(&call[..1]), Err(Failure::PartialTrace)));
        assert!(matches!(Tape::from_events(&pair(2, Operation::Free, 0xa71a08, 0xa71a08, 0, 0, 3)), Err(Failure::UnknownFree { .. })));
        let other = pair(2, Operation::Malloc, 0, 16, 0, 0xa71b08, 3);
        assert!(matches!(Tape::from_events(&[call[0].clone(), other[0].clone()]), Err(Failure::ConcurrentCalls)));
    }
}
