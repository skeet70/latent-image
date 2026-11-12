//! Stock-only census of task 43 (saclac): instruction points at the task entry,
//! the queue-43 producer, the dispatcher and the 37 row functions, and a
//! classified fetch watch over each machine's action range. One scenario per run.
//!
//! The site file declares every address and range; no stock address appears here.

use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::guest::{GpioKey, RouteKey, ShutterStage};
use fujimod_xm1::machine::{Hold, KeyPulse, MENU_PULSE, Machine, RunOutcome, key_pulse};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "saclac-census --run RUN --update STOCK_DAT --boot-rom ROM --sites SITES --scenario cold-boot|menu-route|half-press [--steps STEPS] --output NEW_DIR";
const SITE_LABEL: &str = "saclac-site";
const SITE_LIMIT: usize = 4096;
const CONTEXT_LIMIT: usize = 1024;
const CALLER_LIMIT: usize = 512;
const ADDRESS_LIMIT: usize = 32_768;
const INSTRUCTION_LIMIT: u64 = 1_500_000_000;
const WATCH_LIMIT: usize = 1 << 20;
const SEGMENT_INSTRUCTIONS: u64 = 50_000_000;

const INITIAL_MENU: u64 = 80_000_000;
const SETUP_SETTLE: u64 = 40_000_000;
const SHOOTING_SETTLE: u64 = 90_000_000;
const SETUP_HOLD: u64 = 8_000_000;
const SETUP_RELEASE: u64 = 8_000_000;
const RELEASE_POLL: u64 = 250_000;
const MENU_POLL: u64 = 250_000;
const STEP_STATE_CAP: u64 = 90_000_000;
const STEP_SETTLE: u64 = 20_000_000;
const HALF_PRESS_HOLD: u64 = 8_000_000;
const HALF_PRESS_RELEASE: u64 = 8_000_000;
const HALF_PRESS_SETTLE: u64 = 8_000_000;

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}

fn hex<S: Serializer>(value: &u32, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{:#x}", *value))
}

fn parse_hex(text: &str) -> Result<u32, String> {
    let digits = text.strip_prefix("0x").ok_or_else(|| format!("{text}: hexadecimal prefix missing"))?;
    u32::from_str_radix(digits, 16).map_err(|_| format!("{text}: invalid hexadecimal word"))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Address(#[serde(serialize_with = "hex")] u32);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Actor, Producer, Dispatcher, Row }

#[derive(Clone, Debug, Deserialize)]
struct SitePoint {
    address: String,
    label: String,
    role: Role,
    machine: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ActionRange {
    machine: u8,
    start: String,
    end: String,
}

#[derive(Debug, Deserialize)]
struct Sites {
    schema: u32,
    task_id: u8,
    points: Vec<SitePoint>,
    action_ranges: Vec<ActionRange>,
}

struct Site {
    address: u32,
    label: String,
    role: Role,
    machine: Option<u8>,
}

struct Range {
    machine: u8,
    start: u32,
    end: u32,
}

struct Plan {
    task_id: u8,
    points: Vec<Site>,
    ranges: Vec<Range>,
}

fn load_sites(bytes: &[u8]) -> Result<Plan, String> {
    let sites: Sites = serde_json::from_slice(bytes).map_err(|error| format!("sites: {error}"))?;
    require(sites.schema == 1, "sites: unsupported schema")?;
    require(sites.task_id > 0, "sites: task id is zero")?;
    require(sites.points.len() <= SITE_LIMIT, "sites: more points than the engine admits")?;
    let mut points = Vec::with_capacity(sites.points.len());
    let mut addresses = std::collections::BTreeSet::new();
    let mut labels = std::collections::BTreeSet::new();
    let mut rows = std::collections::BTreeSet::new();
    for point in &sites.points {
        let address = parse_hex(&point.address)?;
        require(address % 4 == 0, "sites: a point address is not word aligned")?;
        require(addresses.insert(address), "sites: two points share an address")?;
        require(!point.label.is_empty() && labels.insert(point.label.clone()), "sites: two points share a label")?;
        match point.role {
            Role::Row => {
                let machine = point.machine.ok_or("sites: a row point names no machine")?;
                require(machine < 37, "sites: a row machine is outside 0..36")?;
                require(rows.insert(machine), "sites: two row points share a machine")?;
            }
            _ => require(point.machine.is_none(), "sites: only a row point names a machine")?,
        }
        points.push(Site { address, label: point.label.clone(), role: point.role, machine: point.machine });
    }
    for role in [Role::Actor, Role::Producer, Role::Dispatcher] {
        require(points.iter().filter(|point| point.role == role).count() == 1, "sites: a role point is missing or repeated")?;
    }
    require(rows.len() == 37, "sites: the 37 row points are not all declared")?;
    let mut ranges = Vec::with_capacity(sites.action_ranges.len());
    let mut machines = std::collections::BTreeSet::new();
    for range in &sites.action_ranges {
        let (start, end) = (parse_hex(&range.start)?, parse_hex(&range.end)?);
        require(start % 4 == 0 && end % 4 == 0 && start < end, "sites: an action range is not a nonempty word range")?;
        require(range.machine < 37 && machines.insert(range.machine), "sites: action ranges repeat or exceed a machine")?;
        ranges.push(Range { machine: range.machine, start, end });
    }
    require(ranges.len() == 37, "sites: the 37 action ranges are not all declared")?;
    ranges.sort_by_key(|range| range.start);
    for pair in ranges.windows(2) {
        require(pair[0].end <= pair[1].start, "sites: two action ranges overlap")?;
    }
    Ok(Plan { task_id: sites.task_id, points, ranges })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Key { Up, Down, Left, Right, Ok, Menu, Back, Disp, Q, Fn }
impl Key {
    fn route(self) -> Option<RouteKey> {
        Some(match self {
            Self::Up => RouteKey::Up,
            Self::Down => RouteKey::Down,
            Self::Left => RouteKey::Left,
            Self::Right => RouteKey::Right,
            Self::Ok | Self::Menu => RouteKey::Ok,
            Self::Back | Self::Disp => RouteKey::Back,
            Self::Q => RouteKey::Q,
            Self::Fn => return None,
        })
    }

    fn pulse(self) -> KeyPulse {
        match self.route() {
            Some(route) => key_pulse(route),
            None => MENU_PULSE,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Scenario { ColdBoot, MenuRoute, HalfPress }

#[derive(Debug, Deserialize)]
struct Trajectory { before_unload: Vec<TrajectoryPhase> }
#[derive(Debug, Deserialize)]
struct TrajectoryPhase { phase: String, steps: Vec<StepShape> }

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StepShape {
    Four((Key, u32, u32, u32)),
    Five((Key, u32, u32, u32, StepCompletion)),
}

#[derive(Debug, Deserialize)]
struct StepCompletion {
    #[serde(rename = "completion")]
    _completion: String,
}

#[derive(Clone, Copy, Debug)]
struct Step { key: Key, expected: MenuTriple }
impl From<StepShape> for Step {
    fn from(shape: StepShape) -> Self {
        let (key, menu, setting, cursor) = match shape {
            StepShape::Four((key, menu, setting, cursor)) => (key, menu, setting, cursor),
            StepShape::Five((key, menu, setting, cursor, _)) => (key, menu, setting, cursor),
        };
        Step { key, expected: MenuTriple { menu, setting, cursor } }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
struct MenuTriple { menu: u32, setting: u32, cursor: u32 }

#[derive(Clone, Copy, Debug, Serialize)]
struct Pulse { key: Key, pressed: u64, released: u64, debounced: bool }

#[derive(Serialize)]
struct Milestone {
    phase: String,
    pulse: Option<Pulse>,
    expected: MenuTriple,
    reached: Option<u64>,
    observed: Option<MenuTriple>,
    finished: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct EntryContext {
    core: usize,
    mode: u32,
    lr: u32,
    sp: u32,
    r0: u32,
    r1: u32,
    r2: u32,
    r3: u32,
}

#[derive(Serialize)]
struct Counted<K> { key: K, count: u64 }

fn counted<K: Clone>(map: &BTreeMap<K, u64>) -> Vec<Counted<K>> {
    let mut entries: Vec<_> = map.iter().map(|(key, &count)| Counted { key: key.clone(), count }).collect();
    entries.sort_by(|left, right| right.count.cmp(&left.count));
    entries
}

#[derive(Default)]
struct PointStats {
    entries: u64,
    first: Option<u64>,
    last: Option<u64>,
    contexts: BTreeMap<EntryContext, u64>,
    context_overflow: u64,
    callers: BTreeMap<u32, u64>,
    caller_overflow: u64,
}
impl PointStats {
    fn enter(&mut self, context: EntryContext, instructions: u64) {
        self.entries += 1;
        self.first.get_or_insert(instructions);
        self.last = Some(instructions);
        if let Some(count) = self.contexts.get_mut(&context) {
            *count += 1;
        } else if self.contexts.len() < CONTEXT_LIMIT {
            self.contexts.insert(context, 1);
        } else {
            self.context_overflow += 1;
        }
        if let Some(count) = self.callers.get_mut(&context.lr) {
            *count += 1;
        } else if self.callers.len() < CALLER_LIMIT {
            self.callers.insert(context.lr, 1);
        } else {
            self.caller_overflow += 1;
        }
    }
}

#[derive(Default)]
struct RangeStats {
    fetches: u64,
    addresses: BTreeMap<u32, u64>,
    address_overflow: u64,
}
impl RangeStats {
    fn fetch(&mut self, address: u32) {
        self.fetches += 1;
        if let Some(count) = self.addresses.get_mut(&address) {
            *count += 1;
        } else if self.addresses.len() < ADDRESS_LIMIT {
            self.addresses.insert(address, 1);
        } else {
            self.address_overflow += 1;
        }
    }
}

struct Stop {
    core: usize,
    address: u32,
    instructions: u64,
    registers: BTreeMap<&'static str, String>,
    sp: String,
    lr: String,
    cpsr: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct FetchRow { source: String, address: u64 }

struct FetchWatch { watch: Box<MemoryWatch>, started: u64 }
impl FetchWatch {
    fn begin(machine: &Machine, maps: &[MemoryMap], classes: Vec<WatchRange>) -> Result<Self, String> {
        let watch = machine.session.board.memory_watch_begin_classified(classes, WATCH_LIMIT, maps.to_vec(), ViewPolicy::Observe)?;
        Ok(Self { watch, started: machine.instructions() })
    }
    fn end(mut self, machine: &Machine, plan: &[Range], stats: &mut [RangeStats]) -> Result<(), String> {
        let state = self.watch.records.borrow().state();
        let ended = machine.session.board.memory_watch_end(&mut self.watch);
        if let Some(violation) = state.get("violation").and_then(serde_json::Value::as_str) {
            return Err(format!("action fetch watch: {violation}"));
        }
        let rows = state.get("records").and_then(serde_json::Value::as_array).ok_or("action fetch watch has no records")?;
        for row in rows {
            let row = FetchRow::deserialize(row).map_err(|error| format!("action fetch row: {error}"))?;
            if row.source != "cpu-fetch" {
                continue;
            }
            let address = u32::try_from(row.address).map_err(|_| "fetch row exceeds 32 bits".to_owned())?;
            let index = plan.partition_point(|range| range.start <= address);
            if let Some(index) = index.checked_sub(1) {
                if address < plan[index].end {
                    stats[index].fetch(address);
                }
            }
        }
        ended
    }
}

#[derive(Serialize)]
struct Binding { path: PathBuf, sha256: String }

#[derive(Serialize)]
struct Limits {
    sites: usize, contexts: usize, callers: usize, addresses: usize, instructions: u64, watch_rows: usize, segment_instructions: u64,
}

#[derive(Serialize)]
struct PointRow<'a> {
    label: &'a str,
    role: Role,
    machine: Option<u8>,
    address: Address,
    entries: u64,
    first: Option<u64>,
    last: Option<u64>,
    contexts: Vec<Counted<EntryContext>>,
    context_overflow: u64,
    callers: Vec<Counted<u32>>,
    caller_overflow: u64,
}

#[derive(Serialize)]
struct RangeRow {
    machine: u8,
    start: Address,
    end: Address,
    fetches: u64,
    addresses: Vec<Counted<u32>>,
    address_overflow: u64,
}

#[derive(Serialize)]
struct Dispatcher {
    entries: u64,
    machines: Vec<Counted<u8>>,
    rows: Vec<Counted<(u8, u32)>>,
}

#[derive(Serialize)]
struct Queue43 {
    task_id: u8,
    posts: u64,
    ops: Vec<Counted<u32>>,
    callers: Vec<Counted<u32>>,
}

#[derive(Serialize)]
struct Report<'a> {
    schema: u32,
    scenario: Scenario,
    inputs: Vec<Binding>,
    limits: Limits,
    instructions: u64,
    counts: Option<[u64; 2]>,
    milestones: &'a [Milestone],
    points: Vec<PointRow<'a>>,
    dispatcher: Dispatcher,
    queue43: Queue43,
    action_ranges: Vec<RangeRow>,
    error: Option<String>,
    machine_failure: Option<String>,
}

struct Census<'a> {
    plan: &'a Plan,
    maps: Vec<MemoryMap>,
    points: Vec<PointStats>,
    ranges: Vec<RangeStats>,
    watch: Option<FetchWatch>,
    phase: String,
    milestones: Vec<Milestone>,
}

impl<'a> Census<'a> {
    fn point_index(&self, address: u32) -> Option<usize> {
        self.plan.points.iter().position(|point| point.address == address)
    }

    fn classes(&self) -> Vec<WatchRange> {
        self.plan.ranges.iter().map(|range| WatchRange {
            start: u64::from(range.start),
            end: u64::from(range.end),
            kinds: WatchKinds::CPU_FETCH,
        }).collect()
    }

    fn restart_watch(&mut self, machine: &Machine) -> Result<(), String> {
        let watch = self.watch.take().ok_or("action fetch watch is not active")?;
        watch.end(machine, &self.plan.ranges, &mut self.ranges)?;
        self.watch = Some(FetchWatch::begin(machine, &self.maps, self.classes())?);
        Ok(())
    }

    fn stop(&mut self, stops: &[Stop]) {
        for stop in stops {
            let Some(index) = self.point_index(stop.address) else { continue };
            let register = |name: &str| stop.registers.get(name).and_then(|text| parse_hex(text).ok()).unwrap_or(0);
            let context = EntryContext {
                core: stop.core,
                mode: stop.cpsr & 0x1f,
                lr: parse_hex(&stop.lr).unwrap_or(0),
                sp: parse_hex(&stop.sp).unwrap_or(0),
                r0: register("r0"),
                r1: register("r1"),
                r2: register("r2"),
                r3: register("r3"),
            };
            self.points[index].enter(context, stop.instructions);
        }
    }

    fn advance(&mut self, machine: &mut Machine, budget: u64) -> Result<(), String> {
        let end = machine.instructions().checked_add(budget).filter(|end| *end <= INSTRUCTION_LIMIT)
            .ok_or("census instruction bound exceeded")?;
        while machine.instructions() < end {
            machine.traces.hits.clear();
            let before = machine.instructions();
            let outcome = machine.run_until(end - before, None);
            machine.require_running()?;
            require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected census run outcome")?;
            let mut stops = Vec::new();
            for hit in &machine.traces.hits {
                require(hit.instructions == machine.instructions(), "census stop is no longer current")?;
                stops.push(Stop {
                    core: hit.core,
                    address: parse_hex(&hit.address)?,
                    instructions: hit.instructions,
                    registers: hit.registers.clone(),
                    sp: hit.sp.clone(),
                    lr: hit.lr.clone(),
                    cpsr: hit.cpsr,
                });
            }
            if stops.is_empty() {
                require(machine.instructions() >= end, "run ended early without a census stop")?;
            } else {
                self.stop(&stops);
            }
            require(machine.instructions() > before || !stops.is_empty(), "census made no progress")?;
            if self.watch.as_ref().is_some_and(|watch| machine.instructions() - watch.started >= SEGMENT_INSTRUCTIONS) {
                self.restart_watch(machine)?;
            }
        }
        Ok(())
    }

    fn hold(&mut self, machine: &mut Machine, hold: Hold) -> Result<(), String> {
        let count = match hold {
            Hold::Instructions(count) => count,
            Hold::BoardTicks(ticks) => machine.hold_bound(ticks),
        };
        self.advance(machine, count)
    }

    fn pulse(&mut self, machine: &mut Machine, key: Key, pulse: KeyPulse) -> Result<Pulse, String> {
        let Some(route) = key.route() else {
            let gpio = machine.document.firmware.fn_key(&machine.session)?;
            let (pressed, released, debounced) = self.gpio_row(machine, gpio, pulse)?;
            return Ok(Pulse { key, pressed, released, debounced });
        };
        let pressed = machine.instructions();
        machine.press(route)?;
        self.hold(machine, pulse.hold)?;
        machine.release_key(route)?;
        let released = machine.instructions();
        let debounced = loop {
            if machine.document.firmware.released(&machine.session, route)? { break true; }
            let spent = machine.instructions() - released;
            if spent >= pulse.release { break false; }
            self.advance(machine, (pulse.release - spent).min(RELEASE_POLL))?;
        };
        Ok(Pulse { key, pressed, released, debounced })
    }

    /// Holds and releases a GPIO key, advancing through the census so a stop inside the hold is recorded.
    fn gpio_row(&mut self, machine: &mut Machine, gpio: GpioKey, pulse: KeyPulse) -> Result<(u64, u64, bool), String> {
        let node = machine.document.gpio.clone();
        let pressed = machine.instructions();
        machine.session.gpio(&node.0, gpio.port, gpio.pin, gpio.pressed_high)?;
        self.hold(machine, pulse.hold)?;
        machine.session.gpio(&node.0, gpio.port, gpio.pin, !gpio.pressed_high)?;
        let released = machine.instructions();
        let debounced = loop {
            if machine.document.firmware.gpio_released(&machine.session, gpio)? { break true; }
            let spent = machine.instructions() - released;
            if spent >= pulse.release { break false; }
            self.advance(machine, (pulse.release - spent).min(RELEASE_POLL))?;
        };
        Ok((pressed, released, debounced))
    }

    fn menu(machine: &Machine) -> Result<Option<MenuTriple>, String> {
        Ok(machine.menu()?.map(|menu| MenuTriple { menu: menu.menu_id, setting: menu.setting_id, cursor: menu.cursor }))
    }

    fn endpoint(&mut self, machine: &Machine, pulse: Option<Pulse>, expected: MenuTriple) -> Result<(), String> {
        let observed = Self::menu(machine)?;
        let reached = (observed == Some(expected)).then(|| machine.instructions());
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse, expected, reached, observed, finished: machine.instructions() });
        require(reached.is_some(), "menu endpoint differs")
    }

    fn state(&mut self, machine: &mut Machine, pulse: Pulse, expected: MenuTriple) -> Result<(), String> {
        let start = machine.instructions();
        let reached = loop {
            if Self::menu(machine)? == Some(expected) { break Some(machine.instructions()); }
            let spent = machine.instructions() - start;
            if spent >= STEP_STATE_CAP { break None; }
            self.advance(machine, (STEP_STATE_CAP - spent).min(MENU_POLL))?;
        };
        if reached.is_some() { self.advance(machine, STEP_SETTLE)?; }
        let observed = Self::menu(machine)?;
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse: Some(pulse), expected, reached, observed, finished: machine.instructions() });
        require(reached.is_some() && observed == Some(expected), "menu trajectory state differs")
    }

    /// Runs the cold start and the initial setup. With `open_menu`, the MENU press that opens
    /// the shooting menu follows; the key routes start from the live view before it.
    fn boot(&mut self, machine: &mut Machine, open_menu: bool) -> Result<(), String> {
        self.phase = "cold-start".into();
        self.advance(machine, INITIAL_MENU)?;
        self.endpoint(machine, None, MenuTriple { menu: 69, setting: 52, cursor: 12 })?;
        self.phase = "initial-setup".into();
        for key in std::iter::repeat_n(Key::Up, 12).chain([Key::Ok, Key::Ok]) {
            self.pulse(machine, key, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        }
        self.advance(machine, SETUP_SETTLE)?;
        if !open_menu {
            return Ok(());
        }
        self.phase = "shooting-menu".into();
        let pulse = self.pulse(machine, Key::Menu, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        self.advance(machine, SHOOTING_SETTLE)?;
        self.endpoint(machine, Some(pulse), MenuTriple { menu: 1, setting: 5, cursor: 2 })
    }

    fn scenario(&mut self, machine: &mut Machine, scenario: Scenario, trajectory: Vec<TrajectoryPhase>) -> Result<(), String> {
        self.boot(machine, scenario == Scenario::ColdBoot)?;
        match scenario {
            Scenario::ColdBoot => Ok(()),
            Scenario::MenuRoute => {
                for phase in trajectory {
                    self.phase = phase.phase.clone();
                    for shape in phase.steps {
                        let step = Step::from(shape);
                        let pulse = self.pulse(machine, step.key, step.key.pulse())?;
                        self.state(machine, pulse, step.expected)?;
                    }
                }
                Ok(())
            }
            Scenario::HalfPress => {
                self.phase = "half-press".into();
                let pulse = KeyPulse::instructions(HALF_PRESS_HOLD, HALF_PRESS_RELEASE);
                let gpio = machine.document.firmware.shutter(&machine.session, ShutterStage::Half)?;
                self.gpio_row(machine, gpio, pulse)?;
                self.advance(machine, HALF_PRESS_SETTLE)?;
                self.phase = "half-press-settled".into();
                self.advance(machine, HALF_PRESS_SETTLE)
            }
        }
    }

    fn point_rows(&self) -> Vec<PointRow<'_>> {
        self.plan.points.iter().zip(&self.points).map(|(point, stats)| PointRow {
            label: &point.label,
            role: point.role,
            machine: point.machine,
            address: Address(point.address),
            entries: stats.entries,
            first: stats.first,
            last: stats.last,
            contexts: counted(&stats.contexts),
            context_overflow: stats.context_overflow,
            callers: counted(&stats.callers),
            caller_overflow: stats.caller_overflow,
        }).collect()
    }

    fn dispatcher(&self) -> Dispatcher {
        let stats = self.plan.points.iter().position(|point| point.role == Role::Dispatcher).map(|index| &self.points[index]);
        let Some(stats) = stats else { return Dispatcher { entries: 0, machines: Vec::new(), rows: Vec::new() } };
        let mut machines: BTreeMap<u8, u64> = BTreeMap::new();
        let mut rows: BTreeMap<(u8, u32), u64> = BTreeMap::new();
        for (context, count) in &stats.contexts {
            let machine = u8::try_from(context.r0).unwrap_or(u8::MAX);
            *machines.entry(machine).or_default() += count;
            *rows.entry((machine, context.r1)).or_default() += count;
        }
        Dispatcher { entries: stats.entries, machines: counted(&machines), rows: counted(&rows) }
    }

    fn queue43(&self) -> Queue43 {
        let stats = self.plan.points.iter().position(|point| point.role == Role::Producer).map(|index| &self.points[index]);
        let Some(stats) = stats else { return Queue43 { task_id: self.plan.task_id, posts: 0, ops: Vec::new(), callers: Vec::new() } };
        let mut ops: BTreeMap<u32, u64> = BTreeMap::new();
        for (context, count) in &stats.contexts {
            *ops.entry(context.r0).or_default() += count;
        }
        Queue43 { task_id: self.plan.task_id, posts: stats.entries, ops: counted(&ops), callers: counted(&stats.callers) }
    }
}

fn arguments() -> Result<BTreeMap<String, PathBuf>, String> {
    const REQUIRED: [&str; 6] = ["--run", "--update", "--boot-rom", "--sites", "--scenario", "--output"];
    let mut arguments = std::env::args_os().skip(1);
    let mut paths = BTreeMap::new();
    while let Some(flag) = arguments.next() {
        let name = flag.to_str().ok_or(USAGE)?.to_owned();
        let value = arguments.next().ok_or(USAGE)?;
        require(REQUIRED.contains(&name.as_str()) || name == "--steps", USAGE)?;
        require(paths.insert(name, PathBuf::from(value)).is_none(), USAGE)?;
    }
    for name in REQUIRED {
        require(paths.contains_key(name), USAGE)?;
    }
    Ok(paths)
}

fn bound(path: &Path) -> Result<(Vec<u8>, Binding), String> {
    let bytes = read(path)?;
    let digest = sha256(&bytes);
    Ok((bytes, Binding { path: path.to_owned(), sha256: digest }))
}

fn parse_scenario(text: &str) -> Result<Scenario, String> {
    Ok(match text {
        "cold-boot" => Scenario::ColdBoot,
        "menu-route" => Scenario::MenuRoute,
        "half-press" => Scenario::HalfPress,
        _ => return Err(format!("{text}: unknown scenario")),
    })
}

fn drive() -> Result<(), String> {
    let paths = arguments()?;
    let output = paths["--output"].clone();
    let scenario = parse_scenario(paths["--scenario"].to_str().ok_or("scenario is not text")?)?;
    let (run_bytes, run_binding) = bound(&paths["--run"])?;
    let (dat, dat_binding) = bound(&paths["--update"])?;
    let (rom, rom_binding) = bound(&paths["--boot-rom"])?;
    let (sites_bytes, sites_binding) = bound(&paths["--sites"])?;
    let plan = load_sites(&sites_bytes)?;
    let (trajectory, trajectory_binding) = match (scenario, paths.get("--steps")) {
        (Scenario::MenuRoute, Some(path)) => {
            let (bytes, binding) = bound(path)?;
            let trajectory: Trajectory = serde_json::from_slice(&bytes).map_err(|error| format!("trajectory: {error}"))?;
            require(!trajectory.before_unload.is_empty(), "trajectory has no phase")?;
            (trajectory, Some(binding))
        }
        (Scenario::MenuRoute, None) => return Err("menu-route needs --steps".into()),
        (_, Some(_)) => return Err("--steps belongs to menu-route only".into()),
        (_, None) => (Trajectory { before_unload: Vec::new() }, None),
    };
    let phases = trajectory.before_unload;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let (_, executable_binding) = bound(&executable)?;
    std::fs::create_dir(&output).map_err(|error| format!("{}: {error}", output.display()))?;

    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(dat), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    write(&output.join("reset.fujimod"), &checkpoint)?;
    let restored = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "reset checkpoint is not preexecution")?;

    let mut census = Census {
        points: plan.points.iter().map(|_| PointStats::default()).collect(),
        ranges: plan.ranges.iter().map(|_| RangeStats::default()).collect(),
        maps,
        plan: &plan,
        watch: None,
        phase: String::new(),
        milestones: Vec::new(),
    };
    machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
    machine.traces.stop_labels = [SITE_LABEL.to_owned()].into_iter().collect();
    let observed = (|| -> Result<(), String> {
        machine.traces.arm(&machine.session, plan.points.iter().map(|point| (point.address, SITE_LABEL.to_owned())).collect())?;
        census.watch = Some(FetchWatch::begin(&machine, &census.maps, census.classes())?);
        census.scenario(&mut machine, scenario, phases)
    })();
    let ended = match census.watch.take() {
        Some(watch) => watch.end(&machine, &plan.ranges, &mut census.ranges),
        None => Ok(()),
    };
    let error = observed.err().or(ended.err());
    let inputs = [Some(run_binding), Some(dat_binding), Some(rom_binding), Some(sites_binding), trajectory_binding, Some(executable_binding)]
        .into_iter().flatten().collect();
    let report = Report {
        schema: 1,
        scenario,
        inputs,
        limits: Limits {
            sites: SITE_LIMIT, contexts: CONTEXT_LIMIT, callers: CALLER_LIMIT, addresses: ADDRESS_LIMIT,
            instructions: INSTRUCTION_LIMIT, watch_rows: WATCH_LIMIT, segment_instructions: SEGMENT_INSTRUCTIONS,
        },
        instructions: machine.instructions(),
        counts: machine.session.board.instruction_counts().try_into().ok(),
        milestones: &census.milestones,
        points: census.point_rows(),
        dispatcher: census.dispatcher(),
        queue43: census.queue43(),
        action_ranges: census.plan.ranges.iter().zip(&census.ranges).map(|(range, stats)| RangeRow {
            machine: range.machine,
            start: Address(range.start),
            end: Address(range.end),
            fetches: stats.fetches,
            addresses: counted(&stats.addresses),
            address_overflow: stats.address_overflow,
        }).collect(),
        error: error.clone(),
        machine_failure: machine.failure.clone(),
    };
    write(&output.join("census.json"), &serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?)?;
    match error { Some(error) => Err(error), None => Ok(()) }
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("saclac-census: {error}");
            ExitCode::FAILURE
        }
    }
}
