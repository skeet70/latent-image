#![forbid(unsafe_code)]

use crate::acquisition::s1::S1Work;
use crate::acquisition::{self, Acquisition, Branch, Completion, Control, S1Steps};
use crate::jobs::Outcome;
use xm1_saclac_controllers::children::{AcquisitionControl, ChildEvent, ChildOutcome, MeasurementOutcome, RetainedRoundStatus};
use xm1_saclac_interface::Completion as PeerCompletion;

/// What the S1 acquisition child works through.
pub trait Ports: acquisition::ports::Ports {
    fn fault(&mut self);
}

pub struct S1Acquisition {
    acquisition: Acquisition,
    work: S1Work,
}

impl Default for S1Acquisition {
    fn default() -> Self { Self::new() }
}

impl S1Acquisition {
    pub const fn new() -> Self { Self { acquisition: Acquisition::new(Branch::S1), work: S1Work::new() } }

    pub fn handle(&mut self, event: ChildEvent<'_, AcquisitionControl>, ports: &mut impl Ports) -> ChildOutcome {
        let (record, control, completed) = match event {
            ChildEvent::Control { control, record, .. } => (record, Some(match control {
                AcquisitionControl::Initialize => Control::Initialize,
                AcquisitionControl::Start => Control::Primary,
                AcquisitionControl::Stop => Control::Stop,
            }), None),
            ChildEvent::Completed { completed, record } => (record, None, Some(completed)),
            ChildEvent::Command { .. } => {
                return ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03));
            }
        };
        let mut steps = S1Steps { record, completed, work: &mut self.work, ports: &mut *ports };
        let outcome = if let Some(control) = control {
            match self.acquisition.control(control, &mut steps) {
                Ok(outcome) => outcome,
                Err(_) => {
                    ports.fault();
                    return ChildOutcome::Fault;
                }
            }
        } else {
            match completed.map(|completed| completed.completion) {
                Some(PeerCompletion::Calculation9b) => self.acquisition.completed(Completion::Primary, &mut steps),
                Some(PeerCompletion::Calculation9c) => self.acquisition.completed(Completion::Secondary, &mut steps),
                _ => return ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)),
            }
        };
        match outcome {
            Outcome::Complete => ChildOutcome::Complete,
            Outcome::Pending => ChildOutcome::Pending,
            Outcome::NoWork => ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status0c)),
            Outcome::Ignored => ChildOutcome::Fault,
            Outcome::Updated(_) | Outcome::SelectionUpdated => {
                ports.fault();
                ChildOutcome::Fault
            }
        }
    }
}

#[cfg(saclac_qualification)]
pub mod qualification;
