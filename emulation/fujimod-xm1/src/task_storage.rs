use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::{read, sha256, write}, machine::{Machine, RunOutcome}, provenance::Guest, run::{self, RunDocument, ValidatedUpdate}, trace::Hit};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, num::NonZeroUsize, path::PathBuf, process::ExitCode};
mod task_storage_check;

const START: u32 = 0x725c6c;
const END: u32 = 0x725dd8;
const STATE: u32 = 0x40a1010c;
const CURSOR: u32 = 0x4089ea98;
const OUTPUT: u32 = 0x40a5b534;
const LOCK: u32 = 0x409c280c;
const LIMIT: usize = 32768;
const USAGE: &str = "task-storage --run JSON --update DAT --boot-rom ROM --analysis DIR --linked BIN --role retained|owned --case natural|reuse|size-error|reserved-error|capacity-error|unaligned|wrapping|alias-cursor|alias-state|failed-lock --output NEW_DIR";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Retained, Owned }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Case { Natural, Reuse, SizeError, ReservedError, CapacityError, Unaligned, Wrapping, AliasCursor, AliasState, FailedLock }
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(transparent)]
struct Address(u32);
#[derive(Deserialize, Serialize)]
#[serde(transparent)]
struct Digest(String);
#[derive(Deserialize, Serialize)]
struct Identity { run: Digest, update: Digest, rom: Digest, observer: Digest, original_image4: Digest, loader_a: Digest, loader_b: Digest, linked: Digest }
#[derive(Deserialize, Serialize)]
struct Word { guest: Address, physical: Address, values: [u32; 2], physical_value: u32 }
#[derive(Deserialize, Serialize)]
struct Buffer { physical: Address, bytes: Vec<u8>, aliases: [Digest; 4] }
#[derive(Deserialize, Serialize)]
struct Sample { hit: usize, tcm: [[u32; 2]; 2], words: Vec<Word>, watch_cursor: Option<usize>, buffer: Option<Buffer> }
#[derive(Deserialize, Serialize)]
struct Mutation { address: Address, before: u32, after: u32 }
#[derive(Deserialize, Serialize)]
struct Perturbation { hit: usize, writes: Vec<Mutation>, registers: Vec<Mutation> }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Access { CpuFetch, CpuWrite, CpuRead, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize)]
struct WatchRecord { source: Access, address: u64, physical: u64, size: u64, core: Option<usize> }
#[derive(Deserialize, Serialize)]
struct WatchSummary { enabled: bool, violation: Option<String> }
#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32, scope: &'static str, role: Role, case: Case, identity: &'a Identity,
    checkpoint_sha256: Digest, checkpoint_reused: bool, instructions: u64,
    accesses_sha256: Digest,
    completed: bool, error: Option<&'a str>, machine_failure: Option<&'a str>,
    hits: &'a [Hit], samples: &'a [Sample], perturbation: &'a Option<Perturbation>,
    watch: Result<WatchSummary, String>, watch_end_error: Option<String>, confinement: Result<[usize; 2], String>,
}

fn number(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text.strip_prefix("0x").ok_or("nonhexadecimal trace value")?, 16).map_err(|error| error.to_string())
}
fn reg(hit: &Hit, name: &str) -> Result<u32, String> { number(hit.registers.get(name).ok_or("missing register")?) }
fn word(machine: &Machine, core: usize, address: u32) -> Result<u32, String> {
    let mut bytes = [0; 4];
    machine.session.board.read_memory(core, u64::from(address), &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn physical(machine: &Machine, address: u32) -> Result<u32, String> {
    let mut bytes = [0; 4];
    machine.session.board.read_physical(u64::from(address), &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn translation(maps: &[MemoryMap], core: usize, address: u32) -> Result<u32, String> {
    let mut selected = maps.iter().filter(|map| map.core == core && map.address <= u64::from(address)
        && u64::from(address) + 4 <= map.address + map.size as u64 && map.permissions & 3 == 3);
    let map = selected.next().ok_or("data word lacks readable/writable static backing")?;
    if selected.next().is_some() { return Err("data word has ambiguous static backing".into()); }
    u32::try_from(map.physical + u64::from(address) - map.address).map_err(|error| error.to_string())
}
fn buffer(machine: &Machine, maps: &[MemoryMap], physical: u32, tcm: &[[u32; 2]; 2]) -> Result<Buffer, String> {
    if physical < 0xa10200 || physical.checked_add(0x400).is_none_or(|end| end > 0xa5a200) { return Err("task103 buffer leaves bounded pool".into()); }
    for value in tcm.iter().flatten() {
        let size = (value >> 2) & 31;
        if value & 1 != 0 && size != 0 {
            if !(3..=14).contains(&size) { return Err("reserved buffer TCM size".into()); }
            let low = u64::from(value & 0xfffff000);
            let high = low + (512u64 << size);
            for start in [physical, physical | 0x40000000] {
                if low < u64::from(start) + 0x400 && u64::from(start) < high { return Err("task103 buffer overlaps active TCM".into()); }
            }
        }
    }
    for core in 0..2 {
        for offset in (0..0x400).step_by(4) {
            for address in [physical + offset, (physical + offset) | 0x40000000] {
                if translation(maps, core, address)? != physical + offset { return Err("task103 buffer backing differs".into()); }
            }
        }
    }
    let mut bytes = vec![0; 0x400];
    machine.session.board.read_physical(u64::from(physical), &mut bytes)?;
    let mut aliases: [Digest; 4] = std::array::from_fn(|_| Digest(String::new()));
    let mut observed = [0; 0x400];
    for core in 0..2 {
        for (index, address) in [physical, physical | 0x40000000].into_iter().enumerate() {
            machine.session.board.read_memory(core, u64::from(address), &mut observed)?;
            if observed.as_slice() != bytes { return Err("task103 buffer alias/core views differ".into()); }
            aliases[2 * core + index] = Digest(sha256(&observed));
        }
    }
    Ok(Buffer { physical: Address(physical), bytes, aliases })
}
fn sample(machine: &Machine, maps: &[MemoryMap], hit: usize) -> Result<Sample, String> {
    let addresses = [STATE - 4, STATE, STATE + 4, STATE + 8, STATE + 12,
        CURSOR - 4, CURSOR, CURSOR + 4, OUTPUT - 4, OUTPUT, OUTPUT + 4, OUTPUT + 8,
        LOCK - 4, LOCK, LOCK + 4, 0x400f33e4, 0x400f33e8, 0x400f33ec, 0x400f33f0, 0x400f33f4, 0x400f33f8, 0x400f33fc,
        0x4096bd20, 0x4096bd24, 0x4004f0f8, 0x400df0b8, 0x4004dc44, 0x4004dc48, 0x400ddc04, 0x400ddc08];
    let mut tcm = [[0; 2]; 2];
    for (core, banks) in tcm.iter_mut().enumerate() {
        let cpu = machine.session.board.cpu(core)?;
        for (bank, value) in banks.iter_mut().enumerate() {
            *value = u32::try_from(cpu.coprocessor(arm::Coprocessor { cp: 15, opc1: 0, crn: 9, crm: 1,
                opc2: bank as u32, secure: 0, wide: false }, None)?).map_err(|error| error.to_string())?;
            let code = (*value >> 2) & 31;
            if *value & 1 != 0 && code != 0 {
                if !(3..=14).contains(&code) { return Err("reserved TCM size".into()); }
                let start = u64::from(*value & 0xfffff000);
                let end = start + (512u64 << code);
                if addresses.iter().any(|address| [*address, *address & !0x40000000].iter()
                    .any(|address| u64::from(*address) < end && start < u64::from(*address) + 4)) {
                    return Err("observed storage word overlaps active TCM".into());
                }
            }
        }
    }
    let words = addresses.into_iter().map(|address| {
        let expected = address & !0x40000000;
        for core in 0..2 {
            if translation(maps, core, address)? != expected || translation(maps, core, expected)? != expected {
                return Err("storage aliases have unexpected backing".into());
            }
            if word(machine, core, address)? != word(machine, core, expected)? { return Err("storage alias values differ".into()); }
        }
        let values = [word(machine, 0, address)?, word(machine, 1, address)?];
        let physical_value = physical(machine, expected)?;
        if values != [physical_value; 2] { return Err("storage core/physical views differ".into()); }
        Ok(Word { guest: Address(address), physical: Address(expected), values, physical_value })
    }).collect::<Result<_, String>>()?;
    Ok(Sample { hit, tcm, words, watch_cursor: None, buffer: None })
}
fn perturb(machine: &mut Machine, case: Case, core: usize, hit: usize) -> Result<Perturbation, String> {
    let (base, size, reserved, cursor, requested, output, lock) = match case {
        Case::Natural => return Err("natural case cannot mutate state".into()),
        Case::Reuse => (0xa10200, 0x400, 0, 0x40a10600, 0x400, OUTPUT, 0),
        Case::SizeError => (0xa10200, 0x404, 0, 0x40a10600, 0x400, OUTPUT, 0),
        Case::ReservedError => (0xa10200, 0x400, 1, 0x40a10600, 0x400, OUTPUT, 0),
        Case::CapacityError => (0, 0, 0, 0x40a5a200, 0x400, OUTPUT, 0),
        Case::Unaligned => (0, 0, 0, 0x40a10200, 3, OUTPUT, 0),
        Case::Wrapping => (0, 0, 0, 0x40a101fc, 4, OUTPUT, 0),
        Case::AliasCursor => (0, 0, 0, 0x40a10200, 0x400, CURSOR, 0),
        Case::AliasState => (0, 0, 0, 0x40a10200, 0x400, STATE, 0),
        Case::FailedLock => (0, 0, 0, 0x40a10200, 0x400, OUTPUT, 1),
    };
    let mut writes = Vec::new();
    for (address, after) in [(STATE, base), (STATE + 4, size), (STATE + 8, reserved), (CURSOR, cursor), (LOCK, lock), (OUTPUT, 0)] {
        let before = word(machine, core, address)?;
        machine.session.board.write_memory(core, u64::from(address), &u32::to_le_bytes(after))?;
        writes.push(Mutation { address: Address(address), before, after });
    }
    let mut registers = Vec::new();
    for (index, after) in [(1, requested), (2, output)] {
        let before = u32::try_from(machine.session.board.read_register(core, Register::Arm32(arm::Register::R(index)))?).map_err(|error| error.to_string())?;
        machine.session.board.write_register(core, Register::Arm32(arm::Register::R(index)), u64::from(after))?;
        registers.push(Mutation { address: Address(u32::from(index)), before, after });
    }
    Ok(Perturbation { hit, writes, registers })
}
fn sites() -> BTreeMap<u32, String> {
    let mut sites: BTreeMap<_, _> = (START..0x725dd0).step_by(4).map(|address| (address, format!("helper-{address:x}"))).collect();
    sites.extend([(0xd0020, "loader-b"), (0xd0aac, "late-initializer"), (0xd0aec, "phase4"),
        (0x69cdf8, "dispatcher"), (0x69cf34, "task103-call"), (0x69cf38, "task103-return"),
        (0x725dd8, "wrapper"), (0x725dfc, "critical-entry"), (0x725e00, "critical-entered"),
        (0x725e50, "wrapper-size-store"), (0x725e74, "helper-call"), (0x725e78, "helper-return"),
        (0x725f54, "consumer-size"), (0x725f5c, "consumer-base"), (0x725f68, "fill-call"),
        (0x725f6c, "fill-return"),
        (0x725f78, "cache-call"), (0x725fa8, "constructor-call"), (0x726024, "service-call"),
        (0x726090, "wrapper-return"), (0x427d4, "try-acquire"), (0x427fc, "release"),
        (0x41140, "irq-mask-a"), (0xd1140, "irq-mask-b"), (0x41164, "irq-restore-a"), (0xd1164, "irq-restore-b")]
        .into_iter().map(|(address, label)| (address, label.into())));
    sites
}
fn confinement(state: &serde_json::Value, hits: &[Hit]) -> Result<[usize; 2], String> {
    let summary: WatchSummary = serde_json::from_value(state.clone()).map_err(|error| error.to_string())?;
    if !summary.enabled || summary.violation.is_some() { return Err("memory watch disabled or violated".into()); }
    let mut seen = false;
    let mut counts = [0; 2];
    for raw in state.get("records").and_then(serde_json::Value::as_array).ok_or("watch records missing")? {
        let row: WatchRecord = serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
        if row.physical >= u64::from(END) || row.physical.checked_add(row.size).ok_or("watch extent overflow")? <= u64::from(START) { continue; }
        if matches!(row.source, Access::CpuFetch) {
            if row.address != row.physical { return Err("unsampled executable helper alias".into()); }
            let core = row.core.filter(|core| *core < 2).ok_or("invalid fetch core")?;
            counts[core] += 1;
            seen = true;
        } else if seen && matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
            return Err("helper write after first helper fetch".into());
        }
    }
    if !seen || !hits.iter().any(|hit| hit.code_word.is_some()) { return Err("helper code observation absent".into()); }
    Ok(counts)
}
fn drive() -> Result<(), String> {
    let check_args: Vec<_> = std::env::args_os().skip(1).collect();
    if check_args.first().is_some_and(|flag| flag == "--compare") {
        if check_args.len() != 6 { return Err("task-storage --compare RETAINED_DIR OWNED_DIR ANALYSIS_ROOT LINKED_BIN OUTPUT_JSON".into()); }
        return task_storage_check::compare(std::path::Path::new(&check_args[1]), std::path::Path::new(&check_args[2]), std::path::Path::new(&check_args[3]), std::path::Path::new(&check_args[4]), std::path::Path::new(&check_args[5]));
    }
    let mut args = std::env::args_os().skip(1);
    let mut paths = BTreeMap::new();
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_| USAGE)?;
        if !["--run", "--update", "--boot-rom", "--analysis", "--linked", "--role", "--case", "--output"].contains(&flag.as_str())
            || paths.insert(flag, PathBuf::from(args.next().ok_or(USAGE)?)).is_some() { return Err(USAGE.into()); }
    }
    let mut take = |flag| paths.remove(flag).ok_or_else(|| USAGE.to_owned());
    let run_bytes = read(&take("--run")?)?;
    let update = read(&take("--update")?)?;
    let rom = read(&take("--boot-rom")?)?;
    let analysis = take("--analysis")?;
    let linked = read(&take("--linked")?)?;
    let role: Role = serde_json::from_value(serde_json::Value::String(take("--role")?.to_str().ok_or(USAGE)?.into())).map_err(|error| error.to_string())?;
    let case: Case = serde_json::from_value(serde_json::Value::String(take("--case")?.to_str().ok_or(USAGE)?.into())).map_err(|error| error.to_string())?;
    let output = take("--output")?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    let image = read(&analysis.join("lz/id4-0061c400.bin"))?;
    let a = read(&analysis.join("decode/loader-40000.bin"))?;
    let b = read(&analysis.join("decode/loader-d0000.bin"))?;
    let identity = Identity { run: Digest(sha256(&run_bytes)), update: Digest(sha256(&update)), rom: Digest(sha256(&rom)),
        observer: Digest(sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)),
        original_image4: Digest(sha256(&image)), loader_a: Digest(sha256(&a)), loader_b: Digest(sha256(&b)), linked: Digest(sha256(&linked)) };
    if identity.run.0 != "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        || identity.rom.0 != "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"
        || identity.original_image4.0 != "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
        || identity.loader_a.0 != "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
        || identity.loader_b.0 != "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
        || identity.linked.0 != "3f9d7945e3adc43d3e7edbbfd3cbdbece6db66fc99cf0724e7722f36086c1768"
        || identity.update.0 != match role { Role::Retained => "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490", Role::Owned => "cb3834a423d288d8acc0af9d7977d919945068ffae807d1ad7a940963cde3c00" }
        || linked.len() != 228 { return Err("noncanonical source, staged candidate or linked input".into()); }
    let mut helper = image[(START - 0x61c400) as usize..(END - 0x61c400) as usize].to_vec();
    if role == Role::Owned { helper[(0x725cc4 - START) as usize..(0x725da8 - START) as usize].copy_from_slice(&linked); }
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let snapshot = machine.session.checkpoint(None)?.export()?;
    write(&output.join("reset.fujimod"), &snapshot)?;
    let restored = machine.session.import_snapshot(&snapshot)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    if machine.instructions() != 0 { return Err("checkpoint not preexecution reset".into()); }
    machine.traces.bind_code_words(NonZeroUsize::new(LIMIT).ok_or("invalid hit bound")?, Guest(START), helper, maps.clone())?;
    let sites = sites();
    machine.traces.stop_labels.extend(sites.values().cloned());
    machine.traces.arm(&machine.session, sites)?;
    let mut classes: Vec<_> = [(0xa0fc44, 0xa101e4), (0xa5b200, 0xa5b5c0), (0x89ea94, 0x89eaa0),
        (0x9c2808, 0x9c2814), (0xf3300, 0xf3420), (0xa10200, 0xa5a200)].into_iter().map(|(start, end)| WatchRange {
            start, end, kinds: WatchKinds::CPU_READ.union(WatchKinds::WRITES),
        }).collect();
    classes.push(WatchRange { start: u64::from(START), end: u64::from(END), kinds: WatchKinds::CPU_FETCH.union(WatchKinds::WRITES) });
    let mut watch = machine.session.board.memory_watch_begin_classified(classes, 131072, maps.clone(), ViewPolicy::Observe)?;
    let mut samples = Vec::new();
    let mut perturbation = None;
    let mut selected = None;
    let mut processed = 0;
    let result = (|| -> Result<(), String> {
        loop {
            let outcome = machine.run(300_000_000u64.saturating_sub(machine.instructions()));
            machine.require_running()?;
            if outcome != RunOutcome::Stopped { return Err("natural caller deadline before selected return".into()); }
            let end = machine.traces.hits.len();
            for index in processed..end {
                let hit = &machine.traces.hits[index];
                let address = number(&hit.address)?;
                let core = hit.core;
                if address == START && reg(hit, "r0")? == 102 && selected.is_none() {
                    if hit.sp != "0x400f33a8" || hit.lr != "0x725e78" || reg(hit, "r1")? != 0x400 || reg(hit, "r2")? != OUTPUT {
                        return Err("task103 helper ABI differs from admitted natural caller".into());
                    }
                    selected = Some(core);
                    if case != Case::Natural { perturbation = Some(perturb(&mut machine, case, core, index)?); }
                }
                if selected.is_some() {
                    let mut observed = sample(&machine, &maps, index)?;
                    if selected == Some(core) && matches!(address, 0x725e78 | 0x69cf38) {
                        observed.watch_cursor = Some(watch.records.borrow().state().get("records")
                            .and_then(serde_json::Value::as_array).ok_or("watch records absent at boundary")?.len());
                    }
                    if selected == Some(core) && case == Case::Natural && matches!(address, 0x725e78 | 0x725f6c | 0x69cf38) {
                        observed.buffer = Some(buffer(&machine, &maps, word(&machine, core, OUTPUT)?, &observed.tcm)?);
                    }
                    samples.push(observed);
                    if selected == Some(core) && address == 0x725e78 && case != Case::Natural { return Ok(()); }
                    if selected == Some(core) && address == 0x69cf38 { return Ok(()); }
                }
            }
            processed = end;
        }
    })();
    let state = watch.records.borrow().state();
    let accesses = serde_json::to_vec_pretty(&state).map_err(|error| error.to_string())?;
    write(&output.join("accesses.json"), &accesses)?;
    let summary = if state.get("enabled").is_some() && state.get("violation").is_some() {
        serde_json::from_value(state.clone()).map_err(|error| error.to_string())
    } else { Err("watch summary fields absent".into()) };
    let confined = confinement(&state, &machine.traces.hits);
    let watch_end_error = machine.session.board.memory_watch_end(&mut watch).err();
    let completed = result.is_ok() && confined.is_ok() && watch_end_error.is_none();
    let receipt = Receipt { schema: 1, scope: "natural-reset-B-phase4-task103-and-labelled-perturbed-helper-controls-only", role, case, identity: &identity,
        checkpoint_sha256: Digest(sha256(&snapshot)), checkpoint_reused: true, instructions: machine.instructions(), completed,
        accesses_sha256: Digest(sha256(&accesses)),
        error: result.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(), hits: &machine.traces.hits, samples: &samples,
        perturbation: &perturbation, watch: summary, watch_end_error, confinement: confined };
    write(&output.join("task-storage.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    if completed { Ok(()) } else { Err("storage observation refused; preserved receipt records the boundary".into()) }
}
fn main() -> ExitCode { match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } } }

#[cfg(test)]
mod tests {
    use super::confinement;
    use fujimod_xm1::trace::{CodeWord, Hit};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn storage_code_refuses_aliases_writes_and_missing_observation() {
        let hit = Hit { site: "helper".into(), address: "0x725cc4".into(), core: 1,
            instructions: 1, registers: BTreeMap::new(), sp: "0x400f3388".into(),
            render_object_provenance_sequence: None,
            lr: "0x725e78".into(), cpsr: 0xd3,
            code_word: Some(CodeWord { guest: 0xe320f000, physical: 0xe320f000,
                expected: 0xe320f000, tcm: [[0; 2]; 2] }), memory: Vec::new(), stack_memory: Vec::new(), observation: None };
        let canonical = json!({"enabled":true,"violation":null,"records":[
            {"source":"cpu-fetch","core":1,"address":0x725cc4,"physical":0x725cc4,"size":4}]});
        assert!(confinement(&canonical, std::slice::from_ref(&hit)).is_ok());
        let alias = json!({"enabled":true,"violation":null,"records":[
            {"source":"cpu-fetch","core":1,"address":0x40725cc4,"physical":0x725cc4,"size":4}]});
        assert!(confinement(&alias, std::slice::from_ref(&hit)).is_err());
        let changed = json!({"enabled":true,"violation":null,"records":[
            {"source":"cpu-fetch","core":1,"address":0x725cc4,"physical":0x725cc4,"size":4},
            {"source":"host-write","core":null,"address":0x725cc4,"physical":0x725cc4,"size":4}]});
        assert!(confinement(&changed, std::slice::from_ref(&hit)).is_err());
        assert!(confinement(&canonical, &[]).is_err());
    }
}
