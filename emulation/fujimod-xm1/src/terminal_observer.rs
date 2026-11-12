//! Terminal-state observer for preserved cold-natural-002.
//! Reuses the witness inputs and stops, takes a caller-supplied instruction
//! budget, stops at the first core-0 entry into loader A and records the first
//! entry into stage-images; each stop records its first hit and counts the rest.
//! The receipt is a diagnosis, not a pass.

use fujimod_emulator_cpu::{arm, Register};
use fujimod_xm1::machine::Machine;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use fujimod_xm1::trace::Hit;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const RUN_SHA256: &str = "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef";
const RECEIPT_SHA256: &str = "96f1972ad9d00855aa60fad40c9a3c2639eb6788ceec89982199c70d1b646b99";
const ROM_SHA256: &str = "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be";
const CONTRACT_SHA256: &str = "80d9a755bd6b4cbf8984fd94e644a88242210cb0509215dfc7827b7e536b88aa";
const ENGINE_REV: &str = "3d90035d7d8d651e126f89f94f30765506e6f53f";
/// The store path Nix computed for this crate's source.
const CRATE_SOURCE_PATH: &str = match option_env!("XM1_CRATE_SOURCE_PATH") {
    Some(value) => value,
    None => "unrecorded",
};
const MODEL_ID: &str = env!("XM1_CHECKPOINT_MODEL_ID");
const SAMPLE_INTERVAL: u64 = 1_000_000;
/// Longest run between loader-A probes and PC-range checks.
const SLICE: u64 = 10_000;
const MAX_READ: usize = 8 << 20;
const MAX_TOTAL_READ: usize = 32 << 20;

const LOADER_A_START: u32 = 0x40000;
const LOADER_A_END: u32 = 0x4dae0;
/// Loader A's staged entry word is its fourth header word.
const LOADER_A_ENTRY_WORD: u32 = 0x4000c;

struct Options {
    run: PathBuf,
    update: PathBuf,
    update_sha256: Option<String>,
    budget: u64,
    boot_rom: PathBuf,
    contract: PathBuf,
    symbols: PathBuf,
    elf: PathBuf,
    /// The image's digests, which an image re-pin changes; a checkpoint model id must not.
    elf_sha256: String,
    symbols_sha256: String,
    receipt: PathBuf,
    output: PathBuf,
}

fn usage() -> String {
    "usage: terminal-observer --run RUN.json --update PACKAGE.DAT [--update-sha256 HEX] --budget INSTRUCTIONS --boot-rom ROM --contract CONTRACT.json --symbols SYMBOLS --symbols-sha256 HEX --elf ELF --elf-sha256 HEX --receipt RECEIPT.json --output NEW_DIRECTORY".into()
}

fn parse_options() -> Result<Options, String> {
    let mut run = None;
    let mut update = None;
    let mut update_sha256 = None;
    let mut budget = None;
    let mut boot_rom = None;
    let mut contract = None;
    let mut symbols = None;
    let mut elf = None;
    let mut elf_sha256 = None;
    let mut symbols_sha256 = None;
    let mut receipt = None;
    let mut output = None;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(usage)?;
        match flag.to_str() {
            Some("--run") => run = Some(PathBuf::from(value)),
            Some("--update") => update = Some(PathBuf::from(value)),
            Some("--update-sha256") => {
                update_sha256 = Some(value.into_string().map_err(|_| usage())?);
            }
            Some("--budget") => {
                let text = value.into_string().map_err(|_| usage())?;
                budget = Some(text.parse::<u64>().map_err(|_| format!("budget is not an instruction count: {text}"))?);
            }
            Some("--boot-rom") => boot_rom = Some(PathBuf::from(value)),
            Some("--contract") => contract = Some(PathBuf::from(value)),
            Some("--symbols") => symbols = Some(PathBuf::from(value)),
            Some("--elf") => elf = Some(PathBuf::from(value)),
            Some("--elf-sha256") => elf_sha256 = Some(value.into_string().map_err(|_| usage())?),
            Some("--symbols-sha256") => symbols_sha256 = Some(value.into_string().map_err(|_| usage())?),
            Some("--receipt") => receipt = Some(PathBuf::from(value)),
            Some("--output") => output = Some(PathBuf::from(value)),
            _ => return Err(usage()),
        }
    }
    match (run, update, budget, boot_rom, contract, symbols, symbols_sha256, elf, elf_sha256, receipt, output) {
        (Some(run), Some(update), Some(budget), Some(boot_rom), Some(contract), Some(symbols), Some(symbols_sha256), Some(elf), Some(elf_sha256), Some(receipt), Some(output)) => {
            Ok(Options { run, update, update_sha256, budget, boot_rom, contract, symbols, elf, elf_sha256, symbols_sha256, receipt, output })
        }
        _ => Err(usage()),
    }
}

fn read_verified(path: &Path, expected: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != expected {
        return Err(format!("{}: sha256 {digest} is not {expected}", path.display()));
    }
    Ok(bytes)
}

fn read_digest(path: &Path) -> Result<(Vec<u8>, String), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok((bytes, digest))
}

fn parse_hex(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text.strip_prefix("0x").ok_or_else(|| format!("not hex: {text}"))?, 16)
        .map_err(|_| format!("not a 32-bit hex value: {text}"))
}

struct Stop {
    label: String,
    address: u32,
}

struct Read {
    label: String,
    address: u32,
    length: usize,
}

struct Contract {
    budget: u64,
    stops: Vec<Stop>,
    reads: Vec<Read>,
}

fn parse_contract(bytes: &[u8]) -> Result<Contract, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| format!("contract: {error}"))?;
    if value.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err("contract schema must be 1".into());
    }
    let budget = value.get("budget").and_then(Value::as_u64).ok_or("contract budget")?;
    let mut labels = BTreeSet::new();
    let mut addresses = BTreeSet::new();
    let stops = value
        .get("stops")
        .and_then(Value::as_array)
        .ok_or("contract stops")?
        .iter()
        .map(|stop| {
            let label = stop.get("label").and_then(Value::as_str).ok_or("stop label")?.to_owned();
            let address = stop.get("address").and_then(Value::as_u64).ok_or("stop address")?;
            let address = u32::try_from(address).map_err(|_| "stop address range")?;
            if address % 4 != 0 || !labels.insert(label.clone()) || !addresses.insert(address) {
                return Err("stop labels and addresses must be unique and word aligned".to_string());
            }
            Ok(Stop { label, address })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if stops.is_empty() {
        return Err("contract stops are empty".into());
    }
    let mut total = 0usize;
    let mut read_labels = BTreeSet::new();
    let reads = value
        .get("reads")
        .and_then(Value::as_array)
        .ok_or("contract reads")?
        .iter()
        .map(|read| {
            let label = read.get("label").and_then(Value::as_str).ok_or("read label")?.to_owned();
            let address = read.get("address").and_then(Value::as_u64).ok_or("read address")?;
            let length = read.get("length").and_then(Value::as_u64).ok_or("read length")?;
            let address = u32::try_from(address).map_err(|_| "read address range")?;
            let length = usize::try_from(length).map_err(|_| "read length range")?;
            if length == 0 || length > MAX_READ || !read_labels.insert(label.clone()) {
                return Err(format!("read {label} is invalid"));
            }
            total = total.checked_add(length).ok_or("read total overflow")?;
            Ok(Read { label, address, length })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if total > MAX_TOTAL_READ {
        return Err(format!("contract reads exceed {MAX_TOTAL_READ} bytes"));
    }
    Ok(Contract { budget, stops, reads })
}

#[derive(Clone)]
struct Symbol {
    start: u32,
    end: u32,
    name: String,
}

fn parse_symbols(bytes: &[u8]) -> Result<Vec<Symbol>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "symbols are not UTF-8")?;
    let mut symbols = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let Ok(start) = u32::from_str_radix(fields[0], 16) else { continue };
        let Ok(size) = u32::from_str_radix(fields[1], 16) else { continue };
        symbols.push(Symbol { start, end: start.saturating_add(size), name: fields[3].to_owned() });
    }
    symbols.sort_by_key(|symbol| symbol.start);
    for index in 0..symbols.len() {
        if symbols[index].end == symbols[index].start {
            symbols[index].end = symbols.get(index + 1).map_or(symbols[index].start + 4, |next| next.start);
        }
    }
    Ok(symbols)
}

fn symbol_at<'a>(symbols: &'a [Symbol], pc: u32) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| symbol.start <= pc && pc < symbol.end)
}

/// The stage-images symbol span is read from the pinned linked image.
fn stage_symbol(symbols: &[Symbol]) -> Result<Symbol, String> {
    symbols
        .iter()
        .find(|symbol| symbol.name.contains("stage_images"))
        .cloned()
        .ok_or_else(|| "symbols lack xm1_first_stage_stage_images".into())
}

fn register(machine: &Machine, core: usize, reg: arm::Register) -> Option<u64> {
    machine.session.board.read_register(core, Register::Arm32(reg)).ok()
}

fn register_u32(machine: &Machine, core: usize, reg: arm::Register) -> Option<u32> {
    register(machine, core, reg).and_then(|value| u32::try_from(value).ok())
}

fn read_word(machine: &Machine, core: usize, address: u32) -> Option<u32> {
    let mut bytes = [0u8; 4];
    machine.session.board.read_memory(core, u64::from(address), &mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn core_state(machine: &Machine, core: usize) -> Value {
    json!({
        "core": core,
        "pc": register(machine, core, arm::Register::R(15)),
        "sp": register(machine, core, arm::Register::R(13)),
        "lr": register(machine, core, arm::Register::R(14)),
        "cpsr": register(machine, core, arm::Register::Cpsr),
    })
}

fn instruction_words(machine: &Machine, core: usize, pc: Option<u64>) -> Vec<(u32, u32)> {
    let Some(pc) = pc.and_then(|pc| u32::try_from(pc).ok()) else { return Vec::new() };
    let start = pc.saturating_sub(4);
    let mut bytes = [0u8; 12];
    if machine.session.board.read_memory(core, u64::from(start), &mut bytes).is_err() {
        return Vec::new();
    }
    bytes
        .chunks_exact(4)
        .enumerate()
        .map(|(index, word)| (start + index as u32 * 4, u32::from_le_bytes([word[0], word[1], word[2], word[3]])))
        .collect()
}

fn word_at(words: &[(u32, u32)], address: u32) -> Option<u32> {
    words.iter().find(|(candidate, _)| *candidate == address).map(|(_, word)| *word)
}

fn branch_target(word: u32, pc: u32) -> Option<u32> {
    if word >> 24 != 0xea {
        return None;
    }
    let immediate = ((word & 0x00ff_ffff) << 8) as i32 >> 8;
    Some(pc.wrapping_add(8).wrapping_add((immediate * 4) as u32))
}

fn is_hint(word: u32) -> bool {
    word == 0xe320f003 || word == 0xe320f001
}

fn is_self_loop(words: &[(u32, u32)], pc: u32) -> bool {
    let at = |address: u32| word_at(words, address);
    (at(pc).is_some_and(is_hint) && at(pc.wrapping_add(4)).and_then(|word| branch_target(word, pc.wrapping_add(4))) == Some(pc))
        || at(pc).and_then(|word| branch_target(word, pc)) == Some(pc)
        || (at(pc).and_then(|word| branch_target(word, pc)) == Some(pc.wrapping_sub(4))
            && at(pc.wrapping_sub(4)).is_some_and(is_hint))
}

/// The receipt's symbol classes. `halt_in_place` and `fail` are the owned terminal paths, so a
/// terminal PC inside either is an owned failure finding rather than a stock one.
fn symbol_class(name: &str) -> &'static str {
    if name.contains("4halt") || name.contains("halt_in_place") {
        "halt"
    } else if name.contains("4fail") {
        "fail"
    } else if name.contains("rust_begin_unwind")
        || name.contains("panic")
        || name.contains("unwrap_failed")
        || name.contains("slice_index_fail")
    {
        "panic"
    } else if name.contains("stage_images")
        || name.contains("images")
        || name.contains("media")
        || name.contains("format")
        || name.contains("copy_loader")
        || name.contains("application_ranges")
    {
        "image"
    } else if name.contains("first_stage_main") || name.contains("first_stage_entry") {
        "orchestration"
    } else {
        "other"
    }
}

fn classify(symbols: &[Symbol], machine: &Machine, core: usize, state: &Value) -> Value {
    let Some(pc) = state.get("pc").and_then(Value::as_u64).and_then(|pc| u32::try_from(pc).ok()) else {
        return json!({"symbol_class": "no-pc", "loop": "none"});
    };
    let thumb = state.get("cpsr").and_then(Value::as_u64).is_some_and(|cpsr| cpsr & 0x20 != 0);
    let words = instruction_words(machine, core, Some(u64::from(pc)));
    let loop_kind = if thumb {
        "thumb"
    } else if is_self_loop(&words, pc) {
        "branch-or-hint-loop"
    } else {
        "none"
    };
    let symbol = symbol_at(symbols, pc);
    let name = symbol.map_or("", |symbol| symbol.name.as_str());
    let symbol_class = symbol_class(name);
    json!({
        "symbol_class": symbol_class,
        "loop": loop_kind,
        "pc": pc,
        "thumb": thumb,
        "symbol": symbol.map(|symbol| symbol.name.as_str()),
        "symbol_start": symbol.map(|symbol| symbol.start),
        "symbol_end": symbol.map(|symbol| symbol.end),
        "instruction_words": words.iter().map(|(address, word)| json!({"address": address, "word": word})).collect::<Vec<_>>(),
    })
}

fn stop_value(hit: &Hit, symbols: &[Symbol]) -> Result<Value, String> {
    let registers = ["r0", "r1", "r2", "r3"]
        .iter()
        .map(|name| {
            let value = hit.registers.get(name).ok_or("trace hit lacks a register")?;
            Ok(json!([name, parse_hex(value)?]))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let address = parse_hex(&hit.address)?;
    let symbol = symbol_at(symbols, address);
    Ok(json!({
        "site": hit.site,
        "address": address,
        "symbol": symbol.map(|symbol| symbol.name.as_str()),
        "symbol_start": symbol.is_some_and(|symbol| symbol.start == address),
        "core": hit.core,
        "instructions": hit.instructions,
        "registers": registers,
        "sp": parse_hex(&hit.sp)?,
        "lr": parse_hex(&hit.lr)?,
        "cpsr": hit.cpsr,
    }))
}

/// A contract stop that is not a symbol start names a byte inside the symbol reported here.
fn resolve_stop(symbols: &[Symbol], stop: &Stop) -> Value {
    let symbol = symbol_at(symbols, stop.address);
    json!({
        "label": stop.label,
        "address": stop.address,
        "symbol": symbol.map(|symbol| symbol.name.as_str()),
        "symbol_start": symbol.is_some_and(|symbol| symbol.start == stop.address),
    })
}

fn handoff_value(hit: &Hit) -> Result<Value, String> {
    Ok(json!({
        "observed": true,
        "source": "entry-site",
        "site": hit.site,
        "core": hit.core,
        "instructions": hit.instructions,
        "pc": parse_hex(&hit.address)?,
        "loader_a": [LOADER_A_START, LOADER_A_END],
        "registers": hit.registers,
        "sp": parse_hex(&hit.sp)?,
        "lr": parse_hex(&hit.lr)?,
        "cpsr": hit.cpsr,
    }))
}

fn read_values(machine: &Machine, reads: &[Read]) -> Result<Vec<Value>, String> {
    reads
        .iter()
        .map(|read| {
            let bytes = machine.read(u64::from(read.address), read.length)?;
            Ok(json!({
                "label": read.label,
                "address": read.address,
                "length": read.length,
                "sha256": format!("{:x}", Sha256::digest(&bytes)),
                "hex": if read.length <= 0x1000 { Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>()) } else { None },
            }))
        })
        .collect()
}

fn reproduction(expected: &Value, stops: &[Value], machine: &Machine, budget: u64, contract_budget: u64) -> Value {
    let mut mismatches = Vec::new();
    if machine.failure.is_some() {
        mismatches.push("machine_failure".to_string());
    }
    let compared = if budget == contract_budget {
        if machine.instructions() != contract_budget {
            mismatches.push(format!("instructions {} != {contract_budget}", machine.instructions()));
        }
        for stop in expected.get("stops").and_then(Value::as_array).into_iter().flatten() {
            let label = stop.get("label").and_then(Value::as_str).unwrap_or("");
            let Some(observed) = stops.iter().find(|observed| observed.get("site").and_then(Value::as_str) == Some(label)) else {
                mismatches.push(format!("{label}: absent"));
                continue;
            };
            for field in ["address", "core", "instructions"] {
                if observed.get(field) != stop.get(field) {
                    mismatches.push(format!("{label}.{field}"));
                }
            }
        }
        vec!["machine_failure", "instructions", "stops.label/address/core/instructions"]
    } else {
        vec!["machine_failure"]
    };
    let matched = if !mismatches.is_empty() {
        Value::Bool(false)
    } else if budget == contract_budget {
        Value::Bool(true)
    } else {
        Value::Null
    };
    json!({
        "matched": matched,
        "compared": compared,
        "mismatches": mismatches,
    })
}

fn run(options: &Options, expected: &Value, machine: &mut Machine, contract: &Contract, symbols: &[Symbol], stage: &Symbol) -> Result<Value, String> {
    let mut sites: BTreeMap<u32, String> =
        contract.stops.iter().map(|stop| (stop.address, stop.label.clone())).collect();
    sites.insert(stage.start, "stage-images-entry".into());
    machine.traces.arm_primary(&machine.session, sites)?;
    let expected_labels: BTreeSet<String> = contract.stops.iter().map(|stop| stop.label.clone()).collect();
    let mut observed = BTreeSet::new();
    let mut stops = Vec::new();
    let mut stop_hits: BTreeMap<String, u64> = BTreeMap::new();
    let mut samples = Vec::new();
    let mut remaining = options.budget;
    let mut hits_seen = 0usize;
    let mut next_sample = SAMPLE_INTERVAL;
    let mut handoff: Option<Value> = None;
    let mut handoff_armed = false;
    let mut stage_images_entry: Option<u64> = None;
    while remaining != 0 && machine.failure.is_none() {
        let mut stop_labels: BTreeSet<String> = expected_labels.difference(&observed).cloned().collect();
        if handoff_armed {
            stop_labels.insert("handoff".into());
        }
        machine.traces.stop_labels = stop_labels;
        let before = machine.instructions();
        let count = remaining.min(SLICE).min(next_sample.saturating_sub(before).max(1));
        machine.run(count);
        let spent = machine.instructions().saturating_sub(before);
        if spent == 0 {
            break;
        }
        remaining = remaining.saturating_sub(spent);
        for hit in &machine.traces.hits[hits_seen..] {
            match hit.site.as_str() {
                "stage-images-entry" => {
                    stage_images_entry.get_or_insert(hit.instructions);
                }
                "handoff" => handoff = Some(handoff_value(hit)?),
                _ => {
                    let address = parse_hex(&hit.address).unwrap_or(u32::MAX);
                    if let Some(stop) = contract.stops.iter().find(|stop| stop.address == address && stop.label == hit.site) {
                        *stop_hits.entry(stop.label.clone()).or_insert(0) += 1;
                        if observed.insert(stop.label.clone()) {
                            stops.push(stop_value(hit, symbols)?);
                        }
                    }
                }
            }
        }
        hits_seen = machine.traces.hits.len();
        if handoff.is_some() {
            break;
        }
        if !handoff_armed {
            if let Some(entry) = read_word(machine, 0, LOADER_A_ENTRY_WORD) {
                if (LOADER_A_START..LOADER_A_END).contains(&entry) && !machine.traces.sites.contains_key(&entry) {
                    machine.traces.add_sites(&machine.session, BTreeMap::from([(entry, "handoff".to_string())]))?;
                    handoff_armed = true;
                }
            }
        }
        if let Some(pc) = register_u32(machine, 0, arm::Register::R(15)) {
            if (LOADER_A_START..LOADER_A_END).contains(&pc) {
                handoff = Some(json!({
                    "observed": true,
                    "source": "range-sample",
                    "core": 0,
                    "instructions": machine.instructions(),
                    "resolution_instructions": SLICE,
                    "pc": pc,
                    "loader_a": [LOADER_A_START, LOADER_A_END],
                    "state": core_state(machine, 0),
                }));
                break;
            }
        }
        if machine.instructions() >= next_sample {
            samples.push(json!({
                "instructions": machine.instructions(),
                "cores": [core_state(machine, 0), core_state(machine, 1)],
            }));
            while next_sample <= machine.instructions() {
                next_sample = next_sample.saturating_add(SAMPLE_INTERVAL);
            }
        }
    }
    for stop in &mut stops {
        let hits = stop.get("site").and_then(Value::as_str).and_then(|label| stop_hits.get(label).copied()).unwrap_or(1);
        if let Some(object) = stop.as_object_mut() {
            object.insert("hits".into(), json!(hits));
        }
    }
    let terminal = json!({
        "instructions": machine.instructions(),
        "per_core_instructions": machine.session.board.instruction_counts(),
        "remaining_budget": remaining,
        "machine_failure": machine.failure,
        "cores": [core_state(machine, 0), core_state(machine, 1)],
        "classification": [classify(symbols, machine, 0, &core_state(machine, 0)), classify(symbols, machine, 1, &core_state(machine, 1))],
    });
    let reproduction = reproduction(expected, &stops, machine, options.budget, contract.budget);
    let reads = read_values(machine, &contract.reads)?;
    let contract_stops: Vec<Value> = contract.stops.iter().map(|stop| resolve_stop(symbols, stop)).collect();
    Ok(json!({
        "schema": 1,
        "kind": "terminal-observer",
        "budget": options.budget,
        "contract_budget": contract.budget,
        "loader_a": {"start": LOADER_A_START, "end": LOADER_A_END, "entry_word": LOADER_A_ENTRY_WORD},
        "handoff": handoff,
        "stage_images": {
            "symbol": stage.name,
            "start": stage.start,
            "end": stage.end,
            "entered_instructions": stage_images_entry,
            "register_map": {
                "path": "0xfff43000 device; the stage-images symbol span replaces the CPU decode loop",
            },
        },
        "contract_stops": contract_stops,
        "source": {
            "run_sha256": RUN_SHA256,
            "update_sha256": options.update_sha256.as_deref().unwrap_or("unpinned"),
            "rom_sha256": ROM_SHA256,
            "contract_sha256": CONTRACT_SHA256,
            "elf_sha256": options.elf_sha256,
            "symbols_sha256": options.symbols_sha256,
            "engine_rev": ENGINE_REV,
            "crate_source_path": CRATE_SOURCE_PATH,
            "model_id": MODEL_ID,
            "sdk_identity_note": "branch fujimod-xm1 SDK; not the pre-merge G1 SDK cold-natural-002 ran under",
            "argv": std::env::args_os().map(|argument| argument.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "observer_sha256": std::env::current_exe().ok().and_then(|path| std::fs::read(path).ok()).map(|bytes| format!("{:x}", Sha256::digest(&bytes))),
            "output": options.output,
        },
        "stops": stops,
        "samples": samples,
        "terminal": terminal,
        "reproduction": reproduction,
        "reads": reads,
        "limits": [
            "Diagnosis only; no cold-boot, handoff, physical-media or acceptance promotion.",
            "The budget is caller-supplied; at a budget other than the contract's only machine_failure is compared, and matched is null.",
            "The run stops at loader-A entry before the contract budget is spent, so at the contract budget the instruction comparison fails by construction.",
            "Contract stop addresses are resolved against the pinned symbols; symbol_start false means the address is a byte inside that symbol, not its entry.",
            "Each stop carries its first hit's registers and the count of every hit; later hits are counted, not recorded.",
            "Classification names a symbol range and instruction words; it does not prove the program's intent.",
            "Decode progress is not sampled on the 0xfff43000 device path; the receipt is not a completion claim.",
        ],
    }))
}

fn diagnose(options: &Options) -> Result<Value, String> {
    let run_bytes = read_verified(&options.run, RUN_SHA256)?;
    let (update, update_digest) = read_digest(&options.update)?;
    if let Some(expected) = &options.update_sha256 {
        if expected != &update_digest {
            return Err(format!("{}: sha256 {update_digest} is not {expected}", options.update.display()));
        }
    }
    let boot_rom = read_verified(&options.boot_rom, ROM_SHA256)?;
    let contract = parse_contract(&read_verified(&options.contract, CONTRACT_SHA256)?)?;
    let symbols = parse_symbols(&read_verified(&options.symbols, &options.symbols_sha256)?)?;
    read_verified(&options.elf, &options.elf_sha256)?;
    let stage = stage_symbol(&symbols)?;
    let expected: Value = serde_json::from_slice(&read_verified(&options.receipt, RECEIPT_SHA256)?)
        .map_err(|error| format!("receipt: {error}"))?;
    if options.output.exists() && std::fs::read_dir(&options.output).map_err(|error| error.to_string())?.next().is_some() {
        return Err(format!("{} is not empty", options.output.display()));
    }
    std::fs::create_dir_all(&options.output).map_err(|error| format!("{}: {error}", options.output.display()))?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &boot_rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    let mut receipt = run(options, &expected, &mut machine, &contract, &symbols, &stage)?;
    if let Some(source) = receipt.get_mut("source").and_then(Value::as_object_mut) {
        source.insert("update_sha256".into(), Value::String(update_digest));
    }
    Ok(receipt)
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match diagnose(&options) {
        Ok(receipt) => {
            let reproduced = receipt.get("reproduction").and_then(|value| value.get("matched")).and_then(Value::as_bool).unwrap_or(false);
            let path = options.output.join("terminal-observer.json");
            match std::fs::write(&path, serde_json::to_vec_pretty(&receipt).unwrap_or_default()) {
                Ok(()) => {
                    println!("{}", path.display());
                    if reproduced { ExitCode::SUCCESS } else { ExitCode::FAILURE }
                }
                Err(error) => {
                    eprintln!("{}: {error}", path.display());
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("terminal observer: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::symbol_class;

    /// `halt_in_place` is the owned halt; a terminal PC in it is an owned failure finding.
    #[test]
    fn the_owned_spin_loops_class_as_failures() {
        assert_eq!(symbol_class("_RNvNtCsg1oEXllPk8C_15xm1_first_stage6images13halt_in_place"), "halt");
        assert_eq!(symbol_class("_RNvCsg1oEXllPk8C_15xm1_first_stage4fail"), "fail");
        assert_eq!(symbol_class("_RNvCsg1oEXllPk8C_15xm1_first_stage9log_event"), "other");
        assert_eq!(symbol_class("_RNvNtCsg1oEXllPk8C_15xm1_first_stage6images11copy_loader"), "image");
    }
}
