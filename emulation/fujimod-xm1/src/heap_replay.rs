mod heap_accesses;
mod heap_state;
mod heap_replay_state;
mod heap_replay_contract;
mod heap_replay_runtime;
mod heap_replay_check;

use heap_accesses::Digest;
use heap_replay_contract::*;
use heap_replay_state::Tape;
use heap_state::{Address, Event, Summary};
use fujimod_xm1::bridge::sha256;
use serde::Serialize;
use std::{collections::BTreeMap, fs::{File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, process::ExitCode};

const LIMIT: u64 = 256 * 1024 * 1024;
const USAGE: &str = "heap-replay --mode run|check|refusal-controls --run RUN --update DAT --boot-rom ROM --analysis ROOT --trace TRACE --admission COMPARISON --seed CHECKPOINT --image BIN --symbols SYMBOLS --backend stock|rlsf|talc|buddy --capacity original|recipe --output NEW_DIRECTORY [--case CASE_JSON]";
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Flag { Mode, Run, Update, BootRom, Analysis, Trace, Admission, Seed, Image, Symbols, Backend, Capacity, Output, Case }
impl Flag {
    fn parse(value: &str) -> Result<Self, Failure> {
        Ok(match value { "--mode" => Self::Mode, "--run" => Self::Run, "--update" => Self::Update,
            "--boot-rom" => Self::BootRom, "--analysis" => Self::Analysis, "--trace" => Self::Trace,
            "--admission" => Self::Admission, "--seed" => Self::Seed, "--image" => Self::Image,
            "--symbols" => Self::Symbols, "--backend" => Self::Backend, "--capacity" => Self::Capacity,
            "--output" => Self::Output, "--case" => Self::Case, _ => return Err(Failure::Evidence) })
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode { Run, Check, RefusalControls }
struct Arguments {
    mode: Mode, backend: Backend, capacity: Capacity, run: PathBuf, update: PathBuf, boot_rom: PathBuf,
    analysis: PathBuf, trace: PathBuf, admission: PathBuf, seed: PathBuf, image: PathBuf, symbols: PathBuf,
    output: PathBuf, case: Option<PathBuf>,
}
impl Arguments {
    fn parse() -> Result<Self, Failure> {
        let mut raw = std::env::args().skip(1);
        let mut values = BTreeMap::new();
        while let Some(flag) = raw.next() {
            let flag = Flag::parse(&flag)?;
            let value = raw.next().ok_or(Failure::Evidence)?;
            if values.insert(flag, value).is_some() { return Err(Failure::Evidence); }
        }
        let mut take = |flag| values.remove(&flag).ok_or(Failure::Evidence);
        let mode = match take(Flag::Mode)?.as_str() { "run" => Mode::Run, "check" => Mode::Check,
            "refusal-controls" => Mode::RefusalControls, _ => return Err(Failure::Evidence) };
        let backend = match take(Flag::Backend)?.as_str() { "stock" => Backend::Stock, "rlsf" => Backend::Rlsf,
            "talc" => Backend::Talc, "buddy" => Backend::Buddy, _ => return Err(Failure::Evidence) };
        let capacity = match take(Flag::Capacity)?.as_str() { "original" => Capacity::Original, "recipe" => Capacity::Recipe,
            _ => return Err(Failure::Evidence) };
        let run = take(Flag::Run)?.into();
        let update = take(Flag::Update)?.into();
        let boot_rom = take(Flag::BootRom)?.into();
        let analysis = take(Flag::Analysis)?.into();
        let trace = take(Flag::Trace)?.into();
        let admission = take(Flag::Admission)?.into();
        let seed = take(Flag::Seed)?.into();
        let image = take(Flag::Image)?.into();
        let symbols = take(Flag::Symbols)?.into();
        let output = take(Flag::Output)?.into();
        let case = values.remove(&Flag::Case).map(PathBuf::from);
        if !values.is_empty() || (mode == Mode::Run) == case.is_some() { return Err(Failure::Evidence); }
        Ok(Self { mode, backend, capacity, run, update, boot_rom, analysis, trace, admission, seed, image, symbols, output, case })
    }
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, Failure> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    if length > limit { return Err(Failure::InputIdentity); }
    let mut bytes = Vec::with_capacity(usize::try_from(length).map_err(|_| Failure::InputIdentity)?);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit { return Err(Failure::InputIdentity); }
    Ok(bytes)
}
fn pinned(path: &Path, wanted: Digest, limit: u64) -> Result<(Vec<u8>, Digest), Failure> {
    let bytes = read_bounded(path, limit)?;
    let actual = Digest(sha256(&bytes));
    if actual != wanted { return Err(Failure::InputIdentity); }
    Ok((bytes, actual))
}
fn symbols(bytes: &[u8], payload_bytes: usize) -> Result<BTreeMap<Symbol, Address>, Failure> {
    let text = std::str::from_utf8(bytes).map_err(|_| Failure::Symbols)?;
    let mut symbols = BTreeMap::new();
    for line in text.lines() {
        let mut columns = line.split_whitespace();
        let Some(address) = columns.next() else { continue; };
        let Some(symbol) = columns.next_back().and_then(Symbol::parse) else { continue; };
        if columns.next().is_none() { return Err(Failure::Symbols); }
        let address = Address(u32::from_str_radix(address, 16).map_err(|_| Failure::Symbols)?);
        if symbols.insert(symbol, address).is_some() { return Err(Failure::Symbols); }
    }
    let required = [Symbol::Init, Symbol::Allocate, Symbol::Free, Symbol::Footprint, Symbol::Statistics,
        Symbol::Zero, Symbol::Copy, Symbol::Status, Symbol::Panic, Symbol::TextStart, Symbol::TextEnd,
        Symbol::ReadOnlyEnd, Symbol::ImageStart, Symbol::ImageEnd, Symbol::BssStart, Symbol::BssEnd];
    if required.iter().any(|symbol| !symbols.contains_key(symbol)) { return Err(Failure::Symbols); }
    let address = |symbol| symbols.get(&symbol).copied().ok_or(Failure::Symbols);
    let text_start = address(Symbol::TextStart)?.0;
    let text_end = address(Symbol::TextEnd)?.0;
    let read_only_end = address(Symbol::ReadOnlyEnd)?.0;
    let bss_start = address(Symbol::BssStart)?.0;
    let bss_end = address(Symbol::BssEnd)?.0;
    if address(Symbol::ImageStart)?.0 != IMAGE_BASE || text_start != IMAGE_BASE || text_start >= text_end
        || text_end > read_only_end || read_only_end > bss_start || bss_start > bss_end || bss_end != address(Symbol::ImageEnd)?.0
        || bss_end > IMAGE_LIMIT || payload_bytes > (bss_start - IMAGE_BASE) as usize || (read_only_end - IMAGE_BASE) as usize > payload_bytes {
        return Err(Failure::Symbols);
    }
    for symbol in [Symbol::Init, Symbol::Allocate, Symbol::Free, Symbol::Footprint, Symbol::Statistics, Symbol::Zero, Symbol::Copy, Symbol::Panic] {
        let entry = address(symbol)?.0;
        if entry < text_start || entry >= text_end || entry % 4 != 0 { return Err(Failure::Symbols); }
    }
    let status = address(Symbol::Status)?.0;
    if status < bss_start || status.checked_add(4).is_none_or(|end| end > bss_end) || status % 4 != 0 { return Err(Failure::Symbols); }
    Ok(symbols)
}
fn prepare(arguments: &Arguments) -> Result<(Inputs, Tape), Failure> {
    let (run, run_digest) = pinned(&arguments.run, Digest("efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef".into()), LIMIT)?;
    let (update, update_digest) = pinned(&arguments.update, Digest("43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490".into()), LIMIT)?;
    let (boot_rom, boot_rom_digest) = pinned(&arguments.boot_rom, Digest("4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be".into()), LIMIT)?;
    let (seed, seed_digest) = pinned(&arguments.seed, Digest("48b229e2d61e2875fd1dd2c54fd39d5f19bcd4f68f5ad487a8acad8d1b243a7b".into()), LIMIT)?;
    let (event_bytes, event_digest) = pinned(&arguments.trace.join("events.json"), Digest("798b8dd160509598a2b01d4f37bb811794c3bd247028bcd7c48510fe528e6bab".into()), LIMIT)?;
    let events: Vec<Event> = serde_json::from_slice(&event_bytes)?;
    drop(event_bytes);
    let (summary_bytes, summary_digest) = pinned(&arguments.trace.join("summary.json"), Digest("db6414549e0b895d1a9242bcffa8ba1996f080a776c169284ee4f571f384665e".into()), LIMIT)?;
    let summary: Summary = serde_json::from_slice(&summary_bytes)?;
    let (_, admission_digest) = pinned(&arguments.admission, Digest("d661a4ee39c4676f2998bbd3875e2477d13c1a921188beb2021b4a99e75a12c2".into()), LIMIT)?;
    let tape = Tape::from_events(&events).map_err(|_| Failure::Tape)?;
    tape.bind_summary(&summary).map_err(|_| Failure::Tape)?;
    let payload = read_bounded(&arguments.image, u64::from(IMAGE_LIMIT - IMAGE_BASE))?;
    let symbol_bytes = read_bounded(&arguments.symbols, 4 * 1024 * 1024)?;
    let symbols = symbols(&symbol_bytes, payload.len())?;
    let payload_digest = Digest(sha256(&payload));
    let symbols_digest = Digest(sha256(&symbol_bytes));
    let limit = heap_accesses::ByteLimit::new(LIMIT).ok_or(Failure::Evidence)?;
    let replay_producer = heap_accesses::digest_file(&std::env::current_exe()?, limit).map_err(|_| Failure::InputIdentity)?;
    let mut images = Vec::with_capacity(3);
    for (path, base, wanted) in [
        ("decode/loader-40000.bin", 0x00040000, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"),
        ("lz/id4-0061c400.bin", 0x0061c400, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"),
        ("lz/id5-00bf1a00.bin", 0x00bf1a00, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"),
    ] {
        let (bytes, _) = pinned(&arguments.analysis.join(path), Digest(wanted.into()), LIMIT)?;
        images.push(CanonicalImage { base: Address(base), bytes });
    }
    let bindings = Bindings { run: run_digest, update: update_digest, boot_rom: boot_rom_digest, seed: seed_digest,
        payload: payload_digest, symbols: symbols_digest, events: event_digest, summary: summary_digest,
        admission: admission_digest, replay_producer };
    Ok((Inputs { run, update, boot_rom, seed, payload, symbols, images, bindings }, tape))
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Failure> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if bytes.len() as u64 > LIMIT { return Err(Failure::Evidence); }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}
#[derive(Serialize)]
struct Admission<'a> { schema: u32, admitted: bool, instruction_model: &'static str, summary: &'a CaseSummary }
fn execute(arguments: Arguments) -> Result<(), Failure> {
    let (inputs, tape) = prepare(&arguments)?;
    std::fs::create_dir(&arguments.output)?;
    write_json(&arguments.output.join("replay-input.json"), &tape)?;
    let case = if arguments.mode == Mode::Run {
        let case = heap_replay_runtime::run_case(&inputs, &tape, arguments.backend, arguments.capacity, &arguments.output);
        write_json(&arguments.output.join("case.json"), &case)?;
        case
    } else {
        let bytes = read_bounded(arguments.case.as_ref().ok_or(Failure::Evidence)?, LIMIT)?;
        serde_json::from_slice::<Case>(&bytes)?
    };
    if case.backend != arguments.backend || case.capacity != arguments.capacity { return Err(Failure::InputIdentity); }
    let admitted = heap_replay_check::admit(&inputs, &tape, &case);
    let summary = match admitted {
        Ok(summary) => summary,
        Err(error) => { write_json(&arguments.output.join("admission-error.json"), &error)?; return Err(error); }
    };
    write_json(&arguments.output.join("admission.json"), &Admission { schema: 1, admitted: true,
        instruction_model: "Cortex-R4F-admitted A32 entered instructions, not silicon cycles; isolated core0 with interrupt delivery suppressed", summary: &summary })?;
    if arguments.mode == Mode::RefusalControls {
        let controls = heap_replay_check::refusal_controls(&inputs, &tape, &case, &arguments.output.join("controls"))?;
        write_json(&arguments.output.join("refusal-controls.json"), &controls)?;
    }
    Ok(())
}
fn main() -> ExitCode {
    match Arguments::parse().and_then(execute) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("{error}\n{USAGE}"); ExitCode::FAILURE }
    }
}
