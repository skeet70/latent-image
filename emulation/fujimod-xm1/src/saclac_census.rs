//! Stock-only census of task 43 (saclac): instruction points at the task entry,
//! the queue-43 producer, the dispatcher and the 37 row functions, and a
//! classified fetch watch over each machine's action range. One scenario per run.
//!
//! The site file declares every address and range; no stock address appears here.

use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::checkpoint::{BootIdentity, PostBootReceipt, SHARED_STORE, post_boot};
use fujimod_xm1::trace::SiteRead;
use fujimod_xm1::guest::{GpioKey, ModeDialPosition, RouteKey, ShutterStage};
use fujimod_xm1::machine::{Hold, KeyPulse, MENU_PULSE, Machine, RunOutcome, key_pulse};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "saclac-census --run RUN --update STOCK_DAT --boot-rom ROM --sites SITES --scenario cold-boot|menu-route|half-press|shoot-without-lens|capture|playback|mode-dial [--steps STEPS] [--checkpoints ROOT] [--start reset|post-boot] --output NEW_DIR";
const SITE_LABEL: &str = "saclac-site";
const SITE_LIMIT: usize = 4096;
const CONTEXT_LIMIT: usize = 1024;
const CALLER_LIMIT: usize = 512;
const ADDRESS_LIMIT: usize = 32_768;
const INSTRUCTION_LIMIT: u64 = 1_500_000_000;
const WATCH_LIMIT: usize = 1 << 20;
const SEGMENT_INSTRUCTIONS: u64 = 50_000_000;
const DISPATCH_LIMIT: usize = 200_000;
const WATCH_DECL_LIMIT: usize = 16;

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
const FULL_PRESS_HOLD: u64 = 8_000_000;
const FULL_PRESS_RELEASE: u64 = 8_000_000;
const PLAY_SETTLE: u64 = 90_000_000;
const MODE_DIAL_SETTLE: u64 = 90_000_000;
/// Shooting-menu cursor of SHOOT WITHOUT LENS; the boot opens the menu at cursor 2.
const LENS_ROW: u32 = 20;
/// Setting id of SHOOT WITHOUT LENS.
const LENS_SETTING: u32 = 0xa3;
/// Cursor of SHOOT WITHOUT LENS ON.
const LENS_ON_CURSOR: u32 = 0;

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

#[derive(Clone, Debug, Deserialize)]
struct DispatchSites {
    trace: String,
    cell_tables: String,
    action_tables: String,
    state_counts: String,
    rows: String,
    states: String,
    machines: u8,
}

#[derive(Clone, Debug, Deserialize)]
struct WatchDecl {
    label: String,
    start: String,
    end: String,
}

#[derive(Debug, Deserialize)]
struct Sites {
    schema: u32,
    task_id: u8,
    state_base: String,
    points: Vec<SitePoint>,
    action_ranges: Vec<ActionRange>,
    #[serde(default)]
    dispatch: Option<DispatchSites>,
    #[serde(default)]
    watch: Vec<WatchDecl>,
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

struct Dispatch {
    trace: u32,
    cell_tables: u32,
    action_tables: u32,
    state_counts: u32,
    rows: u32,
    states: u32,
    machines: u8,
}

struct Watch {
    label: String,
    start: u32,
    end: u32,
}

struct Plan {
    task_id: u8,
    state_base: u32,
    points: Vec<Site>,
    ranges: Vec<Range>,
    dispatch: Option<Dispatch>,
    watch: Vec<Watch>,
}

fn load_sites(bytes: &[u8]) -> Result<Plan, String> {
    let sites: Sites = serde_json::from_slice(bytes).map_err(|error| format!("sites: {error}"))?;
    require(sites.schema == 2, "sites: unsupported schema")?;
    require(sites.task_id > 0, "sites: task id is zero")?;
    let state_base = parse_hex(&sites.state_base)?;
    require(state_base % 2 == 0, "sites: the state base is not halfword aligned")?;
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
    let dispatch = match &sites.dispatch {
        None => None,
        Some(declared) => {
            let dispatch = Dispatch {
                trace: parse_hex(&declared.trace)?,
                cell_tables: parse_hex(&declared.cell_tables)?,
                action_tables: parse_hex(&declared.action_tables)?,
                state_counts: parse_hex(&declared.state_counts)?,
                rows: parse_hex(&declared.rows)?,
                states: parse_hex(&declared.states)?,
                machines: declared.machines,
            };
            require(dispatch.machines > 0 && dispatch.machines <= 37, "sites: dispatch machine count leaves 1..37")?;
            require(dispatch.trace % 4 == 0 && dispatch.cell_tables % 4 == 0 && dispatch.action_tables % 4 == 0
                && dispatch.state_counts % 2 == 0 && dispatch.rows % 2 == 0 && dispatch.states % 2 == 0,
                "sites: a dispatch address is misaligned")?;
            Some(dispatch)
        }
    };
    require(sites.watch.len() <= WATCH_DECL_LIMIT, "sites: more watch ranges than the engine admits")?;
    let mut watch = Vec::with_capacity(sites.watch.len());
    let mut watch_labels = std::collections::BTreeSet::new();
    for declared in &sites.watch {
        let (start, end) = (parse_hex(&declared.start)?, parse_hex(&declared.end)?);
        require(start % 4 == 0 && end % 4 == 0 && start < end, "sites: a watch range is not a nonempty word range")?;
        require(!declared.label.is_empty() && watch_labels.insert(declared.label.clone()), "sites: two watch ranges share a label")?;
        require(!ranges.iter().any(|range| start < range.end && range.start < end), "sites: a watch range overlaps an action range")?;
        require(!watch.iter().any(|range: &Watch| start < range.end && range.start < end), "sites: two watch ranges overlap")?;
        watch.push(Watch { label: declared.label.clone(), start, end });
    }
    watch.sort_by_key(|range| range.start);
    Ok(Plan { task_id: sites.task_id, state_base, points, ranges, dispatch, watch })
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
enum Scenario { ColdBoot, MenuRoute, HalfPress, ShootWithoutLens, Capture, Playback, ModeDial }

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
struct Pulse { key: Key, pressed: u64, released: u64, settled: u64, debounced: bool }

/// One shutter, play or dial input, with the row's debounced release.
#[derive(Clone, Copy, Debug, Serialize)]
struct PressRow { kind: PressKind, pressed: u64, released: u64, settled: u64, debounced: bool }

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PressKind { HalfPress, FullPress, Play }

/// The mode-dial position set and the calibrated window it was held at.
#[derive(Clone, Copy, Debug, Serialize)]
struct DialRow { position: ModeDialPosition, low: u16, high: u16 }

#[derive(Serialize)]
struct Milestone {
    phase: String,
    pulse: Option<Pulse>,
    expected: MenuTriple,
    /// The first instruction at which the expected menu was observed while settling.
    first: Option<u64>,
    reached: Option<u64>,
    observed: Option<MenuTriple>,
    finished: u64,
}

/// One poll of a settling menu, with both cores' instruction counts.
#[derive(Clone, Copy, Debug, Serialize)]
struct MenuSample {
    instructions: u64,
    menu: Option<MenuTriple>,
    counts: Option<[u64; 2]>,
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
    /// The acting machine's state halfword at a dispatcher entry.
    state: Option<u16>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
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
    memory: Vec<String>,
    sp: String,
    lr: String,
    cpsr: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct FetchRow { source: String, address: u64 }

fn containing(ranges: impl Iterator<Item = (u32, u32)>, address: u32) -> Option<usize> {
    ranges.enumerate().find(|(_, (start, end))| *start <= address && address < *end).map(|(index, _)| index)
}

struct FetchWatch { watch: Box<MemoryWatch>, started: u64 }
impl FetchWatch {
    fn begin(machine: &Machine, maps: &[MemoryMap], classes: Vec<WatchRange>) -> Result<Self, String> {
        let watch = machine.session.board.memory_watch_begin_classified(classes, WATCH_LIMIT, maps.to_vec(), ViewPolicy::Observe)?;
        Ok(Self { watch, started: machine.instructions() })
    }
    fn end(
        mut self,
        machine: &Machine,
        plan: &[Range],
        watch: &[Watch],
        stats: &mut [RangeStats],
        watch_stats: &mut [RangeStats],
    ) -> Result<(), String> {
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
            if let Some(index) = containing(plan.iter().map(|range| (range.start, range.end)), address) {
                stats[index].fetch(address);
            } else if let Some(index) = containing(watch.iter().map(|range| (range.start, range.end)), address) {
                watch_stats[index].fetch(address);
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
    states: Vec<Counted<(u8, u16)>>,
    rows: Vec<Counted<(u8, u32)>>,
    transitions: Vec<Counted<(u8, u16, u32)>>,
}

fn dispatcher_from_contexts(entries: u64, contexts: &BTreeMap<EntryContext, u64>) -> Dispatcher {
    let mut machines: BTreeMap<u8, u64> = BTreeMap::new();
    let mut states: BTreeMap<(u8, u16), u64> = BTreeMap::new();
    let mut rows: BTreeMap<(u8, u32), u64> = BTreeMap::new();
    let mut transitions: BTreeMap<(u8, u16, u32), u64> = BTreeMap::new();
    for (context, count) in contexts {
        let machine = u8::try_from(context.r0).unwrap_or(u8::MAX);
        *machines.entry(machine).or_default() += count;
        *rows.entry((machine, context.r1)).or_default() += count;
        if let Some(state) = context.state {
            *states.entry((machine, state)).or_default() += count;
            *transitions.entry((machine, state, context.r1)).or_default() += count;
        }
    }
    Dispatcher { entries, machines: counted(&machines), states: counted(&states), rows: counted(&rows), transitions: counted(&transitions) }
}

#[derive(Serialize)]
struct Queue43 {
    task_id: u8,
    posts: u64,
    ops: Vec<Counted<u32>>,
    callers: Vec<Counted<u32>>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct DispatchEvent {
    machine: u8,
    state: u16,
    row: u16,
    action: Address,
    instructions: u64,
}

#[derive(Serialize)]
struct DispatchTrace {
    events: Vec<DispatchEvent>,
    overflow: u64,
}

#[derive(Serialize)]
struct WatchRow {
    label: String,
    start: Address,
    end: Address,
    fetches: u64,
    addresses: Vec<Counted<u32>>,
    address_overflow: u64,
}

#[derive(Serialize)]
struct Report<'a> {
    schema: u32,
    scenario: Scenario,
    checkpoint: Option<PostBootReceipt>,
    inputs: Vec<Binding>,
    limits: Limits,
    instructions: u64,
    observation_start_instructions: u64,
    executed_instructions: u64,
    counts: Option<[u64; 2]>,
    milestones: &'a [Milestone],
    menu_trace: &'a [MenuSample],
    presses: &'a [PressRow],
    mode_dial: Option<DialRow>,
    points: Vec<PointRow<'a>>,
    dispatcher: Dispatcher,
    queue43: Queue43,
    action_ranges: Vec<RangeRow>,
    dispatch: Option<DispatchTrace>,
    watch: Vec<WatchRow>,
    error: Option<String>,
    machine_failure: Option<String>,
}


fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    require(text.len() % 2 == 0, "a dispatcher site read is not whole bytes")?;
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| "a dispatcher site read is not hexadecimal".to_string()))
        .collect()
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

/// The dispatcher's inputs and selected action at one entry: the row the router
/// stored, the state the arrays hold, and the word the cell selects.
fn dispatch_event(machine: &Machine, dispatch: &Dispatch, stop: &Stop) -> Result<DispatchEvent, String> {
    let register = |name: &str| stop.registers.get(name).and_then(|text| parse_hex(text).ok()).ok_or("a trace entry lacks a register");
    let index = register("r0")?;
    let state = u16::try_from(register("r1")?).map_err(|_| "a trace state exceeds a halfword")?;
    let row = u16::try_from(register("r2")?).map_err(|_| "a trace row exceeds a halfword")?;
    require(index < u32::from(dispatch.machines), "a trace machine index leaves the declared tables")?;
    require(stop.memory.len() == 5, "a trace entry lacks its five site reads")?;
    let cells = decode_hex(&stop.memory[0])?;
    let actions = decode_hex(&stop.memory[1])?;
    let counts = decode_hex(&stop.memory[2])?;
    let machine_index = usize::try_from(index).map_err(|_| "trace machine index exceeds the host")?;
    let cell_base = read_u32(&cells, machine_index * 4);
    let action_base = read_u32(&actions, machine_index * 4);
    let state_count = read_u16(&counts, machine_index * 2);
    let action = if cell_base == 0 {
        0
    } else {
        let cell_index = u32::from(state).wrapping_add(u32::from(row).wrapping_mul(u32::from(state_count))) as u16;
        let mut cell = [0u8; 2];
        machine.session.board.read_memory(stop.core, u64::from(cell_base) + u64::from(cell_index) * 2, &mut cell)?;
        if cell[0] == 0 {
            0
        } else {
            let mut word = [0u8; 4];
            machine.session.board.read_memory(stop.core, u64::from(action_base) + u64::from(cell[0]) * 4, &mut word)?;
            u32::from_le_bytes(word)
        }
    };
    Ok(DispatchEvent {
        machine: u8::try_from(index).map_err(|_| "dispatcher machine index exceeds a byte")?,
        state,
        row,
        action: Address(action),
        instructions: stop.instructions,
    })
}

struct Census<'a> {
    plan: &'a Plan,
    maps: Vec<MemoryMap>,
    points: Vec<PointStats>,
    ranges: Vec<RangeStats>,
    watch_ranges: Vec<RangeStats>,
    dispatch: Vec<DispatchEvent>,
    dispatch_overflow: u64,
    watch: Option<FetchWatch>,
    phase: String,
    milestones: Vec<Milestone>,
    menu_trace: Vec<MenuSample>,
    presses: Vec<PressRow>,
    mode_dial: Option<DialRow>,
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
        }).chain(self.plan.watch.iter().map(|range| WatchRange {
            start: u64::from(range.start),
            end: u64::from(range.end),
            kinds: WatchKinds::CPU_FETCH,
        })).collect()
    }

    fn restart_watch(&mut self, machine: &Machine) -> Result<(), String> {
        let watch = self.watch.take().ok_or("action fetch watch is not active")?;
        watch.end(machine, &self.plan.ranges, &self.plan.watch, &mut self.ranges, &mut self.watch_ranges)?;
        self.watch = Some(FetchWatch::begin(machine, &self.maps, self.classes())?);
        Ok(())
    }

    fn stop(&mut self, machine: &Machine, stops: &[Stop]) -> Result<(), String> {
        for stop in stops {
            if let Some(dispatch) = &self.plan.dispatch {
                if stop.address == dispatch.trace {
                    if self.dispatch.len() >= DISPATCH_LIMIT {
                        self.dispatch_overflow += 1;
                    } else {
                        let event = dispatch_event(machine, dispatch, stop)?;
                        self.dispatch.push(event);
                    }
                }
            }
            let Some(index) = self.point_index(stop.address) else { continue };
            let register = |name: &str| stop.registers.get(name).and_then(|text| parse_hex(text).ok()).unwrap_or(0);
            let r0 = register("r0");
            let state = if self.plan.points[index].role == Role::Dispatcher {
                require(r0 < 37, "dispatcher entry names no machine")?;
                let address = self.plan.state_base + r0 * 2;
                let mut bytes = [0u8; 2];
                machine.session.board.read_physical(u64::from(address), &mut bytes)
                    .map_err(|error| format!("machine {r0} state: {error}"))?;
                Some(u16::from_le_bytes(bytes))
            } else {
                None
            };
            let context = EntryContext {
                core: stop.core,
                mode: stop.cpsr & 0x1f,
                lr: parse_hex(&stop.lr).unwrap_or(0),
                sp: parse_hex(&stop.sp).unwrap_or(0),
                r0,
                r1: register("r1"),
                r2: register("r2"),
                r3: register("r3"),
                state,
            };
            self.points[index].enter(context, stop.instructions);
        }
        Ok(())
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
                    memory: hit.memory.clone(),
                    sp: hit.sp.clone(),
                    lr: hit.lr.clone(),
                    cpsr: hit.cpsr,
                });
            }
            if stops.is_empty() {
                require(machine.instructions() >= end, "run ended early without a census stop")?;
            } else {
                self.stop(machine, &stops)?;
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
            let (pressed, released, settled, debounced) = self.gpio_row(machine, gpio, pulse)?;
            return Ok(Pulse { key, pressed, released, settled, debounced });
        };
        let pressed = machine.instructions();
        machine.press(route)?;
        self.hold(machine, pulse.hold)?;
        machine.release_key(route)?;
        let released = machine.instructions();
        let (settled, debounced) = loop {
            if machine.document.firmware.released(&machine.session, route)? { break (machine.instructions(), true); }
            let spent = machine.instructions() - released;
            if spent >= pulse.release { break (machine.instructions(), false); }
            self.advance(machine, (pulse.release - spent).min(RELEASE_POLL))?;
        };
        Ok(Pulse { key, pressed, released, settled, debounced })
    }

    /// Holds a GPIO key down; a stop inside the hold is recorded.
    fn gpio_down(&mut self, machine: &mut Machine, gpio: GpioKey, hold: Hold) -> Result<u64, String> {
        let node = machine.document.gpio.clone();
        let pressed = machine.instructions();
        machine.session.gpio(&node.0, gpio.port, gpio.pin, gpio.pressed_high)?;
        self.hold(machine, hold)?;
        Ok(pressed)
    }

    /// Releases a GPIO key and waits for the firmware-debounced release byte.
    fn gpio_up(&mut self, machine: &mut Machine, gpio: GpioKey, release: u64) -> Result<(u64, u64, bool), String> {
        let node = machine.document.gpio.clone();
        machine.session.gpio(&node.0, gpio.port, gpio.pin, !gpio.pressed_high)?;
        let released = machine.instructions();
        let (settled, debounced) = loop {
            if machine.document.firmware.gpio_released(&machine.session, gpio)? { break (machine.instructions(), true); }
            let spent = machine.instructions() - released;
            if spent >= release { break (machine.instructions(), false); }
            self.advance(machine, (release - spent).min(RELEASE_POLL))?;
        };
        Ok((released, settled, debounced))
    }

    /// Holds and releases a GPIO key, advancing through the census so a stop inside the hold is recorded.
    fn gpio_row(&mut self, machine: &mut Machine, gpio: GpioKey, pulse: KeyPulse) -> Result<(u64, u64, u64, bool), String> {
        let pressed = self.gpio_down(machine, gpio, pulse.hold)?;
        let (released, settled, debounced) = self.gpio_up(machine, gpio, pulse.release)?;
        Ok((pressed, released, settled, debounced))
    }

    fn press_row(&mut self, machine: &mut Machine, kind: PressKind, gpio: GpioKey, pulse: KeyPulse) -> Result<(), String> {
        let (pressed, released, settled, debounced) = self.gpio_row(machine, gpio, pulse)?;
        self.presses.push(PressRow { kind, pressed, released, settled, debounced });
        Ok(())
    }

    fn sample_menu(&mut self, machine: &Machine) -> Result<(), String> {
        let counts = machine.session.board.instruction_counts().try_into().ok();
        self.menu_trace.push(MenuSample { instructions: machine.instructions(), menu: Self::menu(machine)?, counts });
        Ok(())
    }

    fn menu(machine: &Machine) -> Result<Option<MenuTriple>, String> {
        Ok(machine.menu()?.map(|menu| MenuTriple { menu: menu.menu_id, setting: menu.setting_id, cursor: menu.cursor }))
    }

    fn endpoint(
        &mut self,
        machine: &Machine,
        pulse: Option<Pulse>,
        expected: MenuTriple,
        first: Option<u64>,
    ) -> Result<(), String> {
        let observed = Self::menu(machine)?;
        let reached = (observed == Some(expected)).then(|| machine.instructions());
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse, expected, first, reached, observed, finished: machine.instructions() });
        require(reached.is_some(), "menu endpoint differs")
    }

    /// Advances `budget` instructions in menu polls, recording the first instruction at
    /// which the expected menu is observed.
    fn advance_to(&mut self, machine: &mut Machine, budget: u64, expected: MenuTriple) -> Result<Option<u64>, String> {
        let mut first = None;
        let mut spent = 0;
        while spent < budget {
            let step = (budget - spent).min(MENU_POLL);
            self.advance(machine, step)?;
            spent += step;
            if first.is_none() && Self::menu(machine)? == Some(expected) {
                first = Some(machine.instructions());
            }
        }
        Ok(first)
    }

    fn state(&mut self, machine: &mut Machine, pulse: Pulse, expected: MenuTriple) -> Result<(), String> {
        let start = machine.instructions();
        let reached = loop {
            self.sample_menu(machine)?;
            if Self::menu(machine)? == Some(expected) { break Some(machine.instructions()); }
            let spent = machine.instructions() - start;
            if spent >= STEP_STATE_CAP { break None; }
            self.advance(machine, (STEP_STATE_CAP - spent).min(MENU_POLL))?;
        };
        if reached.is_some() { self.advance(machine, STEP_SETTLE)?; }
        let observed = Self::menu(machine)?;
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse: Some(pulse), expected, first: reached, reached, observed, finished: machine.instructions() });
        require(reached.is_some() && observed == Some(expected), "menu trajectory state differs")
    }

    /// Runs the cold start and the initial setup. With `open_menu`, the MENU press that opens
    /// the shooting menu follows; the key routes start from the live view before it.
    fn boot(&mut self, machine: &mut Machine, open_menu: bool) -> Result<(), String> {
        if machine.instructions() == 0 {
            self.prepare_boot(machine)?;
        }
        self.open_menu(machine, open_menu)
    }

    fn prepare_boot(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.phase = "cold-start".into();
        let erased = MenuTriple { menu: 69, setting: 52, cursor: 12 };
        let first = self.advance_to(machine, INITIAL_MENU, erased)?;
        self.endpoint(machine, None, erased, first)?;
        self.phase = "initial-setup".into();
        for key in std::iter::repeat_n(Key::Up, 12).chain([Key::Ok, Key::Ok]) {
            self.pulse(machine, key, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        }
        self.advance(machine, SETUP_SETTLE)?;
        Ok(())
    }

    fn open_menu(&mut self, machine: &mut Machine, open_menu: bool) -> Result<(), String> {
        if !open_menu { return Ok(()); }
        self.phase = "shooting-menu".into();
        let pulse = self.pulse(machine, Key::Menu, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        let shooting = MenuTriple { menu: 1, setting: 5, cursor: 2 };
        let first = self.advance_to(machine, SHOOTING_SETTLE, shooting)?;
        self.endpoint(machine, Some(pulse), shooting, first)
    }

    /// Opens the shooting menu at SHOOT WITHOUT LENS, selects ON, commits and closes it.
    fn prepare_no_lens(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.phase = "shoot-without-lens".into();
        let mut pulse = None;
        for _ in 2..LENS_ROW {
            pulse = Some(self.pulse(machine, Key::Down, Key::Down.pulse())?);
        }
        let pulse = pulse.ok_or("SHOOT WITHOUT LENS needs a navigation step")?;
        self.state(machine, pulse, MenuTriple { menu: 1, setting: 5, cursor: LENS_ROW })?;
        let pulse = self.pulse(machine, Key::Right, Key::Right.pulse())?;
        self.state(machine, pulse, MenuTriple { menu: 1, setting: LENS_SETTING, cursor: 1 })?;
        let pulse = self.pulse(machine, Key::Up, Key::Up.pulse())?;
        self.state(machine, pulse, MenuTriple { menu: 1, setting: LENS_SETTING, cursor: LENS_ON_CURSOR })?;
        self.pulse(machine, Key::Ok, Key::Ok.pulse())?;
        self.advance(machine, SETUP_SETTLE)?;
        self.pulse(machine, Key::Back, Key::Back.pulse())?;
        self.advance(machine, SETUP_SETTLE)
    }

    fn half_press(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.phase = "half-press".into();
        let pulse = KeyPulse::instructions(HALF_PRESS_HOLD, HALF_PRESS_RELEASE);
        let gpio = machine.document.firmware.shutter(&machine.session, ShutterStage::Half)?;
        self.press_row(machine, PressKind::HalfPress, gpio, pulse)?;
        self.advance(machine, HALF_PRESS_SETTLE)?;
        self.phase = "half-press-settled".into();
        self.advance(machine, HALF_PRESS_SETTLE)
    }

    fn full_press(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.phase = "full-press".into();
        let pulse = KeyPulse::instructions(FULL_PRESS_HOLD, FULL_PRESS_RELEASE);
        let gpio = machine.document.firmware.shutter(&machine.session, ShutterStage::Full)?;
        self.press_row(machine, PressKind::FullPress, gpio, pulse)?;
        self.advance(machine, HALF_PRESS_SETTLE)
    }

    /// S1 held, S2 pressed and released, then S1 released.
    fn capture(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.phase = "s1-s2".into();
        let half = machine.document.firmware.shutter(&machine.session, ShutterStage::Half)?;
        let full = machine.document.firmware.shutter(&machine.session, ShutterStage::Full)?;
        let pressed = self.gpio_down(machine, half, Hold::Instructions(HALF_PRESS_HOLD))?;
        let full_pressed = self.gpio_down(machine, full, Hold::Instructions(FULL_PRESS_HOLD))?;
        let (full_released, full_settled, full_debounced) = self.gpio_up(machine, full, FULL_PRESS_RELEASE)?;
        self.presses.push(PressRow { kind: PressKind::FullPress, pressed: full_pressed, released: full_released, settled: full_settled, debounced: full_debounced });
        self.advance(machine, HALF_PRESS_SETTLE)?;
        let (half_released, half_settled, half_debounced) = self.gpio_up(machine, half, HALF_PRESS_RELEASE)?;
        self.presses.push(PressRow { kind: PressKind::HalfPress, pressed, released: half_released, settled: half_settled, debounced: half_debounced });
        self.advance(machine, HALF_PRESS_SETTLE)
    }

    fn playback(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.phase = "playback".into();
        let gpio = machine.document.firmware.play(&machine.session)?;
        self.press_row(machine, PressKind::Play, gpio, MENU_PULSE)?;
        self.advance(machine, PLAY_SETTLE)
    }

    fn mode_dial(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.phase = "mode-dial".into();
        let window = machine.set_mode_dial(ModeDialPosition::M)?;
        self.mode_dial = Some(DialRow { position: window.position, low: window.low, high: window.high });
        self.advance(machine, MODE_DIAL_SETTLE)
    }

    fn scenario(&mut self, machine: &mut Machine, scenario: Scenario, trajectory: Vec<TrajectoryPhase>) -> Result<(), String> {
        match scenario {
            Scenario::ColdBoot => self.boot(machine, true),
            Scenario::MenuRoute => {
                self.boot(machine, false)?;
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
                self.boot(machine, false)?;
                self.half_press(machine)
            }
            Scenario::ShootWithoutLens => {
                self.boot(machine, true)?;
                self.prepare_no_lens(machine)?;
                self.half_press(machine)?;
                self.full_press(machine)
            }
            Scenario::Capture => {
                self.boot(machine, true)?;
                self.prepare_no_lens(machine)?;
                self.capture(machine)
            }
            Scenario::Playback => {
                self.boot(machine, false)?;
                self.playback(machine)
            }
            Scenario::ModeDial => {
                self.boot(machine, false)?;
                self.mode_dial(machine)
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
        match stats {
            Some(stats) => dispatcher_from_contexts(stats.entries, &stats.contexts),
            None => Dispatcher { entries: 0, machines: Vec::new(), states: Vec::new(), rows: Vec::new(), transitions: Vec::new() },
        }
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
        require(REQUIRED.contains(&name.as_str()) || name == "--steps" || name == "--checkpoints" || name == "--start", USAGE)?;
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
        "shoot-without-lens" => Scenario::ShootWithoutLens,
        "capture" => Scenario::Capture,
        "playback" => Scenario::Playback,
        "mode-dial" => Scenario::ModeDial,
        "movie" => return Err("movie: the fujimod-xm1 input API declares no movie key".into()),
        _ => return Err(format!("{text}: unknown scenario")),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObservationStart { Reset, PostBoot }

fn drive() -> Result<(), String> {
    let paths = arguments()?;
    let output = paths["--output"].clone();
    let scenario = parse_scenario(paths["--scenario"].to_str().ok_or("scenario is not text")?)?;
    require(scenario != Scenario::ColdBoot || !paths.contains_key("--checkpoints"), "--checkpoints is not allowed for cold-boot")?;
    let start = match paths.get("--start").and_then(|path| path.to_str()) {
        None if scenario == Scenario::ColdBoot => ObservationStart::Reset,
        None | Some("post-boot") => ObservationStart::PostBoot,
        Some("reset") => ObservationStart::Reset,
        Some(_) => return Err("--start must be reset or post-boot".into()),
    };
    require(scenario != Scenario::ColdBoot || start == ObservationStart::Reset, "cold-boot requires --start reset")?;
    require(start != ObservationStart::Reset || !paths.contains_key("--checkpoints"), "--checkpoints is not allowed with --start reset")?;
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
    let (executable_bytes, executable_binding) = bound(&executable)?;
    let boot_identity = BootIdentity::new(&run_bytes, &dat, &rom, &[], None,
        b"saclac-post-setup-liveview-v1:80m;up12-ok-ok:8m-8m;settle40m")
        .with_emulator_binary(&executable_bytes);
    std::fs::create_dir(&output).map_err(|error| format!("{}: {error}", output.display()))?;

    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(dat), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    let mut checkpoint = None;

    let mut census = Census {
        points: plan.points.iter().map(|_| PointStats::default()).collect(),
        ranges: plan.ranges.iter().map(|_| RangeStats::default()).collect(),
        watch_ranges: plan.watch.iter().map(|_| RangeStats::default()).collect(),
        dispatch: Vec::new(),
        dispatch_overflow: 0,
        maps,
        plan: &plan,
        watch: None,
        phase: String::new(),
        milestones: Vec::new(),
        menu_trace: Vec::new(),
        presses: Vec::new(),
        mode_dial: None,
    };
    let setup = if start == ObservationStart::PostBoot {
        let root = paths.get("--checkpoints").map(PathBuf::as_path).unwrap_or_else(|| Path::new(SHARED_STORE));
        post_boot(&mut machine, &boot_identity, root, |machine| census.prepare_boot(machine)).map(|receipt| {
            checkpoint = Some(receipt);
            census.milestones.clear();
            census.menu_trace.clear();
        })
    } else { Ok(()) };
    let observation_start_instructions = machine.instructions();
    let reused_instructions = checkpoint.as_ref().filter(|receipt| receipt.lookup == fujimod_xm1::checkpoint::CacheLookup::Hit)
        .map_or(0, |receipt| receipt.record.instructions);
    machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
    machine.traces.stop_labels = [SITE_LABEL.to_owned()].into_iter().collect();
    if let Some(dispatch) = &plan.dispatch {
        let machines = u64::from(dispatch.machines);
        machine.traces.site_reads = BTreeMap::from([(dispatch.trace, vec![
            SiteRead::Absolute([u64::from(dispatch.cell_tables), machines * 4]),
            SiteRead::Absolute([u64::from(dispatch.action_tables), machines * 4]),
            SiteRead::Absolute([u64::from(dispatch.state_counts), machines * 2]),
            SiteRead::Absolute([u64::from(dispatch.rows), machines * 2]),
            SiteRead::Absolute([u64::from(dispatch.states), machines * 2]),
        ])]);
    }
    let mut sites: BTreeMap<u32, String> = plan.points.iter().map(|point| (point.address, SITE_LABEL.to_owned())).collect();
    if let Some(dispatch) = &plan.dispatch {
        sites.insert(dispatch.trace, SITE_LABEL.to_owned());
    }
    let observed = (|| -> Result<(), String> {
        setup?;
        machine.traces.arm(&machine.session, sites)?;
        census.watch = Some(FetchWatch::begin(&machine, &census.maps, census.classes())?);
        census.scenario(&mut machine, scenario, phases)
    })();
    let ended = match census.watch.take() {
        Some(watch) => watch.end(&machine, &plan.ranges, &plan.watch, &mut census.ranges, &mut census.watch_ranges),
        None => Ok(()),
    };
    let error = observed.err().or(ended.err());
    let inputs = [Some(run_binding), Some(dat_binding), Some(rom_binding), Some(sites_binding), trajectory_binding, Some(executable_binding)]
        .into_iter().flatten().collect();
    let report = Report {
        schema: 3,
        scenario,
        checkpoint,
        inputs,
        limits: Limits {
            sites: SITE_LIMIT, contexts: CONTEXT_LIMIT, callers: CALLER_LIMIT, addresses: ADDRESS_LIMIT,
            instructions: INSTRUCTION_LIMIT, watch_rows: WATCH_LIMIT, segment_instructions: SEGMENT_INSTRUCTIONS,
        },
        instructions: machine.instructions(),
        observation_start_instructions,
        executed_instructions: machine.instructions() - reused_instructions,
        counts: machine.session.board.instruction_counts().try_into().ok(),
        milestones: &census.milestones,
        menu_trace: &census.menu_trace,
        presses: &census.presses,
        mode_dial: census.mode_dial,
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
        dispatch: census.plan.dispatch.as_ref().map(|_| DispatchTrace {
            events: census.dispatch.clone(),
            overflow: census.dispatch_overflow,
        }),
        watch: census.plan.watch.iter().zip(&census.watch_ranges).map(|(range, stats)| WatchRow {
            label: range.label.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sites(state_base: Option<&str>, schema: u32) -> Vec<u8> {
        let mut points = vec![
            serde_json::json!({"address": "0x00001000", "label": "task-entry", "role": "actor"}),
            serde_json::json!({"address": "0x00001004", "label": "queue-43-producer", "role": "producer"}),
            serde_json::json!({"address": "0x00001008", "label": "dispatcher", "role": "dispatcher"}),
        ];
        for machine in 0..37u8 {
            points.push(serde_json::json!({
                "address": format!("{:#x}", 0x2000 + u32::from(machine) * 4),
                "label": format!("row-{machine:02}"),
                "role": "row",
                "machine": machine,
            }));
        }
        let ranges: Vec<_> = (0..37u8).map(|machine| {
            let start = 0x10000 + u32::from(machine) * 0x100;
            serde_json::json!({"machine": machine, "start": format!("{start:#x}"), "end": format!("{:#x}", start + 0x100)})
        }).collect();
        let mut document = serde_json::json!({"schema": schema, "task_id": 43, "points": points, "action_ranges": ranges});
        if let Some(base) = state_base {
            document["state_base"] = serde_json::json!(base);
        }
        serde_json::to_vec(&document).unwrap()
    }

    #[test]
    fn sites_declare_a_halfword_state_base() {
        let plan = load_sites(&sites(Some("0x0169d4b4"), 2)).unwrap();
        assert_eq!(plan.state_base, 0x0169d4b4);
        assert_eq!(plan.points.len(), 40);
        assert_eq!(plan.ranges.len(), 37);
        assert!(load_sites(&sites(None, 2)).is_err(), "a sites file without a state base is refused");
        assert!(load_sites(&sites(Some("0x0169d4b5"), 2)).is_err(), "an odd state base is refused");
        assert!(load_sites(&sites(Some("0x0169d4b4"), 1)).is_err(), "schema 1 is refused");
    }

    fn context(machine: u32, row: u32, state: Option<u16>) -> EntryContext {
        EntryContext { core: 0, mode: 0x13, lr: 0, sp: 0, r0: machine, r1: row, r2: 0, r3: 0, state }
    }

    #[test]
    fn dispatcher_groups_machine_state_and_row() {
        let mut contexts = BTreeMap::new();
        contexts.insert(context(12, 0xab, Some(5)), 2);
        contexts.insert(context(12, 0xab, Some(6)), 1);
        contexts.insert(context(8, 0xbc, Some(0)), 4);
        let dispatcher = dispatcher_from_contexts(7, &contexts);
        assert_eq!(dispatcher.entries, 7);
        assert_eq!(dispatcher.machines, vec![Counted { key: 8, count: 4 }, Counted { key: 12, count: 3 }]);
        assert_eq!(dispatcher.states, vec![
            Counted { key: (8, 0), count: 4 },
            Counted { key: (12, 5), count: 2 },
            Counted { key: (12, 6), count: 1 },
        ]);
        assert_eq!(dispatcher.rows, vec![Counted { key: (8, 0xbc), count: 4 }, Counted { key: (12, 0xab), count: 3 }]);
        assert_eq!(dispatcher.transitions, vec![
            Counted { key: (8, 0, 0xbc), count: 4 },
            Counted { key: (12, 5, 0xab), count: 2 },
            Counted { key: (12, 6, 0xab), count: 1 },
        ]);
    }

    #[test]
    fn scenarios_parse_and_serialize() {
        for (name, scenario) in [
            ("cold-boot", Scenario::ColdBoot),
            ("menu-route", Scenario::MenuRoute),
            ("half-press", Scenario::HalfPress),
            ("shoot-without-lens", Scenario::ShootWithoutLens),
            ("capture", Scenario::Capture),
            ("playback", Scenario::Playback),
            ("mode-dial", Scenario::ModeDial),
        ] {
            assert_eq!(parse_scenario(name).unwrap(), scenario);
            assert_eq!(serde_json::to_value(scenario).unwrap(), serde_json::json!(name));
        }
        assert!(parse_scenario("movie").unwrap_err().contains("no movie key"));
        assert!(parse_scenario("still").is_err());
    }
}
