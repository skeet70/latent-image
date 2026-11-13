#![forbid(unsafe_code)]

use crate::acquisition::{Acquisition, Branch, Completion, Control, Effect, Ports};
use crate::jobs::Outcome;
use xm1_saclac_controllers::children::{AcquisitionControl, ChildEvent, ChildOutcome, MeasurementOutcome, RetainedRoundStatus};
use xm1_saclac_interface::{Completion as PeerCompletion, RecordHandle};

pub trait AcquisitionEffects {
    fn effect(&mut self, record: RecordHandle, branch: Branch, effect: Effect);
    fn selection_changed(&mut self, record: RecordHandle) -> bool;
    fn fault(&mut self);
}

struct BoundEffects<'a, P> { record: RecordHandle, effects: &'a mut P }
impl<P: AcquisitionEffects> Ports for BoundEffects<'_, P> {
    fn effect(&mut self, branch: Branch, effect: Effect) { self.effects.effect(self.record, branch, effect); }
    fn selection_changed(&mut self) -> bool { self.effects.selection_changed(self.record) }
}

pub struct S1Acquisition {
    acquisition: Acquisition,
}

impl Default for S1Acquisition {
    fn default() -> Self { Self::new() }
}

impl S1Acquisition {
    pub const fn new() -> Self { Self { acquisition: Acquisition::new(Branch::S1) } }

    pub fn handle(&mut self, event: ChildEvent<'_, AcquisitionControl>, effects: &mut impl AcquisitionEffects) -> ChildOutcome {
        let (record, control, completion) = match event {
            ChildEvent::Control { control, record, .. } => (record, Some(match control {
                AcquisitionControl::Initialize => Control::Initialize,
                AcquisitionControl::Start => Control::Primary,
                AcquisitionControl::Stop => Control::Stop,
            }), None),
            ChildEvent::Completed { completed, record } => (record, None, Some(completed.completion)),
            ChildEvent::Command { .. } => {
                return ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03));
            }
        };
        let mut ports = BoundEffects { record, effects };
        let outcome = if let Some(control) = control {
            match self.acquisition.control(control, &mut ports) {
                Ok(outcome) => outcome,
                Err(_) => {
                    ports.effects.fault();
                    return ChildOutcome::Fault;
                }
            }
        } else {
            match completion {
                Some(PeerCompletion::Calculation9b) => self.acquisition.completed(Completion::Primary, &mut ports),
                Some(PeerCompletion::Calculation9c) => self.acquisition.completed(Completion::Secondary, &mut ports),
                _ => return ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)),
            }
        };
        match outcome {
            Outcome::Complete => ChildOutcome::Complete,
            Outcome::Pending => ChildOutcome::Pending,
            Outcome::NoWork => ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status0c)),
            Outcome::Unhandled => ChildOutcome::Fault,
            Outcome::Updated(_) | Outcome::SelectionUpdated => {
                ports.effects.fault();
                ChildOutcome::Fault
            }
        }
    }
}
