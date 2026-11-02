use super::*;
use fujimod_xm1::{guest::RouteKey, render::Probe};
use fujimod_xm1::trace::{TraceStackScopeConfig, TraceStackScopeSummary, TraceStackSkip, TraceStackViolation};

#[path = "application_pump_capture.rs"]
mod capture;
#[path = "application_pump_check.rs"]
mod application_pump_check;

const CARRIER: Guest = Guest(0xbfbd20);
const CARRIER_BYTES: usize = 32;
const ORIGINAL_DAT: &str = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";
const RUN_SHA: &str = "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef";
const ROM_SHA: &str = "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Mode { Retained, Owned }
impl Mode {
    pub(super) fn parse(value: &Path) -> Result<Self, String> {
        match value.to_str() { Some("retained") => Ok(Self::Retained), Some("owned") => Ok(Self::Owned), _ => Err("application-pump mode must be retained or owned".into()) }
    }
}
pub(super) struct Request {
    pub(super) mode: Mode,
    pub(super) run: PathBuf,
    pub(super) update: PathBuf,
    pub(super) original_update: PathBuf,
    pub(super) boot_rom: PathBuf,
    pub(super) analysis: PathBuf,
    pub(super) output: PathBuf,
    pub(super) code: PathBuf,
    pub(super) symbols: PathBuf,
    pub(super) admission: PathBuf,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct SampleId(usize);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct InputId(usize);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct EpochId(usize);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct RowIndex(usize);
#[derive(Clone, Debug, Deserialize, Serialize)]
struct PumpIdentity {
    original_update: FileBinding,
    actual_update: FileBinding,
    code: FileBinding,
    carrier: FileBinding,
    symbols: FileBinding,
    admission: FileBinding,
    run: FileBinding,
    boot_rom: FileBinding,
    observer: Digest,
    canonical_sources: Vec<SourceBinding>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Qualification { ConditionalSourceOnly }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct MechanismAdmission {
    source_dat_sha256: Digest,
    source_image5_sha256: Digest,
    linked_sha256: Digest,
    symbols_sha256: Digest,
    staged_image5_sha256: Digest,
    carrier_sha256: Digest,
    carrier: [Guest; 2],
    emitted_bytes: ByteCount,
    entry: Guest,
    caller: Guest,
    #[serde(rename = "return")]
    continuation: Guest,
    backedge: Guest,
    retained_calls: [Guest; 3],
    image5_base: Guest,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct PumpAdmission {
    schema: u32,
    qualification: Qualification,
    candidate_dat_sha256: Digest,
    candidate_image5_sha256: Digest,
    application_pump: MechanismAdmission,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Refusal {
    ArtifactIdentity, Publication, CodeIdentity, Mapping, TaskContext, StackContext,
    Initializer, QueueProvenance, HookSequence, NormalizerResult, Handback,
    RenderCompletion, CaptureBound, Guest, RouteUnavailable,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Failure { reason: Refusal, detail: String }
impl Failure {
    fn new(reason: Refusal, detail: impl Into<String>) -> Self { Self { reason, detail: detail.into() } }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Outcome { Captured, Refused { failure: Failure } }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct HostReads { epoch: EpochId, first: RowIndex, end: RowIndex, stopped: Count }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Sample {
    id: SampleId,
    stop: Count,
    resume: Count,
    counts: [u64; 2],
    identities: [u32; 2],
    core: Core,
    pc: Guest,
    registers: Registers,
    code: Words,
    carrier: [Words; 2],
    task: Vec<Words>,
    stack: Vec<Words>,
    state: Vec<Words>,
    packet: Vec<Words>,
    host_reads: HostReads,
    raw_views: FileBinding,
    input: Option<InputId>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Views { Observe, Fixed }
#[derive(Deserialize, Serialize)]
struct PumpEpoch {
    id: EpochId,
    start: Count,
    end: Count,
    views: Views,
    enabled: bool,
    refused: bool,
    teardown_error: Option<String>,
    classes: Vec<Class>,
    record_count: usize,
    raw: FileBinding,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum InputPurpose { FirstStart, Ordinary, BackHandback, AfterBack, UnhandledProbe }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Input {
    id: InputId,
    key: RouteKey,
    purpose: InputPurpose,
    pressed: Count,
    released: Option<Count>,
    debounced: Option<Count>,
    settled: Option<Count>,
    before: Option<Probe>,
    after: Option<Probe>,
}
#[derive(Deserialize, Serialize)]
struct Visible {
    input: Option<InputId>,
    instructions: Count,
    probe: Probe,
    menu: Option<[u32; 3]>,
    state: Vec<Words>,
    task_tables: Vec<Words>,
    png: FileBinding,
    planes: FileBinding,
    scanout: FileBinding,
    graphics: FileBinding,
    host_reads: HostReads,
    raw_views: FileBinding,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Route { RepeatedReceive, PendingWork, BackThenInput, Unhandled, ReceiveFailure, ServiceFailure }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Reachability { Unqualified, Observed { samples: Vec<SampleId> } }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct RouteCoverage { route: Route, reachability: Reachability }
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum TraceScopeRecord {
    Begin { sample: SampleId, epoch: EpochId, config: TraceStackScopeConfig, instructions: Count },
    Site { site: Guest, epoch: EpochId, first_full_hit: RowIndex, instructions: Count },
    Skip { hit: TraceStackSkip },
    Finish { summary: TraceStackScopeSummary },
}
#[derive(Deserialize, Serialize)]
struct LoaderIdentity { run: Digest, update: Digest, rom: Digest, executable: Digest, sources: Vec<SourceBinding> }
#[derive(Deserialize, Serialize)]
struct LoaderEvidence {
    schema: u32,
    identity: LoaderIdentity,
    checkpoint: Digest,
    instructions: Count,
    menu: Result<Option<[u32; 3]>, String>,
    error: Option<String>,
    machine_failure: Option<String>,
    events: Vec<Event>,
    calls: Vec<Call>,
    snapshots: Vec<Snapshot>,
    pulses: Vec<Pulse>,
    epochs: Vec<Epoch>,
    trace: FileBinding,
    static_calls: Vec<StaticCall>,
}
#[derive(Deserialize, Serialize)]
struct PumpReport {
    schema: u32,
    mode: Mode,
    identity: PumpIdentity,
    checkpoint: Digest,
    instructions: Count,
    outcome: Outcome,
    loader: LoaderEvidence,
    samples: Vec<Sample>,
    epochs: Vec<PumpEpoch>,
    inputs: Vec<Input>,
    visible: Vec<Visible>,
    routes: Vec<RouteCoverage>,
    trace_scope: Option<FileBinding>,
}

fn file_identity(path: &Path, bytes: &[u8]) -> FileBinding {
    FileBinding { path: path.to_owned(), sha256: Digest::of(bytes) }
}
fn require_identity(actual: &Digest, expected: &str, name: &str) -> Result<(), String> {
    if actual.as_str() == expected { Ok(()) } else { Err(format!("{name} differs from the canonical reset route")) }
}
fn output_directory(path: &Path) -> Result<(), String> {
    let root = std::fs::canonicalize("private-data/scratch/app154").map_err(|error| error.to_string())?;
    let parent = std::fs::canonicalize(path.parent().ok_or("output has no parent")?).map_err(|error| error.to_string())?;
    if !parent.starts_with(root) { return Err("application-pump output must be a new directory under private-data/scratch/app154".into()); }
    std::fs::create_dir(path).map_err(|error| error.to_string())
}
pub(super) fn run(request: Request) -> Result<(), String> {
    output_directory(&request.output)?;
    let result = produce(&request);
    if let Err(error) = &result {
        #[derive(Serialize)]
        struct ErrorReceipt<'a> { error: &'a str, report_present: bool }
        persist(&request.output.join("observer-error.json"), &ErrorReceipt { error, report_present: request.output.join("application-pump.json").is_file() })?;
    }
    result
}
fn produce(request: &Request) -> Result<(), String> {
    let run_bytes = read(&request.run)?;
    let actual = read(&request.update)?;
    let original = read(&request.original_update)?;
    let rom = read(&request.boot_rom)?;
    let code = read(&request.code)?;
    let symbols = read(&request.symbols)?;
    let admission_bytes = read(&request.admission)?;
    let admission: PumpAdmission = serde_json::from_slice(&admission_bytes).map_err(|error| format!("pump packaging admission: {error}"))?;
    if code.is_empty() || code.len() > CARRIER_BYTES || code.len() % 4 != 0 || symbols.is_empty() { return Err("pump requires aligned4..32-byte linked code and its symbol artifact".into()); }
    let mut carrier = [0; CARRIER_BYTES];
    for word in carrier.chunks_exact_mut(4) { word.copy_from_slice(&0xe320f000u32.to_le_bytes()); }
    carrier[..code.len()].copy_from_slice(&code);
    let mut catalog = Catalog::open(&request.analysis)?;
    let canonical_sources = catalog.0.iter().map(|image| image.binding.clone()).collect();
    let identity = PumpIdentity {
        original_update: file_identity(&request.original_update, &original),
        actual_update: file_identity(&request.update, &actual),
        code: file_identity(&request.code, &code),
        carrier: file_identity(Path::new("pump-carrier.bin"), &carrier),
        symbols: file_identity(&request.symbols, &symbols),
        admission: file_identity(&request.admission, &admission_bytes),
        run: file_identity(&request.run, &run_bytes),
        boot_rom: file_identity(&request.boot_rom, &rom),
        observer: Digest::of(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?),
        canonical_sources,
    };
    persist(&request.output.join("inputs.json"), &identity)?;
    require_identity(&identity.original_update.sha256, ORIGINAL_DAT, "original DAT")?;
    require_identity(&identity.run.sha256, RUN_SHA, "run document")?;
    require_identity(&identity.boot_rom.sha256, ROM_SHA, "boot ROM")?;
    let mechanism = &admission.application_pump;
    require_identity(&mechanism.source_dat_sha256, ORIGINAL_DAT, "admission source DAT")?;
    require_identity(&mechanism.source_image5_sha256, SOURCES[5].sha256, "admission source image5")?;
    require_identity(&mechanism.linked_sha256, identity.code.sha256.as_str(), "admission linked code")?;
    require_identity(&mechanism.symbols_sha256, identity.symbols.sha256.as_str(), "admission symbols")?;
    require_identity(&mechanism.carrier_sha256, identity.carrier.sha256.as_str(), "admission padded carrier")?;
    if admission.schema != 1 || mechanism.carrier != [CARRIER, Guest(CARRIER.0 + CARRIER_BYTES as u32)] || mechanism.emitted_bytes.0 as usize != code.len() || mechanism.image5_base != Guest(0xbf1a00) || mechanism.entry != CARRIER || mechanism.caller != Guest(0xc23274) || mechanism.continuation != Guest(0xc23278) || mechanism.backedge != Guest(0xc22f84) || mechanism.retained_calls != [Guest(0xbfb1b8), Guest(0xbfb20c), Guest(0xbfb1e0)] {
        return Err("packaging admission differs from the pump contract".into());
    }
    if request.mode == Mode::Retained {
        require_identity(&identity.actual_update.sha256, ORIGINAL_DAT, "retained DAT")?;
    } else {
        if identity.actual_update.sha256.as_str() == ORIGINAL_DAT { return Err("owned mode refuses the original DAT".into()); }
        require_identity(&admission.candidate_dat_sha256, identity.actual_update.sha256.as_str(), "admitted candidate DAT")?;
        let image = catalog.0.iter_mut().find(|image| image.binding.kind == SourceKind::Image5).ok_or("image5 absent")?;
        let offset = (CARRIER.0 - image.binding.start.0) as usize;
        if image.bytes[offset..offset + CARRIER_BYTES] == carrier { return Err("owned code is the original pump".into()); }
        image.bytes[offset..offset + CARRIER_BYTES].copy_from_slice(&carrier);
        image.binding.sha256 = Digest::of(&image.bytes);
        require_identity(&admission.candidate_image5_sha256, image.binding.sha256.as_str(), "admitted candidate image5")?;
        require_identity(&mechanism.staged_image5_sha256, image.binding.sha256.as_str(), "staged image5")?;
    }
    application_pump_check::admit_inputs(&original, &actual, request.mode, &catalog)?;
    write(&request.output.join("pump-code.bin"), &code)?;
    write(&request.output.join("pump-carrier.bin"), &carrier)?;
    write(&request.output.join("pump-symbols.txt"), &symbols)?;
    write(&request.output.join("pump-admission.json"), &admission_bytes)?;
    let calls = startup_calls(&catalog)?;
    persist(&request.output.join("source-graph.json"), &calls)?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(actual), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    write(&request.output.join("reset.fujimod"), &checkpoint)?;
    let restored = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    if machine.instructions() != 0 { return Err("pump reset checkpoint is not preexecution".into()); }
    let mut runner = capture::Runner::new(&mut machine, &catalog, &maps, &request.output, &calls)?;
    let result = runner.observe();
    let finish = runner.finish();
    let failure = result.err().or_else(|| finish.err());
    let capture::Capture { loader, samples, epochs, inputs, visible, trace_scope } = runner.into_capture();
    persist(&request.output.join("trace.json"), &machine.traces.hits)?;
    let loader = LoaderEvidence {
        schema: 1,
        identity: LoaderIdentity { run: identity.run.sha256.clone(), update: identity.actual_update.sha256.clone(), rom: identity.boot_rom.sha256.clone(), executable: identity.observer.clone(), sources: catalog.0.iter().map(|image| image.binding.clone()).collect() },
        checkpoint: Digest::of(&checkpoint), instructions: Count(machine.instructions()),
        menu: machine.menu().map(|menu| menu.map(|menu| [menu.menu_id, menu.setting_id, menu.cursor])),
        error: failure.as_ref().map(|failure| failure.detail.clone()), machine_failure: machine.failure.clone(),
        events: loader.events, calls: loader.calls, snapshots: loader.snapshots, pulses: loader.pulses,
        epochs: Vec::new(), trace: binding(&request.output, "trace.json")?, static_calls: calls,
    };
    let report = PumpReport {
        schema: 2, mode: request.mode, identity, checkpoint: Digest::of(&checkpoint), instructions: Count(machine.instructions()),
        outcome: failure.map_or(Outcome::Captured, |failure| Outcome::Refused { failure }),
        loader, samples, epochs, inputs, visible, trace_scope,
        routes: [Route::RepeatedReceive, Route::PendingWork, Route::BackThenInput, Route::Unhandled, Route::ReceiveFailure, Route::ServiceFailure].into_iter().map(|route| RouteCoverage { route, reachability: Reachability::Unqualified }).collect(),
    };
    write_report(&request.output, &report)?;
    if let Outcome::Refused { failure } = report.outcome { return Err(format!("pump observation refused {:?}: {}", failure.reason, failure.detail)); }
    let verified = application_pump_check::check(&request.output, &Catalog::open(&request.analysis)?)?;
    persist(&request.output.join("application-pump-verification.json"), &verified)
}
fn write_report(output: &Path, report: &PumpReport) -> Result<(), String> {
    let file = std::fs::OpenOptions::new().create_new(true).write(true).open(output.join("application-pump.json")).map_err(|error| error.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, report).map_err(|error| error.to_string())?;
    std::io::Write::flush(&mut writer).map_err(|error| error.to_string())
}
pub(super) fn verify(output: &Path, analysis: &Path) -> Result<(), String> {
    let verification = application_pump_check::check(output, &Catalog::open(analysis)?)?;
    println!("{}", serde_json::to_string(&verification).map_err(|error| error.to_string())?);
    Ok(())
}

pub(super) fn compare(retained: &Path, owned: &Path, analysis: &Path, output: &Path) -> Result<(), String> {
    output_directory(output)?;
    application_pump_check::paired(retained, owned, &Catalog::open(analysis)?, output)
}

pub(super) fn controls(source: &Path, analysis: &Path, output: &Path) -> Result<(), String> {
    output_directory(output)?;
    application_pump_check::refusal_controls(source, &Catalog::open(analysis)?, output)
}
