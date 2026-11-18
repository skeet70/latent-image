use fujimod_emulator::board::bus::TransactionBatch;
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::{read, write}, machine::Machine, provenance::Guest, run::{self, RunDocument, ValidatedUpdate}};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, num::NonZeroUsize, path::{Path, PathBuf}, process::ExitCode};

const USAGE: &str = "fujimod-xm1-loading-runtime --run RUN --update DAT --boot-rom ROM --artifact DIR --output NEW_DIR";
const SOURCE_UPDATE: &str = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";
const RUN_SHA256: &str = "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef";
const ROM_SHA256: &str = "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be";
/// Plain (kind 3) is the last of the natural boot requests; the stock natural
/// plain run reaches the shooting menu at 382,000,000 instructions.
const LIMIT: u64 = 400_000_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Digest(String);
impl Digest {
    fn of(bytes: &[u8]) -> Self { Self(format!("{:x}", Sha256::digest(bytes))) }
    fn is(&self, expected: &str) -> bool { self.0 == expected }
}
#[derive(Clone, Debug, Serialize)]
struct FileIdentity { path: PathBuf, bytes: usize, sha256: Digest }
fn identity(path: &Path) -> Result<(Vec<u8>, FileIdentity), String> {
    let bytes = read(path)?;
    Ok((bytes.clone(), FileIdentity { path: path.to_path_buf(), bytes: bytes.len(), sha256: Digest::of(&bytes) }))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum Entry { LoaderInit, LoaderRequest, LoaderCancel, DecoderInit, DecoderFeed, DecoderProgress, DecoderComplete, DecoderFinish }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Export { kind: Entry, symbol: String, veneer: Guest, address: Guest, bytes: u32 }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Publication { address: Guest, old: u32, new: u32 }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeadlinePolicy { scope: String, ticks: u64, formed_once_per_request: bool, cleanup_budget: u32 }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Contract {
    schema: u32, firmware_sha256: Digest, staging_dat_sha256: Digest, candidate_dat_sha256: Digest, candidate_image4_sha256: Digest,
    body_sha256: Digest, symbols_sha256: Digest, body_bytes: u32, body_start: Guest, bss: [Guest; 2],
    entries: Vec<Export>, publications: Vec<Publication>, deadline_policy: DeadlinePolicy,
    drain_scope: String,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Role { FujiModSuccess, RetentionControl }
#[derive(Clone, Debug)]
struct Symbol { address: u32, bytes: u32, kind: char }
fn symbols(bytes: &[u8]) -> Result<Vec<(String, Symbol)>, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut result = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let (address, size, kind, name) = match fields.as_slice() {
            [address, size, kind, name] => (*address, Some(*size), *kind, *name),
            [address, kind, name] => (*address, None, *kind, *name),
            _ => return Err("bank symbol row differs".into()),
        };
        let address = u32::from_str_radix(address, 16).map_err(|error| error.to_string())?;
        let bytes = size.map(|size| u32::from_str_radix(size, 16)).transpose().map_err(|error| error.to_string())?.unwrap_or(0);
        let kind = kind.chars().next().ok_or("bank symbol kind absent")?;
        result.push((name.to_owned(), Symbol { address, bytes, kind }));
    }
    Ok(result)
}
fn required<'a>(symbols: &'a [(String, Symbol)], name: &str) -> Result<&'a Symbol, String> {
    let mut found = symbols.iter().filter(|(candidate, _)| candidate == name).map(|(_, symbol)| symbol);
    match (found.next(), found.next()) {
        (Some(symbol), None) => Ok(symbol),
        (None, _) => Err(format!("bank symbol absent: {name}")),
        _ => Err(format!("bank symbol duplicated: {name}")),
    }
}
/// Code owned by the task60 loader, its NAND port and its policy.
fn loading_text(symbols: &[(String, Symbol)]) -> Vec<(u32, u32)> {
    symbols.iter().filter(|(name, symbol)| matches!(symbol.kind, 't' | 'T') && symbol.bytes != 0
        && ["xm1_executable_loading", "xm1_application_nand", "xm1_loading_policy", "xm1_loading_"].iter().any(|crate_name| name.contains(crate_name)))
        .map(|(_, symbol)| (symbol.address, symbol.bytes)).collect()
}
/// Mutable state owned by the task60 loader and its policy.
fn loading_state(symbols: &[(String, Symbol)], bss: [u32; 2]) -> Vec<(u32, u32)> {
    symbols.iter().filter(|(name, symbol)| symbol.bytes != 0 && symbol.address >= bss[0] && symbol.address < bss[1]
        && ["xm1_executable_loading", "xm1_loading_policy", "XM1_LOADING_DEADLINE_CALLS", "XM1_LOADING_ENVELOPES"].iter().any(|crate_name| name.contains(crate_name)))
        .map(|(_, symbol)| (symbol.address, symbol.bytes)).collect()
}
fn require(condition: bool, message: &str) -> Result<(), String> { if condition { Ok(()) } else { Err(message.into()) } }

#[derive(Clone, Debug, Serialize)]
struct Registers { r: [u32; 13], sp: Guest, lr: Guest, cpsr: u32 }
fn registers(machine: &Machine, core: usize) -> Result<Registers, String> {
    let mut r = [0; 13];
    for (index, value) in r.iter_mut().enumerate() {
        *value = u32::try_from(machine.session.board.read_register(core, Register::Arm32(arm::Register::R(index as u8)))?).map_err(|_| "register exceeds word")?;
    }
    let get = |register| u32::try_from(machine.session.board.read_register(core, Register::Arm32(register))?).map_err(|_| "register exceeds word".to_owned());
    Ok(Registers { r, sp: Guest(get(arm::Register::R(13))?), lr: Guest(get(arm::Register::R(14))?), cpsr: get(arm::Register::Cpsr)? })
}
fn memory(machine: &Machine, address: u32, bytes: usize) -> Result<Vec<u8>, String> {
    let mut result = vec![0; bytes];
    machine.session.board.read_memory(0, u64::from(address), &mut result)?;
    Ok(result)
}
fn words(machine: &Machine, address: u32, count: usize) -> Result<Vec<u32>, String> {
    let bytes = memory(machine, address, count.checked_mul(4).ok_or("word extent overflow")?)?;
    Ok(bytes.chunks_exact(4).map(|word| u32::from_le_bytes(word.try_into().expect("word chunk"))).collect())
}

#[derive(Clone, Debug, Serialize)]
struct State { latches: Vec<u32>, busy: u32, task: u32, nand_state: u32, index: u32, handle: u32, token: u32, completion: u32 }
fn state(machine: &Machine) -> Result<State, String> {
    let nand = words(machine, 0x916528, 9)?;
    Ok(State { latches: words(machine, 0x944440, 9)?, busy: words(machine, 0x957600, 1)?[0], task: words(machine, 0x914830, 1)?[0],
        nand_state: nand[0], index: nand[2], handle: nand[4], token: nand[6], completion: nand[8] })
}
#[derive(Clone, Debug, Serialize)]
struct Event { instructions: u64, core: usize, pc: Guest, registers: Registers, state: State, descriptor: Option<Vec<u32>>, envelope: Option<Vec<u32>> }
#[derive(Clone, Debug, Serialize)]
struct Callback { kind: u32, address: Guest, argument: u32, failed: u32, instructions: u64 }
#[derive(Serialize)]
struct Receipt {
    schema: u32, role: Role, run: FileIdentity, update: FileIdentity, rom: FileIdentity, executable: FileIdentity,
    contract: FileIdentity, body: FileIdentity, symbols: FileIdentity, staging_source: Digest, policy: DeadlinePolicy, drain_scope: String,
    events: Vec<Event>, callbacks: Vec<Callback>, transactions: TransactionBatch, final_state: State,
    loading_text_installed: bool, loading_state_zero: bool, loading_text_ranges: usize, loading_state_ranges: usize,
    deadline_calls: u32, instructions: u64, machine_failure: Option<String>, trace: FileIdentity,
    limits: Vec<String>, passed: bool,
}

fn role(contract: &Contract) -> Result<Role, String> {
    match contract.deadline_policy.scope.as_str() {
        "fujimod-3d90035d" => Ok(Role::FujiModSuccess),
        "synthetic-retention-control" => Ok(Role::RetentionControl),
        _ => Err("runtime qualification policy scope differs".into()),
    }
}
fn parse_args() -> Result<BTreeMap<String, PathBuf>, String> {
    let mut args = std::env::args_os().skip(1);
    let mut values = BTreeMap::new();
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(USAGE)?.to_owned();
        if !["--run", "--update", "--boot-rom", "--artifact", "--output"].contains(&flag.as_str()) { return Err(USAGE.into()); }
        let value = PathBuf::from(args.next().ok_or(USAGE)?);
        if values.insert(flag, value).is_some() { return Err(USAGE.into()); }
    }
    if values.len() == 5 { Ok(values) } else { Err(USAGE.into()) }
}
fn take(values: &mut BTreeMap<String, PathBuf>, key: &str) -> Result<PathBuf, String> { values.remove(key).ok_or_else(|| USAGE.into()) }

fn drive() -> Result<(), String> {
    let mut args = parse_args()?;
    let run_path = take(&mut args, "--run")?;
    let update_path = take(&mut args, "--update")?;
    let rom_path = take(&mut args, "--boot-rom")?;
    let artifact = take(&mut args, "--artifact")?;
    let output = take(&mut args, "--output")?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    let (run_bytes, run_id) = identity(&run_path)?;
    let (update_bytes, update_id) = identity(&update_path)?;
    let (rom_bytes, rom_id) = identity(&rom_path)?;
    let (contract_bytes, contract_id) = identity(&artifact.join("runtime-contract.json"))?;
    let (body_bytes, body_id) = identity(&artifact.join("runtime.bin"))?;
    let (symbol_bytes, symbol_id) = identity(&artifact.join("runtime.symbols"))?;
    let contract: Contract = serde_json::from_slice(&contract_bytes).map_err(|error| error.to_string())?;
    let role = role(&contract)?;
    require(contract.schema == 1 && contract.firmware_sha256.is(SOURCE_UPDATE), "runtime source contract differs")?;
    require(run_id.sha256.is(RUN_SHA256) && rom_id.sha256.is(ROM_SHA256) && update_id.sha256 == contract.candidate_dat_sha256, "runtime input identity differs")?;
    require(body_id.sha256 == contract.body_sha256 && symbol_id.sha256 == contract.symbols_sha256 && body_bytes.len() == contract.body_bytes as usize, "runtime body identity differs")?;
    require(contract.deadline_policy.formed_once_per_request && contract.deadline_policy.cleanup_budget == 0 && contract.deadline_policy.ticks != 0, "runtime deadline contract differs")?;
    require(contract.body_start.0 % 4 == 0 && contract.bss[0].0 >= contract.body_start.0 + contract.body_bytes && contract.bss[1].0 >= contract.bss[0].0 && contract.bss[1].0 <= 0xbf1a00, "bank fixed composition differs")?;
    let symbols = symbols(&symbol_bytes)?;
    let expected_entries = [(Entry::LoaderInit, "xm1_loading_startup", 0x73ac74), (Entry::LoaderRequest, "xm1_loading_legacy_request", 0x73ac94),
        (Entry::LoaderCancel, "xm1_loading_legacy_cancel", 0x73aca4), (Entry::DecoderInit, "xm1_decoder_init", 0x6ae360),
        (Entry::DecoderFeed, "xm1_decoder_feed", 0x6ae500), (Entry::DecoderProgress, "xm1_decoder_progress", 0x6ae1dc),
        (Entry::DecoderComplete, "xm1_decoder_complete", 0x6ae310), (Entry::DecoderFinish, "xm1_decoder_finish", 0x6ae3f4)];
    require(contract.entries.len() == expected_entries.len(), "runtime export count differs")?;
    for (kind, name, veneer) in expected_entries {
        let export = contract.entries.iter().find(|entry| entry.kind == kind).ok_or("runtime export absent")?;
        let symbol = required(&symbols, name)?;
        require(export.symbol == name && export.veneer == Guest(veneer) && export.address == Guest(symbol.address) && export.bytes == symbol.bytes && symbol.bytes != 0, "runtime export binding differs")?;
    }
    let executable_path = std::env::current_exe().map_err(|error| error.to_string())?;
    let (_, executable_id) = identity(&executable_path)?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let mut session = run::open(&document, &ValidatedUpdate(update_bytes), &rom_bytes)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    require(machine.instructions() == 0, "runtime observer did not start from reset")?;
    let task_entry = required(&symbols, "xm1_loading_task")?.address;
    let body_receive = {
        let mut found = symbols.iter().filter(|(name, symbol)| name.contains("xm1_executable_loading") && name.ends_with("6native12receive_once") && matches!(symbol.kind, 't' | 'T'));
        match (found.next(), found.next()) {
            (Some((_, symbol)), None) => symbol.address,
            _ => return Err("task60 receive iteration is not one bank function".into()),
        }
    };
    let body_startup = required(&symbols, "xm1_loading_startup")?.address;
    let body_request = required(&symbols, "xm1_loading_legacy_request")?.address;
    let body_cancel = required(&symbols, "xm1_loading_legacy_cancel")?.address;
    let decoder_finish = contract.entries.iter().find(|entry| entry.kind == Entry::DecoderFinish).ok_or("decoder finish export absent")?.address;
    let deadline_calls_address = required(&symbols, "XM1_LOADING_DEADLINE_CALLS")?.address;
    let envelopes_address = required(&symbols, "XM1_LOADING_ENVELOPES")?.address;
    let text_ranges = loading_text(&symbols);
    let state_ranges = loading_state(&symbols, [contract.bss[0].0, contract.bss[1].0]);
    require(!text_ranges.is_empty() && !state_ranges.is_empty()
        && text_ranges.iter().all(|(address, bytes)| *address >= contract.body_start.0 && address + bytes <= contract.body_start.0 + contract.body_bytes),
        "loading-owned text or state is not in the bank")?;
    let mut sites: BTreeMap<u32, String> =
        [0x73ac74, 0x73ac94, 0x73aca4, task_entry, body_receive, body_startup, body_request, body_cancel, 0x6ae360, 0x6ae3f4]
            .into_iter().map(|address| (address, "loading-runtime".into())).collect();
    for entry in &contract.entries { sites.insert(entry.address.0, "loading-runtime".into()); }
    machine.traces.bound_full_registers(NonZeroUsize::new(131_072).ok_or("zero trace bound")?);
    machine.traces.stop_labels = ["loading-runtime".into(), "loading-service".into(), "loading-callback".into(), "loading-return".into()].into();
    machine.traces.arm(&machine.session, sites)?;
    let mut cursor = 0usize;
    let mut events = Vec::new();
    let mut callbacks = Vec::new();
    let mut callback_kinds = BTreeMap::new();
    let mut body_installed = false;
    let mut state_zero = false;
    let mut transaction_started = false;
    let mut seen_requests = BTreeSet::new();
    let mut returns: BTreeMap<u32, u32> = BTreeMap::new();
    let outcome = (|| -> Result<(), String> {
        loop {
            require(machine.instructions() < LIMIT, "runtime observer deadline exhausted")?;
            let before = machine.instructions();
            machine.run(LIMIT - before);
            machine.require_running()?;
            while cursor < machine.traces.hits.len() {
                let hit = &machine.traces.hits[cursor];
                cursor += 1;
                let core = hit.core;
                let pc = Guest(u32::from_str_radix(hit.address.trim_start_matches("0x"), 16).map_err(|_| "runtime trace address malformed")?);
                require(machine.session.board.read_register(core, Register::Pc)? == u64::from(pc.0), "runtime event is not at live PC")?;
                let regs = registers(&machine, core)?;
                if pc == Guest(body_startup) && !body_installed {
                    body_installed = true;
                    for (address, bytes) in &text_ranges {
                        let offset = (address - contract.body_start.0) as usize;
                        body_installed &= memory(&machine, *address, *bytes as usize)? == body_bytes[offset..offset + *bytes as usize];
                    }
                    machine.traces.add_sites(&machine.session, [(0x724cdc, "loading-service".into()), (0x725dd8, "loading-service".into())].into())?;
                    state_zero = true;
                    for (address, bytes) in &state_ranges {
                        state_zero &= memory(&machine, *address, *bytes as usize)?.iter().all(|byte| *byte == 0);
                    }
                    require(body_installed && state_zero, "loading-owned text or zero-once state differs at startup")?;
                    machine.session.board.transaction_watch_begin(vec![(0xfff52000, 0xfff53000)], 1_000_000)?;
                    transaction_started = true;
                }
                if pc == Guest(body_request) && !machine.traces.sites.contains_key(&0x72503c) {
                    machine.traces.add_sites(&machine.session, [(0x72503c, "loading-service".into())].into())?;
                }
                if pc == Guest(body_receive) {
                    let additions = [(0x725318, "loading-service".into()), (0x657114, "loading-service".into())]
                        .into_iter().filter(|(address, _)| !machine.traces.sites.contains_key(address)).collect();
                    machine.traces.add_sites(&machine.session, additions)?;
                }
                let descriptor = if pc == Guest(0x724cdc) {
                    let mut words = vec![regs.r[0]];
                    words.extend(self::words(&machine, regs.r[1], 2)?);
                    Some(words)
                } else if pc == Guest(0x725dd8) {
                    Some(words(&machine, regs.r[0], 7)?)
                } else { None };
                let envelope = if pc == Guest(0x72503c) {
                    let message = words(&machine, regs.r[1], 3)?;
                    let payload = words(&machine, message[2], 2)?;
                    let kind = u32::from(message[0] as u16).checked_sub(0x4c00).filter(|kind| *kind < 9).ok_or("owned task60 operation differs")?;
                    require(payload[1] == kind, "owned task60 message kind differs")?;
                    let record = words(&machine, envelopes_address + kind * 24, 6)?;
                    seen_requests.insert(kind);
                    Some([message, payload, record].concat())
                } else if pc == Guest(0x725318) {
                    let message = words(&machine, regs.r[1], 3)?;
                    require(regs.r[0] == 60 && regs.r[2] == u32::MAX && message[1] == 8 && message[2] != 0, "owned task60 receive buffer differs")?;
                    Some(message)
                } else { None };
                let snapshot = state(&machine)?;
                if pc == Guest(0x73ac94) && regs.r[1] != 0 {
                    callback_kinds.insert(regs.r[1], regs.r[0]);
                    if !machine.traces.sites.contains_key(&regs.r[1]) { machine.traces.add_sites(&machine.session, [(regs.r[1], "loading-callback".into())].into())?; }
                }
                if let Some(kind) = callback_kinds.get(&pc.0).copied() {
                    callbacks.push(Callback { kind, address: pc, argument: regs.r[0], failed: regs.r[1], instructions: machine.instructions() });
                }
                let link = regs.lr.0;
                events.push(Event { instructions: machine.instructions(), core, pc, registers: regs, state: snapshot.clone(), descriptor, envelope });
                if matches!(pc.0, 0x724cdc | 0x725dd8 | 0x72503c | 0x725318 | 0x657114) {
                    machine.traces.remove_sites(&machine.session, &[pc])?;
                    if !machine.traces.sites.contains_key(&link) {
                        machine.traces.add_sites(&machine.session, [(link, "loading-return".into())].into())?;
                        returns.insert(link, pc.0);
                    }
                } else if returns.remove(&pc.0).is_some() {
                    machine.traces.remove_sites(&machine.session, &[pc])?;
                }
                let terminal = match role {
                    Role::FujiModSuccess => snapshot.latches[0] == 1 && snapshot.latches[3] == 1 && snapshot.busy == 0 && snapshot.nand_state == 0
                        && seen_requests.contains(&0) && seen_requests.contains(&3) && callbacks.iter().any(|callback| callback.kind == 0 && callback.failed == 0)
                        && events.iter().any(|event| event.pc == decoder_finish),
                    Role::RetentionControl => snapshot.busy == 1 && snapshot.nand_state == 2 && snapshot.handle != 0 && snapshot.latches[0] == 0
                        && callbacks.iter().any(|callback| callback.kind == 0 && callback.failed == 1),
                };
                if terminal { return Ok(()); }
            }
            require(machine.instructions() > before, "runtime observer made no progress")?;
        }
    })();
    let transactions = if transaction_started { machine.session.board.transaction_watch_end()? } else { TransactionBatch { transactions: Vec::new(), omitted: 0 } };
    if let Err(error) = &outcome {
        let starts = transactions.transactions.iter().filter(|row| row.operation == "write" && row.address == 0xfff5201c && row.value == 0x00300000).count();
        let failure = serde_json::json!({
            "error": error, "role": role, "instructions": machine.instructions(), "machine_failure": machine.failure,
            "final_state": state(&machine).ok(), "seen_requests": seen_requests, "transaction_starts": starts,
            "transactions": transactions.transactions.len(), "deadline_calls": words(&machine, deadline_calls_address, 1).ok(),
            "callbacks": callbacks, "events": events,
            "stop_context": (0..2usize).map(|core| (machine.session.board.read_register(core, Register::Pc).ok(), registers(&machine, core).ok())).collect::<Vec<_>>(),
        });
        write(&output.join("failure.json"), &serde_json::to_vec_pretty(&failure).map_err(|error| error.to_string())?)?;
    }
    outcome?;
    let final_state = state(&machine)?;
    let starts = transactions.transactions.iter().filter(|row| row.operation == "write" && row.address == 0xfff5201c && row.value == 0x00300000).count();
    require(transactions.omitted == 0 && transactions.transactions.iter().all(|row| row.accepted), "runtime NAND transaction observation was refused or exhausted")?;
    match role {
        // The NAND window is shared with stock storage users; the owner alone issues 23 image5 + 27 plain block starts.
        Role::FujiModSuccess => require(starts >= 50 && final_state.nand_state == 0 && final_state.handle == 0 && final_state.token == 0 && final_state.latches[0] == 1 && final_state.latches[3] == 1 && final_state.busy == 0, "runtime successful read/stop/publication lifecycle differs")?,
        Role::RetentionControl => require(starts >= 1 && final_state.nand_state == 2 && final_state.handle != 0 && final_state.latches[0] == 0 && final_state.busy == 1, "runtime failed drain did not retain the storage lease")?,
    }
    require(machine.failure.is_none() && final_state.task == 60, "owned task60 context or machine state differs")?;
    let sends: Vec<_> = events.iter().filter(|event| event.pc == Guest(0x72503c)).collect();
    let receives: Vec<_> = events.iter().filter(|event| event.pc == Guest(0x725318)).collect();
    require(!sends.is_empty() && sends.len() == receives.len(), "owned task60 send/receive count differs")?;
    for event in sends {
        let envelope = event.envelope.as_ref().ok_or("owned task60 send envelope absent")?;
        // [header, length 8, data, id, kind, version, id, deadline lo, deadline hi, callback, argument]
        require(envelope.len() == 11 && envelope[1] == 8 && envelope[5] == 1 && envelope[6] == envelope[3] && envelope[3] != 0
            && (u64::from(envelope[7]) | (u64::from(envelope[8]) << 32)) != 0,
            "owned task60 message or envelope record differs")?;
    }
    let deadline_calls = words(&machine, deadline_calls_address, 1)?[0];
    require(deadline_calls as usize == receives.len(), "configured deadline provider was not called exactly once per posted request")?;
    let task = events.iter().filter_map(|event| event.descriptor.as_ref())
        .find(|descriptor| descriptor.len() == 7 && descriptor[0] == 60 && descriptor[4] == task_entry)
        .ok_or("owned task60 constructor was not called")?;
    require(task[2] == 4 && task[3] == 0xc00, "owned task60 constructor differs")?;
    let channel = events.iter().filter_map(|event| event.descriptor.as_ref())
        .find(|descriptor| descriptor.as_slice() == [60, 48, 16])
        .ok_or("owned queue60 constructor was not called")?;
    let _ = channel;
    require(events.iter().any(|event| event.pc == Guest(task_entry)) && events.iter().any(|event| event.pc == Guest(body_receive)), "owned task60 receiver did not execute")?;
    write(&output.join("trace.json"), &serde_json::to_vec_pretty(&machine.traces.hits).map_err(|error| error.to_string())?)?;
    let (_, trace_id) = identity(&output.join("trace.json"))?;
    let receipt = Receipt { schema: 1, role, run: run_id, update: update_id, rom: rom_id, executable: executable_id,
        contract: contract_id, body: body_id, symbols: symbol_id, staging_source: contract.staging_dat_sha256.clone(),
        policy: contract.deadline_policy, drain_scope: contract.drain_scope,
        events, callbacks, transactions, final_state, loading_text_installed: body_installed, loading_state_zero: state_zero,
        loading_text_ranges: text_ranges.len(), loading_state_ranges: state_ranges.len(), deadline_calls, instructions: machine.instructions(),
        machine_failure: machine.failure.clone(), trace: trace_id,
        limits: vec!["FujiMod-3d90035d synchronous-copy drain is emulator-only".into(), "retention control uses an explicit false external quiescence leaf".into(), "physical NAND quiescence remains unproved".into()], passed: true };
    write(&output.join("receipt.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    Ok(())
}

fn main() -> ExitCode {
    match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}
