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
    lens: crate::adjustment::Adjustment<ClientCallback>,
}

struct ReservationEffects<'a, E>(&'a mut E);

impl<E: Effects> crate::adjustment::Effects<ClientCallback> for ReservationEffects<'_, E> {
    fn emit(&mut self, effect: crate::adjustment::Effect<ClientCallback>) {
        use crate::adjustment::{Effect as LensEffect, Reply};
        match effect {
            LensEffect::Reply { callback, result } => {
                let status = match result {
                    Reply::Success => 0,
                    Reply::Busy => -95,
                    Reply::SelectionOutOfRange => -94,
                };
                self.0.emit(Effect::Reply { callback, status: CompletionStatus(status) });
                if result == Reply::Success { self.0.emit(Effect::Released); }
            }
            LensEffect::Request(_) | LensEffect::PublishQuery { .. } | LensEffect::SelectionRejected => {}
        }
    }
}

impl Clients {
    pub const fn new() -> Self {
        Self { lens: crate::adjustment::Adjustment::new() }
    }

    pub fn reserve(&mut self, callback: ClientCallback, effects: &mut impl Effects) -> Admission {
        let admission = if self.lens.is_busy() { Admission::Busy } else { Admission::Accepted };
        self.lens.begin(callback, crate::adjustment::LensRequest::ReserveOnly, &mut ReservationEffects(effects));
        if admission == Admission::Accepted { effects.emit(Effect::Occupied); }
        admission
    }

    pub fn begin_lens(
        &mut self,
        callback: ClientCallback,
        request: crate::adjustment::LensRequest,
        effects: &mut impl crate::adjustment::Effects<ClientCallback>,
    ) {
        self.lens.begin(callback, request, effects);
    }

    pub fn configure_lens(
        &mut self,
        callback: ClientCallback,
        selection: crate::adjustment::Selection,
        limit: crate::adjustment::SelectionLimit,
        value: crate::adjustment::DriveAmount,
        effects: &mut impl crate::adjustment::Effects<ClientCallback>,
    ) {
        self.lens.configure(callback, selection, limit, value, effects);
    }

    pub fn configure_default_lens(
        &mut self,
        callback: ClientCallback,
        effects: &mut impl crate::adjustment::Effects<ClientCallback>,
    ) {
        self.lens.configure_default(callback, effects);
    }

    pub const fn selection_rejected(&self) -> bool { self.lens.selection_rejected() }

    pub fn complete_lens(
        &mut self,
        completion: crate::adjustment::Completion,
        effects: &mut impl crate::adjustment::Effects<ClientCallback>,
    ) {
        self.lens.complete(completion, effects);
    }

    pub fn complete(&mut self, completion: Completion, effects: &mut impl Effects) {
        let completion = match completion {
            Completion::Progress => crate::adjustment::Completion::Progress,
            Completion::Finished => crate::adjustment::Completion::Finished,
        };
        self.lens.complete(completion, &mut ReservationEffects(effects));
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
