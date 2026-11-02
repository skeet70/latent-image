use super::*;
#[path = "heap_cutover_contract.rs"]
mod contract;
#[path = "heap_cutover_guard.rs"]
mod guard;
#[path = "heap_cutover_check.rs"]
mod check;
use guard::Role;

const HELP: &str = "heap-observe --cutover-run|--cutover-check|--cutover-controls CONTRACT STOCK_DAT CANDIDATE_DAT ANALYSIS RUN ROM OUTPUT [REPORT_JSON]";
const JOURNAL_LIMIT: u64 = 256 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct Payload { pointer: Address, bytes: Vec<u8> }
#[derive(Deserialize, Serialize)]
struct RawBoundary {
    boundary: Boundary,
    image4_seal: Option<Vec<guard::SealWord>>,
    sentinel: guard::SealWord,
    header: Option<u32>,
    payload: Option<Payload>,
}
#[derive(Deserialize, Serialize)]
struct NaturalReceipt {
    schema: u32, role: Role, complete: bool, error: Option<String>, machine_failure: Option<String>,
    contract_sha256: Digest, run_sha256: Digest, update_sha256: Digest, rom_sha256: Digest, executable_sha256: Digest,
    reset_sha256: Digest, boundaries_sha256: Digest, hits_sha256: Digest, accesses_sha256: Digest,
    events_sha256: Digest, summary_sha256: Digest, instructions: [u64; 2], menus: Vec<Option<[u32; 3]>>,
    frame: Option<Digest>, image5_phase: Image5Phase, access_limits: AccessLimits,
    watch_end_error: Option<String>, watch_finish_error: Option<String>, terminal_access_sha256: Option<Digest>,
    fetches_checked: u64, payload_writes_checked: u64,
    final_sentinel: guard::SealWord,
}
struct Inputs {
    contract: contract::Admitted, run: Vec<u8>, rom: Vec<u8>, stock: ValidatedUpdate, stock_images: Vec<Image>,
}

fn counts(machine: &Machine) -> Result<[u64; 2], String> {
    machine.session.board.instruction_counts().try_into().map_err(|_| "expected two instruction counters".into())
}

fn read_payload(machine: &Machine, maps: &[MemoryMap], pointer: u32, bytes: u64) -> Result<Payload, String> {
    let length = usize::try_from(bytes).map_err(|error| error.to_string())?;
    require(length <= (HEAP_END - HEAP_START) as usize && pointer & 7 == 0, "payload sample exceeds heap bound")?;
    let banks = tcm(machine)?;
    let high = u64::from(pointer).checked_add(bytes).ok_or("payload sample extent overflow")?;
    for register in banks.into_iter().flatten() {
        if let Some((low, end)) = tcm_window(TcmRegister(register))? {
            for alias in [u64::from(pointer & !0x40000000), u64::from(pointer | 0x40000000)] {
                require(alias + bytes <= low || alias >= end, "payload sample overlaps TCM")?;
            }
        }
    }
    let mut result = vec![0; length];
    if length != 0 {
        let mut physical = vec![0; length];
        for core in 0..2 {
            for alias in [pointer & !0x40000000, pointer | 0x40000000] {
                let span = static_span(maps, MemoryCore(core), MemoryAddress(u64::from(alias))..MemoryAddress(u64::from(alias) + bytes))?;
                require(span.physical.0 == u64::from(pointer & !0x40000000), "payload alias physical translation differs")?;
                machine.session.board.read_memory(core, u64::from(alias), &mut result)?;
                machine.session.board.read_physical(span.physical.0, &mut physical)?;
                require(result == physical, "payload guest/physical views disagree")?;
            }
        }
    }
    require(high <= 1 << 32, "payload sample exceeds address space")?;
    Ok(Payload { pointer: Address(pointer), bytes: result })
}

fn previous_request(recorder: &Recorder, resize: &Snapshot) -> Option<u64> {
    if resize.registers[1] == 0 { return None; }
    recorder.records().iter().rev().filter(|event| event.snapshot.counts.iter().zip(resize.counts).all(|(before, after)| *before <= after))
        .find_map(|event| {
            if event.snapshot.registers[0] != resize.registers[1] { return None; }
            match &event.boundary {
                heap_state::Boundary::Return { operation: Operation::Malloc, entry, .. } => Some(u64::from(entry.registers[1])),
                heap_state::Boundary::Return { operation: Operation::Calloc, entry, .. } => Some(u64::from(entry.registers[1]) * u64::from(entry.registers[2])),
                heap_state::Boundary::Return { operation: Operation::Realloc, entry, .. } => Some(u64::from(entry.registers[2])),
                _ => None,
            }
        })
}

fn payload_request(recorder: &Recorder, snapshot: &Snapshot) -> Option<(u32, u64)> {
    let returning = recorder.return_operation(snapshot.context, snapshot.pc, Address(snapshot.registers[13]));
    let summary = recorder.summary();
    let frame = summary.pending.iter().find(|frame| frame.entry.context == snapshot.context
        && frame.entry.registers[14] == snapshot.pc.0 && frame.entry.registers[13] == snapshot.registers[13]);
    if let Some(frame) = frame {
        match returning {
            Some(Operation::Realloc) if snapshot.registers[0] != 0 => {
                previous_request(recorder, &frame.entry).map(|bytes| (snapshot.registers[0], bytes.min(u64::from(frame.entry.registers[2]))))
            }
            Some(Operation::Realloc) => previous_request(recorder, &frame.entry).map(|bytes| (frame.entry.registers[1], bytes)),
            Some(Operation::Calloc) if snapshot.registers[0] != 0 => Some((snapshot.registers[0], u64::from(frame.entry.registers[1]) * u64::from(frame.entry.registers[2]))),
            _ => None,
        }
    } else if heap_state::entry(Address(snapshot.pc.0 & !0x40000000)) == Some(Operation::Realloc) {
        summary.live_allocations.iter().find(|allocation| allocation.address.0 == snapshot.registers[1]).map(|old| (old.address.0, old.requested.0))
    } else { None }
}

fn raw_boundary(machine: &Machine, maps: &[MemoryMap], images: &[Image], recorder: &Recorder, lifecycle: &Image5Lifecycle,
    initial: bool, core: usize, sealing: bool) -> Result<RawBoundary, String> {
    let boundary = sample(machine, maps, images, recorder, lifecycle, initial, core)?;
    let pointer = if recorder.return_operation(boundary.snapshot.context, boundary.snapshot.pc, Address(boundary.snapshot.registers[13])).is_some()
        && matches!(recorder.return_operation(boundary.snapshot.context, boundary.snapshot.pc, Address(boundary.snapshot.registers[13])), Some(Operation::Malloc | Operation::Realloc | Operation::Calloc)) {
        boundary.snapshot.registers[0]
    } else if matches!(heap_state::entry(Address(boundary.snapshot.pc.0 & !0x40000000)), Some(Operation::Free | Operation::Realloc)) { boundary.snapshot.registers[1] } else { 0 };
    let header = (pointer != 0).then(|| word(machine, maps, &boundary.tcm, core, pointer.checked_sub(4).ok_or("payload header underflow")?)).transpose()?;
    let payload = payload_request(recorder, &boundary.snapshot).map(|(pointer, bytes)| read_payload(machine, maps, pointer, bytes)).transpose()?;
    Ok(RawBoundary { boundary, image4_seal: sealing.then(|| guard::seal(machine, maps)).transpose()?,
        sentinel: guard::sentinel(machine, maps)?, header, payload })
}

fn load(arguments: &[std::ffi::OsString]) -> Result<Inputs, String> {
    let contract = contract::admit(Path::new(&arguments[1]), Path::new(&arguments[2]), Path::new(&arguments[3]), Path::new(&arguments[4]))?;
    let run = bounded_read(Path::new(&arguments[5]), 4 * 1024 * 1024)?;
    let rom = bounded_read(Path::new(&arguments[6]), 4 * 1024 * 1024)?;
    require(sha256(&run) == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && sha256(&rom) == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "natural pair requires canonical no-card inputs")?;
    let stock = ValidatedUpdate(bounded_read(Path::new(&arguments[2]), 128 * 1024 * 1024)?);
    let (stock_images, _) = source_images(Path::new(&arguments[4]))?;
    Ok(Inputs { contract, run, rom, stock, stock_images })
}

fn execute(inputs: &Inputs, role: Role, output: &Path) -> Result<(), String> {
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    let (update, images) = match role { Role::Stock => (&inputs.stock, &inputs.stock_images), Role::Owned => (&inputs.contract.candidate, &inputs.contract.images) };
    let mut accesses = access_writer(&output.join("accesses.jsonl.gz"))?;
    let document = RunDocument::parse(&inputs.run)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, update, &inputs.rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    write(&output.join("reset.fujimod"), &checkpoint)?;
    let reset = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&reset)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "natural pair checkpoint is not reset")?;
    let mut recorder = Recorder::new();
    let mut lifecycle = Image5Lifecycle::default();
    let mut retired_guard = guard::Guard::default();
    let mut boundaries: Vec<RawBoundary> = Vec::new();
    let shadows: Vec<_> = contract::CARRIERS.into_iter().flat_map(|(low, high, _)| (low..high).step_by(2))
        .flat_map(|address| [Guest(address), Guest(address | 0x40000000)])
        .filter(|address| !ENTRIES.contains(&(address.0 & !0x40000000))).collect();
    let mut armed: BTreeSet<_> = ENTRIES.into_iter().chain(IMAGE5_LOAD_POINTS)
        .flat_map(|address| [address, address | 0x40000000]).chain(shadows.iter().map(|address| address.0)).collect();
    machine.traces.bound_full_registers(std::num::NonZeroUsize::new(BOUNDARY_LIMIT).ok_or("invalid natural boundary limit")?);
    machine.traces.arm(&machine.session, armed.iter().map(|address| (*address, "heap".into())).collect())?;
    machine.traces.stop_labels = ["heap".into()].into_iter().collect();
    let mut watch = Some(machine.session.board.memory_watch_begin_classified(guard::ranges_for(&recorder), WATCH_LIMIT, maps.clone(), ViewPolicy::Observe)?);
    let mut terminal = None;
    let mut begin = [0; 2];
    let mut processed = 0;
    let mut menus = Vec::new();
    let mut image4_shadows_removed = false;
    let mut image5_shadows_removed = false;
    let result = (|| -> Result<(), String> {
        let mut advance = |machine: &mut Machine, budget: u64| -> Result<(), String> {
            let target = machine.instructions().checked_add(budget).ok_or("natural route budget overflow")?;
            while machine.instructions() < target {
                let outcome = machine.run(target - machine.instructions());
                machine.require_running()?;
                if outcome != RunOutcome::Stopped { break; }
                let hits = &machine.traces.hits[processed..];
                require(hits.len() == 1 && boundaries.len() < BOUNDARY_LIMIT, "natural boundary hit count exceeds admission")?;
                let pc = register(machine, hits[0].core, arm::Register::R(15))? & !0x40000000;
                let sealing = !retired_guard.sealed && contract::CARRIERS.iter().any(|(low, high, image)| *image == 4 && *low <= pc && pc < *high);
                let raw = raw_boundary(machine, &maps, images, &recorder, &lifecycle, boundaries.is_empty(), hits[0].core, sealing)?;
                let mut active = watch.take().ok_or("natural watch interval absent")?;
                let bytes = serde_json::to_vec(&active.records.borrow().state()).map_err(|error| error.to_string())?;
                machine.session.board.memory_watch_end(&mut active)?;
                append_accesses(&mut accesses, output, &mut terminal, &bytes)?;
                let batch: AccessBatch = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
                retired_guard.batch(&batch, &recorder, lifecycle.phase(), role, &inputs.contract.owned, begin,
                    raw.boundary.snapshot.counts, Some(&raw.boundary), raw.image4_seal.as_deref(), &raw.sentinel, images)?;
                let code = admit_image5(&raw.boundary, images, lifecycle.phase(), boundaries.is_empty()).map_err(|error| error.to_string())?;
                lifecycle.observe(&raw.boundary.snapshot, code).map_err(|error| format!("natural image5 lifecycle: {error:?}"))?;
                check::observe(&mut recorder, &raw)?;
                begin = raw.boundary.snapshot.counts;
                boundaries.push(raw);
                processed = machine.traces.hits.len();
                for (number, remove) in [(4, retired_guard.sealed && !image4_shadows_removed), (5, lifecycle.phase() == Image5Phase::Admitted && !image5_shadows_removed)] {
                    if remove {
                        let removed: Vec<_> = shadows.iter().copied().filter(|address| contract::CARRIERS.iter().any(|(low, high, image)| *image == number && *low <= (address.0 & !0x40000000) && (address.0 & !0x40000000) < *high)).collect();
                        machine.traces.remove_sites(&machine.session, &removed)?;
                        for address in removed { armed.remove(&address.0); }
                        if number == 4 { image4_shadows_removed = true; } else { image5_shadows_removed = true; }
                    }
                }
                let additional = recorder.returns().into_iter().filter_map(|address| armed.insert(address.0).then_some((address.0, "heap".into()))).collect::<BTreeMap<_, _>>();
                if !additional.is_empty() { machine.traces.add_sites(&machine.session, additional)?; }
                watch = Some(machine.session.board.memory_watch_begin_classified(guard::ranges_for(&recorder), WATCH_LIMIT, maps.clone(), ViewPolicy::Observe)?);
            }
            require(machine.instructions() == target, "natural route instruction endpoint differs")
        };
        advance(&mut machine, 80_000_000)?;
        menus.push(menu(&machine)?);
        require(menus.last() == Some(&Some([69, 52, 12])), "natural initial menu differs")?;
        for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) {
            machine.press(key)?;
            advance(&mut machine, 8_000_000)?;
            machine.release_key(key)?;
            advance(&mut machine, 8_000_000)?;
        }
        advance(&mut machine, 40_000_000)?;
        machine.press(RouteKey::Ok)?;
        advance(&mut machine, 8_000_000)?;
        machine.release_key(RouteKey::Ok)?;
        advance(&mut machine, 8_000_000)?;
        advance(&mut machine, 90_000_000)?;
        menus.push(menu(&machine)?);
        require(menus.last() == Some(&Some([1, 5, 2])), "natural final menu differs")?;
        recorder.finish().map_err(|error| error.to_string())?;
        require(retired_guard.sealed && lifecycle.phase() == Image5Phase::Admitted, "natural installation lifecycle is incomplete")
    })();
    let final_sentinel = guard::sentinel(&machine, &maps)?;
    let mut watch_end_error = None;
    if let Some(mut active) = watch.take() {
        let bytes = serde_json::to_vec(&active.records.borrow().state()).map_err(|error| error.to_string())?;
        watch_end_error = machine.session.board.memory_watch_end(&mut active).err();
        let drained = (|| -> Result<(), String> {
            append_accesses(&mut accesses, output, &mut terminal, &bytes)?;
            let batch: AccessBatch = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            retired_guard.batch(&batch, &recorder, lifecycle.phase(), role, &inputs.contract.owned, begin, counts(&machine)?, None, None, &final_sentinel, images)
        })();
        if let Err(error) = drained { watch_end_error = Some(error); }
    }
    let watch_finish_error = accesses.finish().err().map(|error| error.to_string());
    let boundaries_bytes = serde_json::to_vec(&boundaries).map_err(|error| error.to_string())?;
    let hits_bytes = serde_json::to_vec(&machine.traces.hits).map_err(|error| error.to_string())?;
    let events_bytes = serde_json::to_vec(recorder.records()).map_err(|error| error.to_string())?;
    let summary_bytes = serde_json::to_vec(&recorder.summary()).map_err(|error| error.to_string())?;
    write(&output.join("boundaries.json"), &boundaries_bytes)?;
    write(&output.join("trace-hits.json"), &hits_bytes)?;
    write(&output.join("events.json"), &events_bytes)?;
    write(&output.join("summary.json"), &summary_bytes)?;
    let frame = machine.frame().map(|value| value.map(|bytes| Digest(sha256(&bytes))));
    let instructions = counts(&machine)?;
    let complete = result.is_ok() && machine.failure.is_none() && frame.as_ref().is_ok_and(|frame| frame.is_some())
        && watch_end_error.is_none() && watch_finish_error.is_none() && terminal.is_none();
    let receipt = NaturalReceipt {
        schema: 1, role, complete, error: result.err().or_else(|| frame.as_ref().err().cloned()), machine_failure: machine.failure.clone(),
        contract_sha256: inputs.contract.contract_digest.clone(), run_sha256: Digest(sha256(&inputs.run)), update_sha256: Digest(sha256(&update.0)),
        rom_sha256: Digest(sha256(&inputs.rom)), executable_sha256: Digest(sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)),
        reset_sha256: Digest(sha256(&checkpoint)), boundaries_sha256: Digest(sha256(&boundaries_bytes)), hits_sha256: Digest(sha256(&hits_bytes)),
        accesses_sha256: heap_accesses::digest_file(&output.join("accesses.jsonl.gz"), ACCESS_LIMITS.compressed()).map_err(|error| error.to_string())?,
        events_sha256: Digest(sha256(&events_bytes)), summary_sha256: Digest(sha256(&summary_bytes)), instructions, menus,
        frame: frame.ok().flatten(), image5_phase: lifecycle.phase(), access_limits: ACCESS_LIMITS, watch_end_error, watch_finish_error,
        terminal_access_sha256: terminal, fetches_checked: retired_guard.fetches_checked, payload_writes_checked: retired_guard.payload_writes_checked,
        final_sentinel,
    };
    write(&output.join("natural-receipt.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    require(complete, "natural heap role refused; raw receipt retained")
}

pub(super) fn command(arguments: &[std::ffi::OsString]) -> Option<Result<(), String>> {
    let name = arguments.first()?.to_str()?;
    if !["--cutover-run", "--cutover-check", "--cutover-controls"].contains(&name) { return None; }
    Some((|| {
        require(arguments.len() == if name == "--cutover-run" { 8 } else { 9 }, HELP)?;
        let inputs = load(arguments)?;
        let output = Path::new(&arguments[7]);
        match name {
            "--cutover-run" => {
                std::fs::create_dir(output).map_err(|error| error.to_string())?;
                fujimod_emulator_exr2_board::register_board_builder();
                fujimod_emulator_models::register();
                let stock = execute(&inputs, Role::Stock, &output.join("stock"));
                let owned = execute(&inputs, Role::Owned, &output.join("owned"));
                stock?;
                owned?;
                check::pair(&inputs, output, &output.join("qualification.json"))
            }
            "--cutover-check" => check::pair(&inputs, output, Path::new(&arguments[8])),
            "--cutover-controls" => check::controls(&inputs, output, Path::new(&arguments[8])),
            _ => unreachable!(),
        }
    })())
}
