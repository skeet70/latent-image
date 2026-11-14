use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::checkpoint::{BootIdentity, PostBootReceipt, SHARED_STORE, post_boot};
use fujimod_xm1::guest::{RouteKey, ShutterStage};
use fujimod_xm1::machine::{Machine, RunOutcome, SCANNED_HOLD_TICKS};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use fujimod_xm1::stimulus::{Deadline, Origin, StimulusClock, StimulusReceipt, Stimuli, TickSchedule};
use fujimod_xm1::trace::{Hit, TaskCapture};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs::File, io::{BufWriter, Write}, num::NonZeroUsize, path::{Path, PathBuf}, process::ExitCode};

mod time_interface;
mod time_failure;

#[derive(Serialize)]
struct FailureIdentity {
    case: time_failure::FailureCase, run_sha256: String, update_sha256: String,
    boot_rom_sha256: String, observer_sha256: String, interface_sha256: String,
    checkpoint: Option<PostBootReceipt>,
    preparation: PreparationReceipt,
}

const LIMIT: u64 = 1_500_000_000;
const EVENT_LIMIT: usize = 500_000;
const HOLD: u64 = 8_000_000;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Scenario { Boot, MenuRoutes, HalfPress, Rtc }

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ObservationStart { Reset, IndependentCheckpoint }

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RawPhase { Boot, Route }

#[derive(Serialize)]
#[serde(transparent)]
struct RawHitCount(usize);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Stimulus { Press { key: RouteKey }, Release { key: RouteKey }, HalfPress, HalfRelease, End }

#[derive(Serialize)]
#[serde(transparent)]
struct ScheduleDigest(String);

enum ClockPlan {
    Instructions,
    BoardTicks { schedule: TickSchedule<Stimulus>, digest: ScheduleDigest },
}

#[derive(Serialize)]
#[serde(tag = "clock", rename_all = "kebab-case")]
enum PreparationBinding {
    Instructions,
    BoardTicks { schedule_sha256: ScheduleDigest },
}

#[derive(Serialize)]
struct PreparationProcedure<'a> {
    schema: u32,
    timing: &'a PreparationBinding,
}

#[derive(Serialize)]
struct PreparationReceipt {
    timing: PreparationBinding,
    origin: Origin,
    boundary: Origin,
    history_boundary: Option<time_interface::Boundary>,
    raw_events: Option<RawHitCount>,
}

impl ClockPlan {
    fn scheduled(path: &Path) -> Result<Self, String> {
        let bytes = read(path)?;
        Ok(Self::BoardTicks { schedule: TickSchedule::parse(&bytes)?, digest: ScheduleDigest(sha256(&bytes)) })
    }

    fn into_preparation(self) -> (PreparationBinding, Option<TickSchedule<Stimulus>>) {
        match self {
            Self::Instructions => (PreparationBinding::Instructions, None),
            Self::BoardTicks { schedule, digest } => (PreparationBinding::BoardTicks { schedule_sha256: digest }, Some(schedule)),
        }
    }
}

fn preparation(args: &BTreeMap<String, PathBuf>, scenario: Scenario, clock: StimulusClock) -> Result<Option<ClockPlan>, String> {
    let ticks = args.get("--preparation-ticks");
    let explicit_clock = args.get("--preparation-clock");
    require(!args.contains_key("--failure") || !args.contains_key("--stimulus-ticks"), "failure probes do not use a scenario tick schedule")?;
    if matches!(scenario, Scenario::Boot) {
        require(ticks.is_none() && explicit_clock.is_none(), "reset boot does not use preparation")?;
        return Ok(None);
    }
    require(ticks.is_none() || explicit_clock.is_none(), "preparation clock and tick schedule are mutually exclusive")?;
    if let Some(path) = ticks {
        require(clock == StimulusClock::BoardTicks || args.contains_key("--failure"), "instruction-clock scenarios require instruction-clock preparation")?;
        return ClockPlan::scheduled(path).map(Some);
    }
    require(clock == StimulusClock::Instructions, "board-clock post-boot scenarios require --preparation-ticks")?;
    match explicit_clock.map(PathBuf::as_path) {
        Some(path) if path == Path::new("instructions") => Ok(Some(ClockPlan::Instructions)),
        Some(_) => Err("--preparation-clock must be instructions; board-clock preparation uses --preparation-ticks".into()),
        None => Err("post-boot scenarios require explicit --preparation-ticks or --preparation-clock instructions".into()),
    }
}

/// A stimulus, the instructions before it on the instruction clock, and the menu required when it is reached.
struct Point { budget: u64, stimulus: Stimulus, menu: Option<[u32; 3]> }
impl Point {
    const fn at(budget: u64, stimulus: Stimulus) -> Self { Self { budget, stimulus, menu: None } }
}

#[derive(Serialize)]
struct ClockedHit<'a> {
    #[serde(flatten)]
    hit: &'a Hit,
    board_tick: Option<u64>,
}
#[derive(Serialize)]
struct Scan {
    board_tick: u64,
    instructions: u64,
    released: Vec<(RouteKey, bool)>,
}

#[derive(Serialize)]
struct Summary<'a> { schema: u32, complete: bool, error: Option<String>, scenario: Scenario, run_sha256: String, update_sha256: String, boot_rom_sha256: String, observer_sha256: String, interface_sha256: String, schedule_sha256: Option<ScheduleDigest>, checkpoint: Option<PostBootReceipt>, preparation: Option<PreparationReceipt>, start: ObservationStart, observation_start_instructions: u64, stimuli: StimulusReceipt<'a, Stimulus>, scans: &'a [Scan], events: usize, raw_phase: RawPhase, raw_phase_events: RawHitCount, entries: BTreeMap<String, u64>, instructions: u64, menu: Option<[u32; 3]> }

fn address(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text.strip_prefix("0x").ok_or("address lacks 0x prefix")?, 16).map_err(|error| error.to_string())
}
fn require(condition: bool, error: &str) -> Result<(), String> { if condition { Ok(()) } else { Err(error.into()) } }

fn key(points: &mut Vec<Point>, budget: u64, key: RouteKey) {
    points.push(Point::at(budget, Stimulus::Press { key }));
    points.push(Point::at(HOLD, Stimulus::Release { key }));
}
/// Reset to shooting menu `(1,5,2)` through the initial setup menu.
fn boot_points() -> Vec<Point> {
    let mut points = Vec::new();
    key(&mut points, 80_000_000, RouteKey::Up);
    points[0].menu = Some([69, 52, 12]);
    for _ in 1..12 { key(&mut points, HOLD, RouteKey::Up); }
    key(&mut points, HOLD, RouteKey::Ok);
    key(&mut points, HOLD, RouteKey::Ok);
    key(&mut points, HOLD + 40_000_000, RouteKey::Ok);
    points.push(Point { budget: 98_000_000, stimulus: Stimulus::End, menu: Some([1, 5, 2]) });
    points
}
fn scenario_points(scenario: Scenario, keys: &[RouteKey]) -> Vec<Point> {
    let mut points = Vec::new();
    match scenario {
        Scenario::Boot => return boot_points(),
        Scenario::MenuRoutes => {
            for (index, route_key) in keys.iter().enumerate() { key(&mut points, if index == 0 { 0 } else { HOLD }, *route_key); }
            points.push(Point::at(HOLD + 40_000_000, Stimulus::End));
        }
        Scenario::HalfPress => {
            key(&mut points, 0, RouteKey::Back);
            points.push(Point::at(HOLD + 40_000_000, Stimulus::HalfPress));
            points.push(Point::at(HOLD, Stimulus::HalfRelease));
            points.push(Point::at(24_000_000, Stimulus::End));
        }
        Scenario::Rtc => points.push(Point::at(100_000_000, Stimulus::End)),
    }
    points
}
fn apply(machine: &mut Machine, stimulus: Stimulus) -> Result<(), String> {
    let half = |machine: &mut Machine, pressed: bool| -> Result<(), String> {
        let gpio = machine.document.firmware.shutter(&machine.session, ShutterStage::Half)?;
        let node = machine.document.gpio.clone();
        machine.session.gpio(&node.0, gpio.port, gpio.pin, if pressed { gpio.pressed_high } else { !gpio.pressed_high })?;
        Ok(())
    };
    match stimulus {
        Stimulus::Press { key } => machine.press(key).map(|_| ()),
        Stimulus::Release { key } => machine.release_key(key),
        Stimulus::HalfPress => half(machine, true),
        Stimulus::HalfRelease => half(machine, false),
        Stimulus::End => Ok(()),
    }
}
fn play(machine: &mut Machine, points: &[Point], mut wait: impl FnMut(&mut Machine, u64, Stimulus) -> Result<(), String>) -> Result<(), String> {
    for point in points {
        wait(machine, point.budget, point.stimulus)?;
        if let Some(expected) = point.menu {
            let menu = machine.menu()?.ok_or_else(|| format!("no menu at {:?}", point.stimulus))?;
            let found = [menu.menu_id, menu.setting_id, menu.cursor];
            require(found == expected, &format!("menu {found:?} at {:?}, expected {expected:?}", point.stimulus))?;
        }
        apply(machine, point.stimulus)?;
    }
    Ok(())
}

fn prepare(machine: &mut Machine, schedule: Option<TickSchedule<Stimulus>>) -> Result<(), String> {
    let mut stimuli = match schedule {
        Some(schedule) => Stimuli::board_ticks(machine, schedule),
        None => Stimuli::instructions(machine),
    };
    play(machine, &boot_points(), |machine, budget, stimulus| {
        let deadline = stimuli.deadline(machine, budget)?;
        while !deadline.reached(machine) {
            let outcome = deadline.run(machine, 1_000_000);
            machine.require_running()?;
            require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected checkpoint boot outcome")?;
            require(machine.instructions() <= LIMIT, "instruction bound")?;
        }
        stimuli.reach(machine, stimulus)
    })?;
    stimuli.require_complete()
}

struct Observer { sites: BTreeMap<u32, (String, Vec<u8>)>, counts: BTreeMap<String, u64>, events: usize, phase_start: RawHitCount, writer: BufWriter<File>, interface: time_interface::Recorder, stimuli: Stimuli<Stimulus>, scans: Vec<Scan> }
impl Observer {
    fn new(contract: &[u8], output: &Path, stimuli: Stimuli<Stimulus>) -> Result<Self, String> {
        let interface = time_interface::Recorder::new(contract, &output.join("interface.jsonl"))?;
        let sites = interface.raw_sites();
        let writer = BufWriter::new(File::create(output.join("traffic.jsonl")).map_err(|error| error.to_string())?);
        Ok(Self { sites, counts: BTreeMap::new(), events: 0, phase_start: RawHitCount(0), writer, interface, stimuli, scans: Vec::new() })
    }
    fn arm(&mut self, machine: &mut Machine) -> Result<(), String> {
        machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
        machine.traces.stop_labels = ["time-interface".into(), "time-return".into(), "key-scan".into()].into_iter().collect();
        let mut sites: BTreeMap<_, _> = self.sites.keys().map(|pc| (*pc, "time-interface".into())).collect();
        // X-M1 1.50 scanner entry and the instruction after its 36-row debounce loop.
        sites.extend([(0x6a33f0, "key-scan".into()), (0x6a34b8, "key-scan".into())]);
        machine.traces.arm_with_task_capture(&machine.session, sites, TaskCapture::Logical)?;
        self.interface.begin_watch(machine)
    }
    fn run_to(&mut self, machine: &mut Machine, deadline: Deadline) -> Result<(), String> {
        while !deadline.reached(machine) {
            machine.traces.hits.clear();
            let (before, before_tick) = (machine.instructions(), machine.session.board.now());
            let outcome = deadline.run(machine, 1_000_000);
            machine.require_running()?;
            require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected timer observation outcome")?;
            require(machine.instructions() <= LIMIT, "instruction bound")?;
            self.interface.end_watch(machine)?;
            let mut added = BTreeMap::new();
            for hit in &machine.traces.hits {
                self.interface.before_hit(hit.instructions)?;
                require(self.events - self.phase_start.0 < EVENT_LIMIT, "timer interface event bound")?;
                let board_tick = (hit.instructions == machine.instructions()).then(|| machine.session.board.now());
                serde_json::to_writer(&mut self.writer, &ClockedHit { hit, board_tick }).map_err(|error| error.to_string())?;
                if hit.address == "0x6a34b8" {
                    let released = [RouteKey::Up, RouteKey::Down, RouteKey::Left, RouteKey::Right, RouteKey::Ok, RouteKey::Back, RouteKey::Q]
                        .into_iter().map(|key| machine.document.firmware.released(&machine.session, key).map(|released| (key, released)))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.scans.push(Scan { board_tick: board_tick.ok_or("scanner hit is not at the stopped board")?, instructions: hit.instructions, released });
                }
                self.writer.write_all(b"\n").map_err(|error| error.to_string())?;
                let label = self.sites.get(&address(&hit.address)?).map(|(label, _)| label.as_str()).unwrap_or("kernel-or-return");
                *self.counts.entry(label.into()).or_default() += 1;
                self.events += 1;
                added.extend(self.interface.observe(machine, hit)?);
            }
            self.interface.after_hits()?;
            self.interface.require_owned()?;
            if !added.is_empty() { machine.traces.add_sites(&machine.session, added)?; }
            self.interface.begin_watch(machine)?;
            let progressed = machine.instructions() > before || machine.session.board.now() > before_tick || !machine.traces.hits.is_empty();
            require(progressed, "timer observer made no progress")?;
        }
        Ok(())
    }
    fn reach(&mut self, machine: &mut Machine, budget: u64, stimulus: Stimulus) -> Result<(), String> {
        let deadline = self.stimuli.deadline(machine, budget)?;
        self.run_to(machine, deadline)?;
        self.stimuli.reach(machine, stimulus)
    }
    fn scenario(&mut self, machine: &mut Machine, points: &[Point]) -> Result<(), String> {
        play(machine, points, |machine, budget, stimulus| self.reach(machine, budget, stimulus))?;
        self.stimuli.require_complete()
    }
}

fn args() -> Result<BTreeMap<String, PathBuf>, String> {
    let mut values = BTreeMap::new();
    let mut args = std::env::args().skip(1);
    while let Some(key) = args.next() {
        require(["--run", "--update", "--boot-rom", "--interface", "--scenario", "--checkpoints", "--keys", "--stimulus-ticks", "--preparation-ticks", "--preparation-clock", "--failure", "--output"].contains(&key.as_str()), "unknown time observer argument")?;
        let value = args.next().ok_or("time observer argument has no value")?;
        require(values.insert(key, PathBuf::from(value)).is_none(), "duplicate time observer argument")?;
    }
    for key in ["--run", "--update", "--boot-rom", "--interface", "--scenario", "--output"] { require(values.contains_key(key), "missing time observer argument")?; }
    Ok(values)
}
fn drive() -> Result<(), String> {
    let args = args()?;
    let scenario: Scenario = serde_json::from_str(&format!("\"{}\"", args["--scenario"].display())).map_err(|error| error.to_string())?;
    let boot = matches!(scenario, Scenario::Boot);
    let run_bytes = read(&args["--run"])?;
    let dat = read(&args["--update"])?;
    let rom = read(&args["--boot-rom"])?;
    let interface = read(&args["--interface"])?;
    let keys = match args.get("--keys") {
        Some(path) => serde_json::from_slice::<Vec<RouteKey>>(&read(path)?).map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    require(!matches!(scenario, Scenario::MenuRoutes) || !keys.is_empty(), "menu-routes needs nonempty --keys")?;
    let schedule = match args.get("--stimulus-ticks") {
        Some(path) => { let bytes = read(path)?; Some((TickSchedule::<Stimulus>::parse(&bytes)?, ScheduleDigest(sha256(&bytes)))) }
        None => None,
    };
    let preparation_plan = preparation(&args, scenario, if schedule.is_some() { StimulusClock::BoardTicks } else { StimulusClock::Instructions })?;
    let output = &args["--output"];
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let mut session = run::open(&document, &ValidatedUpdate(dat.clone()), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    let executable = read(&std::env::current_exe().map_err(|error| error.to_string())?)?;
    let start = if boot || (schedule.is_some() && !args.contains_key("--failure")) {
        ObservationStart::Reset
    } else {
        ObservationStart::IndependentCheckpoint
    };
    let (checkpoint, mut preparation, observed_preparation) = match (start, preparation_plan) {
        (ObservationStart::Reset, plan) => {
            require(machine.instructions() == 0 && machine.session.board.now() == 0, "observation must start from reset")?;
            (None, None, plan.map(ClockPlan::into_preparation))
        }
        (ObservationStart::IndependentCheckpoint, plan) => {
            let (timing, schedule) = plan.ok_or("missing preparation plan")?.into_preparation();
            let procedure = serde_json::to_vec(&PreparationProcedure { schema: 2, timing: &timing }).map_err(|error| error.to_string())?;
            let identity = BootIdentity::new(&run_bytes, &dat, &rom, &[], None, &procedure).with_emulator_binary(&executable);
            let root = args.get("--checkpoints").map(PathBuf::as_path).unwrap_or_else(|| Path::new(SHARED_STORE));
            let origin = Origin { board_tick: machine.session.board.now(), instructions: machine.instructions() };
            require(origin.board_tick == 0 && origin.instructions == 0, "preparation must start from reset")?;
            let checkpoint = post_boot(&mut machine, &identity, root, |machine| prepare(machine, schedule))?;
            let receipt = PreparationReceipt { timing, origin, history_boundary: None, raw_events: None, boundary: Origin { board_tick: machine.session.board.now(), instructions: machine.instructions() } };
            (Some(checkpoint), Some(receipt), None)
        }
    };
    if let Some(case) = args.get("--failure") {
        require(!boot, "failure probes require a post-boot task context")?;
        let preparation = preparation.ok_or("failure probes require preparation")?;
        let case = serde_json::from_str::<time_failure::FailureCase>(&format!("\"{}\"", case.display())).map_err(|error| error.to_string())?;
        let result = time_failure::run(&mut machine, &interface, case, output);
        let identity = FailureIdentity { case, run_sha256: sha256(&run_bytes), update_sha256: sha256(&dat),
            boot_rom_sha256: sha256(&rom), observer_sha256: sha256(&executable), interface_sha256: sha256(&interface), checkpoint, preparation };
        write(&output.join("failure-identity.json"), &serde_json::to_vec_pretty(&identity).map_err(|error| error.to_string())?)?;
        return result;
    }
    let observation_start_instructions = machine.instructions();
    let (stimuli, schedule_sha256, preparation_timing) = match (schedule, observed_preparation) {
        (Some((schedule, digest)), Some((timing, Some(boot_schedule)))) =>
            (Stimuli::board_ticks(&machine, boot_schedule.followed_by(schedule)?), Some(digest), Some(timing)),
        (Some((schedule, digest)), None) => (Stimuli::board_ticks(&machine, schedule), Some(digest), None),
        (None, None) => (Stimuli::instructions(&machine), None, None),
        _ => return Err("observed preparation requires both board-clock schedules".into()),
    };
    let mut observer = Observer::new(&interface, output, stimuli)?;
    observer.arm(&mut machine)?;
    let mut raw_phase = if boot { RawPhase::Boot } else { RawPhase::Route };
    let observed = if let Some(timing) = preparation_timing {
        raw_phase = RawPhase::Boot;
        let origin = Origin { board_tick: machine.session.board.now(), instructions: machine.instructions() };
        let prepared = play(&mut machine, &boot_points(), |machine, budget, stimulus| observer.reach(machine, budget, stimulus));
        let history_boundary = if prepared.is_ok() {
            observer.interface.end_watch(&machine)?;
            observer.interface.after_hits()?;
            let boundary = observer.interface.boundary();
            observer.interface.begin_watch(&machine)?;
            Some(boundary)
        } else { None };
        preparation = Some(PreparationReceipt { timing, origin, history_boundary, raw_events: Some(RawHitCount(observer.events)), boundary: Origin {
            board_tick: machine.session.board.now(), instructions: machine.instructions(),
        } });
        prepared.and_then(|()| {
            observer.phase_start = RawHitCount(observer.events);
            raw_phase = RawPhase::Route;
            observer.scenario(&mut machine, &scenario_points(scenario, &keys))
        })
    } else {
        observer.scenario(&mut machine, &scenario_points(scenario, &keys))
    };
    let ended = observer.interface.end_watch(&machine);
    let coverage = observer.interface.finish()?;
    write(&output.join("interface-summary.json"), &serde_json::to_vec_pretty(&coverage).map_err(|error| error.to_string())?)?;
    observer.writer.flush().map_err(|error| error.to_string())?;
    let observed = observed.and(ended);
    let menu = machine.menu()?.map(|menu| [menu.menu_id, menu.setting_id, menu.cursor]);
    let summary = Summary {
        schema: 4, complete: observed.is_ok(), error: observed.as_ref().err().cloned(), scenario,
        run_sha256: sha256(&run_bytes), update_sha256: sha256(&dat), boot_rom_sha256: sha256(&rom),
        observer_sha256: sha256(&executable), interface_sha256: sha256(&interface), schedule_sha256,
        checkpoint, preparation, start, observation_start_instructions, stimuli: observer.stimuli.receipt(), scans: &observer.scans,
        events: observer.events, entries: observer.counts, instructions: machine.instructions(), menu,
        raw_phase, raw_phase_events: RawHitCount(observer.events - observer.phase_start.0),
    };
    write(&output.join("summary.json"), &serde_json::to_vec_pretty(&summary).map_err(|error| error.to_string())?)?;
    observed
}
fn schedule_from_summary(bytes: &[u8]) -> Result<TickSchedule<Stimulus>, String> {
    #[derive(Deserialize)]
    struct Receipt<'a> { #[serde(borrow)] stimuli: &'a serde_json::value::RawValue }
    let receipt: Receipt<'_> = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    Ok(TickSchedule::parse(receipt.stimuli.get().as_bytes())?
        .widened(SCANNED_HOLD_TICKS, |stimulus| matches!(stimulus, Stimulus::Press { .. } | Stimulus::Release { .. })))
}
fn schedule(input: &Path, output: &Path) -> Result<(), String> {
    let schedule = schedule_from_summary(&read(input)?)?;
    write(output, &serde_json::to_vec_pretty(&schedule).map_err(|error| error.to_string())?)
}
fn main() -> ExitCode {
    let result = if std::env::args().nth(1).as_deref() == Some("owned-interface") {
        let arguments = std::env::args().skip(2).map(PathBuf::from).collect::<Vec<_>>();
        if arguments.len() == 3 { time_interface::owned_plan(&arguments[0], &arguments[1], &arguments[2]) }
        else { Err("usage: time-observer owned-interface CONTRACT SYMBOLS OUTPUT".into()) }
    } else if std::env::args().nth(1).as_deref() == Some("schedule") {
        let arguments = std::env::args().skip(2).map(PathBuf::from).collect::<Vec<_>>();
        if arguments.len() == 2 { schedule(&arguments[0], &arguments[1]) }
        else { Err("usage: time-observer schedule SUMMARY OUTPUT".into()) }
    } else { drive() };
    match result { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("time-observer: {error}"); ExitCode::FAILURE } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_schedule_guarantees_holds_and_release_gaps_without_shortening_long_gaps() {
        let receipt = serde_json::json!({"stimuli":{"points":[
            {"stimulus":{"kind":"press","key":"down"},"tick":0},
            {"stimulus":{"kind":"release","key":"down"},"tick":100},
            {"stimulus":{"kind":"press","key":"up"},"tick":300},
            {"stimulus":{"kind":"release","key":"up"},"tick":1500000},
            {"stimulus":{"kind":"end"},"tick":2500000}
        ]}});
        let schedule = schedule_from_summary(&serde_json::to_vec(&receipt).unwrap()).unwrap();
        let serialized = serde_json::to_value(schedule).unwrap();
        let ticks: Vec<_> = serialized["points"].as_array().unwrap().iter().map(|point| point["tick"].as_u64().unwrap()).collect();
        assert_eq!(ticks, [0, 720000, 1440000, 2939700, 3939700]);
    }
}
