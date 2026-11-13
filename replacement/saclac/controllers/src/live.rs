#![forbid(unsafe_code)]

use xm1_saclac_interface::{CommandKind, Message, RecordHandle};
use crate::root::Outcome;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile { Primary, Secondary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control { Suspend, Resume }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request<'a> {
    Message(Message<'a>),
    Command(CommandKind),
    Control(Control),
}

pub trait Port {
    fn live(&mut self, request: Request<'_>, record: RecordHandle) -> Outcome;
    fn lens(&mut self, request: Request<'_>, record: RecordHandle) -> Outcome;
    fn profile_available(&mut self, profile: Profile, record: RecordHandle) -> bool;
    fn publish_profile(&mut self, profile: Profile, record: RecordHandle);
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage { Suspend, Lens, Live, Resume }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pending {
    profile: Profile,
    record: RecordHandle,
    stage: Stage,
    paired: bool,
}

pub struct Profiles { pending: Option<Pending> }

impl Default for Profiles { fn default() -> Self { Self::new() } }

impl Profiles {
    pub const fn new() -> Self { Self { pending: None } }

    pub fn begin(&mut self, profile: Profile, record: RecordHandle, port: &mut impl Port) -> Outcome {
        if self.pending.is_some() { return Self::fault(port); }
        let paired = profile == Profile::Primary && port.profile_available(profile, record);
        let stage = if paired { Stage::Suspend } else { Stage::Live };
        self.advance(Pending { profile, record, stage, paired }, false, port)
    }

    pub fn handle(&mut self, message: Message<'_>, port: &mut impl Port) -> Outcome {
        let Some(pending) = self.pending else { return Outcome::Inactive; };
        let request = Request::Message(message);
        let outcome = match pending.stage {
            Stage::Lens => port.lens(request, pending.record),
            _ => port.live(request, pending.record),
        };
        match outcome {
            Outcome::Complete => {
                self.pending = None;
                if let Some(stage) = Self::next(pending, true, port) {
                    self.advance(Pending { stage, ..pending }, true, port)
                } else {
                    port.publish_profile(pending.profile, pending.record);
                    Outcome::Complete
                }
            }
            Outcome::Pending | Outcome::Inactive => outcome,
            Outcome::Fault => Self::fault(port),
        }
    }

    fn advance(&mut self, mut pending: Pending, waited: bool, port: &mut impl Port) -> Outcome {
        let command = match pending.profile {
            Profile::Primary => CommandKind::Command02,
            Profile::Secondary => CommandKind::Command03,
        };
        loop {
            let outcome = match pending.stage {
                Stage::Suspend => port.live(Request::Control(Control::Suspend), pending.record),
                Stage::Lens => port.lens(Request::Command(command), pending.record),
                Stage::Live => port.live(Request::Command(command), pending.record),
                Stage::Resume => port.live(Request::Control(Control::Resume), pending.record),
            };
            match outcome {
                Outcome::Complete => {
                    if let Some(stage) = Self::next(pending, waited, port) { pending.stage = stage; }
                    else {
                        port.publish_profile(pending.profile, pending.record);
                        return Outcome::Complete;
                    }
                }
                Outcome::Pending => { self.pending = Some(pending); return Outcome::Pending; }
                Outcome::Inactive => return Outcome::Inactive,
                Outcome::Fault => return Self::fault(port),
            }
        }
    }

    fn next(pending: Pending, waited: bool, port: &mut impl Port) -> Option<Stage> {
        match (pending.profile, pending.stage) {
            (_, Stage::Suspend) => Some(Stage::Lens),
            (Profile::Primary, Stage::Lens) => Some(Stage::Live),
            (Profile::Primary, Stage::Live) if pending.paired && waited => Some(Stage::Resume),
            (Profile::Secondary, Stage::Live) if port.profile_available(Profile::Secondary, pending.record) => {
                Some(if waited { Stage::Suspend } else { Stage::Lens })
            }
            _ => None,
        }
    }

    fn fault(port: &mut impl Port) -> Outcome { port.fault(); Outcome::Fault }
}
