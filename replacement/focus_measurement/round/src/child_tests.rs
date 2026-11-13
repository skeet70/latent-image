use crate::acquisition::Branch;
use crate::children::S1Acquisition;
use crate::fakes::{AcquisitionTraffic, Outbound, Ports};
use xm1_saclac_controllers::children::{AcquisitionControl, ChildEvent, ChildOutcome, MeasurementOutcome, RetainedRoundStatus};
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};

fn control(control: AcquisitionControl, record: RecordHandle) -> ChildEvent<'static, AcquisitionControl> {
    ChildEvent::Control { control, record, parameters: stock::parameters(&[]) }
}

fn s1_requests(ports: &mut Ports) -> usize {
    ports.take().iter().filter(|traffic| matches!(traffic, Outbound::Acquisition(AcquisitionTraffic::Acquisition(Branch::S1, _)))).count()
}

#[test]
fn a_started_s1_child_requests_the_primary_evaluation_on_each_start() {
    let record = stock::record_handle_from_a2(1);
    let mut acquisition = S1Acquisition::new();
    let mut ports = Ports::new(None, ());
    assert_eq!(acquisition.handle(control(AcquisitionControl::Initialize, record), &mut ports), ChildOutcome::Complete);
    assert_eq!(acquisition.handle(control(AcquisitionControl::Start, record), &mut ports), ChildOutcome::Pending);
    assert_eq!(s1_requests(&mut ports), 1);
    let completed = Completed { completion: Completion::Calculation9b, parameters: stock::parameters(&[]) };
    assert_eq!(acquisition.handle(ChildEvent::Completed { completed, record }, &mut ports), ChildOutcome::Pending);
    assert_eq!(ports.faults, 0);
}

#[test]
fn invalid_initialization_faults_without_submitting_a_job() {
    let record = stock::record_handle_from_a2(1);
    let mut acquisition = S1Acquisition::new();
    let mut ports = Ports::new(None, ());
    acquisition.handle(control(AcquisitionControl::Initialize, record), &mut ports);
    ports.take();
    assert_eq!(acquisition.handle(control(AcquisitionControl::Initialize, record), &mut ports), ChildOutcome::Fault);
    assert!(ports.take().is_empty());
    assert_eq!(ports.faults, 1);
}

#[test]
fn unrelated_completion_keeps_the_retained_round_status() {
    let record = stock::record_handle_from_a2(1);
    let mut acquisition = S1Acquisition::new();
    let mut ports = Ports::new(None, ());
    let completed = Completed { completion: Completion::Calculation87, parameters: stock::parameters(&[]) };
    assert_eq!(acquisition.handle(ChildEvent::Completed { completed, record }, &mut ports), ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)));
    assert!(ports.take().is_empty());
    assert_eq!(ports.faults, 0);
}
