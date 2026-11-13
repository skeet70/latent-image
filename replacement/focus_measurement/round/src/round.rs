#![forbid(unsafe_code)]

use crate::lanes::{Lane, Lanes, Selection, ORDER};
use crate::planner::{Cadence, Mode, Planner, Policy, Slot, Work};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl, MeasurementOutcome, MeasurementUpdate, Restart, RetainedRoundStatus};
use xm1_saclac_interface::{ClientCallback, Command, CommandKind, Completed, Completion, Parameters, RecordHandle};

/// A lane child's answer to the round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneStatus { Refused, Complete, Pending, Unhandled, Updated(MeasurementUpdate), Idle }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneSignal<'a> {
    Initialize,
    Enable,
    Stop,
    Suspend,
    Resume,
    Work(Work),
    Command(Command<'a>),
    Completed(Completed<'a>),
}

/// The lane whose enabling announces the search mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchStart { Search, Image }

/// The client of the latest parameter change, answered when its selection
/// clear finishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterClient(pub Option<ClientCallback>);

/// Effects on the measurement record and its peers, in the order the round requests them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordEffect {
    ResetSelection,
    ClearSelection,
    FinishSelectionClear(ParameterClient),
    Geometry,
    CollectReady,
    SelectionHistory,
    SearchMode(SearchStart),
    Planned { primary_considered: bool },
}

pub trait Environment {
    fn signal(&mut self, lane: Lane, signal: LaneSignal<'_>, record: RecordHandle) -> LaneStatus;
    /// `None` when the record offers no selection at all.
    fn available(&mut self, record: RecordHandle) -> Option<Selection>;
    fn search_permitted(&mut self) -> bool;
    fn alternate_search(&mut self, record: RecordHandle) -> bool;
    fn autofocus_geometry(&mut self, record: RecordHandle) -> bool;
    fn planning_mode(&mut self, record: RecordHandle) -> Mode;
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence;
    fn record_effect(&mut self, record: RecordHandle, effect: RecordEffect);
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Dormant, Idle, Armed, Running, Stopping }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fan { Ready, Draining, Stopping }

pub struct Round {
    phase: Phase,
    fan: Fan,
    selected: Lanes,
    enabling: Lanes,
    draining: Lanes,
    search_allowed: bool,
    pending_clear: Option<ParameterClient>,
    planner: Planner,
}

impl Default for Round { fn default() -> Self { Self::new() } }

struct Cadences<'a, E>(&'a mut E);
impl<E: Environment> Policy for Cadences<'_, E> {
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence { self.0.cadence(work, mode) }
}

impl Round {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Dormant,
            fan: Fan::Ready,
            selected: Lanes::empty(),
            enabling: Lanes::empty(),
            draining: Lanes::empty(),
            search_allowed: false,
            pending_clear: None,
            planner: Planner::new(),
        }
    }

    pub fn handle(&mut self, event: ChildEvent<'_, MeasurementControl>, env: &mut impl Environment) -> ChildOutcome {
        match event {
            ChildEvent::Control { control, record, .. } => self.control(control, record, env),
            ChildEvent::Command { command, record } => self.command(command, record, env),
            ChildEvent::Completed { completed, record } => self.completed(completed, record, env),
        }
    }

    fn control(&mut self, control: MeasurementControl, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        if self.phase == Phase::Stopping { return fault(env); }
        let running = matches!(self.phase, Phase::Armed | Phase::Running);
        match control {
            MeasurementControl::Initialize => match self.phase {
                Phase::Dormant => outcome(self.start(record, env)),
                Phase::Idle => ChildOutcome::Complete,
                _ => fault(env),
            },
            MeasurementControl::Stop if running => match self.fan_stop(record, env) {
                LaneStatus::Complete => { self.phase = Phase::Dormant; ChildOutcome::Complete }
                LaneStatus::Pending => { self.phase = Phase::Stopping; ChildOutcome::Pending }
                other => outcome(other),
            },
            MeasurementControl::Stop => { self.phase = Phase::Dormant; ChildOutcome::Complete }
            MeasurementControl::Reconfigure => {
                env.record_effect(record, RecordEffect::Geometry);
                ChildOutcome::Pending
            }
            MeasurementControl::Suspend if running => outcome(self.fan_suspend(record, env)),
            MeasurementControl::Resume if running => outcome(self.fan_resume(record, env)),
            MeasurementControl::Suspend | MeasurementControl::Resume => ChildOutcome::Complete,
        }
    }

    fn command(&mut self, command: Command<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        match command.kind {
            CommandKind::Command14 | CommandKind::Command15 | CommandKind::Command16 | CommandKind::Command2b => match self.phase {
                Phase::Armed | Phase::Running => outcome(self.fan_forward(command, record, env)),
                _ => fault(env),
            },
            CommandKind::Command17 => self.parameters_changed(command.parameters, record, env),
            CommandKind::Autofocus => {
                if self.phase == Phase::Stopping { return fault(env); }
                if env.autofocus_geometry(record) { env.record_effect(record, RecordEffect::Geometry); }
                ChildOutcome::Complete
            }
            _ => self.unrouted(LaneSignal::Command(command), record, env),
        }
    }

    fn completed(&mut self, completed: Completed<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        let signal = LaneSignal::Completed(completed);
        match completed.completion {
            Completion::Image45 => match self.phase {
                Phase::Armed | Phase::Running => { self.search_allowed = true; ChildOutcome::Pending }
                _ => ChildOutcome::Fault,
            },
            Completion::Image46 => self.lane_completed(Lane::Search, signal, record, env),
            Completion::Image47 | Completion::Image66 => self.lane_completed(Lane::Image, signal, record, env),
            Completion::Accessor4b => {
                match self.phase {
                    Phase::Armed => {
                        env.record_effect(record, RecordEffect::CollectReady);
                        if self.fan_run(record, env) != LaneStatus::Complete { self.phase = Phase::Running; }
                    }
                    Phase::Running => env.record_effect(record, RecordEffect::SelectionHistory),
                    _ => {}
                }
                ChildOutcome::Pending
            }
            _ => self.unrouted(signal, record, env),
        }
    }

    fn start(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        self.reset_selection(record, env);
        self.enabling = Lanes::empty();
        self.draining = Lanes::empty();
        env.record_effect(record, RecordEffect::ClearSelection);
        self.planner.reset_cycle();
        match self.fan_initialize(record, env) {
            LaneStatus::Pending => { self.phase = Phase::Armed; LaneStatus::Complete }
            LaneStatus::Complete => { self.phase = Phase::Idle; LaneStatus::Complete }
            other => other,
        }
    }

    fn parameters_changed(&mut self, parameters: Parameters<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        let change = crate::stock::parameter_change(parameters);
        match self.phase {
            Phase::Stopping => fault(env),
            Phase::Dormant => {
                self.pending_clear = Some(change.client);
                self.finish_selection_clear(record, env);
                ChildOutcome::Complete
            }
            Phase::Idle => {
                self.pending_clear = Some(change.client);
                let status = self.start(record, env);
                if status == LaneStatus::Complete { self.finish_selection_clear(record, env); }
                outcome(status)
            }
            Phase::Armed | Phase::Running => {
                self.pending_clear = Some(change.client);
                self.search_allowed = false;
                if self.fan_detect_changes(change.requested, change.offered, record, env) != LaneStatus::Idle {
                    self.finish_selection_clear(record, env);
                    return ChildOutcome::Complete;
                }
                if self.enabling.contains(Lane::Search) {
                    env.record_effect(record, RecordEffect::SearchMode(SearchStart::Search));
                } else if self.enabling.contains(Lane::Image) {
                    env.record_effect(record, RecordEffect::SearchMode(SearchStart::Image));
                }
                ChildOutcome::Pending
            }
        }
    }

    fn lane_completed(&mut self, lane: Lane, signal: LaneSignal<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        match self.phase {
            Phase::Dormant | Phase::Idle => ChildOutcome::Fault,
            Phase::Armed => {
                let status = self.fan_event(signal, record, env);
                if status != LaneStatus::Complete { return outcome(status); }
                self.draining.remove(lane);
                if self.draining.is_empty() {
                    if self.selected.is_empty() {
                        self.phase = Phase::Idle;
                        self.finish_selection_clear(record, env);
                    } else if self.enabling.is_empty() {
                        self.finish_selection_clear(record, env);
                    }
                }
                ChildOutcome::Complete
            }
            Phase::Running => self.running_lane_completed(signal, record, env),
            Phase::Stopping => self.stopping_event(signal, record, env),
        }
    }

    fn running_lane_completed(&mut self, signal: LaneSignal<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        let status = self.fan_event(signal, record, env);
        if status != LaneStatus::Complete { return outcome(status); }
        if !self.enabling.is_empty() { return ChildOutcome::Pending; }
        self.finish_selection_clear(record, env);
        ChildOutcome::Complete
    }

    fn unrouted(&mut self, signal: LaneSignal<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        match self.phase {
            Phase::Dormant | Phase::Idle | Phase::Armed => outcome(LaneStatus::Unhandled),
            Phase::Running => self.round_event(signal, record, env),
            Phase::Stopping => self.stopping_event(signal, record, env),
        }
    }

    fn stopping_event(&mut self, signal: LaneSignal<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        if self.fan_event(signal, record, env) == LaneStatus::Complete {
            self.phase = Phase::Dormant;
            ChildOutcome::Complete
        } else {
            ChildOutcome::Pending
        }
    }

    /// A lane event while a round is running: join it, plan more work, and once the
    /// round is idle apply the parameter changes it deferred.
    fn round_event(&mut self, signal: LaneSignal<'_>, record: RecordHandle, env: &mut impl Environment) -> ChildOutcome {
        let joined = self.fan_event(signal, record, env);
        if joined == LaneStatus::Refused { return ChildOutcome::Fault; }
        if self.fan_run(record, env) != LaneStatus::Complete { return outcome(joined); }
        if self.enabling.is_empty() && self.draining.is_empty() {
            self.phase = Phase::Armed;
            return outcome(joined);
        }
        self.reset_selection(record, env);
        match self.fan_stop_changes(record, env) {
            LaneStatus::Complete => {}
            LaneStatus::Pending => { self.phase = Phase::Armed; return outcome(joined); }
            other => return outcome(other),
        }
        match self.fan_enable_changes(record, env) {
            LaneStatus::Complete => {}
            LaneStatus::Pending => { self.phase = Phase::Armed; return outcome(joined); }
            other => return outcome(other),
        }
        self.finish_selection_clear(record, env);
        if self.selected.is_empty() {
            self.phase = Phase::Idle;
            return ChildOutcome::Complete;
        }
        self.phase = Phase::Armed;
        match joined {
            LaneStatus::Updated(update) => ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart: Restart::Complete }),
            _ => ChildOutcome::Complete,
        }
    }

    fn reset_selection(&mut self, record: RecordHandle, env: &mut impl Environment) {
        let available = env.available(record).unwrap_or_default();
        let permitted = env.search_permitted();
        self.selected = Selection::ALL.shared(available, permitted);
        self.planner.release(Slot::First);
        self.planner.release(Slot::Second);
        env.record_effect(record, RecordEffect::ResetSelection);
    }

    fn finish_selection_clear(&mut self, record: RecordHandle, env: &mut impl Environment) {
        if let Some(client) = self.pending_clear.take() { env.record_effect(record, RecordEffect::FinishSelectionClear(client)); }
    }

    fn fan_initialize(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan != Fan::Ready { env.fault(); return LaneStatus::Refused; }
        for lane in ORDER {
            let signal = if self.selected.contains(lane) { LaneSignal::Enable } else { LaneSignal::Initialize };
            if env.signal(lane, signal, record) != LaneStatus::Complete { return LaneStatus::Refused; }
        }
        if self.selected.is_empty() { LaneStatus::Complete } else { LaneStatus::Pending }
    }

    fn fan_stop(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan != Fan::Ready { env.fault(); return LaneStatus::Refused; }
        self.draining = Lanes::all();
        if let Err(refused) = self.drain(ORDER, record, env) { return refused; }
        if self.draining.is_empty() { return LaneStatus::Complete; }
        self.fan = Fan::Stopping;
        LaneStatus::Pending
    }

    fn fan_stop_changes(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        match self.fan {
            Fan::Ready => {}
            Fan::Draining => return LaneStatus::Pending,
            Fan::Stopping => { env.fault(); return LaneStatus::Refused; }
        }
        if self.draining.is_empty() { return LaneStatus::Complete; }
        if let Err(refused) = self.drain(ORDER, record, env) { return refused; }
        if self.draining.is_empty() { return LaneStatus::Complete; }
        self.fan = Fan::Draining;
        LaneStatus::Pending
    }

    fn drain(&mut self, lanes: [Lane; 7], record: RecordHandle, env: &mut impl Environment) -> Result<(), LaneStatus> {
        for lane in lanes {
            if !self.draining.contains(lane) { continue; }
            match env.signal(lane, LaneSignal::Stop, record) {
                LaneStatus::Complete => self.draining.remove(lane),
                LaneStatus::Pending => {}
                _ => return Err(LaneStatus::Refused),
            }
        }
        Ok(())
    }

    fn fan_enable_changes(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan != Fan::Ready { env.fault(); return LaneStatus::Refused; }
        if self.enabling.is_empty() { return LaneStatus::Complete; }
        for lane in ORDER {
            let deferred = matches!(lane, Lane::Search | Lane::Image) && !self.search_allowed;
            if !self.enabling.contains(lane) || deferred { continue; }
            if env.signal(lane, LaneSignal::Enable, record) != LaneStatus::Complete { return LaneStatus::Refused; }
            self.enabling.remove(lane);
        }
        if self.enabling.is_empty() { LaneStatus::Complete } else { LaneStatus::Pending }
    }

    fn fan_suspend(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan != Fan::Ready { env.fault(); return LaneStatus::Refused; }
        let signal = if env.alternate_search(record) { LaneSignal::Suspend } else { LaneSignal::Stop };
        env.signal(Lane::Search, signal, record)
    }

    fn fan_resume(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan != Fan::Ready { env.fault(); return LaneStatus::Refused; }
        let signal = if env.alternate_search(record) {
            LaneSignal::Resume
        } else if self.selected.contains(Lane::Search) {
            LaneSignal::Enable
        } else {
            LaneSignal::Initialize
        };
        env.signal(Lane::Search, signal, record)
    }

    fn fan_detect_changes(&mut self, requested: Selection, offered: Selection, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan != Fan::Ready { env.fault(); return LaneStatus::Refused; }
        let permitted = env.search_permitted();
        let enable = requested.shared(offered, permitted).difference(self.selected);
        let mut drain = requested.missing(offered);
        if env.available(record).is_none() { drain.insert(Lane::Bookkeeping); }
        if enable.is_empty() && drain.is_empty() { return LaneStatus::Complete; }
        for lane in ORDER {
            if enable.contains(lane) { self.enabling.insert(lane); } else if drain.contains(lane) { self.draining.insert(lane); }
        }
        LaneStatus::Idle
    }

    fn fan_forward(&mut self, command: Command<'_>, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan == Fan::Stopping { env.fault(); return LaneStatus::Refused; }
        env.signal(Lane::Image, LaneSignal::Command(command), record)
    }

    /// Fills both work slots until each is busy or has nothing left this cycle.
    fn fan_run(&mut self, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        if self.fan == Fan::Stopping { env.fault(); return LaneStatus::Refused; }
        loop {
            let mode = env.planning_mode(record);
            let enabled = self.selected;
            let plan = self.planner.next(mode, enabled, &mut Cadences(env));
            env.record_effect(record, RecordEffect::Planned { primary_considered: plan.primary_considered });
            let first = self.dispatch(Slot::First, plan.first, record, env);
            let second = self.dispatch(Slot::Second, plan.second, record, env);
            if let (Some(LaneStatus::Pending) | None, Some(LaneStatus::Pending) | None) = (first, second) {
                return if first.is_none() && second.is_none() && !self.planner.busy(Slot::First) && !self.planner.busy(Slot::Second) {
                    LaneStatus::Complete
                } else {
                    LaneStatus::Pending
                };
            }
        }
    }

    /// `None` when the slot is busy or has no work; otherwise the lane's answer.
    fn dispatch(&mut self, slot: Slot, work: Option<Work>, record: RecordHandle, env: &mut impl Environment) -> Option<LaneStatus> {
        if self.planner.busy(slot) { return None; }
        let work = work?;
        let status = env.signal(work.lane(), LaneSignal::Work(work), record);
        if status == LaneStatus::Pending { self.planner.submitted(slot, work); }
        Some(status)
    }

    fn fan_event(&mut self, signal: LaneSignal<'_>, record: RecordHandle, env: &mut impl Environment) -> LaneStatus {
        let Some((lane, slot)) = route(signal) else { return LaneStatus::Unhandled };
        let status = env.signal(lane, signal, record);
        if let Some(slot) = slot { self.planner.release(slot); }
        match self.fan {
            Fan::Ready => if status == LaneStatus::Idle { LaneStatus::Pending } else { status },
            Fan::Draining | Fan::Stopping if status == LaneStatus::Complete => {
                self.draining.remove(lane);
                if self.draining.is_empty() { self.fan = Fan::Ready; LaneStatus::Complete } else { LaneStatus::Pending }
            }
            Fan::Draining if lane != Lane::Acquisition && status != LaneStatus::Idle => status,
            Fan::Draining | Fan::Stopping => LaneStatus::Pending,
        }
    }
}

/// The lane a forwarded event belongs to, and the work slot its completion frees.
fn route(signal: LaneSignal<'_>) -> Option<(Lane, Option<Slot>)> {
    let LaneSignal::Completed(completed) = signal else { return None };
    Some(match completed.completion {
        Completion::Calculation87 | Completion::Calculation88 => (Lane::Acquisition, Some(Slot::Second)),
        Completion::Calculation8a => (Lane::Auxiliary, Some(Slot::Second)),
        Completion::Calculation89 => (Lane::Search, Some(Slot::First)),
        Completion::Calculation8b => (Lane::Image, Some(Slot::First)),
        Completion::Calculation8d => (Lane::Window, Some(Slot::First)),
        Completion::Calculation9a => (Lane::Source, Some(Slot::First)),
        Completion::Image46 => (Lane::Search, None),
        Completion::Image47 | Completion::Image66 => (Lane::Image, None),
        _ => return None,
    })
}

fn fault(env: &mut impl Environment) -> ChildOutcome {
    env.fault();
    ChildOutcome::Fault
}

fn outcome(status: LaneStatus) -> ChildOutcome {
    match status {
        LaneStatus::Refused => ChildOutcome::Fault,
        LaneStatus::Complete => ChildOutcome::Complete,
        LaneStatus::Pending => ChildOutcome::Pending,
        LaneStatus::Unhandled => ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)),
        LaneStatus::Updated(update) => ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart: Restart::Pending }),
        LaneStatus::Idle => ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status0c)),
    }
}
