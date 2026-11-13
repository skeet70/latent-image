use crate::acquisition::{Branch, Effect};
use crate::children::{AcquisitionEffects, S1Acquisition};
use xm1_saclac_controllers::children::{AcquisitionControl, ChildEvent, ChildOutcome, MeasurementOutcome, RetainedRoundStatus};
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};
use std::vec::Vec;

#[derive(Default)]
struct Effects { events: Vec<(RecordHandle, Branch, Effect)>, faults: usize }
impl AcquisitionEffects for Effects {
    fn effect(&mut self, record: RecordHandle, branch: Branch, effect: Effect) { self.events.push((record, branch, effect)); }
    fn selection_changed(&mut self, _: RecordHandle) -> bool { false }
    fn fault(&mut self) { self.faults += 1; }
}

fn control(control: AcquisitionControl, record: RecordHandle) -> ChildEvent<'static, AcquisitionControl> {
    ChildEvent::Control { control, record, parameters: stock::parameters(&[]) }
}

#[test]
fn typed_s1_child_keeps_the_record_attached_to_each_effect() {
    let first = stock::record_handle_from_a2(1);
    let second = stock::record_handle_from_a2(2);
    let mut acquisition = S1Acquisition::new();
    let mut effects = Effects::default();
    assert_eq!(acquisition.handle(control(AcquisitionControl::Initialize, first), &mut effects), ChildOutcome::Complete);
    assert_eq!(acquisition.handle(control(AcquisitionControl::Start, second), &mut effects), ChildOutcome::Pending);
    assert_eq!(effects.events[0].0, first);
    assert_eq!(effects.events[1].0, second);
    let completed = Completed { completion: Completion::Calculation9b, parameters: stock::parameters(&[]) };
    assert_eq!(acquisition.handle(ChildEvent::Completed { completed, record: first }, &mut effects), ChildOutcome::Pending);
    assert_eq!(effects.events.last().unwrap().0, first);
    assert_eq!(effects.faults, 0);
}

#[test]
fn invalid_initialization_faults_without_submitting_a_job() {
    let record = stock::record_handle_from_a2(1);
    let mut acquisition = S1Acquisition::new();
    let mut effects = Effects::default();
    acquisition.handle(control(AcquisitionControl::Initialize, record), &mut effects);
    let prior = effects.events.clone();
    assert_eq!(acquisition.handle(control(AcquisitionControl::Initialize, record), &mut effects), ChildOutcome::Fault);
    assert_eq!(effects.events, prior);
    assert_eq!(effects.faults, 1);
}

#[test]
fn unrelated_completion_keeps_the_retained_round_status() {
    let record = stock::record_handle_from_a2(1);
    let mut acquisition = S1Acquisition::new();
    let mut effects = Effects::default();
    let completed = Completed { completion: Completion::Calculation87, parameters: stock::parameters(&[]) };
    assert_eq!(acquisition.handle(ChildEvent::Completed { completed, record }, &mut effects), ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)));
    assert!(effects.events.is_empty());
    assert_eq!(effects.faults, 0);
}
