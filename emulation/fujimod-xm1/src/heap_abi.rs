//! Directed stock-ABI qualification: each role cold-boots its DAT through the natural no-card route, halts at the
//! next core-0 heap core entry, then calls the retained ABI entries one at a time under the board harness.
use super::*;
use fujimod_emulator::board::harness;
use heap_abi_scenario::{Counterpart, Executor, Lock, Observation, Request, Returned, ERRNO_MARKER};

#[path = "heap_abi_scenario.rs"]
pub(super) mod heap_abi_scenario;

const REENT: u32 = 0x89c978;
const REENT_POINTER: u32 = 0x89c970;
const LOCK_OWNER: u32 = 0x40a6c7d8;
const LOCK_DEPTH: u32 = 0x40a6c7dc;
const TASK_RECORD: u32 = 0x40054760;
const KERNEL_CONTEXT: u32 = 0x400545a4;
const TASK_TABLE: u32 = 0x4005ff0c;
const STACK_START: u32 = 0x01f80000;
const STACK_END: u32 = 0x01f90000;
const RETURN_ADDRESS: u32 = 0x01ff0000;
const CALL_BUDGET: u64 = 4_000_000;
const SEED_BUDGET: u64 = 200_000_000;
const CORE_ENTRIES: [u32; 5] = [0x89cde4, 0x89e560, 0x89da64, 0xbf1c44, 0x89e464];
/// Lock, current-task query, semaphore take/give and sbrk, fetched by both roles.
const RETAINED_SPANS: [(u32, u32); 3] = [(0x723cb8, 0x723ce8), (0x724b68, 0x724cbc), (0x72b110, 0x72b210)];
/// Stock bodies, excluding the malloc state at `[0x89d4f8,0x89d908)`.
const STOCK_SPANS: [(u32, u32); 3] = [(0x89cda4, 0x89d4f8), (0x89d908, 0x89e7c4), (0xbf1c1c, 0xbf1cd4)];
/// Owned carriers with the retained memmove `[0x89d908,0x89da3c)`; the sentinel word is excluded.
const OWNED_SPANS: [(u32, u32); 4] = [(0x89cda4, 0x89d4f8), (0x89d4fc, 0x89dfc0), (0x89e464, 0x89e7c4), (0xbf1c1c, 0xbf1cd4)];
const LOADER: u32 = 0x40000;

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Seed { pc: u32, task: u32, task_record: u32, counts: [u64; 2], heap_break: u32, errno: u32 }
#[derive(Clone, Deserialize, Serialize)]
struct CodeBinding { start: u32, end: u32, expected: Digest, before: Digest, after: Option<Digest> }
#[derive(Deserialize, Serialize)]
struct AbiJournal {
    schema: u32, role: Role, complete: bool, error: Option<String>,
    contract_sha256: Digest, run_sha256: Digest, update_sha256: Digest, rom_sha256: Digest, executable_sha256: Digest,
    menus: Vec<Option<[u32; 3]>>, seed: Option<Seed>, code: Vec<CodeBinding>,
    /// Loader-image words that differ from the decoded loader at the seed; every other bound span equals its role image.
    loader_runtime_words: Vec<u32>, calls: u64, call_instructions: u64,
    observations: Vec<Observation>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum HarnessEvent { Return { pc: u64 }, Breakpoint { pc: u64 }, Failure { message: String } }
#[derive(Deserialize)]
struct FetchGuard { enabled: bool, checked_fetches: u64, accepted_fetches: u64, vector_fetches: u64, halted: bool }
#[derive(Deserialize)]
struct HarnessState { event: Option<HarnessEvent>, faults: Vec<serde_json::Value>, irq_events: Vec<serde_json::Value>, trace_total: u64, guard: FetchGuard }

struct Frame { registers: [u32; 16], cpsr: u32, counts: [u64; 2], owner: u32, depth: u32 }

struct Board<'a> {
    machine: &'a mut Machine,
    maps: &'a [MemoryMap],
    ranges: Vec<(u64, u64)>,
    guard: fujimod_xm1::r4f::Guard,
    quantum: usize,
    calls: u64,
    instructions: u64,
}

impl Board<'_> {
    fn ram(&self, address: u64, length: usize) -> Result<u64, String> {
        let start = u32::try_from(address).map_err(|error| error.to_string())?;
        let end = u64::from(start).checked_add(length as u64).ok_or("ABI access overflows")?;
        for register in tcm(self.machine)?.into_iter().flatten() {
            if let Some((low, high)) = tcm_window(TcmRegister(register))? {
                require(end <= low || u64::from(start) >= high, "ABI access overlaps active TCM")?;
            }
        }
        Ok(static_span(self.maps, MemoryCore(0), MemoryAddress(u64::from(start))..MemoryAddress(end))?.physical.0)
    }

    fn bytes(&self, address: u64, length: usize) -> Result<Vec<u8>, String> {
        let mut guest = vec![0; length];
        if length == 0 { return Ok(guest); }
        let mut physical = vec![0; length];
        let at = self.ram(address, length)?;
        self.machine.session.board.read_memory(0, address, &mut guest)?;
        self.machine.session.board.read_physical(at, &mut physical)?;
        require(guest == physical, "ABI guest/physical bytes disagree")?;
        Ok(guest)
    }

    fn word(&self, address: u32) -> Result<u32, String> {
        let bytes = self.bytes(u64::from(address), 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn frame(&self) -> Result<Frame, String> {
        let mut registers = [0; 16];
        for (index, value) in registers.iter_mut().enumerate() { *value = register(self.machine, 0, arm::Register::R(index as u8))?; }
        Ok(Frame { registers, cpsr: register(self.machine, 0, arm::Register::Cpsr)?, counts: counts(self.machine)?,
            owner: self.word(LOCK_OWNER)?, depth: self.word(LOCK_DEPTH)? })
    }

    fn set(&self, register: arm::Register, value: u32) -> Result<(), String> {
        self.machine.session.board.write_register(0, Register::Arm32(register), u64::from(value))
    }

    fn invoke(&mut self, function: u32, arguments: [u32; 4]) -> Result<(Frame, Frame), String> {
        for (index, value) in arguments.into_iter().enumerate() { self.set(arm::Register::R(index as u8), value)?; }
        self.set(arm::Register::R(13), STACK_END)?;
        self.set(arm::Register::R(14), RETURN_ADDRESS)?;
        self.set(arm::Register::R(15), function)?;
        let before = self.frame()?;
        let board = &self.machine.session.board;
        let options = harness::Options { core: 0, inject_interrupts: false, deliver_faults: false,
            memory: harness::Memory { aliases: self.maps.iter().filter(|map| map.core == 0).map(|map| harness::Alias {
                address: map.address, physical: map.physical, size: map.size as u64 }).collect(), observed: Vec::new() },
            return_address: u64::from(RETURN_ADDRESS), breakpoints: Vec::new(), stop_addresses: Vec::new(), unarmed: Vec::new(),
            ranges: Some(self.ranges.clone()), vectors: Vec::new(), vector_opcode: 0, stack: (u64::from(STACK_START), u64::from(STACK_END)),
            allowed_stores: None, record_stores: false, include_stack: false, store_limit: 1, coverage: false,
            count_ranges: self.ranges.clone(), trace: 0 };
        board.harness_select_core(0)?;
        let mut active = board.harness_begin(options)?;
        board.harness_interrupt_delivery(0, false);
        board.harness_interrupt_delivery(1, false);
        let end = before.counts[0].checked_add(before.counts[1]).and_then(|total| total.checked_add(CALL_BUDGET)).ok_or("ABI call budget overflows")?;
        let run_result = self.machine.session.board.run_window(end, self.quantum, None, None);
        let board = &self.machine.session.board;
        let raw = serde_json::to_vec(&active.state()).map_err(|error| error.to_string())?;
        let end_result = board.harness_end(&mut active);
        let scan_result = self.guard.scan(board);
        let after = self.frame()?;
        run_result?;
        end_result?;
        scan_result?;
        let state: HarnessState = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
        let elapsed = after.counts[0].checked_sub(before.counts[0]).ok_or("core-0 clock regressed")?;
        let returned = matches!(state.event, Some(HarnessEvent::Return { pc }) if pc == u64::from(RETURN_ADDRESS));
        if !returned {
            let detail = match state.event {
                Some(HarnessEvent::Failure { message }) => message,
                Some(HarnessEvent::Breakpoint { pc }) => format!("breakpoint at {pc:#x}"),
                _ => format!("no return within {elapsed} instructions at pc {:#010x}", after.registers[15]),
            };
            return Err(format!("ABI call {function:#010x}{arguments:x?} did not return: {detail}"));
        }
        require(after.registers[15] == RETURN_ADDRESS && before.registers[4..12] == after.registers[4..12]
            && before.registers[13] == after.registers[13], "ABI call broke the A32 callee-saved contract")?;
        require(before.cpsr & 0x3ff == after.cpsr & 0x3ff && before.counts[1] == after.counts[1], "ABI call changed mode or ran core 1")?;
        require(state.faults.is_empty() && state.irq_events.is_empty() && elapsed > 0 && elapsed <= CALL_BUDGET
            && state.trace_total == elapsed && state.guard.enabled && !state.guard.halted && state.guard.checked_fetches == elapsed
            && state.guard.accepted_fetches == elapsed && state.guard.vector_fetches == 0, "ABI call fetched outside bound code or faulted")?;
        self.calls += 1;
        self.instructions += elapsed;
        Ok((before, after))
    }
}

impl Executor for Board<'_> {
    fn counterpart(&self) -> Option<Counterpart> { None }

    fn call(&mut self, request: Request) -> Result<Returned, String> {
        let pointer = |old: Option<(u64, u32)>| old.map_or(Ok(0), |(address, _)| u32::try_from(address).map_err(|error| error.to_string()));
        let (function, arguments) = match request {
            Request::Malloc { bytes } => (0x89cda4, [bytes, 0, 0, 0]),
            Request::Free { old } => (0x89cdc4, [pointer(old)?, 0, 0, 0]),
            Request::Realloc { old, bytes } => (0x89da3c, [pointer(old)?, bytes, 0, 0]),
            Request::Calloc { count, element_bytes } => (0xbf1c1c, [count, element_bytes, 0, 0]),
            Request::Trim { pad } => (0x89e464, [REENT, pad, 0, 0]),
        };
        let errno = self.ram(u64::from(REENT), 4)?;
        self.machine.session.board.write_physical(errno, &ERRNO_MARKER.to_le_bytes())?;
        let (before, after) = self.invoke(function, arguments)?;
        Ok(Returned { result: u64::from(after.registers[0]), errno: Some(self.word(REENT)?),
            lock: Lock { owner_restored: after.owner == before.owner, depth_change: i64::from(after.depth) - i64::from(before.depth) } })
    }

    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), String> {
        let at = self.ram(address, bytes.len())?;
        self.machine.session.board.write_physical(at, bytes)
    }

    fn read(&mut self, address: u64, length: usize) -> Result<Vec<u8>, String> { self.bytes(address, length) }
}

fn advance(machine: &mut Machine, budget: u64) -> Result<(), String> {
    let target = machine.instructions().checked_add(budget).ok_or("ABI route budget overflow")?;
    while machine.instructions() < target {
        let outcome = machine.run(target - machine.instructions());
        machine.require_running()?;
        if outcome != RunOutcome::Stopped { break; }
    }
    require(machine.instructions() == target, "ABI route instruction endpoint differs")
}

fn route(machine: &mut Machine, menus: &mut Vec<Option<[u32; 3]>>) -> Result<(), String> {
    advance(machine, 80_000_000)?;
    menus.push(menu(machine)?);
    require(menus.last() == Some(&Some([69, 52, 12])), "ABI route initial menu differs")?;
    for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) {
        machine.press(key)?;
        advance(machine, 8_000_000)?;
        machine.release_key(key)?;
        advance(machine, 8_000_000)?;
    }
    advance(machine, 40_000_000)?;
    machine.press(RouteKey::Ok)?;
    advance(machine, 8_000_000)?;
    machine.release_key(RouteKey::Ok)?;
    advance(machine, 8_000_000)?;
    advance(machine, 90_000_000)?;
    menus.push(menu(machine)?);
    require(menus.last() == Some(&Some([1, 5, 2])), "ABI route final menu differs")
}

/// Presses Up on the final menu and halts at the first heap core entry; it must be a core-0 task call with the heap lock free.
fn seed(machine: &mut Machine, maps: &[MemoryMap]) -> Result<Seed, String> {
    let sites: BTreeMap<u32, String> = CORE_ENTRIES.into_iter().flat_map(|address| [address, address | 0x40000000])
        .map(|address| (address, "abi-seed".to_owned())).collect();
    machine.traces.bound_full_registers(std::num::NonZeroUsize::new(16).ok_or("invalid seed hit bound")?);
    machine.traces.arm(&machine.session, sites)?;
    machine.traces.stop_labels = ["abi-seed".into()].into_iter().collect();
    let limit = machine.instructions().checked_add(SEED_BUDGET).ok_or("seed budget overflow")?;
    machine.press(RouteKey::Up)?;
    let outcome = machine.run(SEED_BUDGET);
    machine.require_running()?;
    require(outcome == RunOutcome::Stopped && machine.instructions() < limit && machine.traces.hits.len() == 1,
        &format!("no single heap call after the ABI route: stopped {}, {} hits", outcome == RunOutcome::Stopped, machine.traces.hits.len()))?;
    require(machine.traces.hits[0].core == 0, "first heap call after the ABI route is not on core 0")?;
    machine.disarm()?;
    let banks = tcm(machine)?;
    let read = |address| word(machine, maps, &banks, 0, address);
    let pc = register(machine, 0, arm::Register::R(15))?;
    let cpsr = register(machine, 0, arm::Register::Cpsr)?;
    let task_record = read(TASK_RECORD)?;
    let offset = task_record.checked_sub(TASK_TABLE).ok_or("seed task precedes the task table")?;
    require(CORE_ENTRIES.contains(&(pc & !0x40000000)) && register(machine, 0, arm::Register::R(0))? == REENT && read(REENT_POINTER)? == REENT,
        "seed is not a core entry with the shared context")?;
    require(machine.session.board.cpu(0)?.rprofile_identity(None)? == 0x4003 && cpsr & 0x1f == 0x13 && read(KERNEL_CONTEXT)? == 0
        && offset % 56 == 0 && offset / 56 < 120, "seed is not core-0 task context")?;
    require(read(LOCK_OWNER)? == 0 && read(LOCK_DEPTH)? == 0, "seed holds the heap lock")?;
    Ok(Seed { pc, task: offset / 56 + 1, task_record, counts: counts(machine)?, heap_break: read(0x829a08)?, errno: read(REENT)? })
}

fn spans(role: Role, images: &[Image]) -> Result<Vec<(u32, u32)>, String> {
    let loader = images.iter().find(|image| image.base == LOADER).ok_or("loader image absent")?;
    let loader_end = LOADER.checked_add(u32::try_from(loader.bytes.len()).map_err(|error| error.to_string())?).ok_or("loader extent overflow")?;
    let heap: &[(u32, u32)] = match role { Role::Stock => &STOCK_SPANS, Role::Owned => &OWNED_SPANS };
    let mut spans: Vec<_> = [(LOADER, loader_end & !3)].into_iter().chain(RETAINED_SPANS).chain(heap.iter().copied()).collect();
    spans.sort_unstable();
    Ok(spans)
}

fn expected_bytes(images: &[Image], start: u32, end: u32) -> Result<Vec<u8>, String> {
    let image = images.iter().find(|image| start >= image.base && u64::from(end) <= u64::from(image.base) + image.bytes.len() as u64)
        .ok_or("bound span is outside one role image")?;
    Ok(image.bytes[(start - image.base) as usize..(end - image.base) as usize].to_vec())
}

fn exercise(inputs: &Inputs, role: Role, images: &[Image], machine: &mut Machine, maps: &[MemoryMap], journal: &mut AbiJournal) -> Result<(), String> {
    route(machine, &mut journal.menus)?;
    journal.seed = Some(seed(machine, maps)?);
    let mut board = Board { machine, maps, ranges: Vec::new(), guard: fujimod_xm1::r4f::Guard::default(),
        quantum: RunDocument::parse(&inputs.run)?.execution.quantum, calls: 0, instructions: 0 };
    for (start, end) in spans(role, images)? {
        let expected = expected_bytes(images, start, end)?;
        let observed = board.bytes(u64::from(start), expected.len())?;
        journal.code.push(CodeBinding { start, end, expected: Digest(sha256(&expected)), before: Digest(sha256(&observed)), after: None });
        if observed != expected {
            let differing: Vec<u32> = observed.chunks(4).zip(expected.chunks(4)).enumerate()
                .filter(|(_, (observed, expected))| observed != expected).map(|(index, _)| start + 4 * index as u32).collect();
            if start != LOADER { return Err(format!("bound code differs at {:#010x}", differing[0])); }
            journal.loader_runtime_words = differing;
        }
        board.ranges.push((u64::from(start), u64::from(end)));
    }
    board.guard.arm(&board.machine.session.board)?;
    let observations = heap_abi_scenario::run(&mut board);
    let disarmed = board.guard.disarm(&board.machine.session.board);
    (journal.calls, journal.call_instructions) = (board.calls, board.instructions);
    for binding in &mut journal.code {
        let after = Digest(sha256(&board.bytes(u64::from(binding.start), (binding.end - binding.start) as usize)?));
        binding.after = Some(after.clone());
        require(after == binding.before, "bound code changed during ABI calls")?;
    }
    journal.observations = observations?;
    disarmed
}

fn execute(inputs: &Inputs, role: Role, output: &Path) -> Result<(), String> {
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    let (update, images) = match role { Role::Stock => (&inputs.stock, &inputs.stock_images), Role::Owned => (&inputs.contract.candidate, &inputs.contract.images) };
    let document = RunDocument::parse(&inputs.run)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, update, &inputs.rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    machine.rearm()?;
    let mut journal = AbiJournal { schema: 1, role, complete: false, error: None,
        contract_sha256: inputs.contract.contract_digest.clone(), run_sha256: Digest(sha256(&inputs.run)), update_sha256: Digest(sha256(&update.0)),
        rom_sha256: Digest(sha256(&inputs.rom)), executable_sha256: Digest(sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)),
        menus: Vec::new(), seed: None, code: Vec::new(), loader_runtime_words: Vec::new(), calls: 0, call_instructions: 0, observations: Vec::new() };
    let result = exercise(inputs, role, images, &mut machine, &maps, &mut journal);
    journal.complete = result.is_ok() && machine.failure.is_none();
    journal.error = result.err().or_else(|| machine.failure.clone());
    write(&output.join("abi-journal.json"), &serde_json::to_vec_pretty(&journal).map_err(|error| error.to_string())?)?;
    require(journal.complete, "ABI role refused; journal retained")
}

#[derive(Deserialize)]
struct FrontEndJournal { schema: u32, counterpart: Counterpart, observations: Vec<Observation> }

fn consume(inputs: &Inputs, directory: &Path, role: Role) -> Result<AbiJournal, String> {
    let journal: AbiJournal = serde_json::from_slice(&bounded_read(&directory.join("abi-journal.json"), 64 * 1024 * 1024)?).map_err(|error| error.to_string())?;
    let update = match role { Role::Stock => &inputs.stock, Role::Owned => &inputs.contract.candidate };
    require(journal.schema == 1 && journal.role == role && journal.complete && journal.error.is_none()
        && (role == Role::Stock || journal.contract_sha256 == inputs.contract.contract_digest) && journal.update_sha256.0 == sha256(&update.0)
        && journal.run_sha256.0 == sha256(&inputs.run) && journal.rom_sha256.0 == sha256(&inputs.rom)
        && journal.menus == [Some([69, 52, 12]), Some([1, 5, 2])]
        && journal.code.iter().all(|binding| (binding.start == LOADER || binding.before == binding.expected) && binding.after.as_ref() == Some(&binding.before)),
        "ABI journal identity or completion differs")?;
    Ok(journal)
}

fn front_end(directory: &Path) -> Result<Vec<Observation>, String> {
    let journal: FrontEndJournal = serde_json::from_slice(&bounded_read(&directory.join("front-end").join("abi-journal.json"), 64 * 1024 * 1024)?)
        .map_err(|error| error.to_string())?;
    require(journal.schema == 1 && journal.counterpart == Counterpart::RustFrontEnd, "front-end ABI journal identity differs")?;
    Ok(journal.observations)
}

fn emit(output: &Path, value: &impl Serialize) -> Result<(), String> {
    OpenOptions::new().create_new(true).write(true).open(output)
        .and_then(|mut file| file.write_all(&serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?)).map_err(|error| error.to_string())
}

fn check(inputs: &Inputs, directory: &Path, output: &Path) -> Result<(), String> {
    let stock = consume(inputs, &directory.join("stock"), Role::Stock)?;
    let owned = consume(inputs, &directory.join("owned"), Role::Owned)?;
    let front = front_end(directory)?;
    #[derive(Serialize)]
    struct Report {
        schema: u32, admitted: bool, error: Option<String>, seeds: [Option<Seed>; 2], calls: [u64; 2], call_instructions: [u64; 2], loader_runtime_words: usize,
        owned_abi: Option<heap_abi_scenario::Comparison>, rust_front_end: Option<heap_abi_scenario::Comparison>,
    }
    require(stock.seed.map(|seed| (seed.pc, seed.task)) == owned.seed.map(|seed| (seed.pc, seed.task)), "stock/owned ABI seeds differ")?;
    require(stock.loader_runtime_words == owned.loader_runtime_words, "stock/owned loader runtime words differ")?;
    let owned_abi = heap_abi_scenario::compare(Counterpart::OwnedAbi, &stock.observations, &owned.observations);
    let rust_front_end = heap_abi_scenario::compare(Counterpart::RustFrontEnd, &stock.observations, &front);
    let error = owned_abi.as_ref().err().or(rust_front_end.as_ref().err()).map(|mismatch| mismatch.to_string());
    emit(output, &Report { schema: 1, admitted: error.is_none(), error: error.clone(), seeds: [stock.seed, owned.seed],
        calls: [stock.calls, owned.calls], call_instructions: [stock.call_instructions, owned.call_instructions], loader_runtime_words: stock.loader_runtime_words.len(),
        owned_abi: owned_abi.ok(), rust_front_end: rust_front_end.ok() })?;
    match error { Some(error) => Err(format!("stock/owned ABI comparison refused: {error}")), None => Ok(()) }
}

fn controls(inputs: &Inputs, directory: &Path, output: &Path) -> Result<(), String> {
    let stock = consume(inputs, &directory.join("stock"), Role::Stock)?;
    let owned = consume(inputs, &directory.join("owned"), Role::Owned)?;
    let front = front_end(directory)?;
    let mut refusals = heap_abi_scenario::controls(Counterpart::OwnedAbi, &stock.observations, &owned.observations);
    refusals.extend(heap_abi_scenario::controls(Counterpart::RustFrontEnd, &stock.observations, &front));
    let refused = |counterpart| refusals.iter().filter(|refusal| refusal.counterpart == counterpart && refusal.refused && refusal.targeted).count();
    #[derive(Serialize)]
    struct Controls<'a> { schema: u32, owned_abi_refused: usize, rust_front_end_refused: usize, refusals: &'a [heap_abi_scenario::Refusal] }
    emit(output, &Controls { schema: 1, owned_abi_refused: refused(Counterpart::OwnedAbi), rust_front_end_refused: refused(Counterpart::RustFrontEnd), refusals: &refusals })?;
    require(refusals.iter().filter(|refusal| refusal.counterpart == Counterpart::OwnedAbi).count() == heap_abi_scenario::Fault::ALL.len()
        && refusals.iter().all(|refusal| refusal.refused && refusal.targeted), "an ABI journal control was accepted or refused off target")
}

pub(super) fn command(name: &str, inputs: &Inputs, arguments: &[std::ffi::OsString]) -> Result<(), String> {
    let output = Path::new(&arguments[7]);
    match name {
        "--abi-stock" | "--abi-owned" => {
            fujimod_emulator_exr2_board::register_board_builder();
            fujimod_emulator_models::register();
            execute(inputs, if name == "--abi-stock" { Role::Stock } else { Role::Owned }, output)
        }
        "--abi-check" => check(inputs, output, Path::new(&arguments[8])),
        "--abi-controls" => controls(inputs, output, Path::new(&arguments[8])),
        _ => Err(HELP.into()),
    }
}
