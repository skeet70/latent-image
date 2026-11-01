use crate::heap_accesses::Digest;
use crate::heap_replay_contract::*;
use crate::heap_replay_state::{AllocationOutcome, Bytes, Layout, Request, Tape};
use crate::heap_state::{Address, AllocationId, Counter, Operation};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

fn require(condition: bool, failure: Failure) -> Result<(), Failure> {
    if condition { Ok(()) } else { Err(failure) }
}

fn digest(bytes: &[u8]) -> Digest { Digest(format!("{:x}", Sha256::digest(bytes))) }

fn admit_code(inputs: &Inputs, tape: &Tape, case: &Case) -> Result<Vec<Span>, Failure> {
    let text = inputs.payload_text()?;
    require(text.start.0 == IMAGE_BASE && text.end.0 > text.start.0 && text.end.0 <= IMAGE_LIMIT, Failure::CodeExtent)?;
    let spans = code_spans(inputs, tape)?;
    require(case.code.len() == spans.len(), Failure::CodeExtent)?;
    for (binding, span) in case.code.iter().zip(&spans) {
        require(binding.span == *span, Failure::CodeExtent)?;
        let expected = digest(&expected_code(inputs, *span, case.capacity)?);
        require(binding.expected == expected && binding.before == expected && binding.after.as_ref() == Some(&expected), Failure::CodeWrite)?;
    }
    let watch = case.code_watch.as_ref().ok_or(Failure::CodeWrite)?;
    require(watch.enabled && watch.violation.is_none() && watch.classes.len() == spans.len()
        && watch.classes.iter().zip(&spans).all(|(class, span)| {
            class.range == [u64::from(span.start.0), u64::from(span.end.0)]
                && class.kinds.as_slice() == [AccessKind::CpuWrite, AccessKind::HostWrite, AccessKind::ModelBusWrite]
        })
        && watch.ranges == merged_watch_ranges(&spans) && watch.views == Views::Observe, Failure::CodeWrite)?;
    Ok(spans)
}

pub fn admit_call(call: &Call, spans: &[Span]) -> Result<Instructions, Failure> {
    let before = &call.before;
    let after = &call.after;
    require(before.registers[..4] == call.arguments.map(|word| word.0)
        && before.registers[15] == call.function.0 && before.registers[14] == RETURN_ADDRESS
        && before.registers[13] == STACK_END, Failure::Aapcs)?;
    require(before.registers[4..12] == after.registers[4..12] && before.registers[13] == after.registers[13]
        && before.d8_d15 == after.d8_d15, Failure::Aapcs)?;
    require(after.registers[15] == RETURN_ADDRESS
        && matches!(call.harness.event, Some(HarnessEvent::Return { pc }) if pc == Address(RETURN_ADDRESS)), Failure::Return)?;
    for frame in [before, after] {
        require(frame.cpsr.0 & 0xbf == 0x13 && frame.cpsr.0 & 0x01000000 == 0 && frame.root == Address(0x4004dae0)
            && frame.task_record == Address(0x40060ecc) && frame.kernel_context == Word(0)
            && frame.identity == Word(0x4003) && frame.owner == Word(0) && frame.depth == Word(0), Failure::Context)?;
    }
    require(before.cpsr.0 & 0x3ff == after.cpsr.0 & 0x3ff, Failure::Context)?;
    let elapsed = call.elapsed()?;
    require(elapsed.0 > 0 && elapsed.0 <= CALL_BUDGET, Failure::Budget)?;
    let guard = &call.harness.guard;
    let range_sum = guard.range_fetches.iter().try_fold(0u64, |sum, count| sum.checked_add(count.0).ok_or(Failure::Clock))?;
    let count_sum = call.harness.counts.iter().try_fold(0u64, |sum, count| sum.checked_add(count.0).ok_or(Failure::Clock))?;
    require(before.counts[1] == after.counts[1] && call.harness.trace_total.0 == elapsed.0
        && count_sum == elapsed.0 && range_sum == elapsed.0, Failure::Clock)?;
    require(guard.enabled && !guard.halted && guard.checked_fetches.0 == elapsed.0
        && guard.accepted_fetches.0 == elapsed.0 && guard.vector_fetches.0 == 0
        && guard.range_fetches.len() == spans.len() && call.harness.counts.len() == spans.len()
        && call.harness.counts == guard.range_fetches && call.count_ranges.as_slice() == spans
        && guard.vector_ranges.is_empty() && guard.vector_range_fetches.is_empty()
        && guard.ranges.as_ref().is_some_and(|ranges| ranges.len() == spans.len()
            && ranges.iter().zip(spans).all(|(range, span)| *range == [u64::from(span.start.0), u64::from(span.end.0)])), Failure::Evidence)?;
    require(spans.iter().any(|span| span.contains(call.function, Bytes(4))), Failure::CodeExtent)?;
    Ok(elapsed)
}

pub fn admit_payload(data: &Data, role: DataRole, identity: AllocationId, pointer: Address, bytes: Bytes) -> Result<(), Failure> {
    require(data.role == role && data.identity == identity && data.pointer == pointer
        && data.bytes.0.len() == bytes.0 as usize, Failure::Data)?;
    require(data.bytes.0.iter().enumerate().all(|(offset, byte)| *byte == if role == DataRole::ZeroedCalloc { 0 }
        else { (identity.0 as u8).wrapping_mul(73) ^ (offset as u8).wrapping_mul(151) }), Failure::Data)
}

pub fn admit_calloc_layout(count: Bytes, element: Bytes, layout: Layout) -> Result<(), Failure> {
    require(count.0.checked_mul(element.0) == Some(layout.size.0) && layout.size.0 <= 0x7ffffff4, Failure::Tape)
}

pub fn resize_copy_bytes(previous: Layout, next: Layout) -> Bytes {
    Bytes(previous.size.0.min(next.size.0))
}

pub fn admit_statistics_bounds(stats: Stats, capacity: Capacity, allocated: Bytes) -> Result<Stats, Failure> {
    let accounted = stats.allocated_block_bytes.0.checked_add(stats.free_block_bytes.0).ok_or(Failure::Statistics)?;
    require(stats.allocated_block_bytes == allocated && accounted <= stats.pool_managed_bytes.0
        && stats.pool_managed_bytes.0 <= capacity.bytes().0
        && stats.largest_free_block_bytes.0 <= stats.free_block_bytes.0
        && (stats.free_block_bytes.0 == 0) == (stats.largest_free_block_bytes.0 == 0), Failure::Statistics)?;
    Ok(stats)
}

pub fn distribution(values: &[Instructions]) -> Result<Distribution, Failure> {
    require(!values.is_empty(), Failure::Evidence)?;
    let mut sorted: Vec<_> = values.iter().map(|value| value.0).collect();
    sorted.sort_unstable();
    let percentile = |percent: usize| Instructions(sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)]);
    Ok(Distribution { samples: Counter(sorted.len() as u64), minimum: Instructions(sorted[0]), median: percentile(50),
        p95: percentile(95), p99: percentile(99), maximum: Instructions(sorted[sorted.len() - 1]) })
}

#[derive(Clone, Copy)]
struct Live { pointer: Address, layout: Layout, footprint: Bytes }

fn live_bytes(live: &BTreeMap<AllocationId, Live>) -> Result<Bytes, Failure> {
    live.values().try_fold(0u32, |sum, allocation| sum.checked_add(allocation.footprint.0).ok_or(Failure::Statistics)).map(Bytes)
}

fn requested_bytes(live: &BTreeMap<AllocationId, Live>) -> Result<Bytes, Failure> {
    live.values().try_fold(0u32, |sum, allocation| sum.checked_add(allocation.layout.size.0).ok_or(Failure::Tape)).map(Bytes)
}

fn admit_extent(capacity: Capacity, allocation: Live, live: &BTreeMap<AllocationId, Live>) -> Result<(), Failure> {
    let extent = Span { start: Address(POOL_BASE), end: Address(POOL_BASE + capacity.bytes().0) };
    let payload = Bytes(allocation.layout.size.0.max(1));
    require(allocation.pointer.0 != 0 && allocation.pointer.0 & 7 == 0
        && extent.contains(allocation.pointer, payload) && allocation.footprint.0 >= payload.0
        && allocation.footprint.0 <= capacity.bytes().0, Failure::Footprint)?;
    let end = allocation.pointer.0.checked_add(payload.0).ok_or(Failure::Overlap)?;
    for previous in live.values() {
        let previous_end = previous.pointer.0.checked_add(previous.layout.size.0.max(1)).ok_or(Failure::Overlap)?;
        require(end <= previous.pointer.0 || previous_end <= allocation.pointer.0, Failure::Overlap)?;
    }
    Ok(())
}

struct Cursor<'a> { inputs: &'a Inputs, case: &'a Case, next: usize }
impl<'a> Cursor<'a> {
    fn take(&mut self, role: CallRole, function: Address, arguments: [u32; 4]) -> Result<&'a Call, Failure> {
        let call = self.case.calls.get(self.next).ok_or(Failure::Incomplete)?;
        require(call.identity == CallId(self.next) && call.role == role && call.function == function
            && call.arguments == arguments.map(Word), Failure::Evidence)?;
        self.next += 1;
        Ok(call)
    }
    fn candidate(&mut self, role: CallRole, symbol: Symbol, arguments: [u32; 4]) -> Result<&'a Call, Failure> {
        self.take(role, self.inputs.symbol(symbol)?, arguments)
    }
    fn success(call: &Call) -> Result<(), Failure> {
        require(call.status == Some(TargetStatus::Success), Failure::BackendFailure)
    }
    fn footprint(&mut self, evidence: &Footprint, pointer: Address, layout: Layout) -> Result<Bytes, Failure> {
        match evidence {
            Footprint::Stock { pointer: observed, header } if self.case.backend == Backend::Stock => {
                require(*observed == pointer && header.0 & 2 == 0, Failure::Footprint)?;
                let bytes = header.0 & !3;
                require(bytes >= 16 && bytes & 7 == 0 && bytes.checked_sub(4).is_some_and(|usable| usable >= layout.size.0), Failure::Footprint)?;
                Ok(Bytes(bytes))
            }
            Footprint::Candidate { call } if self.case.backend != Backend::Stock => {
                let raw = self.candidate(CallRole::Footprint, Symbol::Footprint, [pointer.0, layout.size.0, layout.alignment.bytes(), 0])?;
                require(raw.identity == *call, Failure::Footprint)?;
                Self::success(raw).map_err(|_| Failure::Footprint)?;
                Ok(Bytes(raw.after.registers[0]))
            }
            _ => Err(Failure::Footprint),
        }
    }
    fn statistics(&mut self, evidence: &Statistics, live: &BTreeMap<AllocationId, Live>, additional_allocated: Bytes) -> Result<Stats, Failure> {
        let stats = match evidence {
            Statistics::Candidate { call, output, words } if self.case.backend != Backend::Stock => {
                require(*output == Address(STATS_ADDRESS), Failure::Statistics)?;
                let raw = self.candidate(CallRole::Statistics, Symbol::Statistics, [STATS_ADDRESS, 0, 0, 0])?;
                require(raw.identity == *call && raw.after.registers[0] == 0, Failure::Statistics)?;
                Self::success(raw).map_err(|_| Failure::Statistics)?;
                let bss = self.inputs.symbol(Symbol::BssEnd)?.0.checked_sub(self.inputs.symbol(Symbol::BssStart)?.0).ok_or(Failure::Symbols)?;
                require(words[3].0 >= 4 && words[3].0 <= bss, Failure::Statistics)?;
                Stats { allocated_block_bytes: Bytes(words[0].0), free_block_bytes: Bytes(words[1].0), largest_free_block_bytes: Bytes(words[2].0),
                    external_metadata_bytes: Some(Bytes(words[3].0)), pool_managed_bytes: Bytes(words[4].0) }
            }
            Statistics::Stock { heap_break, top, chunks, external_metadata_bytes } if self.case.backend == Backend::Stock => {
                let frame = if self.next == 0 { self.case.calls.first().map(|call| &call.before) }
                    else { self.case.calls.get(self.next - 1).map(|call| &call.after) };
                require(frame.is_none_or(|frame| frame.heap_break == *heap_break), Failure::Statistics)?;
                stock_statistics(*heap_break, *top, chunks, *external_metadata_bytes, live)?
            }
            _ => return Err(Failure::Statistics),
        };
        let allocated = Bytes(live_bytes(live)?.0.checked_add(additional_allocated.0).ok_or(Failure::Statistics)?);
        admit_statistics_bounds(stats, self.case.capacity, allocated)
    }
}

fn stock_statistics(heap_break: Address, top: Address, chunks: &[ChunkHeader], metadata: Option<Bytes>, live: &BTreeMap<AllocationId, Live>) -> Result<Stats, Failure> {
    require(metadata == Some(STOCK_EXTERNAL_METADATA), Failure::Statistics)?;
    if chunks.is_empty() {
        require(live.is_empty() && heap_break == Address(POOL_BASE)
            && !(POOL_BASE..POOL_BASE + Capacity::Original.bytes().0).contains(&top.0), Failure::Statistics)?;
        return Ok(Stats { allocated_block_bytes: Bytes(0), free_block_bytes: Bytes(0), largest_free_block_bytes: Bytes(0),
            external_metadata_bytes: Some(STOCK_EXTERNAL_METADATA), pool_managed_bytes: Bytes(0) });
    }
    require(chunks[0].address == Address(POOL_BASE), Failure::Statistics)?;
    let mut cursor = POOL_BASE;
    let mut allocated = 0u32;
    let mut free = 0u32;
    let mut largest = 0u32;
    let mut found = 0usize;
    let mut saw_top = false;
    for (index, chunk) in chunks.iter().enumerate() {
        let bytes = chunk.word.0 & !3;
        require(chunk.address.0 == cursor && bytes >= 16 && bytes & 7 == 0 && chunk.word.0 & 2 == 0, Failure::Statistics)?;
        cursor = cursor.checked_add(bytes).ok_or(Failure::Statistics)?;
        require(cursor <= heap_break.0, Failure::Statistics)?;
        let occupied = if chunk.address == top {
            require(index + 1 == chunks.len(), Failure::Statistics)?;
            saw_top = true;
            false
        } else {
            chunks.get(index + 1).ok_or(Failure::Statistics)?.word.0 & 1 != 0
        };
        if occupied {
            let pointer = Address(chunk.address.0 + 8);
            let allocation = live.values().find(|allocation| allocation.pointer == pointer).ok_or(Failure::Statistics)?;
            require(allocation.footprint == Bytes(bytes), Failure::Statistics)?;
            found += 1;
            allocated = allocated.checked_add(bytes).ok_or(Failure::Statistics)?;
        } else {
            free = free.checked_add(bytes).ok_or(Failure::Statistics)?;
            largest = largest.max(bytes);
        }
    }
    require(saw_top && cursor == heap_break.0 && found == live.len(), Failure::Statistics)?;
    Ok(Stats { allocated_block_bytes: Bytes(allocated), free_block_bytes: Bytes(free), largest_free_block_bytes: Bytes(largest),
        external_metadata_bytes: Some(STOCK_EXTERNAL_METADATA), pool_managed_bytes: Bytes(heap_break.0.checked_sub(POOL_BASE).ok_or(Failure::Statistics)?) })
}

fn allocate_call<'a>(cursor: &mut Cursor<'a>, request: &Request, old: Option<Live>) -> Result<&'a Call, Failure> {
    let (role, layout) = match request {
        Request::Malloc { layout, .. } => (CallRole::Allocate, *layout),
        Request::Calloc { count, element, layout, .. } => {
            admit_calloc_layout(*count, *element, *layout)?;
            (CallRole::Calloc, *layout)
        }
        Request::Realloc { layout, .. } => (CallRole::Resize, *layout),
        Request::Free { .. } => return Err(Failure::Tape),
    };
    if cursor.case.backend != Backend::Stock {
        return cursor.candidate(role, Symbol::Allocate, [layout.size.0, layout.alignment.bytes(), 0, 0]);
    }
    match request {
        Request::Malloc { .. } => cursor.take(role, Address(0x89cda4), [layout.size.0, 0, 0, 0]),
        Request::Calloc { count, element, .. } => cursor.take(role, Address(0xbf1c1c), [count.0, element.0, 0, 0]),
        Request::Realloc { .. } => cursor.take(role, Address(0x89da3c), [old.map_or(0, |allocation| allocation.pointer.0), layout.size.0, 0, 0]),
        Request::Free { .. } => Err(Failure::Tape),
    }
}

fn free_call(cursor: &mut Cursor<'_>, allocation: Option<Live>) -> Result<(), Failure> {
    let pointer = allocation.map_or(0, |value| value.pointer.0);
    if cursor.case.backend == Backend::Stock {
        cursor.take(CallRole::Free, Address(0x89cdc4), [pointer, 0, 0, 0])?;
    } else {
        let raw = cursor.candidate(CallRole::Free, Symbol::Free,
            [pointer, allocation.map_or(0, |value| value.layout.size.0), allocation.map_or(8, |value| value.layout.alignment.bytes()), 0])?;
        Cursor::success(raw)?;
        require(raw.after.registers[0] == 0, Failure::BackendFailure)?;
    }
    Ok(())
}

fn check_data(records: &[Data], expected: &[(DataRole, AllocationId, Address, Bytes)]) -> Result<(), Failure> {
    require(records.len() == expected.len(), Failure::Data)?;
    for (record, (role, identity, pointer, bytes)) in records.iter().zip(expected) {
        admit_payload(record, *role, *identity, *pointer, *bytes)?;
    }
    Ok(())
}

pub fn admit(inputs: &Inputs, tape: &Tape, case: &Case) -> Result<CaseSummary, Failure> {
    require(case.schema == 1 && case.bindings == inputs.bindings, Failure::InputIdentity)?;
    require(case.complete && case.error.is_none() && case.steps.len() == tape.steps().len(), Failure::Incomplete)?;
    require(inputs.bindings.events.0 != "798b8dd160509598a2b01d4f37bb811794c3bd247028bcd7c48510fe528e6bab"
        || tape.steps().len() == 8529, Failure::Tape)?;
    let spans = admit_code(inputs, tape, case)?;
    let mut previous_counts = if inputs.bindings.events.0 == "798b8dd160509598a2b01d4f37bb811794c3bd247028bcd7c48510fe528e6bab" {
        Some([Instructions(26_185_192), Instructions(3_214_362)])
    } else { None };
    for (index, call) in case.calls.iter().enumerate() {
        require(call.identity == CallId(index), Failure::Evidence)?;
        admit_call(call, &spans)?;
        if let Some(counts) = previous_counts { require(call.before.counts == counts, Failure::Clock)?; }
        previous_counts = Some(call.after.counts);
        require((case.backend == Backend::Stock) == call.status.is_none(), Failure::BackendFailure)?;
        for frame in [&call.before, &call.after] {
            require((POOL_BASE..=POOL_BASE + case.capacity.bytes().0).contains(&frame.heap_break.0), Failure::Statistics)?;
            if case.backend != Backend::Stock { require(frame.heap_break == Address(POOL_BASE), Failure::Statistics)?; }
        }
    }
    let mut cursor = Cursor { inputs, case, next: 0 };
    if let Some(backend) = case.backend.wire() {
        let init = cursor.candidate(CallRole::Initialize, Symbol::Init, [POOL_BASE, case.capacity.bytes().0, backend, 0])?;
        Cursor::success(init)?;
        require(init.after.registers[0] == 0, Failure::BackendFailure)?;
    }
    let mut live = BTreeMap::<AllocationId, Live>::new();
    let initial = cursor.statistics(case.initial_statistics.as_ref().ok_or(Failure::Statistics)?, &live, Bytes(0))?;
    let mut permanent = initial.pool_managed_bytes.0.checked_sub(initial.free_block_bytes.0).ok_or(Failure::Statistics)?;
    let metadata = initial.external_metadata_bytes;
    let mut peak_allocated = 0u32;
    let mut peak_requested = 0u32;
    let mut minimum_hole = if initial.pool_managed_bytes.0 == 0 { case.capacity.bytes().0 } else { initial.largest_free_block_bytes.0 };
    let mut samples = BTreeMap::<Operation, (Vec<Instructions>, Vec<Instructions>, Vec<Instructions>)>::new();
    let mut expected_data = Vec::with_capacity(2);
    for (index, (step, record)) in tape.steps().iter().zip(&case.steps).enumerate() {
        require(record.index == StepIndex(index) && record.sequence == step.sequence, Failure::Incomplete)?;
        require(step.core.0 == 0 && step.context.domain.0 == 0 && step.context.mode == 0x13
            && step.context.exception_depth == 0, Failure::Context)?;
        let first_call = cursor.next;
        expected_data.clear();
        let mut stats_index = 0usize;
        match &step.request {
            Request::Free { identity, layout } => {
                let old = identity.map(|identity| live.get(&identity).copied().ok_or(Failure::Divergence)).transpose()?;
                require(old.map(|allocation| allocation.layout) == *layout, Failure::Divergence)?;
                if let (Some(identity), Some(old)) = (identity, old) {
                    expected_data.push((DataRole::BeforeFree, *identity, old.pointer, old.layout.size));
                }
                require(record.footprint.is_none() && record.result == Address(0), Failure::Footprint)?;
                free_call(&mut cursor, old)?;
                if let Some(identity) = identity { live.remove(identity); }
            }
            request => {
                let (layout, outcome, previous, previous_layout) = match request {
                    Request::Malloc { layout, outcome } | Request::Calloc { layout, outcome, .. } => (*layout, *outcome, None, None),
                    Request::Realloc { previous, previous_layout, layout, outcome } => (*layout, *outcome, *previous, *previous_layout),
                    Request::Free { .. } => return Err(Failure::Tape),
                };
                let old = previous.map(|identity| live.get(&identity).copied().ok_or(Failure::Divergence)).transpose()?;
                require(old.map(|allocation| allocation.layout) == previous_layout, Failure::Divergence)?;
                if let (Some(identity), Some(old)) = (previous, old) {
                    expected_data.push((DataRole::BeforeResize, identity, old.pointer, old.layout.size));
                }
                let allocation_call = allocate_call(&mut cursor, request, old)?;
                let result = Address(allocation_call.after.registers[0]);
                require(record.result == result, Failure::Divergence)?;
                match outcome {
                    AllocationOutcome::Failure => {
                        require(result == Address(0) && record.footprint.is_none(), Failure::Divergence)?;
                        if case.backend != Backend::Stock {
                            require(allocation_call.status == Some(TargetStatus::OutOfMemory), Failure::Divergence)?;
                        }
                        if let (Some(identity), Some(old)) = (previous, old) {
                            expected_data.push((DataRole::PreservedResize, identity, old.pointer, old.layout.size));
                        }
                    }
                    AllocationOutcome::Success { identity } => {
                        require(result != Address(0) && identity.0 != 0 && previous.is_none_or(|previous| previous == identity), Failure::Divergence)?;
                        if case.backend != Backend::Stock { Cursor::success(allocation_call).map_err(|_| Failure::Divergence)?; }
                        let footprint = cursor.footprint(record.footprint.as_ref().ok_or(Failure::Footprint)?, result, layout)?;
                        let allocation = Live { pointer: result, layout, footprint };
                        if case.backend == Backend::Stock && previous.is_some() {
                            live.remove(&identity);
                        }
                        admit_extent(case.capacity, allocation, &live)?;
                        if case.backend != Backend::Stock && old.is_some() {
                            let stats = cursor.statistics(record.statistics.get(stats_index).ok_or(Failure::Statistics)?, &live, footprint)?;
                            stats_index += 1;
                            peak_allocated = peak_allocated.max(stats.allocated_block_bytes.0);
                            check_metadata(stats, metadata, &mut permanent, case.backend)?;
                            minimum_hole = minimum_hole.min(stats.largest_free_block_bytes.0);
                        }
                        if let Some(old) = old {
                            let bytes = resize_copy_bytes(old.layout, layout);
                            expected_data.push((DataRole::PreservedResize, identity, result, bytes));
                            if case.backend != Backend::Stock {
                                let copy = cursor.candidate(CallRole::PayloadCopy, Symbol::Copy, [result.0, old.pointer.0, bytes.0, 0])?;
                                Cursor::success(copy)?;
                                require(copy.after.registers[0] == 0, Failure::BackendFailure)?;
                                free_call(&mut cursor, Some(old))?;
                                live.remove(&identity);
                            }
                        }
                        if matches!(request, Request::Calloc { .. }) {
                            if case.backend != Backend::Stock {
                                let zero = cursor.candidate(CallRole::PayloadZero, Symbol::Zero, [result.0, layout.size.0, 0, 0])?;
                                Cursor::success(zero)?;
                                require(zero.after.registers[0] == 0, Failure::BackendFailure)?;
                            }
                            expected_data.push((DataRole::ZeroedCalloc, identity, result, layout.size));
                        }
                        require(live.insert(identity, allocation).is_none(), Failure::Divergence)?;
                    }
                }
            }
        }
        check_data(&record.data, &expected_data)?;
        require(record.statistics.len() == stats_index + 1, Failure::Statistics)?;
        let stats = cursor.statistics(&record.statistics[stats_index], &live, Bytes(0))?;
        check_metadata(stats, metadata, &mut permanent, case.backend)?;
        peak_allocated = peak_allocated.max(stats.allocated_block_bytes.0);
        peak_requested = peak_requested.max(requested_bytes(&live)?.0);
        minimum_hole = minimum_hole.min(stats.largest_free_block_bytes.0);
        require(record.calls.len() == cursor.next - first_call
            && record.calls.iter().zip(first_call..cursor.next).all(|(call, index)| *call == CallId(index)), Failure::Evidence)?;
        let mut allocator = 0u64;
        let mut payload = 0u64;
        for call in &case.calls[first_call..cursor.next] {
            let elapsed = call.elapsed()?.0;
            match call.role {
                CallRole::Allocate | CallRole::Free | CallRole::Resize | CallRole::Calloc =>
                    allocator = allocator.checked_add(elapsed).ok_or(Failure::Clock)?,
                CallRole::PayloadCopy | CallRole::PayloadZero =>
                    payload = payload.checked_add(elapsed).ok_or(Failure::Clock)?,
                CallRole::Footprint | CallRole::Statistics => {}
                CallRole::Initialize => return Err(Failure::Evidence),
            }
        }
        let entry = allocator.checked_add(payload).ok_or(Failure::Clock)?;
        let sample = samples.entry(step.request.operation()).or_default();
        sample.0.push(Instructions(entry));
        sample.1.push(Instructions(allocator));
        sample.2.push(Instructions(payload));
    }
    require(live.len() == tape.final_live().len() && live.iter().zip(tape.final_live())
        .all(|((identity, value), (expected_identity, layout))| identity == expected_identity && value.layout == *layout), Failure::Divergence)?;
    require(case.final_data.len() == live.len(), Failure::Data)?;
    for (data, (identity, allocation)) in case.final_data.iter().zip(&live) {
        admit_payload(data, DataRole::FinalLive, *identity, allocation.pointer, allocation.layout.size)?;
    }
    if case.backend != Backend::Stock {
        for allocation in live.values() {
            let call = cursor.candidate(CallRole::Footprint, Symbol::Footprint,
                [allocation.pointer.0, allocation.layout.size.0, allocation.layout.alignment.bytes(), 0])?;
            Cursor::success(call).map_err(|_| Failure::Footprint)?;
            require(call.after.registers[0] == allocation.footprint.0, Failure::Footprint)?;
        }
    }
    let final_stats = cursor.statistics(case.final_statistics.as_ref().ok_or(Failure::Statistics)?, &live, Bytes(0))?;
    check_metadata(final_stats, metadata, &mut permanent, case.backend)?;
    minimum_hole = minimum_hole.min(final_stats.largest_free_block_bytes.0);
    require(cursor.next == case.calls.len(), Failure::Incomplete)?;
    let final_requested = requested_bytes(&live)?;
    require(final_requested == tape.requested_live() && Bytes(peak_requested) == tape.requested_peak(), Failure::Tape)?;
    let operations = samples.into_iter().map(|(operation, (entry, allocator, payload))| Ok(OperationDistribution {
        operation, entry_instructions: distribution(&entry)?, allocator_instructions: distribution(&allocator)?, payload_instructions: distribution(&payload)?,
    })).collect::<Result<_, Failure>>()?;
    Ok(CaseSummary { backend: case.backend, capacity: case.capacity, steps: Counter(case.steps.len() as u64),
        physical_peak_scope: if tape.steps().iter().all(|step| matches!(&step.request, Request::Malloc { .. } | Request::Free { .. })) {
            PhysicalPeakScope::CompleteMallocFreeTrace
        } else { PhysicalPeakScope::BoundaryAndCandidateTransient },
        peak_allocated_block_bytes: Bytes(peak_allocated), peak_requested_bytes: Bytes(peak_requested), final_requested_bytes: final_requested,
        final_allocated_block_bytes: final_stats.allocated_block_bytes, final_free_block_bytes: final_stats.free_block_bytes,
        smallest_largest_free_block_bytes: Bytes(minimum_hole), external_metadata_bytes: metadata,
        payload_text_bytes: inputs.payload_text()?.bytes(), operations })
}

fn check_metadata(stats: Stats, metadata: Option<Bytes>, permanent: &mut u32, backend: Backend) -> Result<(), Failure> {
    require(stats.external_metadata_bytes == metadata, Failure::Statistics)?;
    let overhead = stats.pool_managed_bytes.0.checked_sub(stats.allocated_block_bytes.0)
        .and_then(|bytes| bytes.checked_sub(stats.free_block_bytes.0)).ok_or(Failure::Statistics)?;
    if backend != Backend::Stock { require(overhead == *permanent, Failure::Statistics)?; }
    else { *permanent = overhead; }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlKind { DroppedStep, AllocationResult, Footprint, Statistics, Counter, DiagnosticRole, PayloadData, CodeWatch }

impl ControlKind {
    const ALL: [Self; 8] = [Self::DroppedStep, Self::AllocationResult, Self::Footprint, Self::Statistics,
        Self::Counter, Self::DiagnosticRole, Self::PayloadData, Self::CodeWatch];

    fn name(self) -> &'static str {
        match self {
            Self::DroppedStep => "dropped-step", Self::AllocationResult => "allocation-result",
            Self::Footprint => "footprint", Self::Statistics => "statistics", Self::Counter => "counter",
            Self::DiagnosticRole => "diagnostic-role", Self::PayloadData => "payload-data", Self::CodeWatch => "code-watch",
        }
    }

    fn expected(self) -> Failure {
        match self {
            Self::DroppedStep => Failure::Incomplete, Self::AllocationResult => Failure::Divergence,
            Self::Footprint => Failure::Footprint, Self::Statistics => Failure::Statistics,
            Self::Counter => Failure::Clock, Self::DiagnosticRole => Failure::Evidence,
            Self::PayloadData => Failure::Data, Self::CodeWatch => Failure::CodeWrite,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ControlResult { pub control: ControlKind, pub expected: Failure, pub observed: Option<Failure> }

fn mutate(case: &mut Case, kind: ControlKind) -> Result<(), Failure> {
    match kind {
        ControlKind::DroppedStep => { case.steps.pop().ok_or(Failure::Evidence)?; }
        ControlKind::AllocationResult => {
            let step = case.steps.iter_mut().find(|step| step.footprint.is_some()).ok_or(Failure::Evidence)?;
            step.result = Address(0);
            let identity = step.calls.iter().find(|identity| case.calls.get(identity.0)
                .is_some_and(|call| matches!(call.role, CallRole::Allocate | CallRole::Resize | CallRole::Calloc))).copied().ok_or(Failure::Evidence)?;
            let call = case.calls.get_mut(identity.0).ok_or(Failure::Evidence)?;
            call.after.registers[0] = 0;
            if call.status.is_some() { call.status = Some(TargetStatus::OutOfMemory); }
            step.footprint = None;
        }
        ControlKind::Footprint => {
            let footprint = case.steps.iter_mut().find_map(|step| step.footprint.as_mut()).ok_or(Failure::Evidence)?;
            match footprint {
                Footprint::Stock { header, .. } => *header = Word(0),
                Footprint::Candidate { call } => case.calls.get_mut(call.0).ok_or(Failure::Evidence)?.after.registers[0] = 0,
            }
        }
        ControlKind::Statistics => {
            let stats = case.steps.iter_mut().find_map(|step| step.statistics.first_mut()).ok_or(Failure::Evidence)?;
            match stats {
                Statistics::Stock { chunks, .. } => chunks.first_mut().ok_or(Failure::Evidence)?.word.0 ^= 8,
                Statistics::Candidate { words, .. } => words[0].0 ^= 8,
            }
        }
        ControlKind::Counter => {
            let call = case.calls.first_mut().ok_or(Failure::Evidence)?;
            call.harness.trace_total.0 = call.harness.trace_total.0.checked_add(1).ok_or(Failure::Clock)?;
        }
        ControlKind::DiagnosticRole => {
            let call = case.calls.iter_mut().find(|call| matches!(call.role, CallRole::Footprint | CallRole::Statistics));
            if let Some(call) = call { call.role = CallRole::Allocate; }
            else {
                let call = case.calls.first_mut().ok_or(Failure::Evidence)?;
                call.role = CallRole::Statistics;
            }
        }
        ControlKind::PayloadData => {
            let data = case.steps.iter_mut().flat_map(|step| step.data.iter_mut()).chain(case.final_data.iter_mut())
                .find(|data| !data.bytes.0.is_empty()).ok_or(Failure::Evidence)?;
            data.bytes.0[0] ^= 1;
        }
        ControlKind::CodeWatch => case.code_watch.as_mut().ok_or(Failure::Evidence)?.violation = Some("serialized refusal control".into()),
    }
    Ok(())
}

pub fn refusal_controls(inputs: &Inputs, tape: &Tape, case: &Case, output: &Path) -> Result<Vec<ControlResult>, Failure> {
    let baseline = admit(inputs, tape, case)?;
    let components: Vec<_> = output.components().collect();
    let private = [std::path::Component::Normal(std::ffi::OsStr::new("private-data")),
        std::path::Component::Normal(std::ffi::OsStr::new("scratch")),
        std::path::Component::Normal(std::ffi::OsStr::new("heap148"))];
    require(components.windows(private.len()).any(|window| window == private)
        && !components.iter().any(|component| matches!(component, std::path::Component::ParentDir))
        && output.file_name().is_some(), Failure::Evidence)?;
    fs::create_dir(output)?;
    fs::write(output.join("baseline.json"), serde_json::to_vec_pretty(case)?)?;
    fs::write(output.join("baseline-summary.json"), serde_json::to_vec_pretty(&baseline)?)?;
    let mut results = Vec::with_capacity(ControlKind::ALL.len());
    for control in ControlKind::ALL {
        let mut changed = case.clone();
        mutate(&mut changed, control)?;
        let bytes = serde_json::to_vec_pretty(&changed)?;
        fs::write(output.join(format!("{}.json", control.name())), &bytes)?;
        let restored: Case = serde_json::from_slice(&bytes)?;
        let observed = admit(inputs, tape, &restored).err();
        results.push(ControlResult { control, expected: control.expected(), observed });
    }
    fs::write(output.join("refusals.json"), serde_json::to_vec_pretty(&results)?)?;
    require(results.iter().all(|result| result.observed.as_ref() == Some(&result.expected)), Failure::Evidence)?;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::{admit_call, admit_calloc_layout, admit_payload, admit_statistics_bounds, distribution, resize_copy_bytes};
    use crate::heap_replay_contract::*;
    use crate::heap_replay_state::{Alignment, Bytes, Layout};
    use crate::heap_state::{Address, AllocationId, Counter};

    fn raw_call() -> (Call, Vec<Span>) {
        let mut registers = [0; 16];
        registers[0] = 16;
        registers[1] = 8;
        registers[13] = STACK_END;
        registers[14] = RETURN_ADDRESS;
        registers[15] = IMAGE_BASE;
        let before = CpuFrame { registers, d8_d15: [RegisterBits(0); 8], cpsr: Word(0x60000053), counts: [Instructions(90), Instructions(42)],
            root: Address(0x4004dae0), task_record: Address(0x40060ecc), kernel_context: Word(0),
            identity: Word(0x4003), owner: Word(0), depth: Word(0), heap_break: Address(POOL_BASE) };
        let mut after = before.clone();
        after.registers[0] = POOL_BASE + 8;
        after.registers[15] = RETURN_ADDRESS;
        after.counts[0] = Instructions(97);
        let call = Call { identity: CallId(0), role: CallRole::Allocate, function: Address(IMAGE_BASE),
            arguments: [Word(16), Word(8), Word(0), Word(0)], before, after, status: Some(TargetStatus::Success),
            count_ranges: vec![Span { start: Address(IMAGE_BASE), end: Address(IMAGE_BASE + 32) }],
            harness: Harness { event: Some(HarnessEvent::Return { pc: Address(RETURN_ADDRESS) }),
                faults: EmptyEvents, irq_events: EmptyEvents, counts: vec![Counter(7)], trace_total: Counter(7),
                guard: FetchGuard { enabled: true, checked_fetches: Counter(7), accepted_fetches: Counter(7),
                    vector_fetches: Counter(0), range_fetches: vec![Counter(7)], halted: false,
                    ranges: Some(vec![[u64::from(IMAGE_BASE), u64::from(IMAGE_BASE + 32)]]),
                    vector_ranges: vec![], vector_range_fetches: vec![] } } };
        (call, vec![Span { start: Address(IMAGE_BASE), end: Address(IMAGE_BASE + 32) }])
    }

    fn layout(size: u32) -> Layout { Layout { size: Bytes(size), alignment: Alignment::Eight } }

    #[test]
    fn raw_counter_delta_is_emulated_instructions() {
        let (call, spans) = raw_call();
        assert_eq!(admit_call(&call, &spans), Ok(Instructions(7)));
    }

    #[test]
    fn caller_saved_ip_may_change_but_stack_pointer_must_return() {
        let (mut call, spans) = raw_call();
        call.after.registers[12] = 0x12345678;
        assert_eq!(admit_call(&call, &spans), Ok(Instructions(7)));
        call.after.registers[13] -= 8;
        assert_eq!(admit_call(&call, &spans), Err(Failure::Aapcs));
    }

    #[test]
    fn counter_disagreement_other_core_execution_and_wrap_refuse() {
        let (mut call, spans) = raw_call();
        call.harness.trace_total = Counter(6);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Clock));
        let (mut call, spans) = raw_call();
        call.after.counts[1] = Instructions(43);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Clock));
        let (mut call, spans) = raw_call();
        call.after.counts[0] = Instructions(89);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Clock));
    }

    #[test]
    fn incomplete_guard_and_instruction_budget_refuse() {
        let (mut call, spans) = raw_call();
        call.harness.guard.accepted_fetches = Counter(6);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Evidence));
        let (mut call, spans) = raw_call();
        call.after.counts[0] = Instructions(call.before.counts[0].0 + CALL_BUDGET + 1);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Budget));
    }

    #[test]
    fn changed_guard_extents_and_untallied_ranges_refuse() {
        let (mut call, spans) = raw_call();
        call.harness.guard.ranges = Some(vec![[u64::from(IMAGE_BASE), u64::from(IMAGE_BASE + 28)]]);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Evidence));
        let (mut call, spans) = raw_call();
        call.count_ranges[0].end = Address(IMAGE_BASE + 28);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Evidence));
        let (mut call, spans) = raw_call();
        call.harness.counts = vec![Counter(6)];
        assert_eq!(admit_call(&call, &spans), Err(Failure::Clock));
    }

    #[test]
    fn aapcs_return_and_task_context_refuse_independently() {
        let (mut call, spans) = raw_call();
        call.after.registers[8] = 1;
        assert_eq!(admit_call(&call, &spans), Err(Failure::Aapcs));
        let (mut call, spans) = raw_call();
        call.after.d8_d15[0] = RegisterBits(1);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Aapcs));
        let (mut call, spans) = raw_call();
        call.after.registers[15] = RETURN_ADDRESS + 4;
        assert_eq!(admit_call(&call, &spans), Err(Failure::Return));
        let (mut call, spans) = raw_call();
        call.after.kernel_context = Word(1);
        assert_eq!(admit_call(&call, &spans), Err(Failure::Context));
    }

    #[test]
    fn calloc_product_and_resize_prefix_bounds_are_exact() {
        assert!(admit_calloc_layout(Bytes(3), Bytes(16), layout(48)).is_ok());
        assert!(admit_calloc_layout(Bytes(0), Bytes(u32::MAX), layout(0)).is_ok());
        assert_eq!(admit_calloc_layout(Bytes(u32::MAX), Bytes(2), layout(u32::MAX - 1)), Err(Failure::Tape));
        assert_eq!(admit_calloc_layout(Bytes(3), Bytes(16), layout(47)), Err(Failure::Tape));
        assert_eq!(resize_copy_bytes(layout(40), layout(80)), Bytes(40));
        assert_eq!(resize_copy_bytes(layout(80), layout(40)), Bytes(40));
        assert_eq!(resize_copy_bytes(layout(40), layout(0)), Bytes(0));
    }

    #[test]
    fn payload_prefix_and_zero_evidence_are_byte_exact() {
        let identity = AllocationId(1);
        let pointer = Address(POOL_BASE + 8);
        let mut data = Data { role: DataRole::PreservedResize, identity, pointer, bytes: PayloadBytes(vec![73, 222, 103, 140]) };
        assert!(admit_payload(&data, DataRole::PreservedResize, identity, pointer, resize_copy_bytes(layout(8), layout(4))).is_ok());
        data.bytes.0[2] ^= 1;
        assert_eq!(admit_payload(&data, DataRole::PreservedResize, identity, pointer, Bytes(4)), Err(Failure::Data));
        data.role = DataRole::ZeroedCalloc;
        data.bytes = PayloadBytes(vec![0; 4]);
        assert!(admit_payload(&data, DataRole::ZeroedCalloc, identity, pointer, Bytes(4)).is_ok());
        data.bytes.0.push(0);
        assert_eq!(admit_payload(&data, DataRole::ZeroedCalloc, identity, pointer, Bytes(4)), Err(Failure::Data));
    }

    #[test]
    fn physical_accounting_hole_and_capacity_bounds_refuse() {
        let stats = Stats { allocated_block_bytes: Bytes(32), free_block_bytes: Bytes(64), largest_free_block_bytes: Bytes(48),
            external_metadata_bytes: Some(Bytes(4)), pool_managed_bytes: Bytes(112) };
        assert_eq!(admit_statistics_bounds(stats, Capacity::Recipe, Bytes(32)), Ok(stats));
        assert_eq!(admit_statistics_bounds(stats, Capacity::Recipe, Bytes(24)), Err(Failure::Statistics));
        assert_eq!(admit_statistics_bounds(Stats { largest_free_block_bytes: Bytes(65), ..stats }, Capacity::Recipe, Bytes(32)), Err(Failure::Statistics));
        assert_eq!(admit_statistics_bounds(Stats { pool_managed_bytes: Bytes(95), ..stats }, Capacity::Recipe, Bytes(32)), Err(Failure::Statistics));
        assert_eq!(admit_statistics_bounds(Stats { pool_managed_bytes: Bytes(Capacity::Recipe.bytes().0 + 1), ..stats }, Capacity::Recipe, Bytes(32)), Err(Failure::Statistics));
        assert_eq!(admit_statistics_bounds(Stats { allocated_block_bytes: Bytes(u32::MAX), free_block_bytes: Bytes(1), ..stats },
            Capacity::Original, Bytes(u32::MAX)), Err(Failure::Statistics));
    }

    #[test]
    fn percentiles_use_nearest_rank_instructions() {
        let values: Vec<_> = (1..=100).rev().map(Instructions).collect();
        let result = distribution(&values).unwrap();
        assert_eq!(result.samples, Counter(100));
        assert_eq!((result.minimum, result.median, result.p95, result.p99, result.maximum),
            (Instructions(1), Instructions(50), Instructions(95), Instructions(99), Instructions(100)));
        assert!(matches!(distribution(&[]), Err(Failure::Evidence)));
    }
}
