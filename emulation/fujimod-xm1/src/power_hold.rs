//! Opt-in observation of the clock block's power-hold word on the driver's
//! route: every guest access to the word, the clock model's hold bit and
//! supply-off deadline at each driver stop, and fetch-attributed instruction
//! counts for accesses at the word's statically known writer sites.
use crate::machine::Machine;
use fujimod_emulator::board::bus::Transaction;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_devices::exr2::clock::{BASE, POWER_HOLD_RELEASE_LIMIT_TICKS};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;

pub const HOLD: Address = Address(BASE + 0x84);
const HOLD_INDEX: usize = 0x84 / 4;
/// The run document's clock-control node.
const CLOCKS: &str = "clocks";
const ACCESS_LIMIT: usize = 65_536;
const FETCH_LIMIT: usize = 65_536;

/// Stores in the captured 1.50 ROM (`bootrom.dis`) to the hold word or to an
/// address loaded from the configuration block, and the image-4 stop
/// routine's store at `0x00656a20` (`lz/id4.dis`).
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
#[serde(transparent)]
pub struct Core(pub usize);

/// A CPU-view instruction address.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Pc(pub u64);

/// A physical bus address.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Address(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation { Read, Write }

/// One bus transaction overlapping the hold word.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Access {
    pub core: Core,
    pub pc: Pc,
    pub address: Address,
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
    pub core: Core,
    pub pc: Pc,
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
    /// Absent when the model state could not be read.
    pub state: Option<HoldState>,
    pub failure: Option<String>,
    pub accesses: Vec<Access>,
    pub omitted: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum DeadlineEvent {
    Armed { tick: Tick, core: Core, pc: Pc, due: Tick },
    Disarmed { tick: Tick, core: Core, pc: Pc },
}

#[derive(Debug, PartialEq)]
pub struct Transitions {
    pub events: Vec<DeadlineEvent>,
    pub end: HoldState,
}

/// The deadline transitions `accesses` make from `before` under the model
/// law (`exr2/clock.rs`, `Clocks::write`).
pub fn transitions(before: HoldState, accesses: &[Access]) -> Transitions {
    let mut state = before;
    let mut events = Vec::new();
    let writes = accesses.iter().filter(|a| a.operation == Operation::Write && a.accepted && a.size == 4 && a.address == HOLD);
    for access in writes {
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
    Transitions { events, end: state }
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
struct Interval<'a> {
    stop: &'a Stop,
    events: Vec<DeadlineEvent>,
    /// Whether the replayed state equals the model state at the stop.
    matches_model: Option<bool>,
    /// No access of the interval exceeded the transaction limit.
    complete: bool,
}

#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32,
    word: Address,
    release_limit_ticks: u64,
    writer_sites: Vec<String>,
    reset: HoldState,
    /// Written by `finish`.
    finished: bool,
    intervals: Vec<Interval<'a>>,
    fetch_rows: usize,
    fetch_violation: Option<String>,
    errors: &'a [String],
}

pub struct PowerHold {
    file: File,
    watch: Box<MemoryWatch>,
    reset: HoldState,
    stops: Vec<Stop>,
    errors: Vec<String>,
}

impl PowerHold {
    /// Creates the receipt file and begins both watches.
    pub fn begin(machine: &Machine, path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err(format!("XM1_POWER_HOLD_LOG must be absolute: {}", path.display()));
        }
        if machine.instructions() != 0 {
            return Err("the power-hold observation begins before the first guest instruction".into());
        }
        let reset = HoldState::read(machine)?;
        let file = File::options().write(true).create_new(true).open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let maps = crate::run::memory_maps(&machine.document)?;
        let classes = WRITER_SITES.iter().map(|&site| WatchRange { start: u64::from(site), end: u64::from(site) + 4, kinds: WatchKinds::CPU_FETCH }).collect();
        let watch = machine.session.board.memory_watch_begin_classified(classes, FETCH_LIMIT, maps, ViewPolicy::Observe)?;
        machine.session.board.transaction_watch_begin(vec![(HOLD.0, HOLD.0 + 4)], ACCESS_LIMIT)?;
        let mut log = Self { file, watch, reset, stops: Vec::new(), errors: Vec::new() };
        log.write(false)?;
        Ok(log)
    }

    /// Closes the interval since the previous stop and rewrites the receipt.
    pub fn stop(&mut self, machine: &Machine, label: &str) {
        self.close(machine, label, true);
        if let Err(error) = self.write(false) {
            self.errors.push(format!("{label}: {error}"));
        }
    }

    /// Closes the final interval, ends both watches and rewrites the receipt.
    pub fn finish(mut self, machine: &Machine) -> Result<(), String> {
        self.close(machine, "finish", false);
        if let Err(error) = machine.session.board.memory_watch_end(&mut self.watch) {
            self.errors.push(format!("finish: {error}"));
        }
        self.write(true)
    }

    fn close(&mut self, machine: &Machine, label: &str, restart: bool) {
        let board = &machine.session.board;
        let (accesses, omitted) = match board.transaction_watch_end() {
            Ok(batch) => {
                let mut accesses = Vec::with_capacity(batch.transactions.len());
                for transaction in batch.transactions {
                    match access(transaction) {
                        Ok(access) => accesses.push(access),
                        Err(error) => self.errors.push(format!("{label}: {error}")),
                    }
                }
                (accesses, batch.omitted)
            }
            Err(error) => {
                self.errors.push(format!("{label}: {error}"));
                (Vec::new(), 0)
            }
        };
        if restart && let Err(error) = board.transaction_watch_begin(vec![(HOLD.0, HOLD.0 + 4)], ACCESS_LIMIT) {
            self.errors.push(format!("{label}: {error}"));
        }
        let state = HoldState::read(machine).map_err(|error| self.errors.push(format!("{label}: {error}"))).ok();
        if !restart && accesses.is_empty() && omitted == 0 {
            return;
        }
        self.stops.push(Stop {
            label: label.to_owned(),
            instructions: Instructions(machine.instructions()),
            core_instructions: board.instruction_counts().into_iter().map(Instructions).collect(),
            tick: Tick(board.now()),
            state,
            failure: machine.failure.clone(),
            accesses,
            omitted,
        });
    }

    fn write(&mut self, finished: bool) -> Result<(), String> {
        let watched = self.watch.records.borrow().state();
        let fetch_violation = watched.get("violation").and_then(serde_json::Value::as_str).map(str::to_owned);
        let mut rows = Vec::new();
        for row in watched.get("records").and_then(serde_json::Value::as_array).into_iter().flatten() {
            match serde_json::from_value::<FetchRow>(row.clone()) {
                Ok(row) => rows.push(row),
                Err(error) => return Err(format!("fetch row: {error}")),
            }
        }
        let mut stops = self.stops.clone();
        for stop in &mut stops {
            attribute(&mut stop.accesses, &rows);
        }
        let mut before = self.reset;
        let intervals = stops.iter().map(|stop| {
            let Transitions { events, end } = transitions(before, &stop.accesses);
            before = stop.state.unwrap_or(end);
            Interval { stop, events, matches_model: stop.state.map(|state| state == end), complete: stop.omitted == 0 }
        }).collect();
        let receipt = Receipt {
            schema: 1,
            word: HOLD,
            release_limit_ticks: POWER_HOLD_RELEASE_LIMIT_TICKS,
            writer_sites: WRITER_SITES.iter().map(|site| format!("{site:#010x}")).collect(),
            reset: self.reset,
            finished,
            intervals,
            fetch_rows: rows.len(),
            fetch_violation,
            errors: &self.errors,
        };
        let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
        self.file.set_len(0).and_then(|()| self.file.rewind()).and_then(|()| self.file.write_all(&bytes))
            .map_err(|error| format!("power-hold receipt: {error}"))
    }
}

fn access(transaction: Transaction) -> Result<Access, String> {
    let operation = match transaction.operation.as_ref() {
        "read" => Operation::Read,
        "write" => Operation::Write,
        other => return Err(format!("hold-word transaction operation {other}")),
    };
    Ok(Access {
        core: Core(transaction.cpu),
        pc: Pc(transaction.pc),
        address: Address(transaction.address),
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
        Access { core: Core(0), pc: Pc(pc), address: HOLD, size: 4, operation: Operation::Write, value, accepted: true, tick: Tick(tick), instructions: None }
    }

    const RESET: HoldState = HoldState { held: false, power_off_due: None };

    #[test]
    fn a_release_after_a_hold_arms_and_a_later_hold_disarms() {
        let accesses = [write(10, 0x800000ec, 1), write(20, 0x80000330, 0), write(30, 0x80000330, 0), write(40, 0x656a20, 1)];
        assert_eq!(transitions(RESET, &accesses), Transitions {
            events: vec![
                DeadlineEvent::Armed { tick: Tick(20), core: Core(0), pc: Pc(0x80000330), due: Tick(20 + POWER_HOLD_RELEASE_LIMIT_TICKS) },
                DeadlineEvent::Disarmed { tick: Tick(40), core: Core(0), pc: Pc(0x656a20) },
            ],
            end: HoldState { held: true, power_off_due: None },
        });
    }

    #[test]
    fn a_release_from_the_reset_state_arms_once() {
        let end = transitions(RESET, &[write(5, 0x80000330, 0x300), write(9, 0x80000330, 0)]);
        assert_eq!(end.events.len(), 1);
        assert_eq!(end.end, HoldState { held: false, power_off_due: Some(Tick(5 + POWER_HOLD_RELEASE_LIMIT_TICKS)) });
    }

    #[test]
    fn reads_refused_narrow_and_neighbouring_writes_do_not_change_the_deadline() {
        let mut read = write(5, 0x80000330, 0);
        read.operation = Operation::Read;
        let mut refused = write(6, 0x80000330, 0);
        refused.accepted = false;
        let mut byte = write(7, 0x80000330, 0);
        byte.size = 1;
        let mut neighbour = write(8, 0x80000330, 0);
        neighbour.address = Address(HOLD.0 - 4);
        neighbour.size = 8;
        assert_eq!(transitions(RESET, &[read, refused, byte, neighbour]), Transitions { events: vec![], end: RESET });
    }

    #[test]
    fn fetch_rows_of_the_same_core_pc_and_tick_attribute_an_access_only_when_they_agree() {
        let row = |core, pc, tick, count| FetchRow { source: FetchSource::CpuFetch, core: Core(core), pc: Pc(pc), instructions: Instructions(count), tick: Tick(tick) };
        let mut accesses = vec![write(20, 0x80000330, 0), write(30, 0x80000700, 1), write(40, 0x123456, 1), write(50, 0x800000ec, 1)];
        let rows = [
            row(0, 0x80000330, 20, 481), row(1, 0x80000330, 20, 482), row(0, 0x80000700, 30, 700), row(0, 0x80000700, 30, 701),
            row(0, 0x800000ec, 50, 900), row(0, 0x800000ec, 50, 900),
        ];
        attribute(&mut accesses, &rows);
        assert_eq!(accesses.iter().map(|access| access.instructions).collect::<Vec<_>>(), [Some(Instructions(481)), None, None, Some(Instructions(900))]);
    }
}
