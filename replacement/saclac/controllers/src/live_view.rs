#![forbid(unsafe_code)]

//! The live view: brings the image pipeline's live view up and down around the live
//! statistics configuration and the live measurement, and carries profile commands into
//! the running view. The live router enters it as its controller, root enters it for the
//! deferred option and the S1 bridge enters it during S1.

use crate::children::{
    ChildEvent, ChildOutcome, MeasurementControl, MeasurementOutcome, MeasurementUpdate, Restart, RetainedRoundStatus,
    ShotControl, ShotEvent, ShotTarget,
};
use crate::root::Outcome;
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completed, Completion as C, Parameters, RecordHandle};

/// What the live view is entered with. A reissued command carries its last parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal<'a> {
    /// Pause measurement and the view around a profile change.
    Suspend,
    /// End the pause `Suspend` began.
    Resume,
    /// Root quiesces live processing for S1.
    SuspendLive,
    /// Root's deferred option after the parameter join.
    RefreshDeferred,
    /// Root's lens-drive cancellation.
    Cancel,
    Command(Command<'a>),
    Completed(Completed<'a>),
}

/// The live statistics configuration's controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsControl {
    /// Begin once measurement and the view are configured.
    Begin,
    Stop,
    /// Stop ahead of a view restore or refresh.
    Pause,
    /// Hand statistics to the S1 capture.
    Capture,
    /// Release statistics for the Command0a change.
    Release,
    /// Follow a measurement window update.
    Measured(MeasurementUpdate),
    Autofocus,
    /// Refresh the statistics selection the profile owes.
    Refresh,
    Resume,
    Suspend,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsCall<'a> {
    Control(StatisticsControl),
    Forward(Signal<'a>),
    /// A command entered again with its last parameters.
    Reissue(K),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsOutcome {
    Fault,
    Complete,
    Pending,
    Unhandled,
    /// Held for the S1 capture.
    Ready,
    /// Measurement must be reconfigured for the new statistics.
    Reconfigure,
}

impl StatisticsOutcome {
    const fn outcome(self) -> Outcome {
        match self {
            Self::Complete => Outcome::Complete,
            Self::Pending => Outcome::Pending,
            Self::Unhandled => Outcome::Inactive,
            Self::Fault | Self::Ready | Self::Reconfigure => Outcome::Fault,
        }
    }
}

/// The changed-field group of an autofocus or Command22 change, as the view stop's mode
/// codes it (2 to 5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeCode(u8);

impl ChangeCode {
    pub const fn new(code: u8) -> Option<Self> {
        if 2 <= code && code <= 5 { Some(Self(code)) } else { None }
    }
    pub const fn get(self) -> u8 { self.0 }
}

/// Why the view would stop, as preparation found it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StopReason {
    #[default]
    Plain,
    SecondaryProfile,
    Change(ChangeCode),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Preparation {
    pub stop: StopReason,
    /// The live sequence runs once the view has started.
    pub sequence: bool,
}

/// The result of applying a command's parameters to the profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Application {
    Refused,
    Applied,
    /// The change needs the view stopped and started again.
    Restart,
}

/// The secondary profile's options the view restore and stop carry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RestoreOptions { pub first: bool, pub second: bool, pub negative: bool }

impl RestoreOptions {
    const fn any(self) -> bool { self.first || self.second || self.negative }

    fn of(parameters: Parameters<'_>) -> Self {
        let options = word(parameters, 1);
        Self { first: options & 1 != 0, second: options & 2 != 0, negative: options & 0x8000_0000 != 0 }
    }
}

/// The motion-control setting the view stop samples for later view requests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MotionSetting(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopMode { Plain, Secondary, SecondaryNegative, Change(ChangeCode) }

/// The image pipeline's live-view requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewRequest {
    Start { measured: bool },
    Restore(RestoreOptions),
    Refresh(MotionSetting),
    Stop(StopMode),
    Suspend,
    Resume,
    Configure(MotionSetting),
    /// `selection` carries the profile's current selection.
    Adjust { selection: bool },
    Window(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Hold the configuration channel while the view changes.
    Hold,
    ReleaseHold,
    StoreAdjustment,
    BeginRecordMode,
    FinishRecordMode,
    QueryRecordMode,
    /// Set or clear the profile's statistics-preparation flag.
    StatisticsPreparation(bool),
    SelectionTaken,
}

/// Profile and leaf conditions the controller branches on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Condition {
    /// The profile owes a statistics selection refresh.
    SelectionPending,
    /// The view starts without a refresh.
    DirectStart,
    /// The autofocus change touched the view geometry.
    ViewGeometryChanged,
    AutofocusAreaChanged,
    AutofocusModeChanged,
    /// The autofocus mode change needs statistics retuned.
    AutofocusRetune,
    /// The Command1d change restarts the view instead of refreshing it.
    Command1dRestart,
    PreparationRequested,
    PreparationRestorable,
    /// Statistics retune before measurement suspends or resumes.
    StatisticsRetune,
    /// The last applied profile was the primary profile.
    PrimaryApplied,
    /// The applied selection left a selection bit set.
    SelectionActive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyStatus { Accepted, Rejected }

pub trait Port {
    fn statistics(&mut self, call: StatisticsCall<'_>, record: RecordHandle) -> StatisticsOutcome;
    fn measurement(&mut self, event: ChildEvent<'_, MeasurementControl>) -> ChildOutcome;
    /// The shot module's live sequence.
    fn sequence(&mut self, event: ShotEvent<'_>) -> Outcome;
    /// `None` prepares without a command to reply to.
    fn prepare(&mut self, command: Option<K>, record: RecordHandle) -> Preparation;
    fn apply(&mut self, command: K, record: RecordHandle) -> Application;
    fn view(&mut self, request: ViewRequest, record: RecordHandle);
    fn effect(&mut self, effect: Effect, record: RecordHandle);
    fn condition(&mut self, condition: Condition, record: RecordHandle) -> bool;
    fn motion_setting(&mut self) -> MotionSetting;
    fn reply(&mut self, command: Option<K>, status: ReplyStatus, record: RecordHandle);
    fn fault(&mut self);
}

/// The profile's statistics-preparation flag across a view stop and start. The S1
/// statistics configuration reads and writes it too.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PreparationOverride {
    #[default]
    None,
    /// Requested; set when the view is configured.
    Pending,
    /// Set; cleared when the view stops.
    Set,
}

/// Which children reached what the current phase waits for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Ready { statistics: bool, measurement: bool, view: bool }

impl Ready {
    const fn halted(self) -> bool { self.statistics && self.measurement }
    const fn configured(self) -> bool { self.statistics && self.measurement && self.view }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    /// Idle with a statistics or measurement change outstanding.
    IdleBusy,
    /// The view start or restore is outstanding.
    Starting,
    /// The live sequence runs on the started view.
    Sequencing,
    Configuring,
    Running,
    /// Measurement suspended; the view suspend is outstanding.
    Suspending,
    /// Measurement resumed; the view resume is outstanding.
    Resuming,
    ResumeRetune,
    SuspendRetune,
    /// A statistics change is outstanding while running.
    Reconfiguring,
    AutofocusRetune,
    /// The view adjustment after the autofocus retune is outstanding.
    Adjusting,
    /// A Command11 statistics change is outstanding.
    Retuning,
    RetuneSuspending,
    RetuneResuming,
    /// A measurement command is outstanding while running.
    Measuring,
    /// Children stopping; the view restarts once it stopped.
    Restarting,
    StoppingForRefresh,
    Refreshing,
    StoppingForAutofocus,
    RefreshingForAutofocus,
    Windowing,
    /// Children stopping; the view stops.
    Stopping,
    Restoring,
}

impl State {
    /// States where a pause or its end is ignored rather than a fault.
    const fn ignores_pause(self) -> bool {
        matches!(self,
            Self::Starting | Self::Sequencing | Self::Configuring | Self::Measuring | Self::Restarting
            | Self::StoppingForRefresh | Self::Refreshing | Self::StoppingForAutofocus | Self::RefreshingForAutofocus
            | Self::Windowing | Self::Stopping | Self::Restoring)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Suspend,
    Resume,
    SuspendLive,
    RefreshDeferred,
    LensControl,
    Primary,
    Secondary,
    Capture,
    Command0a,
    HalfRelease,
    Command11,
    Command13,
    MeasurementCommand,
    MeasurementForward,
    Command17,
    Prepare,
    Command1d,
    ProfileChange,
    Command1f,
    Command22,
    Autofocus,
    Command23,
    Prepare26,
    Command29,
    Command24,
    Started,
    Restored,
    Configured,
    Stopped,
    ViewSuspended,
    ViewResumed,
    Accessor,
    Adjusted,
    StatisticsSelection,
    StatisticsSettled,
    Windowed,
    OtherImage,
    Statistics77,
    Statistics,
    Calculation,
    Other,
}

impl Event {
    fn of(signal: Signal<'_>) -> Self {
        match signal {
            Signal::Suspend => Self::Suspend,
            Signal::Resume => Self::Resume,
            Signal::SuspendLive => Self::SuspendLive,
            Signal::RefreshDeferred => Self::RefreshDeferred,
            Signal::Cancel => Self::Other,
            Signal::Command(command) => match command.kind {
                K::LensBringUp | K::LensStop => Self::LensControl,
                K::Command02 => Self::Primary,
                K::Command03 => Self::Secondary,
                K::Command09 => Self::Capture,
                K::Command0a => Self::Command0a,
                K::HalfRelease => Self::HalfRelease,
                K::Command11 => Self::Command11,
                K::Command13 => Self::Command13,
                K::Command14 | K::Command2b => Self::MeasurementCommand,
                K::Command15 | K::Command16 => Self::MeasurementForward,
                K::Command17 => Self::Command17,
                K::Prepare => Self::Prepare,
                K::Command1d => Self::Command1d,
                K::Command1e | K::Command20 | K::Command21 => Self::ProfileChange,
                K::Command1f => Self::Command1f,
                K::Command22 => Self::Command22,
                K::Autofocus => Self::Autofocus,
                K::Command23 => Self::Command23,
                K::Prepare26 => Self::Prepare26,
                K::Command29 => Self::Command29,
                K::Command24 => Self::Command24,
                _ => Self::Other,
            },
            Signal::Completed(done) => match done.completion {
                C::Image3f => Self::Started,
                C::Image40 => Self::Restored,
                C::Image41 => Self::Configured,
                C::Image42 => Self::Stopped,
                C::Image43 => Self::ViewSuspended,
                C::Image44 => Self::ViewResumed,
                C::Accessor4a => Self::Accessor,
                C::Image63 => Self::Adjusted,
                C::Statistics7e => Self::StatisticsSelection,
                C::Statistics82 => Self::StatisticsSettled,
                C::Image65 => Self::Windowed,
                C::Statistics77 => Self::Statistics77,
                C::Image45 | C::Image46 | C::Image47 | C::Accessor4b | C::Image4c | C::Image4d | C::Image4e
                | C::Image4f | C::Image50 | C::Image51 | C::Image52 | C::Image53 | C::Image57 | C::Image58
                | C::Image59 | C::Image5a | C::Image5b | C::Image5c | C::Image5d | C::Image5e | C::Image5f
                | C::Image60 | C::Image61 | C::Image62 | C::Image64 | C::Image66 => Self::OtherImage,
                C::Statistics69 | C::Statistics6b | C::Statistics6c | C::Statistics6d | C::Statistics6e
                | C::Statistics6f | C::Statistics70 | C::Statistics71 | C::Statistics72 | C::Statistics73
                | C::Statistics75 | C::Statistics76 | C::Statistics78 | C::Statistics79 | C::Statistics7a
                | C::Statistics7b | C::Statistics7c | C::Statistics7d | C::Statistics7f | C::Statistics80
                | C::Statistics83 => Self::Statistics,
                C::Calculation87 | C::Calculation88 | C::Calculation89 | C::Calculation8a | C::Calculation8b
                | C::Calculation8d | C::Calculation8e | C::Calculation8f | C::Calculation90 | C::Calculation91
                | C::Calculation92 | C::Calculation93 | C::Calculation94 | C::Calculation95 | C::Calculation97
                | C::Calculation98 | C::Calculation99 | C::Calculation9a | C::Calculation9b | C::Calculation9c
                | C::Calculation9d => Self::Calculation,
                _ => Self::Other,
            },
        }
    }

    /// The image completions the view requests itself.
    const fn view_completion(self) -> bool {
        matches!(self,
            Self::Started | Self::Restored | Self::Configured | Self::Stopped | Self::ViewSuspended | Self::ViewResumed
            | Self::Accessor | Self::Adjusted | Self::StatisticsSelection)
    }

    const fn quiet_completion(self) -> bool {
        matches!(self,
            Self::StatisticsSettled | Self::Windowed | Self::OtherImage | Self::Statistics77 | Self::Statistics | Self::Calculation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Measured {
    Complete,
    Pending,
    Update { update: MeasurementUpdate, restarted: bool },
    /// The round waits for its next sample.
    Waiting,
    Other(Outcome),
}

impl Measured {
    fn of(outcome: ChildOutcome) -> Self {
        match outcome {
            ChildOutcome::Complete => Self::Complete,
            ChildOutcome::Pending => Self::Pending,
            ChildOutcome::Inactive => Self::Other(Outcome::Inactive),
            ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart }) => {
                Self::Update { update, restarted: restart == Restart::Complete }
            }
            ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status0c)) => Self::Waiting,
            ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)) => Self::Other(Outcome::Inactive),
            ChildOutcome::Search(_) | ChildOutcome::Fault => Self::Other(Outcome::Fault),
        }
    }

    const fn outcome(self) -> Outcome {
        match self {
            Self::Complete => Outcome::Complete,
            Self::Pending => Outcome::Pending,
            Self::Waiting | Self::Update { .. } => Outcome::Fault,
            Self::Other(outcome) => outcome,
        }
    }
}

fn word(parameters: Parameters<'_>, index: usize) -> u32 {
    let start = index * 4;
    parameters.bytes().get(start..start + 4).map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

struct Call<'p, 's, 'i, P> { port: &'p mut P, shared: &'s mut PreparationOverride, signal: Signal<'i>, record: RecordHandle }

impl<P: Port> Call<'_, '_, '_, P> {
    fn command(&self) -> Option<K> {
        match self.signal { Signal::Command(command) => Some(command.kind), _ => None }
    }
    fn parameter(&self, index: usize) -> u32 {
        match self.signal { Signal::Command(command) => word(command.parameters, index), _ => 0 }
    }
    fn statistics(&mut self, control: StatisticsControl) -> StatisticsOutcome {
        self.port.statistics(StatisticsCall::Control(control), self.record)
    }
    fn reissue(&mut self, command: K) -> StatisticsOutcome { self.port.statistics(StatisticsCall::Reissue(command), self.record) }
    fn forward_statistics(&mut self) -> StatisticsOutcome { self.port.statistics(StatisticsCall::Forward(self.signal), self.record) }
    fn measurement(&mut self, control: MeasurementControl) -> Measured {
        let event = ChildEvent::Control { control, parameters: stock::parameters(&[]), record: self.record };
        Measured::of(self.port.measurement(event))
    }
    fn forward_measurement(&mut self) -> Measured {
        let record = self.record;
        let event = match self.signal {
            Signal::Command(command) => ChildEvent::Command { command, record },
            Signal::Completed(completed) => ChildEvent::Completed { completed, record },
            _ => return Measured::Other(Outcome::Inactive),
        };
        Measured::of(self.port.measurement(event))
    }
    /// Root admits no control while the sequence runs, so only messages reach it.
    fn forward_sequence(&mut self) -> Outcome {
        let record = self.record;
        match self.signal {
            Signal::Command(command) => self.port.sequence(ShotEvent::Command { target: ShotTarget::Live, command, record }),
            Signal::Completed(completed) => self.port.sequence(ShotEvent::Completed { target: ShotTarget::Live, completed, record }),
            _ => Outcome::Inactive,
        }
    }
    fn apply(&mut self) -> Application {
        match self.command() {
            Some(command) => self.port.apply(command, self.record),
            None => Application::Applied,
        }
    }
    fn view(&mut self, request: ViewRequest) { self.port.view(request, self.record) }
    fn effect(&mut self, effect: Effect) { self.port.effect(effect, self.record) }
    fn condition(&mut self, condition: Condition) -> bool { self.port.condition(condition, self.record) }
    fn fault(&mut self) -> Outcome { self.port.fault(); Outcome::Fault }

    fn clear_preparation(&mut self) {
        if *self.shared == PreparationOverride::Set {
            self.effect(Effect::StatisticsPreparation(false));
            self.reissue(K::Prepare);
            *self.shared = PreparationOverride::Pending;
        }
    }

    fn set_preparation(&mut self) {
        self.effect(Effect::StatisticsPreparation(true));
        self.reissue(K::Prepare);
        *self.shared = PreparationOverride::Set;
    }
}

pub struct Controller {
    state: State,
    current: Option<K>,
    measured: bool,
    restore: RestoreOptions,
    ready: Ready,
    preparation: Preparation,
    motion: MotionSetting,
}

impl Default for Controller { fn default() -> Self { Self::new() } }

impl Controller {
    pub const fn new() -> Self {
        Self {
            state: State::Idle,
            current: None,
            measured: false,
            restore: RestoreOptions { first: false, second: false, negative: false },
            ready: Ready { statistics: false, measurement: false, view: false },
            preparation: Preparation { stop: StopReason::Plain, sequence: false },
            motion: MotionSetting(0),
        }
    }

    /// `preparation` is the override the S1 statistics configuration shares.
    pub fn handle(&mut self, signal: Signal<'_>, record: RecordHandle, preparation: &mut PreparationOverride, port: &mut impl Port) -> Outcome {
        let event = Event::of(signal);
        let mut call = Call { port, shared: preparation, signal, record };
        use Event as E;
        use State as S;
        match (self.state, event) {
            (S::Idle, E::SuspendLive | E::Suspend | E::Resume | E::LensControl) => Outcome::Complete,
            (S::Idle, E::Capture) => {
                self.begin(&mut call);
                call.effect(Effect::Hold);
                call.view(ViewRequest::Start { measured: self.measured });
                self.state = S::Starting;
                Outcome::Pending
            }
            (S::Idle, E::Primary) => {
                self.begin(&mut call);
                call.apply();
                self.start_view(self.measured, &mut call);
                self.state = S::Starting;
                Outcome::Pending
            }
            (S::Idle, E::Secondary) => {
                self.prepare(&mut call);
                self.ready = Ready { measurement: true, ..Ready::default() };
                call.apply();
                self.restore = RestoreOptions::of(self.parameters(&call));
                if call.forward_statistics() != StatisticsOutcome::Complete {
                    self.state = S::Stopping;
                    return Outcome::Pending;
                }
                if self.restore.any() {
                    self.state = S::Restoring;
                    call.view(ViewRequest::Restore(self.restore));
                    return Outcome::Pending;
                }
                self.reply(ReplyStatus::Accepted, &mut call);
                Outcome::Complete
            }
            (S::Idle, E::HalfRelease) => {
                self.prepare(&mut call);
                self.ready = Ready::default();
                if self.measured {
                    self.state = S::Starting;
                    self.start_view(true, &mut call);
                } else {
                    self.state = S::Restoring;
                    call.view(ViewRequest::Restore(self.restore));
                }
                Outcome::Pending
            }
            (S::Idle, E::Command11) => {
                call.apply();
                match call.forward_statistics() {
                    StatisticsOutcome::Pending => { self.state = S::IdleBusy; Outcome::Pending }
                    StatisticsOutcome::Reconfigure => Outcome::Complete,
                    outcome => outcome.outcome(),
                }
            }
            (S::Idle, E::Command17) => {
                self.prepare(&mut call);
                if call.apply() == Application::Refused {
                    self.reply(ReplyStatus::Rejected, &mut call);
                    return Outcome::Complete;
                }
                let measured = call.forward_measurement();
                if measured == Measured::Pending { self.state = S::IdleBusy; }
                measured.outcome()
            }
            (S::Idle, E::Command1d | E::ProfileChange | E::Command1f | E::Autofocus) => {
                call.apply();
                let outcome = call.forward_statistics();
                if outcome == StatisticsOutcome::Pending { self.state = S::IdleBusy; }
                outcome.outcome()
            }
            (S::Idle, E::Prepare) => {
                self.prepare(&mut call);
                call.apply();
                if call.condition(Condition::PreparationRequested) {
                    *call.shared = PreparationOverride::Pending;
                } else {
                    call.forward_statistics();
                    *call.shared = PreparationOverride::None;
                }
                self.reply(ReplyStatus::Accepted, &mut call);
                Outcome::Complete
            }
            (S::Idle, E::Command22) => {
                self.prepare(&mut call);
                if call.parameter(1) == 1 {
                    self.reply(ReplyStatus::Accepted, &mut call);
                    return Outcome::Complete;
                }
                self.ready = Ready::default();
                if self.measured {
                    self.state = S::Starting;
                    call.effect(Effect::Hold);
                    call.view(ViewRequest::Start { measured: true });
                } else {
                    self.state = S::Restoring;
                    call.view(ViewRequest::Restore(self.restore));
                }
                Outcome::Pending
            }
            (S::Idle, E::Command23) | (S::Idle | S::Running, E::Prepare26) => {
                self.prepare(&mut call);
                call.apply();
                self.reply(ReplyStatus::Accepted, &mut call);
                Outcome::Complete
            }
            (S::Idle, E::Command29) => {
                self.prepare(&mut call);
                self.ready = Ready::default();
                call.view(ViewRequest::Window(call.parameter(1)));
                self.state = S::Windowing;
                Outcome::Pending
            }
            (S::Idle, E::RefreshDeferred) => {
                self.prepare(&mut call);
                self.ready = Ready::default();
                call.effect(Effect::Hold);
                call.effect(Effect::StoreAdjustment);
                self.configure(true, &mut call)
            }
            (S::IdleBusy, E::Started) => {
                call.effect(Effect::ReleaseHold);
                self.state = S::Idle;
                Outcome::Complete
            }
            (S::IdleBusy, E::Statistics77 | E::Statistics) => {
                let outcome = call.forward_statistics();
                if outcome != StatisticsOutcome::Pending { self.state = S::Idle; }
                outcome.outcome()
            }
            (S::IdleBusy, E::Calculation) => {
                let measured = call.forward_measurement();
                if measured != Measured::Pending { self.state = S::Idle; }
                measured.outcome()
            }
            (S::Starting, E::Started) => {
                if self.preparation.sequence {
                    call.effect(Effect::QueryRecordMode);
                    let record = call.record;
                    match call.port.sequence(ShotEvent::Control { control: ShotControl::StartLiveSequence, record }) {
                        Outcome::Pending => { self.state = S::Sequencing; return Outcome::Pending; }
                        Outcome::Complete => {}
                        outcome => return outcome,
                    }
                }
                self.after_start(true, &mut call)
            }
            (S::Starting, E::Restored) => self.after_start(false, &mut call),
            (S::Sequencing, E::OtherImage | E::Calculation | E::Other) => match call.forward_sequence() {
                Outcome::Complete => self.after_start(true, &mut call),
                outcome => outcome,
            },
            (S::Configuring, E::Configured) => {
                self.ready.view = true;
                if *call.shared == PreparationOverride::Pending && call.condition(Condition::PreparationRestorable) {
                    call.set_preparation();
                }
                self.finish_configuration(&mut call)
            }
            (S::Configuring, E::Accessor) => {
                let control = if self.current == Some(K::Command09) { StatisticsControl::Capture } else { StatisticsControl::Begin };
                match call.statistics(control) {
                    StatisticsOutcome::Complete => {
                        self.ready.statistics = true;
                        self.finish_configuration(&mut call)
                    }
                    outcome => outcome.outcome(),
                }
            }
            (S::Configuring, E::Statistics) => match call.forward_statistics() {
                StatisticsOutcome::Complete => {
                    self.ready.statistics = true;
                    self.finish_configuration(&mut call)
                }
                outcome => outcome.outcome(),
            },
            (S::Configuring, E::StatisticsSelection) => {
                let outcome = call.forward_statistics();
                if outcome == StatisticsOutcome::Complete {
                    self.state = S::Running;
                    self.reply(ReplyStatus::Accepted, &mut call);
                }
                outcome.outcome()
            }
            (S::Running, E::SuspendLive) => {
                self.prepare(&mut call);
                self.restore = RestoreOptions::default();
                match self.halt_together(&mut call) {
                    (StatisticsOutcome::Complete | StatisticsOutcome::Pending, Measured::Complete | Measured::Pending) => Outcome::Pending,
                    (StatisticsOutcome::Complete, _) => Outcome::Complete,
                    (StatisticsOutcome::Pending, _) => Outcome::Pending,
                    (statistics, _) => statistics.outcome(),
                }
            }
            (S::Running, E::Suspend) => self.suspend(S::Suspending, Outcome::Pending, &mut call),
            (S::Retuning, E::Suspend) => self.suspend(S::RetuneSuspending, Outcome::Complete, &mut call),
            (S::Running, E::Resume) => {
                if call.condition(Condition::StatisticsRetune) {
                    self.state = S::ResumeRetune;
                    return call.statistics(StatisticsControl::Resume).outcome();
                }
                self.resume(S::Resuming, Outcome::Pending, &mut call)
            }
            (S::Retuning, E::Resume) => self.resume(S::RetuneResuming, Outcome::Complete, &mut call),
            (S::Running, E::Primary | E::Command1f) => {
                call.apply();
                self.statistics_change(&mut call)
            }
            (S::Running, E::Secondary) => {
                call.apply();
                self.restore = RestoreOptions::of(self.parameters(&call));
                if !self.restore.any() && call.condition(Condition::SelectionActive) {
                    return self.statistics_change(&mut call);
                }
                self.prepare(&mut call);
                self.measured = false;
                self.halt_for_stop(StatisticsControl::Pause, &mut call)
            }
            (S::Running, E::Capture) => {
                let held = call.statistics(StatisticsControl::Capture);
                if held != StatisticsOutcome::Ready { return held.outcome(); }
                self.prepare(&mut call);
                self.halt_for_restart(&mut call)
            }
            (S::Running, E::Command0a) => {
                self.prepare(&mut call);
                let release = call.parameter(1) != 0;
                self.restore = RestoreOptions::default();
                if !release { return self.statistics_change(&mut call); }
                // Stock prepares twice on this path; preparation writes the same values again.
                self.prepare(&mut call);
                self.halt_for_stop(StatisticsControl::Release, &mut call)
            }
            (S::Running, E::HalfRelease) => {
                self.prepare(&mut call);
                self.reply(ReplyStatus::Accepted, &mut call);
                Outcome::Complete
            }
            (S::Running, E::Command11) => {
                call.apply();
                match call.forward_statistics() {
                    StatisticsOutcome::Pending => { self.state = S::Retuning; Outcome::Pending }
                    StatisticsOutcome::Reconfigure => { call.measurement(MeasurementControl::Reconfigure); Outcome::Complete }
                    outcome => outcome.outcome(),
                }
            }
            (S::Suspending, E::Command11) => self.retune(S::RetuneSuspending, &mut call),
            (S::Resuming, E::Command11) => self.retune(S::RetuneResuming, &mut call),
            (S::Running, E::MeasurementCommand) => {
                let measured = call.forward_measurement();
                if measured == Measured::Pending { self.state = S::Measuring; }
                measured.outcome()
            }
            (S::Running, E::Command17) => self.running_command17(&mut call),
            (S::Running, E::Prepare) => {
                self.prepare(&mut call);
                call.apply();
                let requested = call.condition(Condition::PreparationRequested);
                let outcome = call.forward_statistics();
                *call.shared = if requested { PreparationOverride::Set } else { PreparationOverride::None };
                self.reply(ReplyStatus::Accepted, &mut call);
                outcome.outcome()
            }
            (S::Running, E::Command1d) => {
                let restart = call.condition(Condition::Command1dRestart);
                call.apply();
                if restart {
                    self.state = S::Restarting;
                    self.prepare(&mut call);
                    return self.halt_tolerant(&mut call);
                }
                self.state = S::StoppingForRefresh;
                if call.measurement(MeasurementControl::Stop) == Measured::Complete
                    && call.statistics(StatisticsControl::Pause) == StatisticsOutcome::Complete
                {
                    self.stop_view(&mut call);
                }
                Outcome::Pending
            }
            (S::Running, E::ProfileChange) => {
                if call.apply() == Application::Restart {
                    self.state = S::Restarting;
                    self.prepare(&mut call);
                    return self.halt_tolerant(&mut call);
                }
                self.statistics_change(&mut call)
            }
            (S::Running, E::Command22) => {
                self.prepare(&mut call);
                if call.parameter(1) != 1 {
                    self.reply(ReplyStatus::Accepted, &mut call);
                    return Outcome::Complete;
                }
                self.restore = RestoreOptions::default();
                match self.halt_together(&mut call) {
                    (StatisticsOutcome::Complete | StatisticsOutcome::Pending, Measured::Complete | Measured::Pending) => Outcome::Pending,
                    (StatisticsOutcome::Complete | StatisticsOutcome::Pending, measured) => measured.outcome(),
                    (statistics, _) => statistics.outcome(),
                }
            }
            (S::Running, E::Autofocus) => self.running_autofocus(&mut call),
            (S::Running, E::Command23) => {
                self.prepare(&mut call);
                if call.apply() == Application::Restart {
                    self.state = S::Restarting;
                    return self.halt_tolerant(&mut call);
                }
                self.state = S::Reconfiguring;
                if call.condition(Condition::SelectionPending) {
                    call.view(ViewRequest::Adjust { selection: true });
                } else {
                    call.statistics(StatisticsControl::Refresh);
                }
                Outcome::Pending
            }
            (S::Running, E::Command24) => self.statistics_change(&mut call),
            (S::Running | S::Reconfiguring | S::AutofocusRetune | S::Adjusting | S::Retuning, E::Calculation) => {
                match call.forward_measurement() {
                    Measured::Update { update, restarted: false } => call.statistics(StatisticsControl::Measured(update)).outcome(),
                    Measured::Waiting => Outcome::Pending,
                    measured => measured.outcome(),
                }
            }
            (S::Running | S::Reconfiguring | S::AutofocusRetune | S::Adjusting | S::Retuning, E::OtherImage)
            | (S::Running | S::Suspending | S::Resuming | S::Measuring | S::Restarting, E::MeasurementForward) => {
                call.forward_measurement().outcome()
            }
            (S::Running | S::Suspending | S::Resuming | S::RetuneSuspending | S::RetuneResuming | S::Retuning, E::Command13)
            | (S::Idle | S::Running | S::Suspending | S::Resuming, E::Statistics77 | E::Statistics)
            | (S::Configuring, E::Statistics77) => call.forward_statistics().outcome(),
            (S::Suspending | S::Resuming | S::ResumeRetune | S::SuspendRetune | S::RetuneSuspending | S::RetuneResuming | S::Retuning,
             E::Autofocus | E::Command23) => {
                call.apply();
                Outcome::Complete
            }
            (S::Suspending, E::ViewSuspended) => {
                if call.condition(Condition::StatisticsRetune) {
                    self.state = S::SuspendRetune;
                    return call.statistics(StatisticsControl::Suspend).outcome();
                }
                self.state = S::Running;
                Outcome::Complete
            }
            (S::RetuneSuspending, E::ViewSuspended) | (S::RetuneResuming, E::ViewResumed) => {
                self.state = S::Retuning;
                Outcome::Complete
            }
            (S::Resuming, E::ViewResumed) => { self.state = S::Running; Outcome::Complete }
            (S::Suspending | S::RetuneSuspending | S::SuspendRetune, E::OtherImage) => self.after_measurement(ViewRequest::Suspend, &mut call),
            (S::Resuming | S::ResumeRetune | S::RetuneResuming, E::OtherImage) => self.after_measurement(ViewRequest::Resume, &mut call),
            (S::Suspending | S::RetuneSuspending | S::SuspendRetune, E::Calculation) => self.paused_calculation(ViewRequest::Suspend, &mut call),
            (S::Resuming | S::ResumeRetune | S::RetuneResuming, E::Calculation) => self.paused_calculation(ViewRequest::Resume, &mut call),
            (S::RetuneSuspending, E::Statistics77 | E::Statistics) => self.retuned(S::Suspending, &mut call),
            (S::RetuneResuming, E::Statistics77 | E::Statistics) => self.retuned(S::Resuming, &mut call),
            (S::Retuning, E::Statistics77 | E::Statistics) => self.retuned(S::Running, &mut call),
            (S::Measuring, E::Statistics77 | E::Statistics) => match call.forward_statistics() {
                StatisticsOutcome::Reconfigure => call.measurement(MeasurementControl::Reconfigure).outcome(),
                outcome => outcome.outcome(),
            },
            (S::ResumeRetune, E::Statistics77 | E::Statistics) => match call.forward_statistics() {
                StatisticsOutcome::Complete => self.resume(S::Resuming, Outcome::Pending, &mut call),
                outcome => outcome.outcome(),
            },
            (S::SuspendRetune | S::Reconfiguring | S::AutofocusRetune | S::Adjusting, E::Statistics77 | E::Statistics)
            | (S::Adjusting, E::StatisticsSelection) => {
                let outcome = call.forward_statistics();
                if outcome == StatisticsOutcome::Complete { self.state = S::Running; }
                outcome.outcome()
            }
            (S::AutofocusRetune, E::Adjusted) => {
                call.view(ViewRequest::Adjust { selection: true });
                self.state = S::Adjusting;
                Outcome::Pending
            }
            (S::Adjusting, E::Adjusted) => {
                if call.condition(Condition::SelectionPending) {
                    let outcome = call.statistics(StatisticsControl::Refresh);
                    if outcome == StatisticsOutcome::Complete { self.state = S::Running; }
                    return outcome.outcome();
                }
                self.state = S::Running;
                Outcome::Complete
            }
            (S::Reconfiguring, E::Adjusted) => {
                if call.condition(Condition::SelectionPending) {
                    let outcome = call.statistics(StatisticsControl::Refresh);
                    if outcome != StatisticsOutcome::Complete { return outcome.outcome(); }
                }
                self.state = S::Running;
                self.reply(ReplyStatus::Accepted, &mut call);
                Outcome::Complete
            }
            (S::AutofocusRetune, E::StatisticsSelection) => {
                let outcome = call.forward_statistics();
                if outcome != StatisticsOutcome::Complete { return outcome.outcome(); }
                call.effect(Effect::SelectionTaken);
                call.view(ViewRequest::Adjust { selection: false });
                Outcome::Pending
            }
            (S::Reconfiguring, E::StatisticsSelection) => {
                let outcome = call.forward_statistics();
                if outcome != StatisticsOutcome::Complete { return outcome.outcome(); }
                if call.condition(Condition::SelectionPending) {
                    self.state = S::Running;
                    self.reply(ReplyStatus::Accepted, &mut call);
                    return Outcome::Complete;
                }
                call.view(ViewRequest::Adjust { selection: true });
                Outcome::Pending
            }
            (S::Reconfiguring, E::StatisticsSettled) => {
                let outcome = call.forward_statistics();
                if outcome == StatisticsOutcome::Complete { self.state = S::Running; }
                outcome.outcome()
            }
            (S::Measuring, E::OtherImage) => {
                let measured = call.forward_measurement();
                if measured == Measured::Complete { self.state = S::Running; }
                measured.outcome()
            }
            (S::Measuring, E::Calculation) => match call.forward_measurement() {
                Measured::Complete => { self.state = S::Running; Outcome::Complete }
                Measured::Update { update, restarted } => match call.statistics(StatisticsControl::Measured(update)) {
                    StatisticsOutcome::Pending if restarted => { self.state = S::Running; Outcome::Complete }
                    StatisticsOutcome::Pending => Outcome::Pending,
                    outcome => outcome.outcome(),
                },
                Measured::Waiting => Outcome::Pending,
                measured => measured.outcome(),
            },
            (S::Restarting | S::Stopping, E::OtherImage | E::Calculation) => match call.forward_measurement() {
                Measured::Complete => { self.halted(Halted::Measurement, &mut call); Outcome::Pending }
                measured => measured.outcome(),
            },
            (S::Restarting | S::Stopping, E::Statistics77 | E::Statistics) => match call.forward_statistics() {
                StatisticsOutcome::Complete => { self.halted(Halted::Statistics, &mut call); Outcome::Pending }
                outcome => outcome.outcome(),
            },
            (S::Restarting, E::Stopped) => {
                self.ready = Ready::default();
                call.effect(Effect::FinishRecordMode);
                call.clear_preparation();
                self.state = S::Starting;
                call.effect(Effect::Hold);
                call.view(ViewRequest::Start { measured: self.measured });
                Outcome::Pending
            }
            (S::Stopping, E::Stopped) => {
                call.effect(Effect::FinishRecordMode);
                call.clear_preparation();
                if self.restore.any() {
                    self.state = S::Restoring;
                    call.view(ViewRequest::Restore(self.restore));
                    return Outcome::Pending;
                }
                self.state = S::Idle;
                self.reply(ReplyStatus::Accepted, &mut call);
                Outcome::Complete
            }
            (S::StoppingForRefresh, E::Stopped) => self.restart_view(S::Refreshing, &mut call),
            (S::StoppingForAutofocus, E::Stopped) => self.restart_view(S::RefreshingForAutofocus, &mut call),
            (S::StoppingForRefresh, E::Statistics77 | E::Statistics) => match call.forward_statistics() {
                StatisticsOutcome::Complete => { self.stop_view(&mut call); Outcome::Pending }
                outcome => outcome.outcome(),
            },
            (S::StoppingForRefresh, E::Calculation) => match call.forward_measurement() {
                Measured::Complete => match call.statistics(StatisticsControl::Pause) {
                    StatisticsOutcome::Complete => { self.stop_view(&mut call); Outcome::Pending }
                    outcome => outcome.outcome(),
                },
                measured => measured.outcome(),
            },
            (S::StoppingForAutofocus, E::Calculation) => match call.forward_measurement() {
                Measured::Complete => {
                    call.clear_preparation();
                    self.stop_view(&mut call);
                    Outcome::Pending
                }
                measured => measured.outcome(),
            },
            (S::Refreshing, E::Started | E::Restored) => {
                call.effect(Effect::StoreAdjustment);
                self.reconfigure(&mut call)
            }
            (S::RefreshingForAutofocus, E::Started | E::Restored) => self.reconfigure(&mut call),
            (S::Refreshing | S::RefreshingForAutofocus, E::Calculation) => match call.forward_measurement() {
                Measured::Complete => { call.view(ViewRequest::Configure(self.motion)); Outcome::Pending }
                measured => measured.outcome(),
            },
            (S::Refreshing, E::Configured) => {
                let outcome = call.reissue(K::Command1d);
                if outcome == StatisticsOutcome::Complete {
                    self.state = S::Running;
                    call.effect(Effect::ReleaseHold);
                }
                outcome.outcome()
            }
            (S::RefreshingForAutofocus, E::Configured) => {
                if *call.shared == PreparationOverride::Pending { call.set_preparation(); }
                call.effect(Effect::ReleaseHold);
                self.state = S::Running;
                Outcome::Complete
            }
            (S::Refreshing, E::Statistics77 | E::Statistics) => {
                let outcome = call.forward_statistics();
                if outcome == StatisticsOutcome::Complete {
                    self.state = S::Running;
                    call.effect(Effect::ReleaseHold);
                }
                outcome.outcome()
            }
            (S::Configuring | S::StoppingForRefresh | S::StoppingForAutofocus, E::OtherImage)
            | (S::Refreshing | S::RefreshingForAutofocus, E::Accessor)
            | (S::StoppingForAutofocus | S::RefreshingForAutofocus | S::Restoring, E::Statistics77 | E::Statistics) => Outcome::Pending,
            (S::Restoring, E::Restored) | (S::Windowing, E::Windowed) => {
                self.reply(ReplyStatus::Accepted, &mut call);
                self.state = S::Idle;
                Outcome::Complete
            }
            (S::Idle | S::IdleBusy | S::Starting | S::Configuring | S::Suspending | S::RetuneSuspending | S::Resuming
            | S::RetuneResuming | S::Retuning | S::Measuring | S::Stopping, E::Command24) => {
                call.forward_statistics();
                Outcome::Pending
            }
            (S::Idle | S::IdleBusy | S::Starting | S::Configuring | S::Running | S::Suspending | S::RetuneSuspending
            | S::Resuming | S::RetuneResuming | S::Retuning | S::Measuring | S::Stopping, E::StatisticsSettled) => {
                match call.forward_statistics() {
                    StatisticsOutcome::Complete => Outcome::Pending,
                    outcome => outcome.outcome(),
                }
            }
            (state, event) => Self::unhandled(state, event, &mut call),
        }
    }

    /// What a state does with an event it has no handling for: an unknown event is
    /// unhandled, an unexpected command or control is a fault, and a completion that
    /// does not concern the state is dropped.
    fn unhandled<P: Port>(state: State, event: Event, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        use Event as E;
        let quiet = match event {
            E::Other => state == State::Windowing,
            E::LensControl => true,
            E::Suspend | E::Resume => state.ignores_pause(),
            E::Command24 => state == State::Windowing,
            completion if completion.view_completion() => !matches!(state, State::ResumeRetune | State::SuspendRetune),
            completion if completion.quiet_completion() => true,
            _ => false,
        };
        if quiet { return Outcome::Fault; }
        if event == E::Other { return Outcome::Inactive; }
        call.fault()
    }

    fn parameters<'a, P>(&self, call: &Call<'_, '_, 'a, P>) -> Parameters<'a> {
        match call.signal { Signal::Command(command) => command.parameters, _ => stock::parameters(&[]) }
    }

    /// Prepares for the entered command, which the next reply answers.
    fn prepare<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) {
        let command = call.command();
        self.prepare_for(command, call);
    }

    fn prepare_for<P: Port>(&mut self, command: Option<K>, call: &mut Call<'_, '_, '_, P>) {
        self.current = command;
        self.preparation = call.port.prepare(command, call.record);
    }

    fn reply<P: Port>(&mut self, status: ReplyStatus, call: &mut Call<'_, '_, '_, P>) {
        call.port.reply(self.current, status, call.record);
    }

    /// Prepares a view start and decides whether it is for live measurement.
    fn begin<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) {
        self.prepare(call);
        self.ready = Ready::default();
        self.measured = match call.signal {
            Signal::Command(command) => match command.kind {
                K::Command09 => true,
                K::Command02 => word(command.parameters, 4) == 0,
                _ => false,
            },
            _ => false,
        };
    }

    fn start_view<P: Port>(&mut self, measured: bool, call: &mut Call<'_, '_, '_, P>) {
        let direct = call.condition(Condition::DirectStart);
        call.effect(Effect::Hold);
        call.view(if direct { ViewRequest::Start { measured } } else { ViewRequest::Refresh(self.motion) });
    }

    fn restart_view<P: Port>(&mut self, next: State, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        self.state = next;
        self.start_view(true, call);
        Outcome::Pending
    }

    /// Initializes measurement on the started view and configures the view.
    fn configure<P: Port>(&mut self, begin_record_mode: bool, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        match call.measurement(MeasurementControl::Initialize) {
            Measured::Complete => {
                self.state = State::Configuring;
                if begin_record_mode { call.effect(Effect::BeginRecordMode); }
                call.view(ViewRequest::Configure(self.motion));
                self.ready.measurement = true;
                Outcome::Pending
            }
            measured => measured.outcome(),
        }
    }

    fn reconfigure<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        match call.measurement(MeasurementControl::Initialize) {
            Measured::Complete => { call.view(ViewRequest::Configure(self.motion)); Outcome::Pending }
            measured => measured.outcome(),
        }
    }

    /// Continues a started view: configures it for live measurement, or finishes a start
    /// that only showed the view.
    fn after_start<P: Port>(&mut self, begin_record_mode: bool, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        if self.measured {
            call.effect(Effect::StoreAdjustment);
            return self.configure(begin_record_mode, call);
        }
        self.state = State::Idle;
        call.effect(Effect::ReleaseHold);
        self.reply(ReplyStatus::Accepted, call);
        Outcome::Complete
    }

    fn finish_configuration<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        if !self.ready.configured() { return Outcome::Pending; }
        let outcome = if call.condition(Condition::SelectionPending) {
            call.statistics(StatisticsControl::Refresh).outcome()
        } else {
            self.state = State::Running;
            self.reply(ReplyStatus::Accepted, call);
            Outcome::Complete
        };
        call.effect(Effect::ReleaseHold);
        outcome
    }

    fn stop_view<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) {
        self.motion = call.port.motion_setting();
        let mode = match self.preparation.stop {
            StopReason::SecondaryProfile if self.restore.negative => StopMode::SecondaryNegative,
            StopReason::SecondaryProfile if self.restore.any() => StopMode::Secondary,
            StopReason::SecondaryProfile | StopReason::Plain => StopMode::Plain,
            StopReason::Change(code) => StopMode::Change(code),
        };
        self.preparation.stop = StopReason::Plain;
        call.view(ViewRequest::Stop(mode));
    }

    fn halted<P: Port>(&mut self, child: Halted, call: &mut Call<'_, '_, '_, P>) {
        match child {
            Halted::Statistics => self.ready.statistics = true,
            Halted::Measurement => self.ready.measurement = true,
        }
        if self.ready.halted() { self.stop_view(call); }
    }

    /// Stops both children at once; both answering moves to `Stopping`.
    fn halt_together<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> (StatisticsOutcome, Measured) {
        self.ready = Ready::default();
        let statistics = call.statistics(StatisticsControl::Stop);
        let measurement = call.measurement(MeasurementControl::Stop);
        let answered = matches!(statistics, StatisticsOutcome::Complete | StatisticsOutcome::Pending)
            && matches!(measurement, Measured::Complete | Measured::Pending);
        if answered {
            self.state = State::Stopping;
            self.ready.statistics = statistics == StatisticsOutcome::Complete;
            self.ready.measurement = measurement == Measured::Complete;
            if self.ready.halted() { self.stop_view(call); }
        }
        (statistics, measurement)
    }

    /// Stops statistics, then measurement once statistics answered, ahead of a restart.
    fn halt_for_restart<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        self.ready = Ready::default();
        let statistics = call.statistics(StatisticsControl::Stop);
        if !matches!(statistics, StatisticsOutcome::Complete | StatisticsOutcome::Pending) { return statistics.outcome(); }
        self.ready.statistics = statistics == StatisticsOutcome::Complete;
        match call.measurement(MeasurementControl::Stop) {
            measurement @ (Measured::Complete | Measured::Pending) => {
                self.state = State::Restarting;
                self.ready.measurement = measurement == Measured::Complete;
                if self.ready.halted() { self.stop_view(call); }
                Outcome::Pending
            }
            measurement => measurement.outcome(),
        }
    }

    /// Stops both children for a restart; a child still stopping reports later.
    fn halt_tolerant<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        self.ready = Ready::default();
        if call.statistics(StatisticsControl::Stop) == StatisticsOutcome::Complete {
            self.ready.statistics = true;
            if call.measurement(MeasurementControl::Stop) == Measured::Complete { self.halted(Halted::Measurement, call); }
        } else if call.measurement(MeasurementControl::Stop) == Measured::Complete {
            self.ready.measurement = true;
        }
        Outcome::Pending
    }

    /// Stops statistics with `control` and measurement ahead of a view stop.
    fn halt_for_stop<P: Port>(&mut self, control: StatisticsControl, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        self.ready = Ready::default();
        match call.statistics(control) {
            StatisticsOutcome::Complete => self.ready.statistics = true,
            StatisticsOutcome::Fault => return Outcome::Fault,
            _ => {}
        }
        match call.measurement(MeasurementControl::Stop) {
            Measured::Complete => self.ready.measurement = true,
            Measured::Other(Outcome::Fault) => return Outcome::Fault,
            _ => {}
        }
        self.state = State::Stopping;
        if self.ready.halted() { self.stop_view(call); }
        Outcome::Pending
    }

    fn statistics_change<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        let outcome = call.forward_statistics();
        if outcome == StatisticsOutcome::Pending { self.state = State::Reconfiguring; }
        outcome.outcome()
    }

    /// Suspends measurement, then the view; `suspended` is the result once measurement suspended at once.
    fn suspend<P: Port>(&mut self, next: State, suspended: Outcome, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        match call.measurement(MeasurementControl::Suspend) {
            Measured::Pending => { self.state = next; Outcome::Pending }
            Measured::Complete => { self.state = next; call.view(ViewRequest::Suspend); suspended }
            measured => measured.outcome(),
        }
    }

    fn resume<P: Port>(&mut self, next: State, resumed: Outcome, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        match call.measurement(MeasurementControl::Resume) {
            Measured::Pending => { self.state = next; Outcome::Pending }
            Measured::Complete => { self.state = next; call.view(ViewRequest::Resume); resumed }
            measured => measured.outcome(),
        }
    }

    fn retune<P: Port>(&mut self, next: State, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        call.apply();
        let outcome = call.forward_statistics();
        if outcome == StatisticsOutcome::Pending { self.state = next; }
        outcome.outcome()
    }

    fn retuned<P: Port>(&mut self, next: State, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        match call.forward_statistics() {
            StatisticsOutcome::Complete => { self.state = next; Outcome::Complete }
            StatisticsOutcome::Reconfigure => call.measurement(MeasurementControl::Reconfigure).outcome(),
            outcome => outcome.outcome(),
        }
    }

    /// A completion measurement handles while measurement or the view pauses; `request` is
    /// the view request measurement's completion releases.
    fn after_measurement<P: Port>(&mut self, request: ViewRequest, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        match call.forward_measurement() {
            Measured::Complete => { call.view(request); Outcome::Pending }
            measured => measured.outcome(),
        }
    }

    fn paused_calculation<P: Port>(&mut self, request: ViewRequest, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        match call.forward_measurement() {
            Measured::Complete => { call.view(request); Outcome::Pending }
            Measured::Update { update, restarted } => match call.statistics(StatisticsControl::Measured(update)) {
                StatisticsOutcome::Pending => {
                    if restarted { call.view(request); }
                    Outcome::Pending
                }
                outcome => outcome.outcome(),
            },
            Measured::Waiting => Outcome::Pending,
            measured => measured.outcome(),
        }
    }

    fn running_command17<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        self.prepare(call);
        match call.apply() {
            Application::Restart => {
                self.state = State::Restarting;
                self.ready = Ready::default();
                let statistics = call.statistics(StatisticsControl::Stop);
                if !matches!(statistics, StatisticsOutcome::Complete | StatisticsOutcome::Pending) { return statistics.outcome(); }
                self.ready.statistics = statistics == StatisticsOutcome::Complete;
                match call.measurement(MeasurementControl::Stop) {
                    Measured::Complete => { self.halted(Halted::Measurement, call); Outcome::Pending }
                    Measured::Pending => Outcome::Pending,
                    measured => measured.outcome(),
                }
            }
            Application::Refused => {
                self.reply(ReplyStatus::Rejected, call);
                Outcome::Complete
            }
            Application::Applied if !call.condition(Condition::PrimaryApplied) && !call.condition(Condition::SelectionActive) => {
                self.restore = RestoreOptions::default();
                self.ready = Ready { statistics: true, ..Ready::default() };
                self.measured = false;
                let measurement = call.measurement(MeasurementControl::Stop);
                self.state = State::Stopping;
                if measurement != Measured::Pending { self.halted(Halted::Measurement, call); }
                Outcome::Pending
            }
            Application::Applied => {
                let measured = call.forward_measurement();
                if measured == Measured::Pending { self.state = State::Measuring; }
                measured.outcome()
            }
        }
    }

    fn running_autofocus<P: Port>(&mut self, call: &mut Call<'_, '_, '_, P>) -> Outcome {
        let application = call.apply();
        if call.condition(Condition::ViewGeometryChanged) {
            self.state = State::StoppingForAutofocus;
            if call.measurement(MeasurementControl::Stop) != Measured::Complete { return Outcome::Pending; }
            call.clear_preparation();
            self.stop_view(call);
            return Outcome::Pending;
        }
        if application == Application::Restart {
            self.state = State::Restarting;
            self.prepare(call);
            return self.halt_tolerant(call);
        }
        let measured = call.forward_measurement();
        if measured != Measured::Complete { return measured.outcome(); }
        let statistics = call.forward_statistics();
        if statistics == StatisticsOutcome::Pending {
            self.state = State::Reconfiguring;
            return Outcome::Pending;
        }
        if call.condition(Condition::AutofocusAreaChanged) {
            self.state = State::Reconfiguring;
            self.prepare_for(None, call);
            call.view(ViewRequest::Adjust { selection: true });
            return Outcome::Pending;
        }
        if !call.condition(Condition::AutofocusModeChanged) { return statistics.outcome(); }
        if !call.condition(Condition::AutofocusRetune) { return Outcome::Complete; }
        self.state = State::AutofocusRetune;
        call.statistics(StatisticsControl::Autofocus);
        Outcome::Pending
    }
}

#[derive(Clone, Copy)]
enum Halted { Statistics, Measurement }
