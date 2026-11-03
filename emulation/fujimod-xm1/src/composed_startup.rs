use fujimod_emulator::board::observation::{GuardCore, GuardStatus, GuardStatusToken, MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_xm1::{
    attach_state::{self, AttachState, Span},
    bridge::{read, sha256, write},
    composed_catalog::{Contract, Core, Label, Pc, Role, ROW_LIMIT, Sha256},
    composed_receipt::{
        self, CaptureDirectories, GuardObservation, GuardState, HexBytes, Identity, LabelCount, MenuObservation, Outcome,
        ReadCapture, Receipt, ReceiptKind, RecordedPolicy, Row, SchemaTwo, VerdictKind, Word,
    },
    guest::Press,
    machine::{KeyPulse, Machine, RunOutcome},
    run::{self, RunDocument, ValidatedUpdate},
    trace::Hit,
};
use std::{collections::BTreeMap, num::NonZeroUsize, path::{Path, PathBuf}, process::ExitCode};

const USAGE: &str = "composed-startup --role retained|composed --contract CONTRACT --run RUN --update DAT --boot-rom ROM --output NEW_DIR [--attach-diagnostic RANGES]\n       composed-startup --compare RETAINED_RECEIPT OWNED_RECEIPT --contract CONTRACT\n       composed-startup --check-contract CONTRACT --output NEW_FILE";
const RECEIPT: &str = "composed-startup.json";
const WRITERS: &str = "attach-writers.json";
const DEADLINE: u64 = 80_000_000;
/// Instructions per run between hit drains.
const SLICE: u64 = 250_000;
const HIT_BOUND: usize = 65_536;
const WATCH_LIMIT: usize = 1_048_576;
const DIAGNOSTIC_CLASS_LIMIT: usize = 256;
const RUN_SHA256: &str = "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef";
const ROM_SHA256: &str = "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be";
const CANONICAL_DAT_SHA256: &str = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";

enum Command {
    Observe { role: Role, contract: PathBuf, run: PathBuf, update: PathBuf, rom: PathBuf, output: PathBuf, diagnostic: Option<PathBuf> },
    Compare { retained: PathBuf, owned: PathBuf, contract: PathBuf },
    CheckContract { contract: PathBuf, output: PathBuf },
}

fn require(value: bool, message: &str) -> Result<(), String> { if value { Ok(()) } else { Err(message.into()) } }

fn put<T>(slot: &mut Option<T>, value: T) -> Result<(), String> {
    require(slot.is_none(), "duplicate argument")?;
    *slot = Some(value);
    Ok(())
}

fn arguments() -> Result<Command, String> {
    let (mut role, mut contract, mut run, mut update, mut rom, mut output, mut compare) = (None, None, None, None, None, None, None);
    let (mut check, mut diagnostic) = (None, None);
    let mut arguments = std::env::args_os().skip(1);
    while let Some(flag) = arguments.next() {
        let flag = flag.into_string().map_err(|_| USAGE)?;
        let mut value = || arguments.next().map(PathBuf::from).ok_or(USAGE);
        match flag.as_str() {
            "--role" => put(&mut role, value()?.to_str().ok_or(USAGE)?.parse::<Role>()?)?,
            "--contract" => put(&mut contract, value()?)?,
            "--run" => put(&mut run, value()?)?,
            "--update" => put(&mut update, value()?)?,
            "--boot-rom" => put(&mut rom, value()?)?,
            "--output" => put(&mut output, value()?)?,
            "--compare" => { let retained = value()?; put(&mut compare, (retained, value()?))? }
            "--check-contract" => put(&mut check, value()?)?,
            "--attach-diagnostic" => put(&mut diagnostic, value()?)?,
            _ => return Err(USAGE.into()),
        }
    }
    match (check, contract, compare, role, run, update, rom, output, diagnostic) {
        (Some(contract), None, None, None, None, None, None, Some(output), None) => Ok(Command::CheckContract { contract, output }),
        (None, Some(contract), Some((retained, owned)), None, None, None, None, None, None) => Ok(Command::Compare { retained, owned, contract }),
        (None, Some(contract), None, Some(role), Some(run), Some(update), Some(rom), Some(output), diagnostic) =>
            Ok(Command::Observe { role, contract, run, update, rom, output, diagnostic }),
        _ => Err(USAGE.into()),
    }
}

fn number(value: &str) -> Result<u32, String> {
    u32::from_str_radix(value.strip_prefix("0x").ok_or("trace word is not hexadecimal")?, 16).map_err(|error| error.to_string())
}

fn hit_register(hit: &Hit, name: &str) -> Result<Word, String> {
    number(hit.registers.get(name).ok_or("hit lacks full registers")?).map(Word)
}

/// A pre-attach watch over `classes`, ended at the attach stop.
struct Diagnostic {
    classes: Vec<(u64, u64)>,
    watch: Option<Box<MemoryWatch>>,
}

struct Observation {
    rows: Vec<Row>,
    counts: BTreeMap<(Label, Core), u64>,
    recorded: BTreeMap<Label, u32>,
    unguarded: BTreeMap<(Label, Core), u64>,
    off_core: BTreeMap<(Label, Core), u64>,
    attach: Option<AttachState>,
    diagnostic: Option<Diagnostic>,
    watch: Option<Box<MemoryWatch>>,
    token: Option<GuardStatusToken>,
    attach_instructions: u64,
}

/// Writes the diagnostic watch's per-class writer summary.
fn end_diagnostic(machine: &Machine, diagnostic: &mut Diagnostic, output: &Path) -> Result<(), String> {
    let mut watch = diagnostic.watch.take().ok_or("diagnostic watch already ended")?;
    let state = watch.records.borrow().state();
    machine.session.board.memory_watch_end(&mut watch)?;
    require(state["violation"].is_null(), "diagnostic watch reported a violation")?;
    let rows: Vec<composed_receipt::WatchRow> = serde_json::from_value(state["records"].clone())
        .map_err(|error| format!("diagnostic watch rows: {error}"))?;
    let summaries = composed_receipt::carrier_writes(&diagnostic.classes, &[], &rows)?;
    write(&output.join(WRITERS), &serde_json::to_vec_pretty(&summaries).map_err(|error| error.to_string())?)
}

fn observe_hit(machine: &Machine, contract: &Contract, role: Role, sites: &BTreeMap<Pc, Label>, state: &mut Observation, hit: &Hit,
    output: &Path) -> Result<(), String> {
    let pc = Pc(number(&hit.address)?);
    let label = sites.get(&pc).ok_or("hit at an unarmed PC")?;
    let boundary = contract.boundary(label).ok_or("armed label lacks a boundary")?;
    let core = Core::from_index(hit.core)?;
    if !boundary.cores.contains(core) {
        *state.off_core.entry((label.clone(), core)).or_default() += 1;
        return Ok(());
    }
    *state.counts.entry((label.clone(), core)).or_default() += 1;
    if role == Role::Composed && state.watch.is_none() && boundary.owned() && label != &contract.guard.attach {
        *state.unguarded.entry((label.clone(), core)).or_default() += 1;
    }
    if boundary.stop {
        let current = machine.session.board.read_register(core.index(), fujimod_emulator_cpu::Register::Pc)?;
        require(current == u64::from(pc.0), "stopped core moved past its boundary")?;
    }
    let recorded = state.recorded.entry(label.clone()).or_default();
    if *recorded < boundary.record_limit {
        *recorded += 1;
        require(state.rows.len() < ROW_LIMIT, "composed observation row cap reached")?;
        let mut reads = Vec::new();
        for item in contract.reads().filter(|item| &item.boundary == label) {
            let mut bytes = vec![0; item.length as usize];
            machine.session.board.read_memory(core.index(), u64::from(item.address), &mut bytes)?;
            reads.push(ReadCapture { label: item.label.clone(), address: item.address, bytes: HexBytes(bytes) });
        }
        let r = |name| hit_register(hit, name);
        state.rows.push(Row {
            label: label.clone(), core, total_instructions: hit.instructions,
            r0: r("r0")?, r1: r("r1")?, r2: r("r2")?, r3: r("r3")?, r4: r("r4")?, r5: r("r5")?, r6: r("r6")?,
            r7: r("r7")?, r8: r("r8")?, r9: r("r9")?, r10: r("r10")?, r11: r("r11")?, r12: r("r12")?,
            sp: Word(number(&hit.sp)?), lr: Word(number(&hit.lr)?), cpsr: Word(hit.cpsr), reads,
        });
    }
    if label == &contract.guard.attach && state.watch.is_none() {
        if let Some(diagnostic) = state.diagnostic.as_mut() {
            end_diagnostic(machine, diagnostic, output)?;
        }
        let backings: Vec<(u64, u64)> = run::memory_backings(&machine.document)?.into_iter()
            .map(|backing| (backing.base, backing.size as u64)).collect();
        state.attach = Some(attach_state::capture(&machine.session.board, &backings, output)?);
        let maps = run::memory_maps(&machine.document)?;
        let mut watch = machine.session.board.memory_watch_begin_classified(contract.watch_classes(), WATCH_LIMIT, maps, ViewPolicy::Observe)?;
        let attached = machine.session.board.memory_guard_attach(&mut watch, contract.guard_config(role));
        state.watch = Some(watch);
        state.token = Some(attached.map_err(|error| error.to_string())?);
        state.attach_instructions = machine.instructions();
    }
    Ok(())
}

fn trace(machine: &mut Machine, contract: &Contract, role: Role, state: &mut Observation, output: &Path) -> Result<(), String> {
    let plan = contract.sites(role);
    machine.traces.bound_full_registers(NonZeroUsize::new(HIT_BOUND).ok_or("invalid hit bound")?);
    machine.traces.stop_labels.extend(plan.stop_labels());
    machine.traces.arm_mutable(&machine.session, plan.trace_sites())?;
    if let Some(diagnostic) = state.diagnostic.as_mut() {
        let classes = diagnostic.classes.iter().map(|&(start, end)| WatchRange { start, end, kinds: WatchKinds::WRITES }).collect();
        let maps = run::memory_maps(&machine.document)?;
        diagnostic.watch = Some(machine.session.board.memory_watch_begin_classified(classes, WATCH_LIMIT, maps, ViewPolicy::Observe)?);
    }
    while machine.instructions() < DEADLINE {
        let outcome = machine.run(SLICE.min(DEADLINE - machine.instructions()));
        machine.require_running()?;
        require(matches!(outcome, RunOutcome::Completed | RunOutcome::Stopped), "composed observation run did not settle")?;
        let hits: Vec<Hit> = machine.traces.hits.drain(..).collect();
        for hit in &hits {
            observe_hit(machine, contract, role, &plan.sites, state, hit, output)?;
        }
        if let Some(fault) = state.token.as_ref().and_then(|token| token.report().first_fault) {
            return Err(format!("composite guard fault: {fault:?}"));
        }
    }
    require(state.watch.is_some(), "guard attach boundary was not reached")
}

fn menu_route(machine: &mut Machine, menu: &mut MenuObservation) -> Result<(), String> {
    machine.run(DEADLINE.saturating_sub(machine.instructions()));
    machine.require_running()?;
    menu.erased_settings = machine.menu()?.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == (69, 52, 12));
    require(menu.erased_settings, "startup did not reach erased-settings menu")?;
    let pulse = KeyPulse { hold: 8_000_000, release: 8_000_000 };
    for key in std::iter::repeat_n("up", 12).chain(["ok", "ok"]) { machine.press_once(Press::parse(key)?, pulse)?; }
    machine.run(40_000_000);
    machine.require_running()?;
    machine.press_once(Press::parse("menu")?, pulse)?;
    machine.run(90_000_000);
    machine.require_running()?;
    menu.shooting = machine.menu()?.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == (1, 5, 2));
    require(menu.shooting, "route did not reach shooting menu")
}

fn counts(map: BTreeMap<(Label, Core), u64>) -> Vec<LabelCount> {
    map.into_iter().map(|((label, core), total)| LabelCount { label, core, total }).collect()
}

fn digest(bytes: &[u8]) -> Result<Sha256, String> { Sha256::try_from(sha256(bytes)) }

/// `[{start,end}]` physical classes for the diagnostic watch.
fn diagnostic_classes(path: &Path) -> Result<Vec<(u64, u64)>, String> {
    let spans: Vec<Span> = serde_json::from_slice(&read(path)?).map_err(|error| format!("{}: {error}", path.display()))?;
    require(!spans.is_empty() && spans.len() <= DIAGNOSTIC_CLASS_LIMIT, "diagnostic class count is outside its bound")?;
    Ok(spans.into_iter().map(|span| (span.start, span.end)).collect())
}

fn observe(role: Role, contract_path: PathBuf, run_path: PathBuf, update_path: PathBuf, rom_path: PathBuf, output: PathBuf,
    diagnostic: Option<PathBuf>) -> Result<(), String> {
    let contract_bytes = read(&contract_path)?;
    let contract = Contract::parse(&contract_bytes).map_err(|refusal| refusal.to_string())?;
    let (run_bytes, update, rom) = (read(&run_path)?, read(&update_path)?, read(&rom_path)?);
    let identity = Identity {
        run: digest(&run_bytes)?, update: digest(&update)?, boot_rom: digest(&rom)?,
        observer: digest(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)?,
        contract: digest(&contract_bytes)?,
    };
    require(identity.run.as_str() == RUN_SHA256 && identity.boot_rom.as_str() == ROM_SHA256, "observer run/ROM identity differs")?;
    require(identity.update == contract.identities(role).dat_sha256, "update DAT differs from the contract identity for this role")?;
    require(role == Role::Composed || identity.update.as_str() == CANONICAL_DAT_SHA256, "retained update is not the canonical X-M1 1.50 DAT")?;
    let diagnostic = diagnostic.as_deref().map(diagnostic_classes).transpose()?.map(|classes| Diagnostic { classes, watch: None });
    let kind = if diagnostic.is_some() { ReceiptKind::ComposedStartupDiagnostic } else { ReceiptKind::ComposedStartupReceipt };
    std::fs::create_dir(&output).map_err(|error| format!("{}: {error}", output.display()))?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    let mut state = Observation {
        rows: vec![], counts: BTreeMap::new(), recorded: BTreeMap::new(), unguarded: BTreeMap::new(), off_core: BTreeMap::new(),
        attach: None, diagnostic, watch: None, token: None, attach_instructions: 0,
    };
    let traced = trace(&mut machine, &contract, role, &mut state, &output);
    let disarmed = machine.traces.disarm(&machine.session);
    machine.traces.stop_labels.clear();
    let mut menu = MenuObservation { erased_settings: false, shooting: false };
    let routed = traced.and(disarmed).and_then(|()| menu_route(&mut machine, &mut menu));
    let end_instructions = machine.instructions();
    let watch_state = state.watch.as_ref().map(|watch| watch.records.borrow().state());
    let watch_end_error = state.watch.as_mut().and_then(|watch| machine.session.board.memory_watch_end(watch).err());
    let classes: Vec<(u64, u64)> = contract.watch_classes().iter().map(|class| (class.start, class.end)).collect();
    let owned_code = contract.owned_code_spellings();
    let carriers = watch_state.as_ref().map(|state| {
        let rows: Vec<composed_receipt::WatchRow> = serde_json::from_value(state["records"].clone())
            .map_err(|error| format!("carrier watch rows: {error}"))?;
        composed_receipt::carrier_writes(&classes, &owned_code, &rows)
    }).transpose();
    let checks: BTreeMap<Core, Vec<u64>> = Core::ALL.into_iter().map(|core| {
        let guard_core = GuardCore::new(core.index()).expect("two cores are within the guard core bound");
        (core, state.token.as_ref().map_or_else(Vec::new, |token| token.partition_checks(guard_core)))
    }).collect();
    let policies = contract.policies(role).into_iter().map(|policy| {
        let checks = checks[&policy.core].get(usize::from(policy.partition)).copied().unwrap_or(0);
        RecordedPolicy { policy, checks }
    }).collect();
    let guard = state.token.as_ref().map(|token| {
        let report = token.report();
        GuardObservation {
            attach_instructions: state.attach_instructions,
            end_instructions,
            status: match report.status {
                GuardStatus::Armed => GuardState::Armed,
                GuardStatus::Faulted => GuardState::Faulted,
                GuardStatus::Ended => GuardState::Ended,
                GuardStatus::CleanupFailed => GuardState::CleanupFailed,
            },
            first_fault: report.first_fault.map(|fault| format!("{fault:?}")),
            write_checks: report.write_checks,
            mutation_attempts: report.mutation_attempts,
            carrier_writes: carriers.as_ref().ok().cloned().flatten().unwrap_or_default(),
            watch_violation: watch_state.as_ref().and_then(|state| state["violation"].as_str()).map(str::to_owned),
            watch_end_error,
        }
    });
    let result = routed.and(carriers.map(|_| ())).and_then(|()| {
        require(guard.as_ref().is_some_and(GuardObservation::clean), "composite guard report is not clean")
    });
    let receipt = Receipt {
        schema: SchemaTwo,
        kind,
        role,
        identity,
        policies,
        unconfined: contract.unconfined(role),
        outcome: if result.is_ok() { Outcome::Observed } else { Outcome::Refused },
        error: result.as_ref().err().cloned(),
        machine_failure: machine.failure.clone(),
        instructions: machine.instructions(),
        rows: state.rows,
        counts: counts(state.counts),
        off_core: counts(state.off_core),
        unguarded: counts(state.unguarded),
        attach: state.attach,
        guard,
        menu,
    };
    write(&output.join(RECEIPT), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    result
}

fn compare(retained: PathBuf, owned: PathBuf, contract_path: PathBuf) -> Result<VerdictKind, String> {
    let contract_bytes = read(&contract_path)?;
    let contract = Contract::parse(&contract_bytes).map_err(|refusal| refusal.to_string())?;
    let receipt = |path: &PathBuf| -> Result<Receipt, String> {
        serde_json::from_slice(&read(path)?).map_err(|error| format!("{}: {error}", path.display()))
    };
    let directory = |path: &PathBuf| path.parent().map(Path::to_path_buf).ok_or_else(|| format!("{} has no directory", path.display()));
    let (retained_directory, owned_directory) = (directory(&retained)?, directory(&owned)?);
    let directories = CaptureDirectories { retained: &retained_directory, owned: &owned_directory };
    let verdict = composed_receipt::compare(&contract, &digest(&contract_bytes)?, &receipt(&retained)?, &receipt(&owned)?, &directories);
    println!("{}", serde_json::to_string_pretty(&verdict).map_err(|error| error.to_string())?);
    Ok(verdict.verdict)
}

#[derive(serde::Serialize)]
struct CheckedContract {
    contract_sha256: Sha256,
    plan: fujimod_xm1::composed_catalog::Plan,
}

fn check_contract(contract_path: PathBuf, output: PathBuf) -> Result<(), String> {
    use std::io::Write;
    let contract_bytes = read(&contract_path)?;
    let contract = Contract::parse(&contract_bytes).map_err(|refusal| refusal.to_string())?;
    let checked = CheckedContract { contract_sha256: digest(&contract_bytes)?, plan: contract.plan() };
    let bytes = serde_json::to_vec_pretty(&checked).map_err(|error| error.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(&output)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| format!("{}: {error}", output.display()))
}

fn main() -> ExitCode {
    let result = arguments().and_then(|command| match command {
        Command::Observe { role, contract, run, update, rom, output, diagnostic } =>
            observe(role, contract, run, update, rom, output, diagnostic).map(|()| true),
        Command::Compare { retained, owned, contract } => compare(retained, owned, contract).map(|verdict| verdict == VerdictKind::Equal),
        Command::CheckContract { contract, output } => check_contract(contract, output).map(|()| true),
    });
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
