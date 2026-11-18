#![forbid(unsafe_code)]

//! The live statistics configuration: starts, retunes, holds and stops the statistics
//! task's live measurement for the live view, and answers the profile commands the live view
//! forwards to it. Only the live view enters it.

use crate::children::MeasurementUpdate;
use crate::live_view::{Signal, StatisticsCall, StatisticsControl, StatisticsOutcome};
use xm1_saclac_interface::{CommandKind as K, Completion as C, Parameters, RecordHandle};

/// The command whose reply waits for the configuration to finish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Owed {
    /// Releasing the S1 capture finishes the capture's own command, Command09.
    Command0a,
    Command11,
    Command1d,
    Command1e,
    Command1f,
    Command20,
    Command21,
    Command22,
    /// The live view's capture release; finishing it exports the diagnostic buffers.
    Release,
}

/// A command answered at once with the acceptance or refusal status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Answered { Command0a, Command13 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reply<'a> {
    /// The owed command finished.
    Finished(Owed),
    Accepted(Answered),
    Refused(Answered),
    /// Command11 with the profile's descriptor; `selected` once the statistics task selected
    /// a new one.
    Descriptor { selected: bool },
    /// Prepare with the profile's selection flags.
    Prepared,
    /// Command09: the capture configuration is in place.
    CaptureConfigured,
    /// Command09 with the capture's statistics: the completion's payload and the results.
    CaptureResults(Parameters<'a>),
    /// Command24 with the completion's status and value, or refused while a selection is set.
    Command24 { refused: bool, completion: Parameters<'a> },
}

/// The measurement block the change-group and resume/suspend requests carry differs in one
/// word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureWord { Retune, Resume, Suspend }

/// The requests the configuration posts to the statistics task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    Start { selected: bool },
    /// Configure the statistics for the S1 capture.
    Configure,
    Stop { pause: bool, shot_idle: bool },
    Release,
    MeasuredWindow,
    MeasuredSource,
    /// Command11's adjustment.
    Adjust,
    Command13,
    Prepare,
    Output,
    Window,
    Format,
    Exposure,
    Capture(CaptureWord),
    /// `refresh` sends the profile's pending selection; otherwise the request carries none.
    Selection { refresh: bool },
    Command24,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Condition {
    /// The profile's view mode needs the statistics started.
    StartNeeded,
    Selected,
    /// The live view last applied the primary profile.
    PrimaryApplied,
    /// Command22's second word is zero.
    Command22Cleared,
}

/// The parameter-change groups an autofocus change touched.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Groups { pub processing: bool, pub geometry: bool, pub measurement: bool }

impl Groups {
    /// The group the first request covers.
    const fn first(self) -> Option<Group> {
        if self.measurement {
            Some(Group::Measurement)
        } else if self.geometry {
            Some(Group::Geometry)
        } else if self.processing {
            Some(Group::Processing)
        } else {
            None
        }
    }

    /// Clears `done` and names the group the next request covers.
    fn finish(&mut self, done: Option<Group>) -> Option<Group> {
        match done {
            Some(Group::Processing) => self.processing = false,
            Some(Group::Geometry) => self.geometry = false,
            Some(Group::Measurement) => self.measurement = false,
            None => {}
        }
        if self.processing {
            Some(Group::Processing)
        } else if self.geometry {
            Some(Group::Geometry)
        } else if self.measurement {
            Some(Group::Measurement)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Group { Processing, Geometry, Measurement }

impl Group {
    const fn request(self) -> Request {
        match self {
            Self::Processing => Request::Exposure,
            Self::Geometry => Request::Format,
            Self::Measurement => Request::Capture(CaptureWord::Retune),
        }
    }
}

pub trait Port {
    fn request(&mut self, request: Request, record: RecordHandle);
    fn reply(&mut self, reply: Reply<'_>, record: RecordHandle);
    fn condition(&mut self, condition: Condition, record: RecordHandle) -> bool;
    /// The groups the last autofocus change touched.
    fn changes(&mut self, record: RecordHandle) -> Groups;
    /// Hands the statistics task's results to the callback the profile names.
    fn deliver_results(&mut self, record: RecordHandle);
    /// Selects the descriptor the statistics task chose.
    fn select_descriptor(&mut self, descriptor: u32, record: RecordHandle);
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    /// Reconfiguring from idle; returns to idle.
    IdleReconfiguring,
    Paused,
    Starting,
    Running,
    RunningSelected,
    /// Held for the S1 capture.
    Captured,
    Adjusting,
    CapturedAdjusting,
    Reconfiguring,
    /// Releasing the capture back to running.
    Resuming,
    /// Stopping into the pause.
    Pausing,
    Stopping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event<'a> {
    Control(StatisticsControl),
    Command(K),
    Completed(C, Parameters<'a>),
    Other,
}

impl<'a> Event<'a> {
    fn of(call: StatisticsCall<'a>) -> Self {
        match call {
            StatisticsCall::Control(control) => Self::Control(control),
            StatisticsCall::Reissue(kind) => Self::Command(kind),
            StatisticsCall::Forward(Signal::Command(command)) => Self::Command(command.kind),
            StatisticsCall::Forward(Signal::Completed(completed)) => Self::Completed(completed.completion, completed.parameters),
            StatisticsCall::Forward(Signal::Suspend) => Self::Control(StatisticsControl::Suspend),
            StatisticsCall::Forward(Signal::Resume) => Self::Control(StatisticsControl::Resume),
            StatisticsCall::Forward(Signal::ResumeLive | Signal::SuspendLive | Signal::RefreshDeferred | Signal::Cancel) => Self::Other,
        }
    }

    const fn owed(self) -> Option<Owed> {
        match self {
            Self::Command(K::Command0a) => Some(Owed::Command0a),
            Self::Command(K::Command11) => Some(Owed::Command11),
            Self::Command(K::Command1d) => Some(Owed::Command1d),
            Self::Command(K::Command1e) => Some(Owed::Command1e),
            Self::Command(K::Command1f) => Some(Owed::Command1f),
            Self::Command(K::Command20) => Some(Owed::Command20),
            Self::Command(K::Command21) => Some(Owed::Command21),
            Self::Command(K::Command22) => Some(Owed::Command22),
            Self::Control(StatisticsControl::Release) => Some(Owed::Release),
            _ => None,
        }
    }

    /// Events the configuration has no row for.
    const fn unknown(self) -> bool {
        match self {
            Self::Control(_) => false,
            Self::Command(kind) => !matches!(
                kind,
                K::Command02 | K::Command03 | K::Command0a | K::Command11 | K::Command13 | K::Prepare | K::Command1d
                    | K::Command1e | K::Command1f | K::Command20 | K::Command21 | K::Command22 | K::Autofocus
                    | K::Command24
            ),
            Self::Completed(completion, _) => !matches!(
                completion,
                C::Statistics69 | C::Statistics6b | C::Statistics6c | C::Statistics6d | C::Statistics6e
                    | C::Statistics6f | C::Statistics70 | C::Statistics71 | C::Statistics72 | C::Statistics73
                    | C::Statistics80 | C::Statistics75 | C::Statistics76 | C::Statistics77 | C::Statistics78
                    | C::Statistics79 | C::Statistics7e | C::Statistics7f | C::Statistics82
            ),
            Self::Other => true,
        }
    }

    /// Unhandled events that raise the fault reaction; completions and the resume and suspend
    /// controls return the fault result without it.
    const fn faults(self) -> bool {
        !matches!(
            self,
            Self::Completed(..) | Self::Control(StatisticsControl::Resume | StatisticsControl::Suspend)
        )
    }
}

fn word(parameters: Parameters<'_>, index: usize) -> u32 {
    parameters.bytes().get(index * 4..index * 4 + 4).map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

struct Call<'p, P> { port: &'p mut P, record: RecordHandle }

impl<P: Port> Call<'_, P> {
    fn request(&mut self, request: Request) { self.port.request(request, self.record) }
    fn reply(&mut self, reply: Reply<'_>) { self.port.reply(reply, self.record) }
    fn condition(&mut self, condition: Condition) -> bool { self.port.condition(condition, self.record) }
}

pub struct LiveStatistics {
    state: State,
    owed: Option<Owed>,
    groups: Groups,
    /// The last configuration was the S1 capture's.
    captured: bool,
    shot_idle: bool,
}

impl Default for LiveStatistics { fn default() -> Self { Self::new() } }

impl LiveStatistics {
    pub const fn new() -> Self {
        Self { state: State::Idle, owed: None, groups: Groups { processing: false, geometry: false, measurement: false }, captured: false, shot_idle: false }
    }

    /// The S1 router's shot state, which a stop reports to the statistics task.
    pub fn set_shot_idle(&mut self, idle: bool) { self.shot_idle = idle; }

    pub fn send(&mut self, call: StatisticsCall<'_>, record: RecordHandle, port: &mut impl Port) -> StatisticsOutcome {
        let event = Event::of(call);
        let mut call = Call { port, record };
        if event.unknown() {
            return StatisticsOutcome::Unhandled;
        }
        match self.handle(event, &mut call) {
            Some(outcome) => outcome,
            None => {
                if event.faults() {
                    call.port.fault();
                }
                StatisticsOutcome::Fault
            }
        }
    }

    fn owe(&mut self, event: Event<'_>) { self.owed = event.owed(); }

    fn handle<P: Port>(&mut self, event: Event<'_>, call: &mut Call<'_, P>) -> Option<StatisticsOutcome> {
        use State as S;
        use StatisticsControl as Ctl;
        use StatisticsOutcome as O;
        let outcome = match (self.state, event) {
            (S::Idle, Event::Control(Ctl::BindRunning)) => {
                self.owed = None;
                self.groups = Groups { processing: false, geometry: false, measurement: false };
                self.captured = false;
                self.shot_idle = true;
                self.state = if call.condition(Condition::Selected) { S::RunningSelected } else { S::Running };
                O::Complete
            }
            (S::Idle, Event::Control(Ctl::Begin)) => {
                self.owe(event);
                if call.condition(Condition::StartNeeded) && call.condition(Condition::PrimaryApplied) {
                    self.start(call)
                } else {
                    O::Complete
                }
            }
            (S::Paused, Event::Control(Ctl::Begin)) | (S::Idle | S::Paused, Event::Command(K::Command02)) => {
                self.owe(event);
                if call.condition(Condition::StartNeeded) { self.start(call) } else { O::Complete }
            }
            (S::Idle | S::Paused | S::RunningSelected, Event::Control(Ctl::Capture)) => {
                self.owe(event);
                self.captured = true;
                call.request(Request::Configure);
                self.state = S::Starting;
                O::Pending
            }
            (S::Running, Event::Control(Ctl::Capture)) => O::Ready,
            (S::Idle | S::Paused, Event::Control(Ctl::Stop)) | (S::Idle, Event::Control(Ctl::Pause)) => O::Complete,
            (S::Running | S::RunningSelected, Event::Control(Ctl::Stop)) => self.stop(event, false, S::Stopping, call),
            (S::Paused | S::Running | S::RunningSelected, Event::Control(Ctl::Pause))
            | (S::Paused | S::Running | S::RunningSelected, Event::Command(K::Command03)) => self.stop(event, true, S::Stopping, call),
            (S::Running | S::RunningSelected, Event::Command(K::Command22)) => self.stop(event, false, S::Pausing, call),
            (S::Idle | S::Paused | S::RunningSelected, Event::Control(Ctl::Release) | Event::Command(K::Command0a)) => {
                call.reply(Reply::Refused(Answered::Command0a));
                O::Complete
            }
            (S::Captured, Event::Control(Ctl::Release) | Event::Command(K::Command0a)) => {
                self.owe(event);
                self.captured = false;
                call.request(Request::Release);
                call.reply(Reply::Accepted(Answered::Command0a));
                self.state = if event == Event::Command(K::Command0a) { S::Resuming } else { S::Pausing };
                O::Pending
            }
            (state, Event::Control(Ctl::Measured(update))) => {
                let listening = matches!(state, S::Idle) || state.running();
                match update {
                    MeasurementUpdate::Window if listening => call.request(Request::MeasuredWindow),
                    MeasurementUpdate::Source if listening => call.request(Request::MeasuredSource),
                    _ => {}
                }
                O::Pending
            }
            (S::Running | S::RunningSelected, Event::Control(Ctl::Autofocus)) => {
                self.owe(event);
                call.request(Request::Selection { refresh: false });
                self.state = S::Reconfiguring;
                O::Pending
            }
            (S::Idle, Event::Command(K::Command03 | K::Command1d | K::Command1e | K::Command1f | K::Command20 | K::Command21 | K::Command22))
            | (S::Running | S::RunningSelected, Event::Command(K::Command20)) => {
                self.owe(event);
                self.finish(call);
                O::Complete
            }
            (S::Idle, Event::Command(K::Command11)) => {
                self.owe(event);
                self.finish(call);
                O::Reconfigure
            }
            (S::Running | S::RunningSelected | S::Captured, Event::Command(K::Command11)) => {
                self.owe(event);
                call.request(Request::Adjust);
                self.state = if self.state == S::Captured { S::CapturedAdjusting } else { S::Adjusting };
                O::Pending
            }
            (S::Idle | S::Paused | S::Running | S::RunningSelected | S::Captured, Event::Command(K::Command13)) => {
                call.reply(Reply::Refused(Answered::Command13));
                O::Complete
            }
            (S::Adjusting | S::CapturedAdjusting, Event::Command(K::Command13)) => {
                call.request(Request::Command13);
                call.reply(Reply::Accepted(Answered::Command13));
                O::Pending
            }
            (
                S::Idle | S::IdleReconfiguring | S::Paused | S::Starting | S::Running | S::RunningSelected | S::Captured,
                Event::Command(K::Prepare),
            ) => {
                call.request(Request::Prepare);
                O::Complete
            }
            (S::Running | S::RunningSelected, Event::Command(K::Command1d)) => {
                self.state = S::Reconfiguring;
                self.owe(event);
                if call.condition(Condition::StartNeeded) {
                    call.request(Request::Output);
                } else {
                    self.stop_request(true, call);
                }
                O::Pending
            }
            (S::Running | S::RunningSelected, Event::Command(K::Command1e)) => self.reconfigure(event, Request::Output, call),
            (S::Running, Event::Command(K::Command1f)) => self.reconfigure(event, Request::Window, call),
            (S::Running | S::RunningSelected, Event::Command(K::Command21)) => self.reconfigure(event, Request::Format, call),
            (S::Paused, Event::Command(K::Command22)) => {
                self.owe(event);
                if call.condition(Condition::Command22Cleared) {
                    self.start(call)
                } else {
                    self.finish(call);
                    O::Complete
                }
            }
            (S::Idle | S::Paused | S::Running | S::RunningSelected | S::Captured, Event::Command(K::Autofocus)) => {
                self.owe(event);
                self.groups = call.port.changes(call.record);
                match (self.state, self.groups.first()) {
                    (S::Idle, Some(Group::Measurement)) => {
                        self.state = S::IdleReconfiguring;
                        call.request(Group::Measurement.request());
                        O::Pending
                    }
                    (S::Paused, Some(group @ Group::Measurement)) | (S::Running | S::RunningSelected | S::Captured, Some(group)) => {
                        self.state = S::Reconfiguring;
                        call.request(group.request());
                        O::Pending
                    }
                    _ => O::Complete,
                }
            }
            (S::Idle | S::Running | S::RunningSelected | S::Captured, Event::Control(Ctl::Refresh)) => {
                self.owe(event);
                call.request(Request::Selection { refresh: true });
                self.state = if self.state == S::Idle { S::IdleReconfiguring } else { S::Reconfiguring };
                O::Pending
            }
            (S::Running, Event::Control(control @ (Ctl::Resume | Ctl::Suspend))) => {
                let word = if control == Ctl::Resume { CaptureWord::Resume } else { CaptureWord::Suspend };
                call.request(Request::Capture(word));
                self.state = S::Reconfiguring;
                O::Pending
            }
            (S::Idle | S::Starting | S::Adjusting | S::Stopping | S::Running, Event::Command(K::Command24)) => {
                call.request(Request::Command24);
                if self.state == S::Running { self.state = S::Reconfiguring; }
                O::Pending
            }
            (state, Event::Completed(completion, parameters)) => return self.completed(state, completion, parameters, call),
            _ => return None,
        };
        Some(outcome)
    }

    fn completed<P: Port>(&mut self, state: State, completion: C, parameters: Parameters<'_>, call: &mut Call<'_, P>) -> Option<StatisticsOutcome> {
        use State as S;
        use StatisticsOutcome as O;
        let outcome = match (state, completion) {
            (S::Starting, C::Statistics69) => self.finished(S::Running, call),
            (S::Starting, C::Statistics6c) => { self.state = S::Captured; O::Complete }
            (S::Reconfiguring | S::Stopping, C::Statistics6b) => self.finished(S::Idle, call),
            (S::Pausing, C::Statistics6b | C::Statistics6d) => self.finished(S::Paused, call),
            (S::Resuming, C::Statistics6d) => self.finished(S::RunningSelected, call),
            (S::IdleReconfiguring, C::Statistics6f | C::Statistics70 | C::Statistics80 | C::Statistics7e) => self.finished(S::Idle, call),
            (S::Adjusting, C::Statistics6e) | (S::Reconfiguring, C::Statistics6f | C::Statistics70 | C::Statistics75) => {
                self.settle(false, call);
                self.finish(call);
                O::Complete
            }
            (S::CapturedAdjusting, C::Statistics6e) => {
                self.state = S::Captured;
                call.reply(Reply::Descriptor { selected: false });
                O::Complete
            }
            (S::Reconfiguring, C::Statistics71) => self.next_group(Some(Group::Geometry), call),
            (S::Reconfiguring, C::Statistics72) => self.next_group(None, call),
            (S::Reconfiguring, C::Statistics73) => self.next_group(Some(Group::Processing), call),
            (S::Reconfiguring, C::Statistics80) => self.next_group(Some(Group::Measurement), call),
            (S::Adjusting | S::CapturedAdjusting, C::Statistics76) => {
                call.port.select_descriptor(word(parameters, 1), call.record);
                call.reply(Reply::Descriptor { selected: true });
                O::Reconfigure
            }
            (S::Idle | S::Starting | S::Running | S::RunningSelected | S::Captured, C::Statistics77) => {
                call.reply(Reply::Prepared);
                O::Pending
            }
            (_, C::Statistics77) => O::Pending,
            (S::Starting, C::Statistics78) => {
                call.reply(Reply::CaptureConfigured);
                O::Pending
            }
            (S::Starting | S::Captured | S::CapturedAdjusting, C::Statistics79) => {
                call.reply(Reply::CaptureResults(parameters));
                O::Pending
            }
            (S::Resuming | S::Pausing, C::Statistics79) => O::Pending,
            (S::Reconfiguring, C::Statistics7e) => {
                self.settle(self.captured, call);
                self.finish(call);
                O::Complete
            }
            (
                S::Idle | S::Running | S::RunningSelected | S::Captured | S::Adjusting | S::Reconfiguring | S::Pausing
                | S::Stopping,
                C::Statistics7f,
            ) => {
                call.port.deliver_results(call.record);
                O::Pending
            }
            (S::Idle | S::Starting | S::Adjusting | S::Stopping, C::Statistics82) => {
                call.reply(Reply::Command24 { refused: false, completion: parameters });
                O::Pending
            }
            (S::Reconfiguring, C::Statistics82) => {
                let selected = self.settle(false, call);
                call.reply(Reply::Command24 { refused: selected, completion: parameters });
                O::Complete
            }
            _ => return None,
        };
        Some(outcome)
    }

    fn start<P: Port>(&mut self, call: &mut Call<'_, P>) -> StatisticsOutcome {
        self.state = State::Starting;
        let selected = call.condition(Condition::Selected);
        self.captured = false;
        self.shot_idle = true;
        call.request(Request::Start { selected });
        StatisticsOutcome::Pending
    }

    fn stop_request<P: Port>(&mut self, pause: bool, call: &mut Call<'_, P>) {
        self.captured = false;
        call.request(Request::Stop { pause, shot_idle: self.shot_idle });
    }

    fn stop<P: Port>(&mut self, event: Event<'_>, pause: bool, next: State, call: &mut Call<'_, P>) -> StatisticsOutcome {
        self.owe(event);
        self.stop_request(pause, call);
        self.state = next;
        StatisticsOutcome::Pending
    }

    fn reconfigure<P: Port>(&mut self, event: Event<'_>, request: Request, call: &mut Call<'_, P>) -> StatisticsOutcome {
        self.owe(event);
        call.request(request);
        self.state = State::Reconfiguring;
        StatisticsOutcome::Pending
    }

    fn finish<P: Port>(&mut self, call: &mut Call<'_, P>) {
        if let Some(owed) = self.owed {
            call.reply(Reply::Finished(owed));
        }
    }

    fn finished<P: Port>(&mut self, next: State, call: &mut Call<'_, P>) -> StatisticsOutcome {
        self.state = next;
        self.finish(call);
        StatisticsOutcome::Complete
    }

    /// Settles into running by the profile's selection; a selected capture configuration
    /// stays held when `held`. Returns the selection.
    fn settle<P: Port>(&mut self, held: bool, call: &mut Call<'_, P>) -> bool {
        let selected = call.condition(Condition::Selected);
        self.state = match (selected, held) {
            (false, _) => State::Running,
            (true, true) => State::Captured,
            (true, false) => State::RunningSelected,
        };
        selected
    }

    fn next_group<P: Port>(&mut self, done: Option<Group>, call: &mut Call<'_, P>) -> StatisticsOutcome {
        match self.groups.finish(done) {
            Some(group) => {
                call.request(group.request());
                StatisticsOutcome::Pending
            }
            None => {
                self.settle(false, call);
                self.finish(call);
                StatisticsOutcome::Complete
            }
        }
    }
}

impl State {
    /// States in which measurement updates reach the statistics task.
    const fn running(self) -> bool {
        matches!(
            self,
            Self::Running | Self::RunningSelected | Self::Captured | Self::Adjusting | Self::CapturedAdjusting | Self::Reconfiguring
                | Self::Resuming
        )
    }
}
