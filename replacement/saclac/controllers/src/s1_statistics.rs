#![forbid(unsafe_code)]

//! The S1 statistics configuration: from the S1 router's begin it brings up the S1 imaging
//! job and the statistics task's S1 measurement, runs the S1 acquisition and search children
//! from the half press, answers the commands the router forwards meanwhile and takes the
//! measurement down again. Only the S1 router enters it.

use crate::children::{AcquisitionControl, ChildEvent, ChildOutcome, SearchControl};
use crate::root::{Input, Outcome};
use crate::s1::StatisticsEvent;
use xm1_saclac_interface::{CommandKind as K, Completed, Completion as C, Message, Parameters, RecordHandle};

/// The requests the configuration posts to the imaging and statistics tasks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    /// Starts the S1 imaging job (completions 0x57, 0x58, 0x59).
    BeginImaging,
    EndImaging,
    /// Starts the statistics task's S1 measurement from the snapshot (completion 0x7b).
    Start,
    /// Stops it (completion 0x7c).
    Stop { pause: bool },
    /// Completions 0x7e and 0x7f.
    Selection,
    /// Sends the capture block again (completion 0x80).
    Retune,
    /// Sends the output block after Command1d (completion 0x75).
    Output,
    /// Command27's lens option.
    LensOption,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reply { Autofocus, Command02, Command03, Command1d }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Apply {
    Command17,
    Prepare,
    /// Command1d's two parameter words.
    Command1d,
    /// The resume clears the selection options and refreshes the lens values.
    Resume,
    /// Hands the statistics task's results to the callback the profile names.
    Results,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Condition {
    /// The profile's view mode or output word asks for the output block.
    OutputNeeded,
    /// The last parameter change restarts the S1 measurement.
    RestartRequired,
}

/// What the autofocus command changed for the running S1 measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutofocusChange {
    None,
    /// The capture block changed.
    Retune,
    /// The lens target changed.
    Lens,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub view_active: bool,
}

pub trait Port {
    fn acquisition(&mut self, event: ChildEvent<'_, AcquisitionControl>) -> ChildOutcome;
    fn search(&mut self, event: ChildEvent<'_, SearchControl>) -> ChildOutcome;
    /// Takes the profile values every request of this half press carries.
    fn snapshot(&mut self, record: RecordHandle) -> Snapshot;
    /// Holds or releases the display record mode for the S1 measurement.
    fn record_mode(&mut self, held: bool);
    fn request(&mut self, request: Request, record: RecordHandle);
    fn reply(&mut self, reply: Reply, record: RecordHandle);
    fn apply(&mut self, apply: Apply, record: RecordHandle);
    fn condition(&mut self, condition: Condition, record: RecordHandle) -> bool;
    fn autofocus(&mut self, record: RecordHandle) -> AutofocusChange;
    /// Moves the lens to the changed target; true when it needs no move (completion 0x35
    /// otherwise).
    fn move_lens(&mut self, record: RecordHandle) -> bool;
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    /// Waiting for the imaging job to be configured.
    Imaging,
    Starting,
    Selecting,
    Running,
    /// Waiting for the imaging job to end.
    Ending,
    Stopping,
    /// Stopping for a restart; waits for the stop and the acquisition.
    Restarting,
    /// The half press came while the measurement was starting.
    Prepared,
    /// The children run from the half press.
    Measuring,
    Retuning,
    Command1dPausing,
    /// Answering Command02 or Command03.
    Answering,
    /// A restart was asked for before the imaging job was configured.
    ImagingRestart,
    StartingRestart,
    /// Ending the imaging job to restart it.
    Reimaging,
}

#[derive(Clone, Copy, Debug)]
enum Event<'a> {
    Begin,
    Prepare { cancellable: bool },
    Cancel,
    Resume,
    Command(K),
    Completed(C, Completed<'a>),
    Other,
}

impl<'a> Event<'a> {
    fn of(event: StatisticsEvent<'a>) -> Self {
        match event {
            StatisticsEvent::Begin => Self::Begin,
            StatisticsEvent::Prepare => Self::Prepare { cancellable: false },
            StatisticsEvent::PrepareCancellable => Self::Prepare { cancellable: true },
            StatisticsEvent::Cancel => Self::Cancel,
            StatisticsEvent::Resume => Self::Resume,
            StatisticsEvent::Forward(Input::Message(Message::Command(command))) => match command.kind {
                K::Command02 | K::Command03 | K::Autofocus | K::Command17 | K::Prepare | K::Command1d | K::Command27 => {
                    Self::Command(command.kind)
                }
                _ => Self::Other,
            },
            StatisticsEvent::Forward(Input::Message(Message::Completed(completed))) => match completed.completion {
                C::Image57 | C::Image58 | C::Image59 | C::Image5a | C::Statistics7b | C::Statistics7c | C::Statistics7e
                | C::Statistics7f | C::Statistics80 | C::Statistics75 | C::Calculation9b | C::Calculation9c
                | C::Calculation9d | C::Lens35 => Self::Completed(completed.completion, completed),
                _ => Self::Other,
            },
            StatisticsEvent::Forward(Input::Control(_)) => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Finished { acquisition: bool, search: bool }

/// Kept across restarts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Restarted { stopped: bool, acquisition: bool }

struct Call<'p, P> { port: &'p mut P, record: RecordHandle }

impl<P: Port> Call<'_, P> {
    fn request(&mut self, request: Request) { self.port.request(request, self.record) }
    fn reply(&mut self, reply: Reply) { self.port.reply(reply, self.record) }
    fn apply(&mut self, apply: Apply) { self.port.apply(apply, self.record) }
    fn condition(&mut self, condition: Condition) -> bool { self.port.condition(condition, self.record) }

    fn acquire(&mut self, control: AcquisitionControl) -> bool {
        let event = ChildEvent::Control { control, parameters: no_parameters(), record: self.record };
        self.port.acquisition(event) == ChildOutcome::Complete
    }

    fn search(&mut self, control: SearchControl) -> ChildOutcome {
        self.port.search(ChildEvent::Control { control, parameters: no_parameters(), record: self.record })
    }

    fn forward_acquisition(&mut self, completed: Completed<'_>) -> bool {
        self.port.acquisition(ChildEvent::Completed { completed, record: self.record }) == ChildOutcome::Complete
    }

    fn forward_search(&mut self, completed: Completed<'_>) -> ChildOutcome {
        self.port.search(ChildEvent::Completed { completed, record: self.record })
    }
}

fn no_parameters() -> Parameters<'static> { xm1_saclac_interface::stock::parameters(&[]) }

const fn child_outcome(child: ChildOutcome) -> Outcome {
    match child {
        ChildOutcome::Complete => Outcome::Complete,
        ChildOutcome::Fault => Outcome::Fault,
        ChildOutcome::Inactive => Outcome::Inactive,
        ChildOutcome::Pending | ChildOutcome::Measurement(_) | ChildOutcome::Search(_) => Outcome::Pending,
    }
}

pub struct S1Statistics {
    state: State,
    view_active: bool,
    /// An autofocus change left the capture block to be sent again.
    retune_pending: bool,
    restart_requested: bool,
    /// An autofocus change moves the lens before its reply.
    lens_pending: bool,
    /// The statistics task's results are no longer reported (completion 0x7f).
    results_closed: bool,
    /// Acquisition and search finished from the half press.
    finished: Finished,
    restarted: Restarted,
}

impl Default for S1Statistics { fn default() -> Self { Self::new() } }

impl S1Statistics {
    pub const fn new() -> Self {
        Self {
            state: State::Idle,
            view_active: false,
            retune_pending: false,
            restart_requested: false,
            lens_pending: false,
            results_closed: false,
            finished: Finished { acquisition: false, search: false },
            restarted: Restarted { stopped: false, acquisition: false },
        }
    }

    /// Whether the results callback still reports completion 0x7f.
    pub const fn results_open(&self) -> bool { !self.results_closed }

    pub fn send(&mut self, event: StatisticsEvent<'_>, record: RecordHandle, port: &mut impl Port) -> Outcome {
        let event = Event::of(event);
        if matches!(event, Event::Other) {
            return Outcome::Inactive;
        }
        let mut call = Call { port, record };
        match self.handle(event, &mut call) {
            Handled::Outcome(outcome) => outcome,
            Handled::Empty => Outcome::Fault,
            Handled::Unexpected => {
                call.port.fault();
                Outcome::Fault
            }
        }
    }

    fn snapshot<P: Port>(&mut self, call: &mut Call<'_, P>) {
        self.view_active = call.port.snapshot(call.record).view_active;
        self.results_closed = false;
        self.retune_pending = false;
        self.finished = Finished::default();
    }

    fn begin_imaging<P: Port>(&mut self, call: &mut Call<'_, P>) -> Outcome {
        call.acquire(AcquisitionControl::Initialize);
        self.snapshot(call);
        call.request(Request::BeginImaging);
        self.state = State::Imaging;
        Outcome::Pending
    }

    fn handle<P: Port>(&mut self, event: Event<'_>, call: &mut Call<'_, P>) -> Handled {
        use State as S;
        let outcome = match (self.state, event) {
            (S::Idle, Event::Begin) => {
                call.acquire(AcquisitionControl::Initialize);
                call.search(SearchControl::Initialize);
                call.port.record_mode(true);
                self.snapshot(call);
                call.request(Request::BeginImaging);
                self.state = S::Imaging;
                Outcome::Pending
            }
            (S::Imaging | S::ImagingRestart, Event::Prepare { .. }) => {
                self.start_children(call);
                call.request(Request::EndImaging);
                self.state = S::Ending;
                Outcome::Pending
            }
            (S::Starting | S::StartingRestart | S::Selecting, Event::Prepare { .. }) => {
                self.start_children(call);
                if self.state == S::Selecting {
                    self.results_closed = true;
                }
                self.state = S::Prepared;
                Outcome::Pending
            }
            (S::Running, Event::Prepare { cancellable }) => {
                let search = if cancellable { SearchControl::BeginCancellable } else { SearchControl::Begin };
                if call.acquire(AcquisitionControl::Start) {
                    self.finished.acquisition = true;
                    if call.search(search) == ChildOutcome::Complete {
                        self.finished.search = true;
                        self.stop_finished(call);
                    } else {
                        self.state = S::Measuring;
                    }
                } else {
                    self.state = S::Measuring;
                    if call.search(search) == ChildOutcome::Complete {
                        self.finished.search = true;
                    }
                }
                Outcome::Pending
            }
            (S::Imaging | S::Starting | S::Selecting | S::Running, Event::Cancel) => child_outcome(call.search(SearchControl::Cancel)),
            (S::Imaging | S::Starting | S::Selecting | S::Running, Event::Resume) => {
                call.apply(Apply::Resume);
                child_outcome(call.search(SearchControl::Resume))
            }
            (state, Event::Command(command)) => return self.command(state, command, call),
            (state, Event::Completed(completion, completed)) => return self.completed(state, completion, completed, call),
            _ => return Handled::Unexpected,
        };
        Handled::Outcome(outcome)
    }

    fn command<P: Port>(&mut self, state: State, command: K, call: &mut Call<'_, P>) -> Handled {
        use State as S;
        let outcome = match (state, command) {
            (S::Imaging | S::Starting | S::Selecting | S::Running, K::Autofocus) => self.autofocus(call),
            (S::Running, K::Command1d) => {
                call.apply(Apply::Command1d);
                if call.condition(Condition::OutputNeeded) {
                    call.request(Request::Output);
                } else {
                    self.state = S::Command1dPausing;
                    call.request(Request::Stop { pause: true });
                }
                Outcome::Pending
            }
            (S::Running, K::Command02) => {
                self.snapshot(call);
                call.request(Request::Start);
                self.state = S::Answering;
                Outcome::Pending
            }
            (S::Running, K::Command03) => {
                call.request(Request::Stop { pause: true });
                self.state = S::Answering;
                Outcome::Pending
            }
            (S::Imaging | S::Starting | S::Selecting | S::Running | S::ImagingRestart | S::StartingRestart, K::Command17) => {
                call.apply(Apply::Command17);
                Outcome::Pending
            }
            (S::Imaging | S::Starting | S::Selecting | S::Running | S::ImagingRestart | S::StartingRestart, K::Command27) => {
                call.request(Request::LensOption);
                Outcome::Pending
            }
            (S::Running, K::Prepare) => {
                call.apply(Apply::Prepare);
                Outcome::Complete
            }
            _ => return Handled::Unexpected,
        };
        Handled::Outcome(outcome)
    }

    fn completed<P: Port>(&mut self, state: State, completion: C, completed: Completed<'_>, call: &mut Call<'_, P>) -> Handled {
        use State as S;
        let outcome = match (state, completion) {
            (S::Imaging, C::Image57) => self.start(S::Starting, call),
            (S::ImagingRestart, C::Image57) => self.start(S::StartingRestart, call),
            (S::Ending, C::Image57) => Outcome::Pending,
            (S::Ending, C::Image58) => {
                call.port.record_mode(false);
                self.state = S::Idle;
                Outcome::Complete
            }
            (S::Reimaging, C::Image58) => self.begin_imaging(call),
            (S::Starting, C::Statistics7b) => self.started(false, call),
            (S::StartingRestart, C::Statistics7b) => self.started(true, call),
            (S::Stopping | S::Restarting, C::Statistics7b) => Outcome::Pending,
            (S::Prepared, C::Statistics7b | C::Statistics7e) => {
                call.request(Request::Stop { pause: false });
                self.state = S::Stopping;
                Outcome::Pending
            }
            (S::Answering, C::Statistics7b) => self.answered(Reply::Command02, call),
            (S::Stopping, C::Statistics7c) => {
                call.request(Request::EndImaging);
                self.state = S::Ending;
                Outcome::Pending
            }
            (S::Restarting, C::Statistics7c) => {
                self.restarted.stopped = true;
                self.restart_when_stopped(call);
                Outcome::Pending
            }
            (S::Command1dPausing, C::Statistics7c) => self.answered(Reply::Command1d, call),
            (S::Answering, C::Statistics7c) => self.answered(Reply::Command03, call),
            (S::Reimaging, C::Statistics7c) => {
                call.reply(Reply::Autofocus);
                self.state = S::Ending;
                Outcome::Complete
            }
            (S::Selecting, C::Statistics7e) => {
                if self.retune_pending {
                    self.retune(call)
                } else {
                    self.state = S::Running;
                    self.move_lens_if_pending(call);
                    Outcome::Pending
                }
            }
            (S::Running, C::Statistics7f) => {
                call.apply(Apply::Results);
                Outcome::Pending
            }
            (S::Idle | S::Selecting | S::Ending | S::Stopping | S::Restarting | S::Prepared | S::Measuring | S::Retuning, C::Statistics7f) => {
                Outcome::Pending
            }
            (S::Retuning, C::Statistics80) => {
                call.reply(Reply::Autofocus);
                self.state = S::Running;
                Outcome::Complete
            }
            (S::Running, C::Statistics75) => {
                call.reply(Reply::Command1d);
                Outcome::Complete
            }
            (S::Running | S::Retuning, C::Image59) => {
                call.acquire(AcquisitionControl::Stop);
                call.search(SearchControl::ImagingReported);
                Outcome::Pending
            }
            (S::Measuring, C::Image59) => {
                call.search(SearchControl::ImagingReported);
                Outcome::Pending
            }
            (S::Command1dPausing | S::Answering, C::Image59) => return Handled::Unexpected,
            (_, C::Image59) => Outcome::Pending,
            (S::Restarting, C::Calculation9b | C::Calculation9c) => {
                if call.forward_acquisition(completed) {
                    self.restarted.acquisition = true;
                }
                self.restart_when_stopped(call);
                Outcome::Pending
            }
            (S::Measuring, C::Calculation9b | C::Calculation9c) => {
                if call.forward_acquisition(completed) {
                    self.finished.acquisition = true;
                    self.stop_when_finished(call);
                }
                Outcome::Pending
            }
            (
                S::Imaging | S::Starting | S::Selecting | S::Running | S::Ending | S::Retuning | S::ImagingRestart | S::Reimaging,
                C::Calculation9b | C::Calculation9c,
            ) => {
                call.forward_acquisition(completed);
                Outcome::Pending
            }
            (S::Imaging | S::Starting | S::Selecting | S::Running, C::Calculation9d | C::Image5a) => child_outcome(call.forward_search(completed)),
            (S::Measuring, C::Calculation9d | C::Image5a) => {
                if call.forward_search(completed) == ChildOutcome::Complete {
                    self.finished.search = true;
                    self.stop_when_finished(call);
                }
                Outcome::Pending
            }
            (S::Ending | S::Restarting | S::Retuning | S::ImagingRestart | S::Reimaging, C::Calculation9d | C::Image5a) => {
                call.forward_search(completed);
                Outcome::Pending
            }
            (S::Idle | S::Stopping | S::Prepared | S::StartingRestart, C::Calculation9b | C::Calculation9c | C::Calculation9d | C::Image5a)
            | (S::Idle, C::Lens35) => return Handled::Empty,
            (S::Running, C::Lens35) => {
                call.reply(Reply::Autofocus);
                self.lens_pending = false;
                Outcome::Pending
            }
            _ => return Handled::Unexpected,
        };
        Handled::Outcome(outcome)
    }

    fn start_children<P: Port>(&mut self, call: &mut Call<'_, P>) {
        call.acquire(AcquisitionControl::Start);
        call.search(SearchControl::Begin);
    }

    fn start<P: Port>(&mut self, next: State, call: &mut Call<'_, P>) -> Outcome {
        call.request(Request::Start);
        self.state = next;
        Outcome::Pending
    }

    /// The measurement started; `restart` when an autofocus change asked for a restart
    /// before it.
    fn started<P: Port>(&mut self, restart: bool, call: &mut Call<'_, P>) -> Outcome {
        if self.view_active {
            self.state = State::Selecting;
            call.request(Request::Selection);
        } else if self.retune_pending {
            self.retune(call);
        } else if self.restart_requested && restart {
            self.state = State::Restarting;
            call.request(Request::Stop { pause: false });
            self.restarted = Restarted::default();
            if call.acquire(AcquisitionControl::Start) {
                self.restarted.acquisition = true;
            }
        } else if self.restart_requested {
            self.state = State::Running;
            call.reply(Reply::Autofocus);
        } else {
            self.state = State::Running;
            self.move_lens_if_pending(call);
        }
        Outcome::Pending
    }

    fn retune<P: Port>(&mut self, call: &mut Call<'_, P>) -> Outcome {
        self.state = State::Retuning;
        call.request(Request::Retune);
        Outcome::Pending
    }

    /// Returns whether the autofocus reply went out.
    fn move_lens_if_pending<P: Port>(&mut self, call: &mut Call<'_, P>) -> bool {
        if !self.lens_pending || !call.port.move_lens(call.record) {
            return false;
        }
        call.reply(Reply::Autofocus);
        self.lens_pending = false;
        true
    }

    fn autofocus<P: Port>(&mut self, call: &mut Call<'_, P>) -> Outcome {
        self.retune_pending = false;
        self.restart_requested = false;
        let change = call.port.autofocus(call.record);
        let running = self.state == State::Running;
        if change == AutofocusChange::Retune {
            if running {
                return self.retune(call);
            }
            self.retune_pending = true;
            return Outcome::Pending;
        }
        if self.state != State::Selecting && call.condition(Condition::RestartRequired) {
            self.restart_requested = true;
            self.state = match self.state {
                State::Imaging => State::ImagingRestart,
                State::Starting => State::StartingRestart,
                _ => {
                    call.request(Request::Stop { pause: false });
                    State::Restarting
                }
            };
            return Outcome::Pending;
        }
        if change == AutofocusChange::Lens {
            self.lens_pending = true;
            if running && self.move_lens_if_pending(call) {
                return Outcome::Complete;
            }
            return Outcome::Pending;
        }
        call.reply(Reply::Autofocus);
        Outcome::Complete
    }

    fn answered<P: Port>(&mut self, reply: Reply, call: &mut Call<'_, P>) -> Outcome {
        call.reply(reply);
        self.state = State::Running;
        Outcome::Complete
    }

    fn stop_finished<P: Port>(&mut self, call: &mut Call<'_, P>) {
        self.state = State::Stopping;
        self.results_closed = true;
        call.request(Request::Stop { pause: false });
    }

    fn stop_when_finished<P: Port>(&mut self, call: &mut Call<'_, P>) {
        if self.finished.acquisition && self.finished.search {
            self.stop_finished(call);
        }
    }

    fn restart_when_stopped<P: Port>(&mut self, call: &mut Call<'_, P>) {
        if self.restarted.stopped && self.restarted.acquisition {
            self.state = State::Reimaging;
            call.request(Request::EndImaging);
        }
    }
}

enum Handled {
    Outcome(Outcome),
    /// A cell with no action: the fault result without the fault reaction.
    Empty,
    Unexpected,
}
