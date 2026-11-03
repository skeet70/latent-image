//! Opt-in observation of the clock block's power-hold word on the driver's
//! route: every guest access to the word, the clock model's hold bit and
//! supply-off deadline at each driver stop, and fetch-attributed instruction
//! counts for accesses at the word's statically known writer sites.
use crate::machine::Machine;
use fujimod_emulator::board::bus::Transaction;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_devices::exr2::clock::{BASE, POWER_HOLD_RELEASE_LIMIT_TICKS};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const HOLD: u64 = BASE + 0x84;
const HOLD_INDEX: usize = 0x84 / 4;
/// The run document's clock-control node.
const CLOCKS: &str = "clocks";
const ACCESS_LIMIT: usize = 65_536;
const FETCH_LIMIT: usize = 65_536;

/// Every store in the captured 1.50 ROM (`bootrom.dis`) whose address is the
/// hold word or a configuration-block word, and the image-4 stop routine's
/// store at `0x00656a20` (`lz/id4.dis`).
pub const WRITER_SITES: [u32; 14] = [
    0x800000ec, 0x800002fc, 0x80000330, 0x80000754, 0x8000085c, 0x80000980, 0x80000ae0,
    0x80000b48, 0x80000524, 0x80000538, 0x80000614, 0x80000628, 0x80000700, 0x00656a20,
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Tick(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Instructions(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation { Read, Write }

/// One bus transaction on the hold word.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Access {
    pub core: usize,
    pub pc: u64,
    pub size: u8,
    pub operation: Operation,
    pub value: u64,
    pub accepted: bool,
    pub tick: Tick,
    /// Board entered-instruction count of the fetch rows at the same core,
    /// PC and tick; absent unless those rows exist and agree.
    pub instructions: Option<Instructions>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FetchSource { CpuFetch }

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct FetchRow {
    source: FetchSource,
    pub core: usize,
    pub pc: u64,
    pub instructions: Instructions,
    pub tick: Tick,
}

/// The clock model's power-hold state, read from its saved device state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HoldState {
    pub held: bool,
    pub power_off_due: Option<Tick>,
}

#[derive(Deserialize)]
struct ClockState { registers: Vec<u32>, power_off_due: Option<u64> }

impl HoldState {
    fn read(machine: &Machine) -> Result<Self, String> {
        let node = machine.session.node(CLOCKS)?;
        let state: ClockState = serde_json::from_value(machine.session.board.device_save(node)?)
            .map_err(|error| format!("clock-control state: {error}"))?;
        let word = state.registers.get(HOLD_INDEX).ok_or("clock-control state has no hold word")?;
        Ok(Self { held: word & 1 != 0, power_off_due: state.power_off_due.map(Tick) })
    }
}

/// One driver stop and the accesses since the previous stop.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Stop {
    pub label: String,
    pub instructions: Instructions,
    pub core_instructions: Vec<Instructions>,
    pub tick: Tick,
    pub state: HoldState,
    pub failure: Option<String>,
    pub accesses: Vec<Access>,
    pub omitted: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum DeadlineEvent {
    Armed { tick: Tick, core: usize, pc: u64, due: Tick },
    Disarmed { tick: Tick, core: usize, pc: u64 },
}

/// The deadline transitions the stores of `stop` make from the state
/// `before`, under the model law (`exr2/clock.rs`, `Clocks::write`), and
/// whether they end in the state the model reported at `stop`.
pub fn transitions(before: HoldState, stop: &Stop) -> (Vec<DeadlineEvent>, bool) {
    let mut state = before;
    let mut events = Vec::new();
    for access in stop.accesses.iter().filter(|a| a.operation == Operation::Write && a.accepted && a.size == 4) {
        state.held = access.value & 1 != 0;
        if state.held {
            if state.power_off_due.take().is_some() {
                events.push(DeadlineEvent::Disarmed { tick: access.tick, core: access.core, pc: access.pc });
            }
        } else if state.power_off_due.is_none() {
            let due = Tick(access.tick.0 + POWER_HOLD_RELEASE_LIMIT_TICKS);
            state.power_off_due = Some(due);
            events.push(DeadlineEvent::Armed { tick: access.tick, core: access.core, pc: access.pc, due });
        }
    }
    (events, state == stop.state)
}

pub fn attribute(accesses: &mut [Access], rows: &[FetchRow]) {
    for access in accesses {
        let mut matching = rows.iter().filter(|row| row.core == access.core && row.pc == access.pc && row.tick == access.tick)
            .map(|row| row.instructions);
        let first = matching.next();
        if matching.all(|count| Some(count) == first) {
            access.instructions = first;
        }
    }
}

#[derive(Serialize)]
struct Interval<'a> { stop: &'a Stop, events: Vec<DeadlineEvent>, consistent: bool }

#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32,
    word: String,
    release_limit_ticks: u64,
    writer_sites: Vec<String>,
    reset: HoldState,
    intervals: Vec<Interval<'a>>,
    fetch_rows: usize,
    fetch_violation: Option<String>,
    error: Option<String>,
}

pub struct PowerHold {
    path: PathBuf,
    watch: Box<MemoryWatch>,
    reset: HoldState,
    stops: Vec<Stop>,
    error: Option<String>,
}

impl PowerHold {
    pub fn begin(machine: &Machine, path: PathBuf) -> Result<Self, String> {
        if machine.instructions() != 0 {
            return Err("the power-hold observation begins before the first guest instruction".into());
        }
        let reset = HoldState::read(machine)?;
        let maps = crate::run::memory_maps(&machine.document)?;
        let classes = WRITER_SITES.iter().map(|&site| WatchRange { start: u64::from(site), end: u64::from(site) + 4, kinds: WatchKinds::CPU_FETCH }).collect();
        let watch = machine.session.board.memory_watch_begin_classified(classes, FETCH_LIMIT, maps, ViewPolicy::Observe)?;
        machine.session.board.transaction_watch_begin(vec![(HOLD, HOLD + 4)], ACCESS_LIMIT)?;
        Ok(Self { path, watch, reset, stops: Vec::new(), error: None })
    }

    /// Closes the interval since the previous stop.
    pub fn stop(&mut self, machine: &Machine, label: &str) {
        if let Err(error) = self.try_stop(machine, label) {
            self.error.get_or_insert(error);
        }
    }

    fn try_stop(&mut self, machine: &Machine, label: &str) -> Result<(), String> {
        let board = &machine.session.board;
        let batch = board.transaction_watch_end()?;
        board.transaction_watch_begin(vec![(HOLD, HOLD + 4)], ACCESS_LIMIT)?;
        let accesses = batch.transactions.into_iter().map(access).collect::<Result<_, _>>()?;
        self.stops.push(Stop {
            label: label.to_owned(),
            instructions: Instructions(machine.instructions()),
            core_instructions: board.instruction_counts().into_iter().map(Instructions).collect(),
            tick: Tick(board.now()),
            state: HoldState::read(machine)?,
            failure: machine.failure.clone(),
            accesses,
            omitted: batch.omitted,
        });
        Ok(())
    }

    pub fn finish(mut self, machine: &Machine) -> Result<(), String> {
        let board = &machine.session.board;
        if let Err(error) = board.transaction_watch_end() {
            self.error.get_or_insert(error);
        }
        let state = self.watch.records.borrow().state();
        if let Err(error) = board.memory_watch_end(&mut self.watch) {
            self.error.get_or_insert(error);
        }
        let fetch_violation = state.get("violation").and_then(serde_json::Value::as_str).map(str::to_owned);
        let rows = state.get("records").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
        let rows = rows.into_iter().map(|row| serde_json::from_value::<FetchRow>(row).map_err(|error| format!("fetch row: {error}")))
            .collect::<Result<Vec<_>, _>>();
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => { self.error.get_or_insert(error); Vec::new() }
        };
        for stop in &mut self.stops {
            attribute(&mut stop.accesses, &rows);
        }
        let mut before = self.reset;
        let intervals = self.stops.iter().map(|stop| {
            let (events, consistent) = transitions(before, stop);
            before = stop.state;
            Interval { stop, events, consistent }
        }).collect();
        let receipt = Receipt {
            schema: 1,
            word: format!("{HOLD:#x}"),
            release_limit_ticks: POWER_HOLD_RELEASE_LIMIT_TICKS,
            writer_sites: WRITER_SITES.iter().map(|site| format!("{site:#010x}")).collect(),
            reset: self.reset,
            intervals,
            fetch_rows: rows.len(),
            fetch_violation,
            error: self.error.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
        std::fs::write(&self.path, bytes).map_err(|error| format!("{}: {error}", self.path.display()))
    }
}

fn access(transaction: Transaction) -> Result<Access, String> {
    let operation = match transaction.operation.as_ref() {
        "read" => Operation::Read,
        "write" => Operation::Write,
        other => return Err(format!("hold-word transaction operation {other}")),
    };
    Ok(Access {
        core: transaction.cpu,
        pc: transaction.pc,
        size: transaction.size,
        operation,
        value: transaction.value,
        accepted: transaction.accepted,
        tick: Tick(transaction.tick),
        instructions: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(tick: u64, pc: u64, value: u64) -> Access {
        Access { core: 0, pc, size: 4, operation: Operation::Write, value, accepted: true, tick: Tick(tick), instructions: None }
    }

    fn stop(accesses: Vec<Access>, state: HoldState) -> Stop {
        Stop { label: "run".into(), instructions: Instructions(0), core_instructions: vec![], tick: Tick(0), state, failure: None, accesses, omitted: 0 }
    }

    const RESET: HoldState = HoldState { held: false, power_off_due: None };

    #[test]
    fn a_release_after_a_hold_arms_and_a_later_hold_disarms() {
        let accesses = vec![write(10, 0x800000ec, 1), write(20, 0x80000330, 0), write(30, 0x80000330, 0), write(40, 0x656a20, 1)];
        let (events, consistent) = transitions(RESET, &stop(accesses, HoldState { held: true, power_off_due: None }));
        assert!(consistent);
        assert_eq!(events, [
            DeadlineEvent::Armed { tick: Tick(20), core: 0, pc: 0x80000330, due: Tick(20 + POWER_HOLD_RELEASE_LIMIT_TICKS) },
            DeadlineEvent::Disarmed { tick: Tick(40), core: 0, pc: 0x656a20 },
        ]);
    }

    #[test]
    fn a_release_at_reset_state_arms_and_a_differing_model_state_is_inconsistent() {
        let armed = HoldState { held: false, power_off_due: Some(Tick(5 + POWER_HOLD_RELEASE_LIMIT_TICKS)) };
        let (events, consistent) = transitions(RESET, &stop(vec![write(5, 0x80000330, 0x300)], armed));
        assert!(consistent);
        assert_eq!(events.len(), 1);
        let (_, consistent) = transitions(RESET, &stop(vec![write(5, 0x80000330, 0x300)], RESET));
        assert!(!consistent);
    }

    #[test]
    fn reads_refused_and_narrow_writes_do_not_change_the_deadline() {
        let mut read = write(5, 0x80000330, 0);
        read.operation = Operation::Read;
        let mut refused = write(6, 0x80000330, 0);
        refused.accepted = false;
        let mut byte = write(7, 0x80000330, 0);
        byte.size = 1;
        let (events, consistent) = transitions(RESET, &stop(vec![read, refused, byte], RESET));
        assert!(events.is_empty() && consistent);
    }

    #[test]
    fn fetch_rows_of_the_same_core_pc_and_tick_attribute_an_access_only_when_they_agree() {
        let row = |core, pc, tick, count| FetchRow { source: FetchSource::CpuFetch, core, pc, instructions: Instructions(count), tick: Tick(tick) };
        let mut accesses = vec![write(20, 0x80000330, 0), write(30, 0x80000700, 1), write(40, 0x123456, 1), write(50, 0x800000ec, 1)];
        let rows = [
            row(0, 0x80000330, 20, 481), row(1, 0x80000330, 20, 482), row(0, 0x80000700, 30, 700), row(0, 0x80000700, 30, 701),
            row(0, 0x800000ec, 50, 900), row(0, 0x800000ec, 50, 900),
        ];
        attribute(&mut accesses, &rows);
        assert_eq!(accesses.iter().map(|access| access.instructions).collect::<Vec<_>>(), [Some(Instructions(481)), None, None, Some(Instructions(900))]);
    }
}
