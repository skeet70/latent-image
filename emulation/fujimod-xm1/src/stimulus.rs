//! Scenario stimulus timing. A run fires each stimulus after an instruction budget, or at a
//! scheduled board tick. Board ticks are the guest time the device models count, the system
//! timer included.

use crate::machine::{Machine, RunOutcome};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt::Debug;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StimulusClock {
    /// A point is due once the scenario's instruction budget for it has run.
    Instructions,
    /// A point is due at its scheduled tick ordinal.
    BoardTicks,
}

/// Board ticks since a run's observation origin.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TickOrdinal(pub u64);

/// The board tick and total instruction count a run counts from: reset, or its restored checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Origin {
    pub board_tick: u64,
    pub instructions: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScheduledPoint<S> {
    pub stimulus: S,
    pub tick: TickOrdinal,
}

/// The points a board-tick run reaches, in order, with nondecreasing ticks. A receipt's
/// stimulus section parses as one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TickSchedule<S>(Vec<ScheduledPoint<S>>);

impl<S: DeserializeOwned> TickSchedule<S> {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Document<S> { points: Vec<ScheduledPoint<S>> }
        let Document { points } = serde_json::from_slice::<Document<S>>(bytes).map_err(|error| format!("tick schedule: {error}"))?;
        if points.is_empty() {
            return Err("tick schedule: no point".into());
        }
        if points.windows(2).any(|pair| pair[1].tick < pair[0].tick) {
            return Err("tick schedule: a tick precedes its predecessor".into());
        }
        Ok(Self(points))
    }
}

impl<S> TickSchedule<S> {
    /// Delays each point that follows one `opens` matches, and every later point with it, until it
    /// comes at least `gap` ticks after that point.
    pub fn widened(self, gap: u64, opens: impl Fn(&S) -> bool) -> Self {
        let mut shift = 0;
        let mut floor: Option<u64> = None;
        Self(self.0.into_iter().map(|point| {
            let scheduled = point.tick.0 + shift;
            let tick = floor.map_or(scheduled, |floor| scheduled.max(floor));
            shift += tick - scheduled;
            floor = opens(&point.stimulus).then_some(tick + gap);
            ScheduledPoint { stimulus: point.stimulus, tick: TickOrdinal(tick) }
        }).collect())
    }
}

impl<S: Serialize> Serialize for TickSchedule<S> {
    fn serialize<Z: serde::Serializer>(&self, serializer: Z) -> Result<Z::Ok, Z::Error> {
        #[derive(Serialize)]
        struct Document<'a, S> { points: &'a [ScheduledPoint<S>] }
        Document { points: &self.0 }.serialize(serializer)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReachedPoint<S> {
    pub stimulus: S,
    pub tick: TickOrdinal,
    pub instructions: u64,
}

#[derive(Debug, Serialize)]
pub struct StimulusReceipt<'a, S> {
    pub clock: StimulusClock,
    pub origin: Origin,
    pub points: &'a [ReachedPoint<S>],
}

/// Where a run segment ends: an instruction count, a board tick, or whichever comes first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Deadline {
    instructions: u64,
    board_tick: Option<u64>,
}

impl Deadline {
    pub const fn instructions(end: u64) -> Self {
        Self { instructions: end, board_tick: None }
    }

    pub const fn board_tick(tick: u64) -> Self {
        Self { instructions: u64::MAX, board_tick: Some(tick) }
    }

    /// This deadline, or `slice` instructions from now when that comes first.
    pub fn within(self, machine: &Machine, slice: u64) -> Self {
        Self { instructions: self.instructions.min(machine.instructions().saturating_add(slice)), ..self }
    }

    pub fn reached(self, machine: &Machine) -> bool {
        machine.instructions() >= self.instructions
            || self.board_tick.is_some_and(|tick| machine.session.board.now() >= tick)
    }

    /// Runs toward the deadline for at most `limit` instructions; a trace stop can end it sooner.
    pub fn run(self, machine: &mut Machine, limit: u64) -> RunOutcome {
        let remaining = self.instructions.saturating_sub(machine.instructions());
        machine.run_until(remaining.min(limit), self.board_tick)
    }
}

enum Timing<S> {
    Instructions,
    BoardTicks(TickSchedule<S>),
}

/// A run's stimulus clock and the points it has reached.
pub struct Stimuli<S> {
    timing: Timing<S>,
    origin: Origin,
    reached: Vec<ReachedPoint<S>>,
}

impl<S: Clone + Debug + PartialEq> Stimuli<S> {
    /// The instruction clock, counting from the machine's current state.
    pub fn instructions(machine: &Machine) -> Self {
        Self { timing: Timing::Instructions, origin: origin(machine), reached: Vec::new() }
    }

    /// The board-tick clock, counting from the machine's current state.
    pub fn board_ticks(machine: &Machine, schedule: TickSchedule<S>) -> Self {
        Self { timing: Timing::BoardTicks(schedule), origin: origin(machine), reached: Vec::new() }
    }

    pub fn clock(&self) -> StimulusClock {
        match self.timing {
            Timing::Instructions => StimulusClock::Instructions,
            Timing::BoardTicks(_) => StimulusClock::BoardTicks,
        }
    }

    pub fn tick(&self, machine: &Machine) -> TickOrdinal {
        TickOrdinal(machine.session.board.now() - self.origin.board_tick)
    }

    /// When the next point is due: `budget` instructions from now on the instruction clock,
    /// its scheduled tick on the board-tick clock.
    pub fn deadline(&self, machine: &Machine, budget: u64) -> Result<Deadline, String> {
        match &self.timing {
            Timing::Instructions => machine.instructions().checked_add(budget).map(Deadline::instructions)
                .ok_or_else(|| "instruction deadline overflows".into()),
            Timing::BoardTicks(TickSchedule(points)) => {
                let point = points.get(self.reached.len()).ok_or("the tick schedule has no point left")?;
                self.origin.board_tick.checked_add(point.tick.0).map(Deadline::board_tick)
                    .ok_or_else(|| "board tick deadline overflows".into())
            }
        }
    }

    /// Records the next point at the machine's current state. On the board-tick clock it must be
    /// the scheduled stimulus at its scheduled tick.
    pub fn reach(&mut self, machine: &Machine, stimulus: S) -> Result<(), String> {
        let tick = self.tick(machine);
        if let Timing::BoardTicks(TickSchedule(points)) = &self.timing {
            let index = self.reached.len();
            let point = points.get(index).ok_or_else(|| format!("point {index} ({stimulus:?}) is past the tick schedule"))?;
            if point.stimulus != stimulus {
                return Err(format!("point {index}: the schedule fires {:?}, the scenario {stimulus:?}", point.stimulus));
            }
            if point.tick != tick {
                return Err(format!("point {index} ({stimulus:?}) is at tick {} instead of {}", tick.0, point.tick.0));
            }
        }
        self.reached.push(ReachedPoint { stimulus, tick, instructions: machine.instructions() });
        Ok(())
    }

    /// Fails when a board-tick run left scheduled points unreached.
    pub fn require_complete(&self) -> Result<(), String> {
        match &self.timing {
            Timing::BoardTicks(TickSchedule(points)) if points.len() > self.reached.len() => Err(format!(
                "the run reached {} of {} scheduled points", self.reached.len(), points.len())),
            _ => Ok(()),
        }
    }

    pub fn receipt(&self) -> StimulusReceipt<'_, S> {
        StimulusReceipt { clock: self.clock(), origin: self.origin, points: &self.reached }
    }
}

fn origin(machine: &Machine) -> Origin {
    Origin { board_tick: machine.session.board.now(), instructions: machine.instructions() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::tests::machine_with_code;

    const SPIN: u32 = 0xeaff_fffe;
    const WFI: u32 = 0xe320_f003;
    /// `b` back to the preceding word.
    const BACK: u32 = 0xeaff_fffd;

    fn schedule(points: &[(&str, u64)]) -> TickSchedule<String> {
        let points: Vec<_> = points.iter().map(|(stimulus, tick)| serde_json::json!({"stimulus": stimulus, "tick": tick})).collect();
        TickSchedule::parse(&serde_json::to_vec(&serde_json::json!({"points": points})).unwrap()).unwrap()
    }

    fn fire(machine: &mut Machine, stimuli: &mut Stimuli<String>, budget: u64, stimulus: &str) -> Result<(), String> {
        let deadline = stimuli.deadline(machine, budget)?;
        while !deadline.reached(machine) {
            deadline.run(machine, u64::MAX);
            machine.require_running()?;
        }
        stimuli.reach(machine, stimulus.into())
    }

    fn ticks(stimuli: &Stimuli<String>) -> Vec<u64> {
        stimuli.receipt().points.iter().map(|point| point.tick.0).collect()
    }

    #[test]
    fn board_tick_points_land_on_their_ticks_from_the_origin() {
        let mut machine = machine_with_code(&[SPIN]);
        machine.run(12_345);
        let start = machine.session.board.now();
        let mut stimuli = Stimuli::board_ticks(&machine, schedule(&[("press", 0), ("release", 1_000), ("end", 25_000)]));
        for stimulus in ["press", "release", "end"] {
            fire(&mut machine, &mut stimuli, 7, stimulus).unwrap();
        }
        stimuli.require_complete().unwrap();
        assert_eq!(ticks(&stimuli), [0, 1_000, 25_000]);
        assert_eq!(machine.session.board.now(), start + 25_000);
        assert_eq!(stimuli.receipt().origin, Origin { board_tick: start, instructions: 12_345 });
    }

    #[test]
    fn board_tick_points_do_not_depend_on_executed_instructions() {
        let mut busy = machine_with_code(&[SPIN]);
        let mut idle = machine_with_code(&[WFI, BACK]);
        let plan = [("press", 0), ("release", 20_000), ("end", 50_000)];
        let mut busy_stimuli = Stimuli::board_ticks(&busy, schedule(&plan));
        let mut idle_stimuli = Stimuli::board_ticks(&idle, schedule(&plan));
        for (stimulus, _) in plan {
            fire(&mut busy, &mut busy_stimuli, 0, stimulus).unwrap();
            fire(&mut idle, &mut idle_stimuli, 0, stimulus).unwrap();
        }
        assert_eq!(ticks(&busy_stimuli), ticks(&idle_stimuli));
        assert!(idle.instructions() < busy.instructions() / 100, "idle {} busy {}", idle.instructions(), busy.instructions());
    }

    #[test]
    fn an_instruction_clock_receipt_is_a_tick_schedule_for_the_same_points() {
        let mut machine = machine_with_code(&[SPIN]);
        let mut stimuli = Stimuli::instructions(&machine);
        for (stimulus, budget) in [("press", 0), ("release", 24_000), ("end", 48_000)] {
            fire(&mut machine, &mut stimuli, budget, stimulus).unwrap();
        }
        let receipt = serde_json::to_vec(&stimuli.receipt()).unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&receipt).unwrap()["clock"], "instructions");
        assert_eq!(ticks(&stimuli), [0, 1_000, 3_000]);
        assert_eq!(TickSchedule::<String>::parse(&receipt).unwrap(), schedule(&[("press", 0), ("release", 1_000), ("end", 3_000)]));
    }

    #[test]
    fn a_board_tick_run_refuses_a_point_off_its_schedule() {
        let mut machine = machine_with_code(&[SPIN]);
        let mut stimuli = Stimuli::board_ticks(&machine, schedule(&[("press", 0), ("release", 1_000)]));
        assert!(stimuli.reach(&machine, "release".into()).is_err(), "a different stimulus is refused");
        stimuli.reach(&machine, "press".into()).unwrap();
        machine.run(240);
        assert!(stimuli.reach(&machine, "release".into()).is_err(), "a point before its tick is refused");
        assert!(stimuli.require_complete().is_err(), "an unreached point leaves the run incomplete");
        fire(&mut machine, &mut stimuli, 0, "release").unwrap();
        stimuli.require_complete().unwrap();
        assert!(stimuli.deadline(&machine, 0).is_err(), "no point is left");
        assert!(stimuli.reach(&machine, "end".into()).is_err(), "a point past the schedule is refused");
    }

    #[test]
    fn a_schedule_needs_points_in_tick_order() {
        assert!(TickSchedule::<String>::parse(br#"{"points":[]}"#).is_err());
        assert!(TickSchedule::<String>::parse(br#"{"points":[{"stimulus":"a","tick":5},{"stimulus":"b","tick":4}]}"#).is_err());
        assert!(TickSchedule::<String>::parse(br#"{"points":[{"stimulus":"a","tick":5},{"stimulus":"b","tick":5}]}"#).is_ok());
    }

    #[test]
    fn widening_delays_short_gaps_after_a_release_and_everything_after_them() {
        let plan = [("press", 0), ("release", 1_000), ("press", 1_100), ("release", 3_000), ("press", 4_000), ("release", 5_000), ("end", 5_200)];
        let widened = schedule(&plan).widened(500, |stimulus| stimulus == "release");
        assert_eq!(widened, schedule(&[("press", 0), ("release", 1_000), ("press", 1_500), ("release", 3_400), ("press", 4_400),
            ("release", 5_400), ("end", 5_900)]));
        assert_eq!(TickSchedule::<String>::parse(&serde_json::to_vec(&widened).unwrap()).unwrap(), widened);
        assert_eq!(schedule(&plan).widened(0, |_| true), schedule(&plan));
    }
}
