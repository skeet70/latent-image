#![forbid(unsafe_code)]

//! The S1 branch: the router root enters during a half press and the bridge
//! that carries S1-time commands to the live controller.

use crate::root::{Control as RootControl, Input, Outcome};
use xm1_saclac_interface::{CommandKind, Completion, Message, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShotEvent<'a> { Forward(Input<'a>), Begin, Release }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsEvent<'a> {
    Forward(Input<'a>),
    Begin,
    Prepare,
    /// `Prepare` for a record whose lens may be cancelled.
    PrepareCancellable,
    Cancel,
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureEvent<'a> { Forward(Input<'a>), Begin }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensEvent<'a> { Forward(Input<'a>), Reissue(CommandKind) }

/// Entered from the bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveEvent<'a> { Forward(Input<'a>), Reissue(CommandKind) }

/// The S1 progress other subsystems read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Waiting { pub shot: bool, pub capture: bool, pub bridge: bool, pub release: bool }

impl Waiting {
    const fn any_work(self) -> bool { self.shot || self.capture || self.bridge }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Published {
    Waiting(Waiting),
    /// Shot starts still owed after the current one.
    Deferred(u8),
    LensStopRequested(bool),
    ShotIdle(bool),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    ResetShotProgress,
    ApplyHalfPressOption,
    ResetCaptureRecord,
    BeginBusy,
    EndBusy,
    NotifyBridge(CommandKind),
    ReplyAutofocus,
}

pub trait Port {
    fn shot(&mut self, event: ShotEvent<'_>, record: RecordHandle) -> Outcome;
    fn statistics(&mut self, event: StatisticsEvent<'_>, record: RecordHandle) -> Outcome;
    fn capture(&mut self, event: CaptureEvent<'_>, record: RecordHandle) -> Outcome;
    fn lens(&mut self, event: LensEvent<'_>, record: RecordHandle) -> Outcome;
    fn live(&mut self, event: LiveEvent<'_>, record: RecordHandle) -> Outcome;
    fn publish(&mut self, published: Published);
    fn effect(&mut self, effect: Effect, record: RecordHandle);
    /// Whether a second shot start follows the first.
    fn second_shot(&mut self, record: RecordHandle) -> bool;
    fn lens_cancellable(&mut self, record: RecordHandle) -> bool;
    fn release_allowed(&mut self) -> bool;
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    Shooting,
    ShotDeferred,
    Statistics,
    Capturing,
    Bridging,
    CapturingCommand08,
    LensStopping,
    LensDriving,
    StatisticsResuming,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event<'a> {
    Query,
    HalfPress,
    Command07,
    Command09,
    Begin,
    Release,
    Command17,
    Command27,
    Command08,
    Cancel,
    Command0f,
    Command10,
    Lens33,
    Lens34,
    Other(Input<'a>),
}

impl<'a> Event<'a> {
    fn of(input: Input<'a>) -> Self {
        use CommandKind as K;
        match input {
            Input::Control(RootControl::QueryS1) => Self::Query,
            Input::Control(RootControl::BeginS1) => Self::Begin,
            Input::Control(RootControl::ReleaseS1) => Self::Release,
            Input::Control(RootControl::Cancel) => Self::Cancel,
            Input::Message(Message::Command(command)) => match command.kind {
                K::HalfPress => Self::HalfPress,
                K::Command07 => Self::Command07,
                K::Command09 => Self::Command09,
                K::Command17 => Self::Command17,
                K::Command27 => Self::Command27,
                K::Command08 => Self::Command08,
                K::Cancel => Self::Command0f,
                K::Command10 => Self::Command10,
                _ => Self::Other(input),
            },
            Input::Message(Message::Completed(done)) => match done.completion {
                Completion::Lens33 => Self::Lens33,
                Completion::Lens34 => Self::Lens34,
                _ => Self::Other(input),
            },
            Input::Control(_) => Self::Other(input),
        }
    }
}

pub struct Router {
    state: State,
    waiting: Waiting,
    deferred: u8,
    lens_stop: bool,
    bridge: Bridge,
}

impl Default for Router { fn default() -> Self { Self::new() } }

struct Call<'p, P> { port: &'p mut P, record: RecordHandle }

impl<P: Port> Call<'_, P> {
    fn shot(&mut self, event: ShotEvent<'_>) -> Outcome { self.port.shot(event, self.record) }
    fn statistics(&mut self, event: StatisticsEvent<'_>) -> Outcome { self.port.statistics(event, self.record) }
    fn capture(&mut self, event: CaptureEvent<'_>) -> Outcome { self.port.capture(event, self.record) }
    fn lens(&mut self, event: LensEvent<'_>) -> Outcome { self.port.lens(event, self.record) }
    fn effect(&mut self, effect: Effect) { self.port.effect(effect, self.record) }
}

impl Router {
    pub const fn new() -> Self {
        Self { state: State::Idle, waiting: Waiting { shot: false, capture: false, bridge: false, release: false }, deferred: 0, lens_stop: false, bridge: Bridge::new() }
    }

    pub const fn waiting(&self) -> Waiting { self.waiting }

    pub fn handle(&mut self, input: Input<'_>, record: RecordHandle, port: &mut impl Port) -> Outcome {
        let mut call = Call { port, record };
        let state = self.state;
        match (state, Event::of(input)) {
            (State::Idle | State::Shooting | State::ShotDeferred | State::Capturing | State::Bridging | State::CapturingCommand08, Event::Query) => Outcome::Complete,
            (_, Event::Query) => Outcome::Pending,
            (State::Idle | State::Shooting, Event::HalfPress) => {
                self.wait_shot(&mut call);
                call.effect(Effect::ResetShotProgress);
                call.effect(Effect::ApplyHalfPressOption);
                if state == State::Idle { call.effect(Effect::ResetCaptureRecord); }
                Outcome::Pending
            }
            (State::ShotDeferred, Event::HalfPress) => {
                self.state = State::Shooting;
                self.take_deferred(&mut call);
                call.shot(ShotEvent::Begin)
            }
            (State::Statistics, Event::HalfPress) => {
                self.wait_shot(&mut call);
                let outcome = call.statistics(StatisticsEvent::Prepare);
                if outcome != Outcome::Complete { return outcome; }
                self.state = State::Shooting;
                self.start_shot(&mut call)
            }
            (State::Idle | State::Shooting | State::ShotDeferred, Event::Command07) => {
                self.set_waiting(&mut call, |w| w.capture = true);
                Outcome::Pending
            }
            (State::Statistics, Event::Command07) => {
                self.set_waiting(&mut call, |w| w.capture = true);
                let prepare = if call.port.lens_cancellable(call.record) { StatisticsEvent::PrepareCancellable } else { StatisticsEvent::Prepare };
                let outcome = call.statistics(prepare);
                if outcome != Outcome::Complete { return outcome; }
                self.begin_capture(&mut call)
            }
            (State::Idle | State::Shooting | State::ShotDeferred, Event::Command09) => {
                self.set_waiting(&mut call, |w| w.bridge = true);
                Outcome::Pending
            }
            (State::Statistics, Event::Command09) => {
                self.set_waiting(&mut call, |w| w.bridge = true);
                let outcome = call.statistics(StatisticsEvent::Prepare);
                if outcome != Outcome::Complete { return outcome; }
                self.begin_bridge(&mut call)
            }
            (State::Idle, Event::Begin) => {
                call.effect(Effect::BeginBusy);
                let second = self.waiting.shot && call.port.second_shot(call.record);
                self.deferred = if !self.waiting.shot { 0 } else if second { 2 } else { 1 };
                call.port.publish(Published::Deferred(self.deferred));
                if self.waiting.shot {
                    self.state = State::Shooting;
                    let outcome = self.start_shot(&mut call);
                    if outcome != Outcome::Complete { return outcome; }
                    if self.waiting.capture { return self.begin_capture(&mut call); }
                    self.state = State::Statistics;
                    return call.statistics(StatisticsEvent::Begin);
                }
                if self.waiting.capture { return self.begin_capture(&mut call); }
                if self.waiting.bridge { return self.begin_bridge(&mut call); }
                Outcome::Pending
            }
            (State::Idle | State::Shooting | State::ShotDeferred | State::Statistics, Event::Release) => {
                if state == State::Idle && !self.waiting.any_work() {
                    call.port.fault();
                    return Outcome::Fault;
                }
                self.set_waiting(&mut call, |w| w.release = true);
                match state {
                    State::Idle | State::Shooting => {
                        if call.port.release_allowed() { call.shot(ShotEvent::Release) } else { Outcome::Pending }
                    }
                    State::ShotDeferred => Outcome::Pending,
                    _ => {
                        let outcome = call.statistics(StatisticsEvent::Prepare);
                        if outcome != Outcome::Complete { return outcome; }
                        self.finish(&mut call)
                    }
                }
            }
            (State::Statistics, Event::Command17 | Event::Command27) => call.statistics(StatisticsEvent::Forward(input)),
            (State::Bridging, Event::Command17) => self.bridge.handle(input, record, call.port),
            (State::Idle, Event::Command08) => {
                self.state = State::CapturingCommand08;
                call.capture(CaptureEvent::Forward(input))
            }
            (State::Statistics, Event::Cancel) => {
                if call.statistics(StatisticsEvent::Cancel) == Outcome::Complete {
                    self.state = State::LensDriving;
                    call.lens(LensEvent::Forward(input));
                } else {
                    self.state = State::LensStopping;
                }
                Outcome::Pending
            }
            (State::LensStopping, Event::Command10) => {
                self.lens_stop = true;
                call.port.publish(Published::LensStopRequested(true));
                Outcome::Pending
            }
            (State::LensStopping, Event::Command0f | Event::Lens33 | Event::Lens34) => Outcome::Pending,
            (State::LensDriving, Event::Command0f | Event::Command10 | Event::Lens33 | Event::Lens34) => {
                if call.lens(LensEvent::Forward(input)) == Outcome::Complete { self.resume_statistics(&mut call); }
                Outcome::Pending
            }
            (State::Idle, Event::Other(input)) => call.capture(CaptureEvent::Forward(input)),
            (State::Shooting, Event::Other(input)) => {
                let outcome = call.shot(ShotEvent::Forward(input));
                if outcome != Outcome::Complete { return outcome; }
                if self.waiting.shot {
                    self.state = State::Shooting;
                    return self.start_shot(&mut call);
                }
                if self.deferred != 0 {
                    self.state = State::ShotDeferred;
                    return Outcome::Pending;
                }
                self.next_after_statistics(&mut call).unwrap_or_else(|| {
                    self.state = State::Statistics;
                    call.statistics(StatisticsEvent::Begin)
                })
            }
            (State::ShotDeferred, Event::Other(input)) => call.shot(ShotEvent::Forward(input)),
            (State::Statistics, Event::Other(input)) => {
                let outcome = call.statistics(StatisticsEvent::Forward(input));
                if outcome != Outcome::Complete { return outcome; }
                if self.waiting.shot {
                    self.state = State::Shooting;
                    return self.start_shot(&mut call);
                }
                self.next_after_statistics(&mut call).unwrap_or(Outcome::Pending)
            }
            (State::Capturing, Event::Other(input)) => {
                let outcome = call.capture(CaptureEvent::Forward(input));
                if outcome != Outcome::Complete { return outcome; }
                self.state = State::Idle;
                call.effect(Effect::EndBusy);
                Outcome::Complete
            }
            (State::Bridging, Event::Other(input)) => {
                let outcome = self.bridge.handle(input, record, call.port);
                if outcome != Outcome::Complete { return outcome; }
                self.state = State::Idle;
                call.effect(Effect::EndBusy);
                Outcome::Complete
            }
            (State::CapturingCommand08, Event::Other(input)) => {
                let outcome = call.capture(CaptureEvent::Forward(input));
                if outcome == Outcome::Complete { self.state = State::Idle; }
                outcome
            }
            (State::LensStopping, Event::Other(input)) => {
                if call.statistics(StatisticsEvent::Forward(input)) == Outcome::Complete {
                    if self.lens_stop {
                        self.lens_stop = false;
                        call.port.publish(Published::LensStopRequested(false));
                        call.lens(LensEvent::Reissue(CommandKind::Command10));
                        self.resume_statistics(&mut call);
                    } else {
                        self.state = State::LensDriving;
                        call.lens(LensEvent::Forward(Input::Control(RootControl::Cancel)));
                    }
                }
                Outcome::Pending
            }
            (State::LensDriving, Event::Other(input)) => {
                call.statistics(StatisticsEvent::Forward(input));
                Outcome::Pending
            }
            (State::StatisticsResuming, Event::Other(input)) => {
                if call.statistics(StatisticsEvent::Forward(input)) == Outcome::Complete { self.state = State::Statistics; }
                Outcome::Pending
            }
            _ => {
                call.port.fault();
                Outcome::Fault
            }
        }
    }

    fn set_waiting<P: Port>(&mut self, call: &mut Call<'_, P>, change: impl FnOnce(&mut Waiting)) {
        change(&mut self.waiting);
        call.port.publish(Published::Waiting(self.waiting));
    }

    fn wait_shot<P: Port>(&mut self, call: &mut Call<'_, P>) {
        self.set_waiting(call, |w| w.shot = true);
        call.port.publish(Published::ShotIdle(false));
    }

    fn take_deferred<P: Port>(&mut self, call: &mut Call<'_, P>) {
        if self.deferred != 0 {
            self.deferred -= 1;
            call.port.publish(Published::Deferred(self.deferred));
        }
    }

    fn start_shot<P: Port>(&mut self, call: &mut Call<'_, P>) -> Outcome {
        self.set_waiting(call, |w| w.shot = false);
        call.port.publish(Published::ShotIdle(true));
        self.take_deferred(call);
        call.shot(ShotEvent::Begin)
    }

    fn begin_capture<P: Port>(&mut self, call: &mut Call<'_, P>) -> Outcome {
        self.state = State::Capturing;
        self.set_waiting(call, |w| w.capture = false);
        call.capture(CaptureEvent::Begin)
    }

    fn begin_bridge<P: Port>(&mut self, call: &mut Call<'_, P>) -> Outcome {
        self.state = State::Bridging;
        self.set_waiting(call, |w| w.bridge = false);
        self.bridge.begin(call.record, call.port)
    }

    fn finish<P: Port>(&mut self, call: &mut Call<'_, P>) -> Outcome {
        self.state = State::Idle;
        self.set_waiting(call, |w| w.release = false);
        call.effect(Effect::EndBusy);
        Outcome::Complete
    }

    /// The next owed S1 step once statistics no longer need to run first.
    fn next_after_statistics<P: Port>(&mut self, call: &mut Call<'_, P>) -> Option<Outcome> {
        if self.waiting.capture { return Some(self.begin_capture(call)); }
        if self.waiting.bridge { return Some(self.begin_bridge(call)); }
        if self.waiting.release { return Some(self.finish(call)); }
        None
    }

    fn resume_statistics<P: Port>(&mut self, call: &mut Call<'_, P>) {
        self.state = if call.statistics(StatisticsEvent::Resume) == Outcome::Complete { State::Statistics } else { State::StatisticsResuming };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BridgeState { Idle, Ready, Finishing }

/// Carries S1-time commands to the live controller until Command0a ends them.
pub struct Bridge { state: BridgeState }

impl Default for Bridge { fn default() -> Self { Self::new() } }

impl Bridge {
    pub const fn new() -> Self { Self { state: BridgeState::Idle } }

    pub fn begin(&mut self, record: RecordHandle, port: &mut impl Port) -> Outcome {
        if self.state != BridgeState::Idle {
            port.fault();
            return Outcome::Fault;
        }
        port.effect(Effect::NotifyBridge(CommandKind::Command09), record);
        if port.live(LiveEvent::Reissue(CommandKind::Command09), record) == Outcome::Complete { self.state = BridgeState::Ready; }
        Outcome::Pending
    }

    /// Command17, Autofocus and Command0a are refused, without the fault
    /// reaction, unless the bridge is ready.
    pub fn handle(&mut self, input: Input<'_>, record: RecordHandle, port: &mut impl Port) -> Outcome {
        use CommandKind as K;
        let command = match input {
            Input::Message(Message::Command(command)) => Some(command.kind),
            _ => None,
        };
        let ending = match (self.state, command) {
            (BridgeState::Ready, Some(K::Command0a)) | (BridgeState::Finishing, None) => true,
            (BridgeState::Finishing, Some(kind)) => !matches!(kind, K::Command17 | K::Autofocus | K::Command0a),
            (BridgeState::Idle, Some(K::Command17 | K::Autofocus | K::Command0a)) => return Outcome::Fault,
            _ => false,
        };
        if self.state == BridgeState::Finishing && !ending { return Outcome::Fault; }
        let outcome = port.live(LiveEvent::Forward(input), record);
        if command == Some(K::Autofocus) { port.effect(Effect::ReplyAutofocus, record); }
        match (ending, outcome) {
            (true, Outcome::Complete) => {
                self.state = BridgeState::Idle;
                port.effect(Effect::NotifyBridge(CommandKind::Command0a), record);
                Outcome::Complete
            }
            (true, _) => {
                self.state = BridgeState::Finishing;
                Outcome::Pending
            }
            (false, Outcome::Complete) => {
                self.state = BridgeState::Ready;
                Outcome::Pending
            }
            (false, _) => Outcome::Pending,
        }
    }
}
