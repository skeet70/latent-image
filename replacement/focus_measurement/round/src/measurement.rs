#![forbid(unsafe_code)]

use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl, MeasurementOutcome, MeasurementUpdate, Restart, RetainedRoundStatus};
use xm1_saclac_interface::{CommandKind, Completion, RecordHandle};
use crate::scheduling::{Lane, Lanes};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoundControl { Initialize, Stop, EnableChanges, StopChanges, Suspend, Resume, DetectChanges, Run }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoundOutcome { Complete, Pending, NoWork, Updated(MeasurementUpdate), Unhandled, Fault }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordEffect { Initialize, PublishMissing, Geometry, CollectReady, SelectionHistory }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchNotification { Window, WindowAlternate, Image }

pub trait Ports {
    fn refresh_selection(&mut self, record: RecordHandle);
    fn round_control(&mut self, record: RecordHandle, control: RoundControl) -> RoundOutcome;
    fn round_event(&mut self, event: ChildEvent<'_, MeasurementControl>) -> RoundOutcome;
    fn record_effect(&mut self, record: RecordHandle, effect: RecordEffect);
    fn enabled(&self) -> Lanes;
    fn enabling(&self) -> Lanes;
    fn draining(&self) -> Lanes;
    fn allow_search(&mut self, allowed: bool);
    fn search_is_alternate(&self, record: RecordHandle) -> bool;
    fn geometry_changed(&self, record: RecordHandle) -> bool;
    fn notify_search(&mut self, record: RecordHandle, notification: SearchNotification);
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State { Dormant, Idle, Measuring, Restarting, Stopping }

pub struct Measurement { state: State, publication_pending: bool }
impl Default for Measurement { fn default() -> Self { Self::new() } }

impl Measurement {
    pub const fn new() -> Self { Self { state: State::Dormant, publication_pending: false } }

    pub fn handle(&mut self, event: ChildEvent<'_, MeasurementControl>, ports: &mut impl Ports) -> ChildOutcome {
        let record = match event {
            ChildEvent::Control { record, .. } | ChildEvent::Command { record, .. } | ChildEvent::Completed { record, .. } => record,
        };
        match event {
            ChildEvent::Control { control, .. } => self.control(record, control, ports),
            ChildEvent::Command { command, .. } if command.kind == CommandKind::Command17 => self.parameters_changed(record, ports),
            ChildEvent::Command { command, .. } if command.kind == CommandKind::Autofocus => {
                if self.state == State::Stopping { return self.fail(ports); }
                if ports.geometry_changed(record) { ports.record_effect(record, RecordEffect::Geometry); }
                ChildOutcome::Complete
            }
            ChildEvent::Command { command, .. } if matches!(command.kind, CommandKind::Command14 | CommandKind::Command15 | CommandKind::Command16 | CommandKind::Command2b) => {
                if !self.running() { return self.fail(ports); }
                Self::outcome(ports.round_event(event))
            }
            ChildEvent::Completed { completed, .. } => match completed.completion {
                Completion::Image45 if self.running() => { ports.allow_search(true); ChildOutcome::Pending }
                Completion::Image45 => ChildOutcome::Pending,
                Completion::Image46 | Completion::Image47 | Completion::Image66 if !matches!(self.state, State::Dormant | State::Idle) => self.lane_completed(record, event, ports),
                Completion::Image46 | Completion::Image47 | Completion::Image66 => ChildOutcome::Pending,
                Completion::Accessor4b if self.state == State::Measuring => {
                    ports.record_effect(record, RecordEffect::CollectReady);
                    if ports.round_control(record, RoundControl::Run) != RoundOutcome::Complete { self.state = State::Restarting; }
                    ChildOutcome::Pending
                }
                Completion::Accessor4b if self.state == State::Restarting => {
                    ports.record_effect(record, RecordEffect::SelectionHistory);
                    ChildOutcome::Pending
                }
                Completion::Accessor4b => ChildOutcome::Pending,
                _ => self.completed(record, event, ports),
            },
            _ => self.completed(record, event, ports),
        }
    }

    fn running(&self) -> bool { matches!(self.state, State::Measuring | State::Restarting) }

    fn control(&mut self, record: RecordHandle, control: MeasurementControl, ports: &mut impl Ports) -> ChildOutcome {
        if self.state == State::Stopping { return self.fail(ports); }
        match control {
            MeasurementControl::Initialize => match self.state {
                State::Dormant => self.initialize(record, ports),
                State::Idle => ChildOutcome::Complete,
                _ => self.fail(ports),
            },
            MeasurementControl::Stop => match self.state {
                State::Dormant => ChildOutcome::Complete,
                State::Idle => { self.state = State::Dormant; ChildOutcome::Complete }
                _ => {
                    let outcome = ports.round_control(record, RoundControl::Stop);
                    match outcome {
                        RoundOutcome::Complete => self.state = State::Dormant,
                        RoundOutcome::Pending => self.state = State::Stopping,
                        _ => return Self::outcome(outcome),
                    }
                    Self::outcome(outcome)
                }
            },
            MeasurementControl::Reconfigure => { ports.record_effect(record, RecordEffect::Geometry); ChildOutcome::Pending }
            MeasurementControl::Suspend | MeasurementControl::Resume if self.running() => {
                Self::outcome(ports.round_control(record, if control == MeasurementControl::Suspend { RoundControl::Suspend } else { RoundControl::Resume }))
            }
            MeasurementControl::Suspend | MeasurementControl::Resume => ChildOutcome::Complete,
        }
    }

    fn initialize(&mut self, record: RecordHandle, ports: &mut impl Ports) -> ChildOutcome {
        ports.refresh_selection(record);
        ports.record_effect(record, RecordEffect::Initialize);
        let outcome = ports.round_control(record, RoundControl::Initialize);
        self.state = match outcome {
            RoundOutcome::Complete => State::Idle,
            RoundOutcome::Pending => State::Measuring,
            _ => return Self::outcome(outcome),
        };
        ChildOutcome::Complete
    }

    fn parameters_changed(&mut self, record: RecordHandle, ports: &mut impl Ports) -> ChildOutcome {
        if self.state == State::Stopping { return self.fail(ports); }
        self.publication_pending = true;
        match self.state {
            State::Dormant => { self.publish_missing(record, ports); ChildOutcome::Complete }
            State::Idle => {
                let outcome = self.initialize(record, ports);
                if outcome == ChildOutcome::Complete { self.publish_missing(record, ports); }
                outcome
            }
            _ => {
                ports.allow_search(false);
                let outcome = ports.round_control(record, RoundControl::DetectChanges);
                if outcome != RoundOutcome::NoWork {
                    self.publish_missing(record, ports);
                    return ChildOutcome::Complete;
                }
                if ports.enabling().contains(Lane::Search) {
                    let notification = if ports.search_is_alternate(record) { SearchNotification::WindowAlternate } else { SearchNotification::Window };
                    ports.notify_search(record, notification);
                } else if ports.enabling().contains(Lane::Image) {
                    ports.notify_search(record, SearchNotification::Image);
                }
                ChildOutcome::Pending
            }
        }
    }

    fn lane_completed(&mut self, record: RecordHandle, event: ChildEvent<'_, MeasurementControl>, ports: &mut impl Ports) -> ChildOutcome {
        let outcome = ports.round_event(event);
        if self.state == State::Stopping {
            if outcome == RoundOutcome::Complete { self.state = State::Dormant; }
            return if outcome == RoundOutcome::Complete { ChildOutcome::Complete } else { ChildOutcome::Pending };
        }
        if self.state != State::Measuring {
            if outcome == RoundOutcome::Complete && ports.enabling().is_empty() { self.publish_missing(record, ports); return ChildOutcome::Complete; }
            return ChildOutcome::Pending;
        }
        if outcome != RoundOutcome::Complete { return Self::outcome(outcome); }
        if !ports.draining().is_empty() { return ChildOutcome::Complete; }
        if ports.enabled().is_empty() {
            self.state = State::Idle;
            self.publish_missing(record, ports);
        } else if ports.enabling().is_empty() { self.publish_missing(record, ports); }
        ChildOutcome::Complete
    }

    fn completed(&mut self, record: RecordHandle, event: ChildEvent<'_, MeasurementControl>, ports: &mut impl Ports) -> ChildOutcome {
        if matches!(self.state, State::Dormant | State::Idle) { return ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)); }
        if self.state == State::Stopping {
            let outcome = ports.round_event(event);
            if outcome == RoundOutcome::Complete { self.state = State::Dormant; return ChildOutcome::Complete; }
            return ChildOutcome::Pending;
        }
        let outcome = ports.round_event(event);
        if self.state == State::Measuring { return Self::outcome(outcome); }
        if outcome == RoundOutcome::Fault { return ChildOutcome::Fault; }
        if self.state == State::Restarting && ports.round_control(record, RoundControl::Run) != RoundOutcome::Complete { return Self::outcome(outcome); }
        let mut restart = Restart::Pending;
        if !ports.enabling().is_empty() || !ports.draining().is_empty() {
            ports.refresh_selection(record);
            match ports.round_control(record, RoundControl::StopChanges) {
                RoundOutcome::Complete => match ports.round_control(record, RoundControl::EnableChanges) {
                    RoundOutcome::Complete => {
                        self.publish_missing(record, ports);
                        self.state = if ports.enabled().is_empty() { State::Idle } else { State::Measuring };
                        restart = Restart::Complete;
                    }
                    RoundOutcome::Pending => self.state = State::Measuring,
                    other => return Self::outcome(other),
                },
                RoundOutcome::Pending => self.state = State::Measuring,
                other => return Self::outcome(other),
            }
        } else { self.state = State::Measuring; }
        if restart == Restart::Complete {
            if self.state == State::Idle { return ChildOutcome::Complete; }
            match outcome {
                RoundOutcome::Updated(update) => ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart }),
                _ => ChildOutcome::Complete,
            }
        } else { Self::outcome(outcome) }
    }

    fn publish_missing(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        if self.publication_pending { ports.record_effect(record, RecordEffect::PublishMissing); self.publication_pending = false; }
    }

    fn fail(&mut self, ports: &mut impl Ports) -> ChildOutcome { ports.fault(); ChildOutcome::Fault }

    fn outcome(outcome: RoundOutcome) -> ChildOutcome {
        match outcome {
            RoundOutcome::Complete => ChildOutcome::Complete,
            RoundOutcome::Pending => ChildOutcome::Pending,
            RoundOutcome::NoWork => ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status0c)),
            RoundOutcome::Updated(update) => ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart: Restart::Pending }),
            RoundOutcome::Unhandled => ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)),
            RoundOutcome::Fault => ChildOutcome::Fault,
        }
    }
}
