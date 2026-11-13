#![forbid(unsafe_code)]

use crate::jobs::{Calculations, Primary, Return, Secondary};
use crate::outcome::{fault, Outcome};
use crate::ports::{CaptureEffect as E, CaptureQuery as Q, Effect, Ports, Query, Reply, Signal};
use xm1_saclac_interface::{CommandKind, Completion, CompletionStatus, RecordHandle};

pub const IDLE_COMMAND_STATUS: CompletionStatus = CompletionStatus(21);
pub const JOB_STARTED_STATUS: CompletionStatus = CompletionStatus(7);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State { Idle, AwaitingSetup, Returning, Secondary, Primary, Setting }

pub struct Capture {
    state: State,
    record: Option<RecordHandle>,
    lens_return: Return,
    secondary: Secondary,
    primary: Primary,
    calculations: Calculations,
}

impl Default for Capture { fn default() -> Self { Self::new() } }

impl Capture {
    pub const fn new() -> Self {
        Self { state: State::Idle, record: None, lens_return: Return::new(), secondary: Secondary::new(), primary: Primary::new(), calculations: Calculations::new() }
    }

    pub fn start(&mut self, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        if self.state != State::Idle { return fault(ports); }
        self.bind(record, ports);
        ports.effect(Effect::Capture(E::RequestStatisticsSetup));
        self.state = State::AwaitingSetup;
        Outcome::Pending
    }

    pub fn command(&mut self, command: CommandKind, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match (self.state, command) {
            (State::Idle, CommandKind::Command0c | CommandKind::Command28) => {
                ports.reply(Reply { command, record: None, status: IDLE_COMMAND_STATUS });
                Outcome::Pending
            }
            (State::Returning | State::Secondary, CommandKind::Command0c | CommandKind::Command28) => {
                self.primary.command(command, record, ports)
            }
            (State::Primary, CommandKind::Command0c | CommandKind::Command28) => {
                let outcome = self.primary.command(command, record, ports);
                if outcome == Outcome::Complete { self.conclude(ports); }
                outcome
            }
            (State::Idle, CommandKind::Command08) => {
                self.bind(record, ports);
                ports.effect(Effect::Capture(E::PrepareSetting));
                ports.effect(Effect::Capture(E::RequestSetting));
                self.state = State::Setting;
                Outcome::Pending
            }
            (_, CommandKind::Command0c | CommandKind::Command28 | CommandKind::Command08) => fault(ports),
            _ => self.other(Signal::Command(command), record, ports),
        }
    }

    pub fn completed(&mut self, completion: Completion, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match (self.state, completion) {
            (State::AwaitingSetup, Completion::Statistics83) => {
                if ports.query(Query::Capture(Q::LensReturn)) {
                    let outcome = self.lens_return.start(record, ports);
                    if outcome != Outcome::Complete {
                        self.state = State::Returning;
                        return outcome;
                    }
                }
                let outcome = self.select_job(record, ports);
                self.reply_job_started(ports);
                outcome
            }
            (State::Secondary | State::Primary, Completion::Calculation95) => self.secondary.signal(Signal::Completion(completion), record, ports),
            (State::Setting, Completion::Statistics85) => {
                ports.effect(Effect::Capture(E::CommitSetting));
                ports.reply(Reply { command: CommandKind::Command08, record: self.record, status: CompletionStatus(0) });
                self.state = State::Idle;
                Outcome::Complete
            }
            (_, Completion::Statistics83 | Completion::Calculation95 | Completion::Statistics85) => fault(ports),
            _ => self.other(Signal::Completion(completion), record, ports),
        }
    }

    fn other(&mut self, signal: Signal, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match self.state {
            State::Idle | State::AwaitingSetup => {
                if signal == Signal::Completion(Completion::Statistics7f) { Outcome::Pending } else { Outcome::Unhandled }
            }
            State::Returning => {
                let mut outcome = self.lens_return.signal(signal, ports);
                if outcome == Outcome::Complete { outcome = self.select_job(record, ports); }
                self.reply_job_started(ports);
                outcome
            }
            State::Secondary => {
                let outcome = self.secondary.signal(signal, record, ports);
                if outcome != Outcome::Complete { return outcome; }
                self.state = State::Primary;
                self.primary.start(record, ports)
            }
            State::Primary => match self.primary.signal(signal, record, &mut self.calculations, ports) {
                Outcome::Complete => {
                    self.state = State::Idle;
                    ports.effect(Effect::Capture(E::Finalize));
                    self.conclude(ports);
                    Outcome::Complete
                }
                Outcome::Fault | Outcome::Aborted => {
                    self.state = State::Idle;
                    ports.effect(Effect::Capture(E::Finalize));
                    Outcome::Complete
                }
                outcome => outcome,
            },
            State::Setting => Outcome::Unhandled,
        }
    }

    fn bind(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        self.record = Some(record);
        ports.effect(Effect::Capture(E::BindRecord(record)));
    }

    fn select_job(&mut self, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        ports.effect(Effect::Capture(E::PrepareRecord));
        ports.effect(Effect::Capture(E::PrepareJob));
        if ports.query(Query::Capture(Q::SecondaryJob)) {
            self.state = State::Secondary;
            ports.effect(Effect::Capture(E::ClearSecondary));
            self.secondary.start(record, ports)
        } else {
            self.state = State::Primary;
            self.primary.start(record, ports)
        }
    }

    fn reply_job_started(&mut self, ports: &mut impl Ports) {
        ports.reply(Reply { command: CommandKind::Command07, record: self.record, status: JOB_STARTED_STATUS });
    }

    fn conclude(&mut self, ports: &mut impl Ports) {
        self.state = State::Idle;
        ports.reply(Reply { command: CommandKind::Command07, record: self.record, status: CompletionStatus(0) });
        ports.effect(Effect::Capture(E::Release));
    }
}
