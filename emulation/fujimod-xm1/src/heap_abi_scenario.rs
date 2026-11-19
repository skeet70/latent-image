//! Directed general-heap ABI scenario, its per-call journal and the stock comparison.
//! Executors resolve slots to addresses; journals carry only logical requests and outcomes.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ERRNO_MARKER: u32 = 0x45;
const ERRNO_FAILURE: u32 = 12;
const EXHAUSTION_BYTES: u32 = 0x2_0000;
const EXHAUSTION_ATTEMPTS: u8 = 16;
/// The fragmented request needs released holes 1, 3, 5 and 7 flanked by live blocks up to 8.
const FRAGMENTATION_SUCCESSES: usize = 9;
const FIRST_EXHAUSTION_SLOT: u8 = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Slot(pub u8);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum Operation {
    Malloc { bytes: u32, into: Slot },
    Calloc { count: u32, element_bytes: u32, into: Slot },
    /// On failure the old slot keeps its allocation.
    Realloc { slot: Option<Slot>, bytes: u32, into: Slot },
    Free { slot: Slot },
    FreeNull,
    Trim { pad: u32 },
}

/// Approved stock-behaviour corrections, reports/general-heap-cutover.md#declared-corrections-and-caller-evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Correction { CheckedCallocProduct, OversizedResizeRejectedBeforeLock, TrimReturnsZero }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Movement { Compared, Unconstrained }

#[derive(Clone, Copy, Debug)]
pub struct Step { pub label: &'static str, pub operation: Operation, pub correction: Option<Correction>, pub movement: Movement }

fn step(label: &'static str, operation: Operation) -> Step {
    Step { label, operation, correction: None, movement: Movement::Compared }
}

fn corrected(label: &'static str, operation: Operation, correction: Correction) -> Step {
    Step { label, operation, correction: Some(correction), movement: Movement::Compared }
}

fn exhaustion(steps: &mut Vec<Step>, labels: &'static [&'static str; EXHAUSTION_ATTEMPTS as usize]) {
    for (index, label) in labels.iter().enumerate() {
        steps.push(step(label, Operation::Malloc { bytes: EXHAUSTION_BYTES, into: Slot(FIRST_EXHAUSTION_SLOT + index as u8) }));
    }
}

fn release_exhaustion(steps: &mut Vec<Step>, released: &[(&'static str, u8)]) {
    for &(label, index) in released {
        steps.push(step(label, Operation::Free { slot: Slot(FIRST_EXHAUSTION_SLOT + index) }));
    }
}

pub fn scenario() -> Vec<Step> {
    use Operation::*;
    let (zero, resized, moved, calloced, empty, dirty, rejected, wrapped, wrapped_zero, fragment, coalesced, reused) =
        (Slot(0), Slot(1), Slot(2), Slot(3), Slot(4), Slot(5), Slot(6), Slot(7), Slot(24), Slot(25), Slot(26), Slot(27));
    let first = Slot(FIRST_EXHAUSTION_SLOT);
    let mut steps = vec![
        step("zero-malloc", Malloc { bytes: 0, into: zero }),
        step("malloc", Malloc { bytes: 100, into: resized }),
        step("shrink", Realloc { slot: Some(resized), bytes: 40, into: resized }),
        Step { movement: Movement::Unconstrained, ..step("grow", Realloc { slot: Some(resized), bytes: 4000, into: resized }) },
        step("same-chunk-resize", Realloc { slot: Some(resized), bytes: 3996, into: resized }),
        step("zero-resize", Realloc { slot: Some(resized), bytes: 0, into: resized }),
        step("null-resize", Realloc { slot: None, bytes: 64, into: moved }),
        step("failed-resize", Realloc { slot: Some(moved), bytes: 0x40_0000, into: moved }),
        step("calloc", Calloc { count: 25, element_bytes: 4, into: calloced }),
        step("zero-calloc", Calloc { count: 0, element_bytes: 7, into: empty }),
        step("dirty-malloc", Malloc { bytes: 256, into: dirty }),
        step("dirty-free", Free { slot: dirty }),
        step("reused-calloc", Calloc { count: 64, element_bytes: 4, into: dirty }),
        step("null-free", FreeNull),
        step("capacity-malloc", Malloc { bytes: 0x7fff_fff4, into: rejected }),
        step("capacity-calloc", Calloc { count: 1, element_bytes: 0x40_0000, into: rejected }),
    ];
    exhaustion(&mut steps, &["exhaust-0", "exhaust-1", "exhaust-2", "exhaust-3", "exhaust-4", "exhaust-5", "exhaust-6", "exhaust-7",
        "exhaust-8", "exhaust-9", "exhaust-10", "exhaust-11", "exhaust-12", "exhaust-13", "exhaust-14", "exhaust-15"]);
    steps.extend([
        step("exhausted-resize", Realloc { slot: Some(first), bytes: 2 * EXHAUSTION_BYTES, into: first }),
        step("exhausted-calloc", Calloc { count: 1, element_bytes: EXHAUSTION_BYTES, into: rejected }),
    ]);
    release_exhaustion(&mut steps, &[("release-1", 1), ("release-3", 3), ("release-5", 5), ("release-7", 7)]);
    steps.push(step("fragmented-malloc", Malloc { bytes: 2 * EXHAUSTION_BYTES, into: fragment }));
    release_exhaustion(&mut steps, &[("release-0", 0), ("release-2", 2), ("release-4", 4), ("release-6", 6), ("release-8", 8), ("release-9", 9),
        ("release-10", 10), ("release-11", 11), ("release-12", 12), ("release-13", 13), ("release-14", 14), ("release-15", 15)]);
    steps.extend([
        step("fragment-free", Free { slot: fragment }),
        step("coalesced-malloc", Malloc { bytes: 10 * EXHAUSTION_BYTES, into: coalesced }),
        step("coalesced-free", Free { slot: coalesced }),
    ]);
    exhaustion(&mut steps, &["reuse-0", "reuse-1", "reuse-2", "reuse-3", "reuse-4", "reuse-5", "reuse-6", "reuse-7",
        "reuse-8", "reuse-9", "reuse-10", "reuse-11", "reuse-12", "reuse-13", "reuse-14", "reuse-15"]);
    release_exhaustion(&mut steps, &[("reuse-release-0", 0), ("reuse-release-1", 1), ("reuse-release-2", 2), ("reuse-release-3", 3),
        ("reuse-release-4", 4), ("reuse-release-5", 5), ("reuse-release-6", 6), ("reuse-release-7", 7), ("reuse-release-8", 8),
        ("reuse-release-9", 9), ("reuse-release-10", 10), ("reuse-release-11", 11), ("reuse-release-12", 12), ("reuse-release-13", 13),
        ("reuse-release-14", 14), ("reuse-release-15", 15)]);
    steps.extend([
        step("reused-large-calloc", Calloc { count: EXHAUSTION_BYTES, element_bytes: 1, into: reused }),
        step("reused-large-free", Free { slot: reused }),
        step("zero-free", Free { slot: zero }),
        step("resized-free", Free { slot: resized }),
        step("calloc-free", Free { slot: calloced }),
        step("empty-free", Free { slot: empty }),
        step("reused-calloc-free", Free { slot: dirty }),
        corrected("trim", Trim { pad: 0 }, Correction::TrimReturnsZero),
        corrected("wrapped-calloc", Calloc { count: 0x1_0000, element_bytes: 0x1_0001, into: wrapped }, Correction::CheckedCallocProduct),
        corrected("wrapped-zero-calloc", Calloc { count: 0x4000_0000, element_bytes: 4, into: wrapped_zero }, Correction::CheckedCallocProduct),
        corrected("oversized-resize", Realloc { slot: Some(moved), bytes: 0x7fff_fff5, into: moved }, Correction::OversizedResizeRejectedBeforeLock),
        corrected("maximum-resize", Realloc { slot: Some(moved), bytes: u32::MAX, into: moved }, Correction::OversizedResizeRejectedBeforeLock),
        step("resized-after-oversized-free", Free { slot: moved }),
    ]);
    steps
}

/// A request with addresses resolved by the executor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    Malloc { bytes: u32 },
    Calloc { count: u32, element_bytes: u32 },
    Realloc { old: Option<(u64, u32)>, bytes: u32 },
    Free { old: Option<(u64, u32)> },
    Trim { pad: u32 },
}

/// The logical form of a request recorded in a journal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum LogicalRequest {
    Malloc { bytes: u32 },
    Calloc { count: u32, element_bytes: u32 },
    Realloc { old_bytes: Option<u32>, bytes: u32 },
    Free { live: bool },
    Trim { pad: u32 },
}

impl Request {
    fn logical(self) -> LogicalRequest {
        match self {
            Self::Malloc { bytes } => LogicalRequest::Malloc { bytes },
            Self::Calloc { count, element_bytes } => LogicalRequest::Calloc { count, element_bytes },
            Self::Realloc { old, bytes } => LogicalRequest::Realloc { old_bytes: old.map(|(_, bytes)| bytes), bytes },
            Self::Free { old } => LogicalRequest::Free { live: old.is_some() },
            Self::Trim { pad } => LogicalRequest::Trim { pad },
        }
    }
}

/// Requests the Rust `GlobalAlloc` front end has no operation for; its callers handle them before the allocator.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inapplicable { ZeroSize, ProductOverflow, NullFree, Trim }

impl LogicalRequest {
    pub fn front_end(self) -> Option<Inapplicable> {
        match self {
            Self::Malloc { bytes: 0 } | Self::Realloc { bytes: 0, .. } => Some(Inapplicable::ZeroSize),
            Self::Calloc { count, element_bytes } => match count.checked_mul(element_bytes) {
                None => Some(Inapplicable::ProductOverflow),
                Some(0) => Some(Inapplicable::ZeroSize),
                Some(_) => None,
            },
            Self::Free { live: false } => Some(Inapplicable::NullFree),
            Self::Trim { .. } => Some(Inapplicable::Trim),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Lock { pub owner_restored: bool, pub depth_change: i64 }

impl Lock {
    pub const BALANCED: Self = Self { owner_restored: true, depth_change: 0 };
}

#[derive(Clone, Copy, Debug)]
pub struct Returned { pub result: u64, pub errno: Option<u32>, pub lock: Lock }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Outcome {
    NotApplicable { reason: Inapplicable },
    Null,
    Allocation { aligned: bool, moved: Option<bool> },
    Unit,
    Value { word: u32 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Payload {
    /// A successful resize keeps the smaller extent of the old bytes.
    Preserved { bytes: u32, intact: bool },
    /// A failed resize keeps the old allocation's bytes.
    Retained { bytes: u32, intact: bool },
    /// Calloc returns the product's bytes zeroed.
    Zeroed { bytes: u32, zero: bool },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Observation {
    pub step: usize,
    pub request: LogicalRequest,
    pub outcome: Outcome,
    pub errno: Option<u32>,
    pub lock: Option<Lock>,
    pub payload: Option<Payload>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Counterpart { OwnedAbi, RustFrontEnd }

pub trait Executor {
    fn counterpart(&self) -> Option<Counterpart>;
    fn call(&mut self, request: Request) -> Result<Returned, String>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), String>;
    fn read(&mut self, address: u64, length: usize) -> Result<Vec<u8>, String>;
}

fn pattern(slot: Slot, generation: u32, offset: usize) -> u8 {
    (u32::from(slot.0).wrapping_mul(97) ^ generation.wrapping_mul(29) ^ (offset as u32).wrapping_mul(151) ^ (offset as u32 >> 8)) as u8
}

#[derive(Clone, Copy)]
struct Live { address: u64, bytes: u32, generation: u32 }

fn fill(executor: &mut impl Executor, slot: Slot, live: Live) -> Result<(), String> {
    let bytes: Vec<u8> = (0..live.bytes as usize).map(|offset| pattern(slot, live.generation, offset)).collect();
    if bytes.is_empty() { Ok(()) } else { executor.write(live.address, &bytes) }
}

fn intact(executor: &mut impl Executor, slot: Slot, live: Live, address: u64, bytes: u32) -> Result<bool, String> {
    let observed = executor.read(address, bytes as usize)?;
    Ok(observed.iter().enumerate().all(|(offset, byte)| *byte == pattern(slot, live.generation, offset)))
}

/// Calloc's caller-visible extent: the checked product, or the wrapped product stock hands out.
fn calloc_extent(count: u32, element_bytes: u32) -> u32 { count.wrapping_mul(element_bytes) }

pub fn run(executor: &mut impl Executor) -> Result<Vec<Observation>, String> {
    let mut slots = BTreeMap::<Slot, Live>::new();
    let mut generation = 0u32;
    let mut journal = Vec::new();
    for (index, step) in scenario().iter().enumerate() {
        generation += 1;
        let old = |slot: Option<Slot>| slot.and_then(|slot| slots.get(&slot)).map(|live| (live.address, live.bytes));
        let request = match step.operation {
            Operation::Malloc { bytes, .. } => Request::Malloc { bytes },
            Operation::Calloc { count, element_bytes, .. } => Request::Calloc { count, element_bytes },
            Operation::Realloc { slot, bytes, .. } => Request::Realloc { old: old(slot), bytes },
            Operation::Free { slot } => Request::Free { old: old(Some(slot)) },
            Operation::FreeNull => Request::Free { old: None },
            Operation::Trim { pad } => Request::Trim { pad },
        };
        let logical = request.logical();
        if executor.counterpart() == Some(Counterpart::RustFrontEnd) {
            if let Some(reason) = logical.front_end() {
                journal.push(Observation { step: index, request: logical, outcome: Outcome::NotApplicable { reason }, errno: None, lock: None, payload: None });
                continue;
            }
        }
        let returned = executor.call(request)?;
        let mut payload = None;
        let outcome = match (step.operation, request) {
            (Operation::Malloc { into, .. } | Operation::Calloc { into, .. } | Operation::Realloc { into, .. }, _) if returned.result == 0 => {
                if let (Operation::Realloc { slot: Some(slot), .. }, Request::Realloc { old: Some((address, bytes)), .. }) = (step.operation, request) {
                    let live = slots[&slot];
                    payload = Some(Payload::Retained { bytes, intact: intact(executor, slot, live, address, bytes)? });
                } else {
                    slots.remove(&into);
                }
                Outcome::Null
            }
            (Operation::Malloc { bytes, into }, _) => {
                let live = Live { address: returned.result, bytes, generation };
                fill(executor, into, live)?;
                slots.insert(into, live);
                Outcome::Allocation { aligned: returned.result % 8 == 0, moved: None }
            }
            (Operation::Calloc { count, element_bytes, into }, _) => {
                let bytes = calloc_extent(count, element_bytes);
                let observed = executor.read(returned.result, bytes as usize)?;
                payload = Some(Payload::Zeroed { bytes, zero: observed.iter().all(|byte| *byte == 0) });
                let live = Live { address: returned.result, bytes, generation };
                fill(executor, into, live)?;
                slots.insert(into, live);
                Outcome::Allocation { aligned: returned.result % 8 == 0, moved: None }
            }
            (Operation::Realloc { slot, bytes, into }, Request::Realloc { old, .. }) => {
                let moved = old.map(|(address, _)| address != returned.result);
                if let (Some(slot), Some((_, old_bytes))) = (slot, old) {
                    let kept = old_bytes.min(bytes);
                    payload = Some(Payload::Preserved { bytes: kept, intact: intact(executor, slot, slots[&slot], returned.result, kept)? });
                }
                let live = Live { address: returned.result, bytes, generation };
                fill(executor, into, live)?;
                slots.insert(into, live);
                Outcome::Allocation { aligned: returned.result % 8 == 0, moved }
            }
            (Operation::Free { slot }, _) => {
                slots.remove(&slot);
                Outcome::Unit
            }
            (Operation::FreeNull, _) => Outcome::Unit,
            (Operation::Trim { .. }, _) => Outcome::Value { word: u32::try_from(returned.result).map_err(|_| "trim result exceeds a word")? },
            _ => return Err("scenario request and operation disagree".into()),
        };
        journal.push(Observation { step: index, request: logical, outcome, errno: returned.errno, lock: Some(returned.lock), payload });
    }
    Ok(journal)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Property { Alignment, Errno, Lock, Payload, Outcome, Movement, Request, Exhaustion }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side { Stock, Counterpart }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Mismatch {
    Shape { side: Side, observed: usize, expected: usize },
    RoleLocal { side: Side, step: usize, label: String, property: Property },
    Applicability { step: usize, label: String },
    Correction { step: usize, label: String, correction: Correction },
    Divergence { step: usize, label: String, property: Property },
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "{self:?}") }
}

fn allocating(request: LogicalRequest) -> bool {
    matches!(request, LogicalRequest::Malloc { .. } | LogicalRequest::Calloc { .. } | LogicalRequest::Realloc { .. })
}

fn role_local(side: Side, journal: &[Observation], steps: &[Step]) -> Result<(), Mismatch> {
    if journal.len() != steps.len() || journal.iter().enumerate().any(|(index, observation)| observation.step != index) {
        return Err(Mismatch::Shape { side, observed: journal.len(), expected: steps.len() });
    }
    for (observation, step) in journal.iter().zip(steps) {
        let fail = |property| Mismatch::RoleLocal { side, step: observation.step, label: step.label.into(), property };
        let leak_declared = side == Side::Stock && step.correction == Some(Correction::OversizedResizeRejectedBeforeLock);
        if let Some(lock) = observation.lock {
            if lock != Lock::BALANCED && !(leak_declared && lock.depth_change == 1) { return Err(fail(Property::Lock)); }
        }
        match observation.outcome {
            Outcome::Allocation { aligned: false, .. } => return Err(fail(Property::Alignment)),
            Outcome::Null if allocating(observation.request) && observation.errno.is_some_and(|errno| errno != ERRNO_FAILURE) => return Err(fail(Property::Errno)),
            Outcome::Allocation { .. } | Outcome::Unit | Outcome::Value { .. } if observation.errno.is_some_and(|errno| errno != ERRNO_MARKER) => return Err(fail(Property::Errno)),
            Outcome::NotApplicable { .. } if side == Side::Stock => return Err(fail(Property::Outcome)),
            _ => {}
        }
        match observation.payload {
            Some(Payload::Preserved { intact: false, .. } | Payload::Retained { intact: false, .. } | Payload::Zeroed { zero: false, .. }) => return Err(fail(Property::Payload)),
            _ => {}
        }
    }
    let first = successes(journal, steps, "exhaust-");
    if first < FRAGMENTATION_SUCCESSES || first >= usize::from(EXHAUSTION_ATTEMPTS) || successes(journal, steps, "reuse-") != first {
        return Err(Mismatch::RoleLocal { side, step: journal.len(), label: "exhaustion".into(), property: Property::Exhaustion });
    }
    Ok(())
}

fn declared(correction: Correction, counterpart: Counterpart, stock: &Observation, other: &Observation) -> bool {
    let balanced = |observation: &Observation| observation.lock == Some(Lock::BALANCED);
    match correction {
        Correction::CheckedCallocProduct => matches!(stock.outcome, Outcome::Allocation { .. }) && balanced(stock)
            && matches!(stock.request, LogicalRequest::Calloc { count, element_bytes } if count.checked_mul(element_bytes).is_none())
            && match counterpart {
                Counterpart::OwnedAbi => other.outcome == Outcome::Null && other.errno == Some(ERRNO_FAILURE) && balanced(other),
                Counterpart::RustFrontEnd => other.outcome == (Outcome::NotApplicable { reason: Inapplicable::ProductOverflow }),
            },
        Correction::OversizedResizeRejectedBeforeLock => stock.outcome == Outcome::Null && stock.errno == Some(ERRNO_FAILURE)
            && stock.lock.is_some_and(|lock| lock.depth_change == 1)
            && other.outcome == Outcome::Null && other.errno == stock.errno.filter(|_| counterpart == Counterpart::OwnedAbi)
            && balanced(other) && stock.payload.is_some() && other.payload == stock.payload,
        Correction::TrimReturnsZero => matches!(stock.outcome, Outcome::Value { word: 0 | 1 }) && balanced(stock)
            && match counterpart {
                Counterpart::OwnedAbi => other.outcome == (Outcome::Value { word: 0 }) && balanced(other) && other.errno == stock.errno,
                Counterpart::RustFrontEnd => other.outcome == (Outcome::NotApplicable { reason: Inapplicable::Trim }),
            },
    }
}

fn divergence(step: &Step, stock: &Observation, other: &Observation, counterpart: Counterpart) -> Option<Property> {
    if stock.request != other.request { return Some(Property::Request); }
    let class = |outcome: Outcome| match outcome {
        Outcome::Allocation { aligned, moved } => Outcome::Allocation { aligned, moved: moved.filter(|_| step.movement == Movement::Compared) },
        other => other,
    };
    if class(stock.outcome) != class(other.outcome) {
        return Some(match (stock.outcome, other.outcome) {
            (Outcome::Allocation { moved: a, .. }, Outcome::Allocation { moved: b, .. }) if a != b => Property::Movement,
            _ => Property::Outcome,
        });
    }
    if counterpart == Counterpart::OwnedAbi && stock.errno != other.errno { return Some(Property::Errno); }
    if stock.lock != other.lock { return Some(Property::Lock); }
    if stock.payload != other.payload { return Some(Property::Payload); }
    None
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Comparison {
    pub steps: usize,
    pub compared: usize,
    pub inapplicable: BTreeMap<String, usize>,
    pub corrections: Vec<CorrectionCase>,
    pub operations: BTreeMap<String, usize>,
    pub null_results: usize,
    pub first_exhaustion_successes: usize,
    pub reuse_exhaustion_successes: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CorrectionCase { pub label: String, pub correction: Correction, pub stock: Outcome, pub counterpart: Outcome, pub stock_lock: Option<Lock>, pub counterpart_lock: Option<Lock> }

fn successes(journal: &[Observation], steps: &[Step], prefix: &str) -> usize {
    journal.iter().zip(steps).filter(|(observation, step)| step.label.starts_with(prefix) && !step.label.contains("release")
        && matches!(observation.outcome, Outcome::Allocation { .. })).count()
}

pub fn compare(counterpart: Counterpart, stock: &[Observation], other: &[Observation]) -> Result<Comparison, Mismatch> {
    let steps = scenario();
    role_local(Side::Stock, stock, &steps)?;
    role_local(Side::Counterpart, other, &steps)?;
    let mut comparison = Comparison { steps: steps.len(), ..Comparison::default() };
    for ((stock, other), step) in stock.iter().zip(other).zip(&steps) {
        let operation = match stock.request {
            LogicalRequest::Malloc { .. } => "malloc", LogicalRequest::Calloc { .. } => "calloc", LogicalRequest::Realloc { .. } => "realloc",
            LogicalRequest::Free { .. } => "free", LogicalRequest::Trim { .. } => "trim",
        };
        *comparison.operations.entry(operation.into()).or_default() += 1;
        if stock.outcome == Outcome::Null { comparison.null_results += 1; }
        if let Some(correction) = step.correction {
            if stock.request != other.request || !declared(correction, counterpart, stock, other) {
                return Err(Mismatch::Correction { step: stock.step, label: step.label.into(), correction });
            }
            comparison.corrections.push(CorrectionCase { label: step.label.into(), correction, stock: stock.outcome, counterpart: other.outcome,
                stock_lock: stock.lock, counterpart_lock: other.lock });
            continue;
        }
        // An inapplicable front-end step leaves its slot unchanged, so later requests on that slot may differ in extent or liveness.
        if let Outcome::NotApplicable { reason } = other.outcome {
            if counterpart != Counterpart::RustFrontEnd || std::mem::discriminant(&stock.request) != std::mem::discriminant(&other.request)
                || other.request.front_end() != Some(reason) {
                return Err(Mismatch::Applicability { step: stock.step, label: step.label.into() });
            }
            *comparison.inapplicable.entry(format!("{reason:?}")).or_default() += 1;
            continue;
        }
        if let Some(property) = divergence(step, stock, other, counterpart) {
            return Err(Mismatch::Divergence { step: stock.step, label: step.label.into(), property });
        }
        comparison.compared += 1;
    }
    comparison.first_exhaustion_successes = successes(stock, &steps, "exhaust-");
    comparison.reuse_exhaustion_successes = successes(stock, &steps, "reuse-");
    Ok(comparison)
}

/// Serialized counterpart-journal defects and the comparison failure each must produce.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fault { FailedMalloc, ErrnoOnSuccess, LeakedFreeDepth, LostResizePrefix, LostFailedResizeBytes, DirtyCalloc, MovedShrink, TrimReleasesBacking, WrappedCalloc, OversizedResizeLeak, MissingStep, ReorderedSteps }

impl Fault {
    pub const ALL: [Self; 12] = [Self::FailedMalloc, Self::ErrnoOnSuccess, Self::LeakedFreeDepth, Self::LostResizePrefix, Self::LostFailedResizeBytes,
        Self::DirtyCalloc, Self::MovedShrink, Self::TrimReleasesBacking, Self::WrappedCalloc, Self::OversizedResizeLeak, Self::MissingStep, Self::ReorderedSteps];

    fn index(label: &str) -> usize {
        scenario().iter().position(|step| step.label == label).unwrap_or(usize::MAX)
    }

    fn target(self) -> &'static str {
        match self {
            Self::FailedMalloc | Self::ErrnoOnSuccess | Self::MissingStep | Self::ReorderedSteps => "malloc",
            Self::LeakedFreeDepth => "dirty-free",
            Self::LostResizePrefix => "grow",
            Self::LostFailedResizeBytes => "failed-resize",
            Self::DirtyCalloc => "reused-calloc",
            Self::MovedShrink => "shrink",
            Self::TrimReleasesBacking => "trim",
            Self::WrappedCalloc => "wrapped-calloc",
            Self::OversizedResizeLeak => "oversized-resize",
        }
    }

    /// `None` when the targeted observation is absent or has no call to corrupt.
    pub fn apply(self, counterpart: Counterpart, journal: &[Observation]) -> Option<Vec<Observation>> {
        let mut faulty = journal.to_vec();
        let index = Self::index(self.target());
        let observation = faulty.get_mut(index)?;
        if matches!(observation.outcome, Outcome::NotApplicable { .. }) || (self == Self::ErrnoOnSuccess && counterpart == Counterpart::RustFrontEnd) {
            return None;
        }
        match self {
            Self::FailedMalloc => { observation.outcome = Outcome::Null; observation.errno = observation.errno.map(|_| ERRNO_FAILURE); }
            Self::ErrnoOnSuccess => observation.errno = Some(ERRNO_FAILURE),
            Self::LeakedFreeDepth | Self::OversizedResizeLeak => observation.lock = Some(Lock { owner_restored: false, depth_change: 1 }),
            Self::LostResizePrefix => match &mut observation.payload { Some(Payload::Preserved { intact, .. }) => *intact = false, _ => return None },
            Self::LostFailedResizeBytes => match &mut observation.payload { Some(Payload::Retained { intact, .. }) => *intact = false, _ => return None },
            Self::DirtyCalloc => match &mut observation.payload { Some(Payload::Zeroed { zero, .. }) => *zero = false, _ => return None },
            Self::MovedShrink => match &mut observation.outcome { Outcome::Allocation { moved, .. } => *moved = Some(true), _ => return None },
            Self::TrimReleasesBacking => observation.outcome = Outcome::Value { word: 1 },
            Self::WrappedCalloc => { observation.outcome = Outcome::Allocation { aligned: true, moved: None }; observation.errno = Some(ERRNO_MARKER); }
            Self::MissingStep => { faulty.pop()?; }
            Self::ReorderedSteps => {
                let next = Self::index("shrink");
                if next >= faulty.len() { return None; }
                faulty.swap(index, next);
                faulty[index].step = index;
                faulty[next].step = next;
            }
        }
        Some(faulty)
    }

    pub fn targets(self, mismatch: &Mismatch) -> bool {
        match (self, mismatch) {
            (Self::FailedMalloc, Mismatch::Divergence { property: Property::Outcome, .. })
            | (Self::ErrnoOnSuccess, Mismatch::RoleLocal { side: Side::Counterpart, property: Property::Errno, .. })
            | (Self::LeakedFreeDepth, Mismatch::RoleLocal { side: Side::Counterpart, property: Property::Lock, .. })
            | (Self::LostResizePrefix | Self::LostFailedResizeBytes | Self::DirtyCalloc, Mismatch::RoleLocal { side: Side::Counterpart, property: Property::Payload, .. })
            | (Self::MovedShrink, Mismatch::Divergence { property: Property::Movement, .. })
            | (Self::TrimReleasesBacking | Self::WrappedCalloc, Mismatch::Correction { .. })
            | (Self::OversizedResizeLeak, Mismatch::RoleLocal { side: Side::Counterpart, property: Property::Lock, .. })
            | (Self::MissingStep, Mismatch::Shape { side: Side::Counterpart, .. })
            | (Self::ReorderedSteps, Mismatch::Divergence { property: Property::Request, .. }) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Refusal { pub fault: Fault, pub counterpart: Counterpart, pub refused: bool, pub targeted: bool, pub observed: Option<Mismatch> }

pub fn controls(counterpart: Counterpart, stock: &[Observation], other: &[Observation]) -> Vec<Refusal> {
    Fault::ALL.into_iter().filter_map(|fault| fault.apply(counterpart, other).map(|faulty| {
        let observed = compare(counterpart, stock, &faulty).err();
        Refusal { fault, counterpart, refused: observed.is_some(), targeted: observed.as_ref().is_some_and(|mismatch| fault.targets(mismatch)), observed }
    })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq)]
    enum Model { Stock, Owned, FrontEnd, OwnedDirtyCalloc, OwnedLeakingFree }

    const BASE: u64 = 0x1_0000;

    /// First-fit heap over a byte arena with the stock-versus-owned differences under test.
    struct Heap { model: Model, memory: Vec<u8>, blocks: Vec<(u64, u64, bool)>, errno: u32 }

    impl Heap {
        fn new(model: Model, bytes: u64) -> Self {
            Self { model, memory: vec![0x5a; bytes as usize], blocks: vec![(BASE, bytes, true)], errno: 0 }
        }

        fn allocate(&mut self, bytes: u64) -> u64 {
            let size = (bytes.max(1) + 7) & !7;
            let Some(index) = self.blocks.iter().position(|&(_, length, free)| free && length >= size) else { self.errno = ERRNO_FAILURE; return 0; };
            let (start, length, _) = self.blocks[index];
            self.blocks[index] = (start, size, false);
            if length > size { self.blocks.insert(index + 1, (start + size, length - size, true)); }
            start
        }

        fn release(&mut self, address: u64) {
            let index = self.blocks.iter().position(|&(start, _, free)| start == address && !free).expect("model frees a live block");
            self.blocks[index].2 = true;
            let mut merged: Vec<(u64, u64, bool)> = Vec::new();
            for block in self.blocks.drain(..) {
                match merged.last_mut() {
                    Some(last) if last.2 && block.2 => last.1 += block.1,
                    _ => merged.push(block),
                }
            }
            self.blocks = merged;
        }

        fn length(&self, address: u64) -> u64 {
            self.blocks.iter().find(|&&(start, _, _)| start == address).map(|&(_, length, _)| length).expect("model block")
        }
    }

    impl Executor for Heap {
        fn counterpart(&self) -> Option<Counterpart> {
            match self.model { Model::Stock => None, Model::FrontEnd => Some(Counterpart::RustFrontEnd), _ => Some(Counterpart::OwnedAbi) }
        }

        fn call(&mut self, request: Request) -> Result<Returned, String> {
            self.errno = ERRNO_MARKER;
            let stock = self.model == Model::Stock;
            let mut lock = Lock::BALANCED;
            let result = match request {
                Request::Malloc { bytes } if bytes > 0x7fff_fff4 => { self.errno = ERRNO_FAILURE; 0 }
                Request::Malloc { bytes } => self.allocate(u64::from(bytes)),
                Request::Calloc { count, element_bytes } => match count.checked_mul(element_bytes) {
                    None if !stock => { self.errno = ERRNO_FAILURE; 0 }
                    product => {
                        let bytes = product.unwrap_or(count.wrapping_mul(element_bytes));
                        let address = self.allocate(u64::from(bytes));
                        if address != 0 && self.model != Model::OwnedDirtyCalloc {
                            let offset = (address - BASE) as usize;
                            self.memory[offset..offset + bytes as usize].fill(0);
                        }
                        address
                    }
                },
                Request::Realloc { old: None, bytes } => self.allocate(u64::from(bytes)),
                Request::Realloc { old: Some(_), bytes } if bytes > 0x7fff_fff4 => {
                    self.errno = ERRNO_FAILURE;
                    if stock { lock = Lock { owner_restored: false, depth_change: 1 }; }
                    0
                }
                Request::Realloc { old: Some((address, old_bytes)), bytes } => {
                    if u64::from(bytes.max(1)) <= self.length(address) { address } else {
                        let moved = self.allocate(u64::from(bytes));
                        if moved != 0 {
                            let (from, to) = ((address - BASE) as usize, (moved - BASE) as usize);
                            self.memory.copy_within(from..from + old_bytes as usize, to);
                            self.release(address);
                        }
                        moved
                    }
                }
                Request::Free { old } => {
                    if let Some((address, _)) = old { self.release(address); }
                    if self.model == Model::OwnedLeakingFree && old.is_some() { lock.depth_change = 1; }
                    0
                }
                Request::Trim { .. } => u64::from(stock),
            };
            Ok(Returned { result, errno: (self.model != Model::FrontEnd).then_some(self.errno), lock })
        }

        fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), String> {
            let offset = (address - BASE) as usize;
            self.memory[offset..offset + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }

        fn read(&mut self, address: u64, length: usize) -> Result<Vec<u8>, String> {
            let offset = (address - BASE) as usize;
            Ok(self.memory[offset..offset + length].to_vec())
        }
    }

    fn journal(model: Model) -> Vec<Observation> {
        run(&mut Heap::new(model, 0x18_0000)).expect("model scenario runs")
    }

    #[test]
    fn owned_and_front_end_models_match_stock_except_declared_corrections() {
        let stock = journal(Model::Stock);
        let owned = compare(Counterpart::OwnedAbi, &stock, &journal(Model::Owned)).expect("owned model matches stock");
        assert_eq!(owned.compared + owned.corrections.len(), owned.steps);
        assert_eq!(owned.corrections.iter().map(|case| case.correction).collect::<Vec<_>>(), [Correction::TrimReturnsZero,
            Correction::CheckedCallocProduct, Correction::CheckedCallocProduct, Correction::OversizedResizeRejectedBeforeLock,
            Correction::OversizedResizeRejectedBeforeLock]);
        assert!(owned.first_exhaustion_successes > 0 && owned.first_exhaustion_successes == owned.reuse_exhaustion_successes);
        let front = compare(Counterpart::RustFrontEnd, &stock, &journal(Model::FrontEnd)).expect("front-end model matches stock");
        assert!(front.inapplicable.values().sum::<usize>() > 0 && front.compared < owned.compared);
    }

    #[test]
    fn executed_defects_are_refused() {
        let stock = journal(Model::Stock);
        assert!(matches!(compare(Counterpart::OwnedAbi, &stock, &journal(Model::OwnedDirtyCalloc)),
            Err(Mismatch::RoleLocal { side: Side::Counterpart, property: Property::Payload, .. })));
        assert!(matches!(compare(Counterpart::OwnedAbi, &stock, &journal(Model::OwnedLeakingFree)),
            Err(Mismatch::RoleLocal { side: Side::Counterpart, property: Property::Lock, .. })));
        assert!(matches!(compare(Counterpart::OwnedAbi, &stock, &journal(Model::Stock)),
            Err(Mismatch::RoleLocal { side: Side::Counterpart, property: Property::Lock, label, .. }) if label == "oversized-resize"));
    }

    #[test]
    fn every_journal_control_is_refused_on_target() {
        let stock = journal(Model::Stock);
        let owned = controls(Counterpart::OwnedAbi, &stock, &journal(Model::Owned));
        assert_eq!(owned.len(), Fault::ALL.len());
        let front = controls(Counterpart::RustFrontEnd, &stock, &journal(Model::FrontEnd));
        assert!(!front.is_empty());
        for refusal in owned.iter().chain(&front) {
            assert!(refusal.refused && refusal.targeted, "{refusal:?}");
        }
    }
}
