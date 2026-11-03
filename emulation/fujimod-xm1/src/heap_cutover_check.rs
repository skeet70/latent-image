use super::*;
use heap_state::{Classification, FreeKind};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Fault { WrongSizeClass, UnbalancedLock, HeaderCorruption, StockBodyFetch }

/// `allocation` is the creating context and its per-context creation ordinal, so identities survive cross-context reordering.
#[derive(Deserialize, Eq, PartialEq, Serialize)]
struct InterfaceCall {
    operation: Operation, context: ContextKey, request: u64, allocation: Option<(ContextKey, u64)>,
    succeeded: Option<bool>, errno_before: Option<u32>, errno_after: Option<u32>,
}

pub(super) fn observe(recorder: &mut Recorder, raw: &RawBoundary, role: Role) -> Result<(), String> {
    let snapshot = &raw.boundary.snapshot;
    let returning = recorder.return_operation(snapshot.context, snapshot.pc, Address(snapshot.registers[13]));
    let operation = returning.or_else(|| heap_state::entry(Address(snapshot.pc.0 & !0x40000000)));
    let pointer = if returning.is_some() && matches!(operation, Some(Operation::Malloc | Operation::Calloc | Operation::Realloc)) {
        snapshot.registers[0]
    } else if returning.is_none() && matches!(operation, Some(Operation::Free | Operation::Realloc)) { snapshot.registers[1] } else { 0 };
    require(raw.header.is_some() == (pointer != 0), "raw boundary header shape differs")?;
    if let Some(header) = raw.header {
        require(header & 2 == 0 && (header & !3).checked_sub(4) == snapshot.chunk, "raw boundary header is corrupt")?;
    }
    let payload = payload_request(recorder, snapshot);
    require(payload.is_some() == raw.payload.is_some(), "observable payload sample is missing")?;
    if let (Some((pointer, bytes)), Some(observed)) = (payload, &raw.payload) {
        require(observed.pointer.0 == pointer && observed.bytes.len() as u64 == bytes, "observable payload extent differs")?;
        if returning == Some(Operation::Calloc) { require(observed.bytes.iter().all(|byte| *byte == 0), "calloc returned nonzero payload bytes")?; }
    }
    if returning.is_some() {
        let summary = recorder.summary();
        let frame = summary.pending.iter().find(|frame| frame.entry.context == snapshot.context
            && frame.entry.registers[14] == snapshot.pc.0 && frame.entry.registers[13] == snapshot.registers[13]).ok_or("return has no invocation")?;
        if matches!(frame.operation, Operation::Malloc | Operation::Free | Operation::Realloc | Operation::Calloc | Operation::Trim)
            && (snapshot.owner, snapshot.depth) != (frame.entry.owner, frame.entry.depth) {
            let resize = frame.operation == Operation::Realloc && frame.entry.registers[1] != 0 && frame.entry.registers[2] > MAX_STOCK_REQUEST;
            return Err(format!("interface returned with unbalanced heap lock{}", if resize { " (approved redesign oversized-resize-rejected-before-lock)" } else { "" }));
        }
        if let (Role::Owned, Some(header)) = (role, raw.header) {
            let request = match frame.operation {
                Operation::Malloc => Some(u64::from(frame.entry.registers[1])),
                Operation::Calloc => Some(u64::from(frame.entry.registers[1]) * u64::from(frame.entry.registers[2])),
                Operation::Realloc => Some(u64::from(frame.entry.registers[2])),
                _ => None,
            };
            if let Some(request) = request {
                require(u64::from(header & !3) == ((request + 11) & !7).max(16), "owned allocation size class differs from its request")?;
            }
        }
        if frame.operation == Operation::Lock {
            require(frame.entry.depth.checked_add(1) == Some(snapshot.depth) && snapshot.owner != 0, "heap lock return has no balanced acquisition")?;
        } else if frame.operation == Operation::Unlock {
            require(snapshot.depth.checked_add(1) == Some(frame.entry.depth)
                && if snapshot.depth == 0 { snapshot.owner == 0 } else { snapshot.owner == frame.entry.owner },
                "heap unlock return has no balanced release")?;
        }
    }
    let mut normalized = snapshot.clone();
    if returning.is_none() && heap_state::entry(Address(snapshot.pc.0 & !0x40000000)).is_some() { normalized.pc.0 &= !0x40000000; }
    recorder.observe(normalized).map_err(|error| error.to_string())
}

fn context(raw: &RawBoundary, images: &[Image]) -> Result<(), String> {
    let boundary = &raw.boundary;
    let snapshot = &boundary.snapshot;
    require(snapshot.core.0 < 2 && matches!(boundary.identities[snapshot.core.0], 0x4003 | 0x5003), "raw boundary core identity differs")?;
    let domain = usize::from(boundary.identities[snapshot.core.0] != 0x4003);
    let offset = boundary.task_record_word.0.checked_sub([0x4005ff0c, 0x400efecc][domain]).ok_or("raw task record precedes table")?;
    require(offset % 56 == 0 && offset / 56 < 120 && snapshot.context.task.0 == offset / 56 + 1
        && snapshot.context.domain.0 == domain && boundary.root.0 == [0x4004dae0, 0x400ddaa0][domain]
        && snapshot.context.task_record == boundary.task_record_word && snapshot.context.exception_depth == boundary.context_word,
        "raw boundary task binding differs")?;
    require(snapshot.registers[15] == snapshot.pc.0 && boundary.code_word == expected(images, snapshot.pc.0)?, "raw instruction word differs from admitted bytes")?;
    for register in boundary.tcm.into_iter().flatten() {
        if let Some((low, high)) = tcm_window(TcmRegister(register))? {
            for (start, end, _) in contract::CARRIERS {
                for alias in [0, 0x40000000] {
                    require(u64::from(end | alias) <= low || u64::from(start | alias) >= high, "retired carrier is shadowed by TCM")?;
                }
            }
        }
    }
    Ok(())
}

fn logical(classification: Classification) -> bool {
    matches!(classification, Classification::Allocation { logical: true, .. } | Classification::Free { free_kind: FreeKind::Client | FreeKind::Null, .. })
}

fn interfaces(events: &[heap_state::Event]) -> Result<Vec<InterfaceCall>, String> {
    let mut result = Vec::new();
    let mut allocations = BTreeMap::<u32, (ContextKey, u64)>::new();
    let mut ordinals = BTreeMap::<ContextKey, u64>::new();
    let mut create = |context: ContextKey| { let ordinal = ordinals.entry(context).or_insert(0); *ordinal += 1; (context, *ordinal) };
    let roots: BTreeSet<_> = events.iter().filter_map(|event| match &event.boundary {
        heap_state::Boundary::Enter { invocation, parent: None, .. } => Some(*invocation), _ => None,
    }).collect();
    for event in events {
        if let heap_state::Boundary::Return { invocation, operation, entry } = &event.boundary {
            if !logical(event.classification) && !(*operation == Operation::Trim && roots.contains(invocation)) { continue; }
            if matches!(event.classification, Classification::Free { free_kind: FreeKind::Null, .. }) && !roots.contains(invocation) { continue; }
            let request = match operation {
                Operation::Malloc => u64::from(entry.registers[1]), Operation::Realloc => u64::from(entry.registers[2]),
                Operation::Calloc => u64::from(entry.registers[1]) * u64::from(entry.registers[2]),
                Operation::Trim => u64::from(entry.registers[1]), _ => 0,
            };
            let pointer = event.snapshot.registers[0];
            let allocation = match operation {
                Operation::Free => {
                    if entry.registers[1] == 0 { None } else { Some(allocations.remove(&entry.registers[1]).ok_or("interface frees unknown logical allocation")?) }
                }
                Operation::Realloc => {
                    let previous = if entry.registers[1] == 0 { None } else { Some(*allocations.get(&entry.registers[1]).ok_or("interface resizes unknown logical allocation")?) };
                    if pointer == 0 { previous } else {
                        if entry.registers[1] != 0 { allocations.remove(&entry.registers[1]); }
                        let identity = previous.unwrap_or_else(|| create(entry.context));
                        allocations.insert(pointer, identity);
                        Some(identity)
                    }
                }
                Operation::Malloc | Operation::Calloc if pointer != 0 => {
                    let identity = create(entry.context);
                    require(allocations.insert(pointer, identity).is_none(), "interface aliases a live allocation")?;
                    Some(identity)
                }
                _ => None,
            };
            result.push(InterfaceCall { operation: *operation, context: entry.context, request, allocation,
                succeeded: matches!(operation, Operation::Malloc | Operation::Calloc | Operation::Realloc | Operation::Trim).then_some(pointer != 0),
                errno_before: entry.errno, errno_after: event.snapshot.errno });
        }
    }
    require(!result.is_empty(), "natural journal has no interface lifetimes")?;
    Ok(result)
}

fn verify_payloads(boundaries: &[RawBoundary], events: &[heap_state::Event]) -> Result<usize, String> {
    let mut before = BTreeMap::<u64, &[u8]>::new();
    let mut checked = 0;
    for event in events {
        if !logical(event.classification) { continue; }
        let raw = boundaries.iter().find(|raw| raw.boundary.snapshot.counts == event.snapshot.counts).ok_or("payload boundary is absent")?;
        match &event.boundary {
            heap_state::Boundary::Enter { invocation, operation: Operation::Realloc, .. } => {
                if let Some(payload) = &raw.payload { before.insert(*invocation, &payload.bytes); }
            }
            heap_state::Boundary::Return { invocation, operation: Operation::Realloc, .. } => {
                if let Some(previous) = before.remove(invocation) {
                    let after = &raw.payload.as_ref().ok_or("realloc payload sample absent")?.bytes;
                    require(after.len() <= previous.len() && after.as_slice() == &previous[..after.len()], "realloc did not preserve observable payload bytes")?;
                    checked += 1;
                }
            }
            _ => {}
        }
    }
    Ok(checked)
}

struct Consumed { receipt: NaturalReceipt, calls: Vec<InterfaceCall>, payload_checks: usize }

fn consume(inputs: &Inputs, directory: &Path, role: Role, fault: Option<Fault>) -> Result<Consumed, String> {
    let receipt: NaturalReceipt = serde_json::from_slice(&bounded_read(&directory.join("natural-receipt.json"), 128 * 1024)?).map_err(|error| error.to_string())?;
    require(receipt.schema == 1 && receipt.role == role && receipt.complete && receipt.error.is_none() && receipt.machine_failure.is_none()
        && receipt.watch_end_error.is_none() && receipt.watch_finish_error.is_none() && receipt.terminal_access_sha256.is_none()
        && receipt.access_limits == ACCESS_LIMITS && receipt.image5_phase == Image5Phase::Admitted,
        "natural receipt is incomplete")?;
    let (update, images) = match role { Role::Stock => (&inputs.stock, &inputs.stock_images), Role::Owned => (&inputs.contract.candidate, &inputs.contract.images) };
    require(receipt.contract_sha256 == inputs.contract.contract_digest && receipt.run_sha256.0 == sha256(&inputs.run)
        && receipt.rom_sha256.0 == sha256(&inputs.rom) && receipt.update_sha256.0 == sha256(&update.0)
        && receipt.executable_sha256.0 == sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?), "natural receipt input identities differ")?;
    let boundary_bytes = bounded_read(&directory.join("boundaries.json"), JOURNAL_LIMIT)?;
    let hits_bytes = bounded_read(&directory.join("trace-hits.json"), JOURNAL_LIMIT)?;
    let event_bytes = bounded_read(&directory.join("events.json"), JOURNAL_LIMIT)?;
    let summary_bytes = bounded_read(&directory.join("summary.json"), 8 * 1024 * 1024)?;
    let access_path = directory.join("accesses.jsonl.gz");
    require(receipt.boundaries_sha256.0 == sha256(&boundary_bytes) && receipt.hits_sha256.0 == sha256(&hits_bytes)
        && receipt.events_sha256.0 == sha256(&event_bytes) && receipt.summary_sha256.0 == sha256(&summary_bytes)
        && receipt.accesses_sha256 == heap_accesses::digest_file(&access_path, ACCESS_LIMITS.compressed()).map_err(|error| error.to_string())?
        && receipt.reset_sha256.0 == sha256(&bounded_read(&directory.join("reset.fujimod"), 128 * 1024 * 1024)?), "natural raw journal digest differs")?;
    let mut boundaries: Vec<RawBoundary> = serde_json::from_slice(&boundary_bytes).map_err(|error| error.to_string())?;
    let hits: Vec<RecordedHit> = serde_json::from_slice(&hits_bytes).map_err(|error| error.to_string())?;
    require(!boundaries.is_empty() && boundaries.len() <= BOUNDARY_LIMIT && boundaries.len() == hits.len(), "natural boundary count differs")?;
    let mut targeted = false;
    match fault {
        Some(Fault::UnbalancedLock) => {
            let events: Vec<heap_state::Event> = serde_json::from_slice(&event_bytes).map_err(|error| error.to_string())?;
            let returned = events.iter().rev().find(|event| logical(event.classification)
                && matches!(&event.boundary, heap_state::Boundary::Return { .. })).ok_or("lock control has no returned interface")?;
            if let Some(raw) = boundaries.iter_mut().find(|raw| raw.boundary.snapshot.counts == returned.snapshot.counts) {
                raw.boundary.snapshot.depth = raw.boundary.snapshot.depth.checked_add(1).ok_or("control depth overflow")?;
                targeted = true;
            }
        }
        Some(Fault::HeaderCorruption) => {
            if let Some(raw) = boundaries.iter_mut().find(|raw| raw.header.is_some()) { raw.header = raw.header.map(|header| header ^ 2); targeted = true; }
        }
        Some(Fault::WrongSizeClass) => {
            if let Some(raw) = boundaries.iter_mut().find(|raw| raw.header.is_some() && heap_state::entry(Address(raw.boundary.snapshot.pc.0 & !0x40000000)).is_none()) {
                raw.header = raw.header.map(|header| header + 8);
                raw.boundary.snapshot.chunk = raw.boundary.snapshot.chunk.map(|chunk| chunk + 8);
                targeted = true;
            }
        }
        _ => {}
    }
    let mut recorder = Recorder::new();
    let mut lifecycle = Image5Lifecycle::default();
    let mut retired_guard = guard::Guard::default();
    let mut lines = AccessReader::new(File::open(&access_path).map_err(|error| error.to_string())?, ACCESS_LIMITS);
    let mut begin = [0; 2];
    for (index, (raw, hit)) in boundaries.iter().zip(&hits).enumerate() {
        let boundary = &raw.boundary;
        require(hit.core == boundary.snapshot.core && hit.address.decode()? == boundary.snapshot.pc.0
            && hit.instructions.0 == boundary.snapshot.counts[0].checked_add(boundary.snapshot.counts[1]).ok_or("raw hit clock overflow")?
            && hit.registers.decode()? == boundary.snapshot.registers[..13]
            && hit.sp.decode()? == boundary.snapshot.registers[13] && hit.lr.decode()? == boundary.snapshot.registers[14]
            && hit.cpsr == boundary.snapshot.cpsr, "raw hit and boundary registers differ")?;
        context(raw, images)?;
        let mut batch: AccessBatch = serde_json::from_slice(lines.next_line().map_err(|error| error.to_string())?.ok_or("natural watch interval missing")?).map_err(|error| error.to_string())?;
        if matches!(fault, Some(Fault::StockBodyFetch)) && !targeted {
            if let Some(row) = batch.records.iter_mut().find(|row| row.source == Access::CpuFetch && contract::CARRIERS.iter().any(|(low, high, _)| u64::from(*low) <= row.physical && row.physical < u64::from(*high))) {
                let forbidden = contract::CARRIERS.iter().flat_map(|(low, high, _)| (*low..*high).step_by(4)).find(|pc| !inputs.contract.owned.contains(pc)).ok_or("artifact has no retired stock-body control PC")?;
                row.address = u64::from(forbidden); row.physical = u64::from(forbidden); row.pc = Some(u64::from(forbidden)); targeted = true;
            }
        }
        retired_guard.batch(&batch, &recorder, lifecycle.phase(), role, &inputs.contract.owned, begin,
            boundary.snapshot.counts, Some(boundary), raw.image4_seal.as_deref(), &raw.sentinel, images)?;
        let code = admit_image5(boundary, images, lifecycle.phase(), index == 0).map_err(|error| error.to_string())?;
        lifecycle.observe(&boundary.snapshot, code).map_err(|error| format!("raw image5 lifecycle differs: {error:?}"))?;
        observe(&mut recorder, raw, role)?;
        if role == Role::Owned {
            for allocation in recorder.summary().live_allocations {
                contract::admit_pointer(&inputs.contract, allocation.address, allocation.usable.0)?;
            }
        }
        begin = boundary.snapshot.counts;
    }
    let batch: AccessBatch = serde_json::from_slice(lines.next_line().map_err(|error| error.to_string())?.ok_or("natural final watch interval missing")?).map_err(|error| error.to_string())?;
    retired_guard.batch(&batch, &recorder, lifecycle.phase(), role, &inputs.contract.owned, begin, receipt.instructions, None, None, &receipt.final_sentinel, images)?;
    retired_guard.finish(role)?;
    require(lines.next_line().map_err(|error| error.to_string())?.is_none(), "natural watch has extra intervals")?;
    require(retired_guard.sealed && lifecycle.phase() == Image5Phase::Admitted && receipt.instructions[0].checked_add(receipt.instructions[1]) == Some(ROUTE_INSTRUCTIONS)
        && receipt.fetches_checked == retired_guard.fetches_checked && receipt.payload_writes_checked == retired_guard.payload_writes_checked,
        "natural continuous guard projection differs")?;
    require(receipt.menus == [Some([69, 52, 12]), Some([1, 5, 2])] && receipt.frame.is_some(), "natural menu endpoint differs")?;
    recorder.finish().map_err(|error| error.to_string())?;
    require(serde_json::to_vec(recorder.records()).map_err(|error| error.to_string())? == event_bytes
        && serde_json::to_vec(&recorder.summary()).map_err(|error| error.to_string())? == summary_bytes,
        "natural lifetimes do not reconstruct from raw boundaries")?;
    let payload_checks = verify_payloads(&boundaries, recorder.records())?;
    require(fault.is_none() || targeted, "natural control target absent")?;
    Ok(Consumed { receipt, calls: interfaces(recorder.records())?, payload_checks })
}

fn by_context(calls: &[InterfaceCall]) -> BTreeMap<ContextKey, Vec<&InterfaceCall>> {
    let mut grouped = BTreeMap::<ContextKey, Vec<&InterfaceCall>>::new();
    for call in calls { grouped.entry(call.context).or_default().push(call); }
    grouped
}

const MAX_STOCK_REQUEST: u32 = 0x7fff_fff4;

/// The approved redesign a differing stock/owned call falls under.
fn redesign(stock: &InterfaceCall, owned: &InterfaceCall) -> Option<contract::Redesign> {
    if stock.operation != owned.operation || stock.request != owned.request { return None; }
    match stock.operation {
        Operation::Trim => Some(contract::Redesign::TrimReturnsZero),
        Operation::Calloc if stock.request > u64::from(u32::MAX) => Some(contract::Redesign::CheckedCallocProduct),
        Operation::Realloc if stock.request > u64::from(MAX_STOCK_REQUEST) => Some(contract::Redesign::OversizedResizeRejectedBeforeLock),
        Operation::Malloc | Operation::Calloc | Operation::Realloc if stock.request == 0 => Some(contract::Redesign::ZeroRequestMinimumAllocation),
        _ => None,
    }
}

fn first_difference(stock: &[InterfaceCall], owned: &[InterfaceCall]) -> Option<String> {
    let (stock, owned) = (by_context(stock), by_context(owned));
    let contexts: BTreeSet<_> = stock.keys().chain(owned.keys()).copied().collect();
    contexts.into_iter().find_map(|context| {
        let (left, right) = (stock.get(&context).map_or(&[][..], Vec::as_slice), owned.get(&context).map_or(&[][..], Vec::as_slice));
        let index = (0..left.len().max(right.len())).find(|index| left.get(*index) != right.get(*index))?;
        let named = left.get(index).zip(right.get(index)).and_then(|(left, right)| redesign(left, right));
        Some(format!("stock/owned interface calls differ for {context:?} at call {index}{}",
            named.map_or(String::new(), |item| format!(" (approved redesign {item:?})"))))
    })
}

pub(super) fn pair(inputs: &Inputs, directory: &Path, output: &Path) -> Result<(), String> {
    let stock = consume(inputs, &directory.join("stock"), Role::Stock, None)?;
    let owned = consume(inputs, &directory.join("owned"), Role::Owned, None)?;
    if let Some(difference) = first_difference(&stock.calls, &owned.calls) { return Err(difference); }
    require(stock.receipt.menus == owned.receipt.menus, "stock/owned natural menu outcomes differ")?;
    let global_order_identical = stock.calls == owned.calls;
    #[derive(Serialize)]
    struct Comparison<'a> {
        schema: u32, admitted: bool, qualification: &'static str, calls: usize, contexts: usize, global_order_identical: bool,
        stock: NaturalReceipt, owned: NaturalReceipt, observable_realloc_checks: [usize; 2],
        conditional_premises: &'a [String], limitations: [&'static str; 5],
    }
    let comparison = Comparison { schema: 1, admitted: true, qualification: "conditional-natural-no-card-menu-only", calls: stock.calls.len(),
        contexts: by_context(&stock.calls).len(), global_order_identical,
        stock: stock.receipt, owned: owned.receipt, observable_realloc_checks: [stock.payload_checks, owned.payload_checks], conditional_premises: &inputs.contract.premises,
        limitations: [
            "Payload evidence is allocator-origin write confinement for other live requested payloads, plus exact realloc/calloc byte samples only when those calls occur.",
            "The memory journal contains addresses, sizes and PCs, not store values. It cannot reconstruct arbitrary application payload bytes or establish unknown metadata-reader behavior.",
            "Initial image4 model-bus installation is admitted only before first fetch and exact full-carrier seal; image5 installation uses the retained producer/decoder lifecycle.",
            "Candidate DAT to staged decompressed images is the admitted host stager boundary; runtime rechecks exact identities and staged retained-byte equality.",
            "Finite no-card menu execution on one SDK does not qualify capture, storage, concurrency or silicon behavior.",
        ] };
    OpenOptions::new().create_new(true).write(true).open(output)
        .and_then(|mut file| file.write_all(&serde_json::to_vec_pretty(&comparison).map_err(std::io::Error::other)?)).map_err(|error| error.to_string())
}

pub(super) fn controls(inputs: &Inputs, directory: &Path, output: &Path) -> Result<(), String> {
    consume(inputs, &directory.join("stock"), Role::Stock, None)?;
    consume(inputs, &directory.join("owned"), Role::Owned, None)?;
    #[derive(Serialize)]
    struct Refusal { fault: Fault, refused: bool, reason: String }
    let mut refusals = Vec::new();
    for fault in [Fault::WrongSizeClass, Fault::UnbalancedLock, Fault::HeaderCorruption, Fault::StockBodyFetch] {
        let result = consume(inputs, &directory.join("owned"), Role::Owned, Some(fault));
        require(result.is_err(), "natural raw-journal control was accepted")?;
        refusals.push(Refusal { fault, refused: true, reason: result.err().ok_or("control refusal disappeared")? });
    }
    OpenOptions::new().create_new(true).write(true).open(output)
        .and_then(|mut file| file.write_all(&serde_json::to_vec_pretty(&refusals).map_err(std::io::Error::other)?)).map_err(|error| error.to_string())
}
