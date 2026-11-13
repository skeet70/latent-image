#![forbid(unsafe_code)]

//! The router root enters while live processing runs. Every child call carries
//! the record the root supplied with the event.

use crate::root::{Control as RootControl, Input, Outcome};
use xm1_saclac_interface::{CommandKind, Completion, Message, Parameters, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile { Primary, Secondary }

impl Profile {
    pub const fn command(self) -> CommandKind {
        match self {
            Self::Primary => CommandKind::Command02,
            Self::Secondary => CommandKind::Command03,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerEvent<'a> {
    Forward(Input<'a>),
    /// The command is entered again without a new queue message.
    Reissue(CommandKind),
    Suspend,
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensEvent<'a> {
    Forward(Input<'a>),
    /// The command is entered again without a new queue message.
    Reissue(CommandKind),
    /// Sent while the controller has not yet suspended for a drive.
    Prepare,
    /// The drive-stopped completion, entered without a new queue message.
    DriveStopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveOrigin { Command, Cancel }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect<'a> {
    ReplyProfile(Profile),
    ReplyAutofocus,
    PrepareAutofocusLens,
    /// Carries the lens completion whose payload the report quotes.
    ReportLensStatus(Parameters<'a>),
    CaptureLensPosition,
    RecordDriveOrigin(DriveOrigin),
    /// Carries the drive-stopped completion whose result is recorded.
    RecordDriveResult(Parameters<'a>),
}

pub trait Port {
    fn controller(&mut self, event: ControllerEvent<'_>, record: RecordHandle) -> Outcome;
    fn lens(&mut self, event: LensEvent<'_>, record: RecordHandle) -> Outcome;
    fn lens_follows(&mut self, profile: Profile, record: RecordHandle) -> bool;
    fn effect(&mut self, effect: Effect<'_>, record: RecordHandle);
    fn drive_origin(&mut self) -> DriveOrigin;
    /// Whether a stopped drive is checked for a correction.
    fn drive_check_applies(&mut self, record: RecordHandle) -> bool;
    /// Issues a correcting drive when one is needed; `Pending` when issued.
    fn correct_drive(&mut self, record: RecordHandle) -> Outcome;
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrimaryStage { Suspending, Lens, Unpaired, Paired, Resuming }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SecondaryStage { Live, Suspending, Lens, Resuming }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DriveStage { Suspending, Prepared, Lens, Resuming }

/// Whether the controller still holds a forwarded Command11 during a drive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Hold { Free, Command11 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    Primary(PrimaryStage),
    Secondary(SecondaryStage),
    Autofocus,
    Command23,
    Command11,
    Drive(DriveStage, Hold),
    Command14,
    Command2b,
    Command2c,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event<'a> {
    Profile(Profile),
    Autofocus,
    Command23,
    Drive(DriveOrigin),
    Command11,
    Command14,
    Command2b,
    Command2c,
    Command2d,
    Lens33(Parameters<'a>),
    Lens34,
    Lens37,
    Lens35,
    Lens3d,
    Statistics6e,
    Statistics82,
    Image47,
    Image66,
    Other,
}

impl<'a> Event<'a> {
    fn of(input: Input<'a>) -> Self {
        use CommandKind as K;
        use Completion as C;
        match input {
            Input::Control(RootControl::Cancel) => Self::Drive(DriveOrigin::Cancel),
            Input::Control(_) => Self::Other,
            Input::Message(Message::Command(command)) => match command.kind {
                K::Command02 => Self::Profile(Profile::Primary),
                K::Command03 => Self::Profile(Profile::Secondary),
                K::Autofocus => Self::Autofocus,
                K::Command23 => Self::Command23,
                K::LensDrive => Self::Drive(DriveOrigin::Command),
                K::Command11 => Self::Command11,
                K::Command14 => Self::Command14,
                K::Command2b => Self::Command2b,
                K::Command2c => Self::Command2c,
                K::Command2d => Self::Command2d,
                _ => Self::Other,
            },
            Input::Message(Message::Completed(done)) => match done.completion {
                C::Lens33 => Self::Lens33(done.parameters),
                C::Lens34 => Self::Lens34,
                C::Lens37 => Self::Lens37,
                C::Lens35 => Self::Lens35,
                C::Lens3d => Self::Lens3d,
                C::Statistics6e => Self::Statistics6e,
                C::Statistics82 => Self::Statistics82,
                C::Image47 => Self::Image47,
                C::Image66 => Self::Image66,
                _ => Self::Other,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct Joint { lens: Outcome, controller: Outcome }

impl Joint {
    fn waiting(self) -> Outcome {
        if self.lens == Outcome::Inactive && self.controller == Outcome::Inactive { Outcome::Inactive } else { Outcome::Pending }
    }
}

struct Call<'p, 'i, P> { port: &'p mut P, input: Input<'i>, record: RecordHandle }

impl<P: Port> Call<'_, '_, P> {
    fn controller(&mut self) -> Outcome { self.port.controller(ControllerEvent::Forward(self.input), self.record) }
    fn lens(&mut self) -> Outcome { self.port.lens(LensEvent::Forward(self.input), self.record) }
    fn controller_event(&mut self, event: ControllerEvent<'_>) -> Outcome { self.port.controller(event, self.record) }
    fn lens_event(&mut self, event: LensEvent<'_>) -> Outcome { self.port.lens(event, self.record) }
    fn effect(&mut self, effect: Effect<'_>) { self.port.effect(effect, self.record) }
}

pub struct Router { state: State }

impl Default for Router { fn default() -> Self { Self::new() } }

impl Router {
    pub const fn new() -> Self { Self { state: State::Idle } }

    pub fn handle(&mut self, input: Input<'_>, record: RecordHandle, port: &mut impl Port) -> Outcome {
        let event = Event::of(input);
        match (event, input) {
            (Event::Drive(origin), _) => port.effect(Effect::RecordDriveOrigin(origin), record),
            (Event::Lens34, Input::Message(Message::Completed(done))) => port.effect(Effect::RecordDriveResult(done.parameters), record),
            _ => {}
        }
        let mut call = Call { port, input, record };
        match (self.state, event) {
            (State::Idle, Event::Profile(Profile::Primary)) => self.begin_primary(&mut call),
            (State::Idle, Event::Profile(Profile::Secondary)) => self.begin_secondary(&mut call),
            (State::Primary(stage), Event::Other) => self.primary(stage, &mut call),
            (State::Secondary(stage), Event::Other) => self.secondary(stage, &mut call),
            (State::Idle, Event::Autofocus) => {
                call.effect(Effect::PrepareAutofocusLens);
                let outcome = call.controller();
                match outcome {
                    Outcome::Complete => call.effect(Effect::ReplyAutofocus),
                    Outcome::Pending => self.state = State::Autofocus,
                    _ => {}
                }
                outcome
            }
            (State::Autofocus, Event::Other) => {
                let outcome = call.controller();
                if outcome == Outcome::Complete {
                    self.state = State::Idle;
                    call.effect(Effect::ReplyAutofocus);
                }
                outcome
            }
            (State::Command11 | State::Drive(..), Event::Autofocus) => {
                call.controller();
                call.effect(Effect::ReplyAutofocus);
                Outcome::Pending
            }
            (State::Idle, Event::Command23) => self.hold(State::Command23, call.controller(), false),
            (State::Command23, Event::Other) => self.release(call.controller()),
            (State::Idle, Event::Drive(_)) => self.begin_drive(Hold::Free, &mut call),
            (State::Command11, Event::Drive(_)) => self.begin_drive(Hold::Command11, &mut call),
            (State::Drive(stage, hold), Event::Other) => self.drive(stage, hold, &mut call),
            (State::Drive(DriveStage::Lens, _), Event::Lens33(parameters)) => {
                call.effect(Effect::ReportLensStatus(parameters));
                Outcome::Pending
            }
            (State::Drive(DriveStage::Lens, hold), Event::Lens34) => {
                if call.port.drive_origin() == DriveOrigin::Cancel && call.port.drive_check_applies(record) {
                    let corrected = call.port.correct_drive(record);
                    if corrected != Outcome::Complete { return corrected; }
                }
                self.drive_stopped(hold, &mut call)
            }
            (State::Drive(DriveStage::Lens, hold), Event::Lens37) => self.drive_stopped(hold, &mut call),
            (State::Idle, Event::Command11) => self.hold(State::Command11, call.controller(), true),
            (State::Drive(stage, Hold::Free), Event::Command11) => match call.controller() {
                Outcome::Complete => Outcome::Pending,
                Outcome::Pending => { self.state = State::Drive(stage, Hold::Command11); Outcome::Pending }
                outcome => outcome,
            },
            (State::Command11, Event::Statistics6e) => self.release(call.controller()),
            (State::Drive(stage, Hold::Command11), Event::Statistics6e) => match call.controller() {
                Outcome::Complete => { self.state = State::Drive(stage, Hold::Free); Outcome::Pending }
                outcome => outcome,
            },
            (State::Command11, Event::Other) => {
                call.lens();
                call.controller()
            }
            (State::Idle, Event::Command14) => self.hold(State::Command14, call.controller(), true),
            (State::Command14, Event::Image47) => self.release(call.controller()),
            (State::Idle, Event::Command2b) => self.hold(State::Command2b, call.controller(), true),
            (State::Command2b, Event::Image66) => self.release(call.controller()),
            (State::Idle, Event::Statistics82) => call.controller(),
            (State::Primary(_) | State::Secondary(_) | State::Command11 | State::Drive(..), Event::Statistics82) => {
                call.controller();
                Outcome::Pending
            }
            (State::Idle, Event::Command2c) => self.hold(State::Command2c, call.lens(), true),
            (State::Command2c, Event::Lens3d) => self.release(call.lens()),
            (State::Idle, Event::Command2d) => call.lens(),
            (State::Command2c, Event::Command2d) => {
                call.lens();
                Outcome::Pending
            }
            (_, Event::Command2d) => Outcome::Fault,
            (State::Command14 | State::Command2b | State::Command2c, Event::Other) => call.controller(),
            (State::Idle, Event::Other) => {
                let lens = call.lens();
                if lens == Outcome::Fault { return Outcome::Fault; }
                let controller = call.controller();
                if controller == Outcome::Fault { return Outcome::Fault; }
                if lens == Outcome::Complete || controller == Outcome::Complete { Outcome::Complete }
                else { Joint { lens, controller }.waiting() }
            }
            _ => {
                call.port.fault();
                Outcome::Fault
            }
        }
    }

    /// Enters `pending` when the child is pending. With `settle`, a refused or
    /// inactive child is reported as pending.
    fn hold(&mut self, pending: State, outcome: Outcome, settle: bool) -> Outcome {
        match outcome {
            Outcome::Complete => Outcome::Complete,
            Outcome::Pending => { self.state = pending; Outcome::Pending }
            _ if settle => Outcome::Pending,
            outcome => outcome,
        }
    }

    fn release(&mut self, outcome: Outcome) -> Outcome {
        if outcome == Outcome::Complete { self.state = State::Idle; }
        outcome
    }

    fn finish_profile<P: Port>(&mut self, profile: Profile, call: &mut Call<'_, '_, P>) -> Outcome {
        self.state = State::Idle;
        call.effect(Effect::ReplyProfile(profile));
        Outcome::Complete
    }

    fn begin_primary<P: Port>(&mut self, call: &mut Call<'_, '_, P>) -> Outcome {
        if !call.port.lens_follows(Profile::Primary, call.record) {
            return match call.controller() {
                Outcome::Complete => self.finish_profile(Profile::Primary, call),
                Outcome::Pending => { self.state = State::Primary(PrimaryStage::Unpaired); Outcome::Pending }
                outcome => outcome,
            };
        }
        match call.controller_event(ControllerEvent::Suspend) {
            Outcome::Complete => {}
            Outcome::Pending => { self.state = State::Primary(PrimaryStage::Suspending); return Outcome::Pending; }
            outcome => return outcome,
        }
        match call.lens() {
            Outcome::Complete => {}
            Outcome::Pending => { self.state = State::Primary(PrimaryStage::Lens); return Outcome::Pending; }
            outcome => return outcome,
        }
        match call.controller() {
            Outcome::Complete => self.finish_profile(Profile::Primary, call),
            Outcome::Pending => { self.state = State::Primary(PrimaryStage::Paired); Outcome::Pending }
            outcome => outcome,
        }
    }

    fn primary<P: Port>(&mut self, stage: PrimaryStage, call: &mut Call<'_, '_, P>) -> Outcome {
        let reissue = ControllerEvent::Reissue(CommandKind::Command02);
        match stage {
            PrimaryStage::Suspending => {
                let outcome = call.controller();
                if outcome != Outcome::Complete { return outcome; }
                match call.lens_event(LensEvent::Reissue(CommandKind::Command02)) {
                    Outcome::Complete => self.primary_live(reissue, call),
                    Outcome::Pending => { self.state = State::Primary(PrimaryStage::Lens); Outcome::Pending }
                    outcome => outcome,
                }
            }
            PrimaryStage::Lens => {
                call.controller();
                match call.lens() {
                    Outcome::Complete => self.primary_live(reissue, call),
                    Outcome::Fault => Outcome::Fault,
                    _ => Outcome::Pending,
                }
            }
            PrimaryStage::Unpaired | PrimaryStage::Resuming => {
                let outcome = call.controller();
                if outcome == Outcome::Complete { self.finish_profile(Profile::Primary, call) } else { outcome }
            }
            PrimaryStage::Paired => {
                let outcome = call.controller();
                if outcome != Outcome::Complete { return outcome; }
                self.resume_profile(Profile::Primary, call)
            }
        }
    }

    fn primary_live<P: Port>(&mut self, reissue: ControllerEvent<'_>, call: &mut Call<'_, '_, P>) -> Outcome {
        match call.controller_event(reissue) {
            Outcome::Complete => self.resume_profile(Profile::Primary, call),
            Outcome::Pending => { self.state = State::Primary(PrimaryStage::Paired); Outcome::Pending }
            outcome => outcome,
        }
    }

    fn resume_profile<P: Port>(&mut self, profile: Profile, call: &mut Call<'_, '_, P>) -> Outcome {
        match call.controller_event(ControllerEvent::Resume) {
            Outcome::Complete => self.finish_profile(profile, call),
            Outcome::Pending => {
                self.state = match profile {
                    Profile::Primary => State::Primary(PrimaryStage::Resuming),
                    Profile::Secondary => State::Secondary(SecondaryStage::Resuming),
                };
                Outcome::Pending
            }
            outcome => outcome,
        }
    }

    fn begin_secondary<P: Port>(&mut self, call: &mut Call<'_, '_, P>) -> Outcome {
        match call.controller() {
            Outcome::Complete => {}
            Outcome::Pending => { self.state = State::Secondary(SecondaryStage::Live); return Outcome::Pending; }
            outcome => return outcome,
        }
        if !call.port.lens_follows(Profile::Secondary, call.record) {
            return self.finish_profile(Profile::Secondary, call);
        }
        match call.lens() {
            Outcome::Complete => self.finish_profile(Profile::Secondary, call),
            Outcome::Pending => { self.state = State::Secondary(SecondaryStage::Lens); Outcome::Pending }
            outcome => outcome,
        }
    }

    fn secondary<P: Port>(&mut self, stage: SecondaryStage, call: &mut Call<'_, '_, P>) -> Outcome {
        let reissue = LensEvent::Reissue(CommandKind::Command03);
        match stage {
            SecondaryStage::Live => {
                let outcome = call.controller();
                if outcome != Outcome::Complete { return outcome; }
                if !call.port.lens_follows(Profile::Secondary, call.record) {
                    return self.finish_profile(Profile::Secondary, call);
                }
                match call.controller_event(ControllerEvent::Suspend) {
                    Outcome::Complete => {}
                    Outcome::Pending => { self.state = State::Secondary(SecondaryStage::Suspending); return Outcome::Pending; }
                    outcome => return outcome,
                }
                match call.lens_event(reissue) {
                    Outcome::Complete => self.finish_profile(Profile::Secondary, call),
                    Outcome::Pending => { self.state = State::Secondary(SecondaryStage::Lens); Outcome::Pending }
                    outcome => outcome,
                }
            }
            SecondaryStage::Suspending => {
                let outcome = call.controller();
                if outcome != Outcome::Complete { return outcome; }
                match call.lens_event(reissue) {
                    Outcome::Complete => self.resume_profile(Profile::Secondary, call),
                    Outcome::Pending => { self.state = State::Secondary(SecondaryStage::Lens); Outcome::Pending }
                    outcome => outcome,
                }
            }
            SecondaryStage::Lens => {
                let controller = call.controller();
                if !matches!(controller, Outcome::Pending | Outcome::Inactive) { return Outcome::Fault; }
                match call.lens() {
                    Outcome::Complete => self.resume_profile(Profile::Secondary, call),
                    Outcome::Fault => Outcome::Fault,
                    lens => Joint { lens, controller }.waiting(),
                }
            }
            SecondaryStage::Resuming => {
                let outcome = call.controller();
                if outcome == Outcome::Complete { self.finish_profile(Profile::Secondary, call) } else { outcome }
            }
        }
    }

    fn begin_drive<P: Port>(&mut self, hold: Hold, call: &mut Call<'_, '_, P>) -> Outcome {
        match call.controller_event(ControllerEvent::Suspend) {
            Outcome::Complete => {}
            Outcome::Pending => {
                self.state = State::Drive(DriveStage::Suspending, hold);
                return call.lens_event(LensEvent::Prepare);
            }
            outcome => return outcome,
        }
        match call.lens() {
            Outcome::Complete => {}
            Outcome::Pending => { self.state = State::Drive(DriveStage::Lens, hold); return Outcome::Pending; }
            outcome => return outcome,
        }
        match call.controller_event(ControllerEvent::Resume) {
            Outcome::Complete => match hold {
                Hold::Free => Outcome::Complete,
                Hold::Command11 => Outcome::Pending,
            },
            Outcome::Pending => { self.state = State::Drive(DriveStage::Resuming, hold); Outcome::Pending }
            outcome => outcome,
        }
    }

    fn drive_done(&mut self, hold: Hold) -> Outcome {
        match hold {
            Hold::Free => { self.state = State::Idle; Outcome::Complete }
            Hold::Command11 => { self.state = State::Command11; Outcome::Pending }
        }
    }

    /// Issues the drive after the controller suspended and the lens prepared.
    fn issue_drive<P: Port>(&mut self, hold: Hold, then_stop: bool, call: &mut Call<'_, '_, P>) -> Outcome {
        match call.lens_event(LensEvent::Reissue(CommandKind::LensDrive)) {
            Outcome::Complete => match call.controller_event(ControllerEvent::Resume) {
                Outcome::Complete => self.drive_done(hold),
                Outcome::Pending => { self.state = State::Drive(DriveStage::Resuming, hold); Outcome::Pending }
                outcome => outcome,
            },
            Outcome::Pending => {
                self.state = State::Drive(DriveStage::Lens, hold);
                if then_stop { call.lens_event(LensEvent::Reissue(CommandKind::Command10)); }
                Outcome::Pending
            }
            outcome => outcome,
        }
    }

    fn drive_stopped<P: Port>(&mut self, hold: Hold, call: &mut Call<'_, '_, P>) -> Outcome {
        call.effect(Effect::CaptureLensPosition);
        match call.controller_event(ControllerEvent::Resume) {
            Outcome::Complete => match call.lens() {
                Outcome::Complete => self.drive_done(hold),
                _ => Outcome::Fault,
            },
            outcome => { self.state = State::Drive(DriveStage::Resuming, hold); outcome }
        }
    }

    fn drive<P: Port>(&mut self, stage: DriveStage, hold: Hold, call: &mut Call<'_, '_, P>) -> Outcome {
        match (stage, hold) {
            (DriveStage::Suspending | DriveStage::Prepared, Hold::Free) => {
                let lens = call.lens();
                if lens == Outcome::Fault { return Outcome::Fault; }
                let controller = call.controller();
                if controller == Outcome::Fault { return Outcome::Fault; }
                let lens_done = lens == Outcome::Complete;
                let suspended = controller == Outcome::Complete;
                if stage == DriveStage::Suspending && lens_done && !suspended {
                    self.state = State::Drive(DriveStage::Prepared, hold);
                    Outcome::Pending
                } else if suspended && !lens_done {
                    self.issue_drive(hold, stage == DriveStage::Prepared, call)
                } else {
                    Joint { lens, controller }.waiting()
                }
            }
            (DriveStage::Suspending, Hold::Command11) => {
                if call.lens() == Outcome::Complete {
                    self.state = State::Drive(DriveStage::Prepared, hold);
                    return Outcome::Pending;
                }
                let controller = call.controller();
                if controller != Outcome::Complete { return controller; }
                self.issue_drive(hold, false, call)
            }
            (DriveStage::Prepared, Hold::Command11) => {
                call.lens();
                let controller = call.controller();
                if controller != Outcome::Complete { return controller; }
                self.issue_drive(hold, true, call)
            }
            (DriveStage::Lens, _) => {
                let lens = call.lens();
                if !matches!(lens, Outcome::Pending | Outcome::Inactive) { return Outcome::Fault; }
                let controller = call.controller();
                if !matches!(controller, Outcome::Pending | Outcome::Inactive) { return Outcome::Fault; }
                Joint { lens, controller }.waiting()
            }
            (DriveStage::Resuming, _) => {
                let lens = call.lens();
                if lens == Outcome::Fault { return Outcome::Fault; }
                let lens = if lens == Outcome::Inactive { Outcome::Inactive } else { Outcome::Pending };
                match call.controller() {
                    Outcome::Complete => match call.lens_event(LensEvent::DriveStopped) {
                        Outcome::Complete => self.drive_done(hold),
                        _ => Outcome::Fault,
                    },
                    Outcome::Fault => Outcome::Fault,
                    controller => Joint { lens, controller }.waiting(),
                }
            }
        }
    }
}
