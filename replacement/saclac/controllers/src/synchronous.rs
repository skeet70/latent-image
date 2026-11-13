#![forbid(unsafe_code)]

use xm1_saclac_interface::{ClientCallback, CompletionStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    Progress,
    Finished,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Reply { callback: ClientCallback, status: CompletionStatus },
    Occupied,
    Released,
}

pub trait Effects {
    fn emit(&mut self, effect: Effect);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Accepted,
    Busy,
}

#[derive(Default)]
pub struct Clients {
    pending: Option<ClientCallback>,
}

impl Clients {
    pub const fn new() -> Self {
        Self { pending: None }
    }

    pub fn reserve(&mut self, callback: ClientCallback, effects: &mut impl Effects) -> Admission {
        if self.pending.is_some() {
            effects.emit(Effect::Reply { callback, status: CompletionStatus(-95) });
            return Admission::Busy;
        }
        self.pending = Some(callback);
        effects.emit(Effect::Occupied);
        Admission::Accepted
    }

    pub fn complete(&mut self, completion: Completion, effects: &mut impl Effects) {
        if completion == Completion::Finished {
            if let Some(callback) = self.pending {
                effects.emit(Effect::Reply { callback, status: CompletionStatus(0) });
                self.pending = None;
                effects.emit(Effect::Released);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensPositionCode(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoviePhase {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Readiness {
    Ready,
    Busy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageStatus(pub u32);

pub trait Queries {
    fn position_availability(&mut self) -> Availability;
    fn position(&mut self) -> LensPositionCode;
    fn motion_availability(&mut self) -> Availability;
    fn motion(&mut self) -> crate::adjustment::LensPosition;
    fn report_missing_lens(&mut self);
    fn image_primary(&mut self) -> ImageStatus;
    fn image_secondary(&mut self) -> ImageStatus;
    fn movie_readiness(&mut self, phase: MoviePhase) -> Readiness;
}

pub fn lens_position(queries: &mut impl Queries) -> LensPositionCode {
    match queries.position_availability() {
        Availability::Unavailable => LensPositionCode(128),
        Availability::Available => queries.position(),
    }
}

pub fn lens_motion(queries: &mut impl Queries) -> crate::adjustment::LensPosition {
    match queries.motion_availability() {
        Availability::Unavailable => {
            queries.report_missing_lens();
            crate::adjustment::LensPosition(0)
        }
        Availability::Available => queries.motion(),
    }
}

pub fn notify(callback: Option<ClientCallback>, effects: &mut impl Effects) {
    if let Some(callback) = callback {
        effects.emit(Effect::Reply { callback, status: CompletionStatus(0) });
    }
}

pub fn movie_status(phase: MoviePhase, queries: &mut impl Queries) -> Readiness {
    queries.movie_readiness(phase)
}
