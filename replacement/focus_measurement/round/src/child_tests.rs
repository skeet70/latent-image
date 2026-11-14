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
fn a_start_before_initialization_answers_zero_without_work_or_a_fault() {
    let record = stock::record_handle_from_a2(1);
    let mut acquisition = S1Acquisition::new();
    let mut ports = Ports::new(None, ());
    assert_eq!(acquisition.handle(control(AcquisitionControl::Start, record), &mut ports), ChildOutcome::Fault);
    assert!(ports.take().is_empty());
    assert_eq!(ports.faults, 0);
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

fn completed(completion: Completion, record: RecordHandle) -> ChildEvent<'static, AcquisitionControl> {
    ChildEvent::Completed { completed: Completed { completion, parameters: stock::parameters(&[]) }, record }
}

#[test]
fn s1_runs_four_primary_passes_then_restores_selects_and_posts_focus_on_each_cycle() {
    use crate::acquisition::ports::*;
    use crate::acquisition_fixtures as fixture;
    use crate::fakes::ShotCall;
    use xm1_saclac_interface::round_acquisition::*;
    let record = stock::record_handle_from_a2(1);
    let mut child = S1Acquisition::new();
    let mut ports = crate::fakes::Ports::new(None, ());
    fixture::configure(&mut ports);
    ports.acquisition.memory.mark_value_changed();
    assert_eq!(child.handle(control(AcquisitionControl::Initialize, record), &mut ports), ChildOutcome::Complete);
    assert_eq!(ports.acquisition.s1_level, 310);
    ports.take();
    let result = fixture::selection_result();
    ports.acquisition.acquisition_result = Some(fixture::primary_result());
    ports.acquisition.selection_result = Some(result);
    for cycle in 0..2 {
        if cycle == 1 { ports.acquisition.no_program = true; }
        for pass in 0..4 {
            assert_eq!(child.handle(control(AcquisitionControl::Start, record), &mut ports), ChildOutcome::Pending);
            assert_eq!(ports.take(), [Outbound::Acquisition(AcquisitionTraffic::Acquisition(Branch::S1, fixture::primary_request(Branch::S1, cycle == 0 && pass == 0)))]);
            assert_eq!(ports.acquisition.submitted_grids.last(), Some(&xm1_saclac_interface::calc::AcquisitionGrid([0; 1236])));
            // A duplicate start while pending must not send another request.
            assert_eq!(child.handle(control(AcquisitionControl::Start, record), &mut ports), ChildOutcome::Pending);
            assert!(ports.take().is_empty());
            assert_eq!(child.handle(completed(Completion::Calculation9b, record), &mut ports), ChildOutcome::Pending);
            assert_eq!(ports.acquisition.s1_level, 900);
            assert_eq!(ports.acquisition.block.eligible, 1);
            let traffic = ports.take();
            let released = Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Acquisition(fixture::primary_result().address)));
            if pass < 3 {
                assert_eq!(traffic, [released]);
            } else {
                assert_eq!(traffic, [released, Outbound::Acquisition(AcquisitionTraffic::Selection(Branch::S1, fixture::selection_request(Branch::S1, cycle == 0, true)))]);
                assert_eq!(ports.acquisition.restored_settings.last(), Some(&(Branch::S1, RestoredSetting { margin: 65488, reference: 100, lens_halfword: 25, lens_reference: 26 })));
                assert_eq!(ports.acquisition.windows.last().map(|entry| entry.0), Some(Branch::S1));
                assert_eq!(ports.acquisition.settings_written.last().map(|entry| (entry.0, entry.1.margin, entry.1.selection, entry.1.program_word)), Some((Branch::S1, 65488, 1, 38)));
            }
        }
        assert_eq!(child.handle(completed(Completion::Calculation9c, record), &mut ports), ChildOutcome::Pending);
        let selected = SelectionNotice { kind: 3, level: 120, word: 0x8181, limit_level: 135, flags: [1, 1], recording: 1,
            levels: [131, 132, 133, 134], position: 0x8282 };
        let focus = FocusTable {
            primary: SelectionEntry { timing: 0x8080, code: 0x44, selected: 2, gain_step: 3, exposure: 0x8000, level: 204,
                second: 238, offset: 212, word: 0, reference: 122, decade: 126 },
            secondary: SelectionEntry { timing: 0x8080, code: 0x44, selected: 2, gain_step: 3, exposure: 0x9000, level: 304,
                second: 338, offset: 312, word: 0, reference: 122, decade: 126 },
        };
        let released = Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Selection { result: result.address, shot: None }));
        assert_eq!(ports.take(), [
            Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Selection { result: result.address, shot: Some((result.shot_record, result.saved_record)) })),
            Outbound::Acquisition(AcquisitionTraffic::Selected(selected)), released,
            Outbound::Acquisition(AcquisitionTraffic::FocusTable(focus)), released,
        ]);
        assert_eq!(ports.acquisition.saved_publications, std::vec![ports.acquisition.saved; cycle + 1]);
        assert_eq!(ports.acquisition.restore_levels, Some((122, [118, 119])));
        assert_eq!(ports.acquisition.slot, SelectionSlot { word: 0x8181, level: 120, kind: 3, limit_level: 135, flags: [1, 1], levels: [131, 132, 133, 134] });
        assert_eq!(ports.acquisition.positions[0], [0x8282, 0x8181]);
        assert_eq!(ports.acquisition.positions[3], [0x8282, 0x8181]);
        assert_eq!(ports.acquisition.selection, SelectionRecord { position: 0x8282, word: 0x8181, level: 120, levels: [131, 132, 133, 134],
            target: 121, limit_level: 135, reference: 114, flags: [1, 2, 3, 1], limit: 129, tracked: 1 });
        if cycle == 0 {
            assert!(ports.acquisition.shot_calls.ends_with(&[ShotCall::Write(result.shot_record), ShotCall::Copy(result.saved_record), ShotCall::Apply(0x8282), ShotCall::ValueChange]));
        } else {
            assert!(ports.acquisition.shot_calls.ends_with(&[ShotCall::Write(result.shot_record), ShotCall::Save(0x8282, result.save, 0x8181)]));
        }
    }
    assert_eq!(child.handle(control(AcquisitionControl::Stop, record), &mut ports), ChildOutcome::Complete);
    assert_eq!(ports.acquisition.slot.word, 26);
    assert!(!ports.acquisition.memory.value_changed());
    assert!(ports.take().is_empty());
    assert_eq!(ports.faults, 0);
}

#[test]
fn s1_stop_drains_primary_or_secondary_without_submitting_more_work_or_a_focus_table() {
    use crate::acquisition_fixtures as fixture;
    use crate::acquisition::ports::ReleasedResult;
    for secondary in [false, true] {
        let record = stock::record_handle_from_a2(1);
        let mut child = S1Acquisition::new();
        let mut ports = Ports::new(None, ());
        fixture::configure(&mut ports);
        child.handle(control(AcquisitionControl::Initialize, record), &mut ports);
        ports.acquisition.acquisition_result = Some(fixture::primary_result());
        ports.acquisition.selection_result = Some(fixture::selection_result());
        for _ in 0..if secondary { 4 } else { 1 } {
            child.handle(control(AcquisitionControl::Start, record), &mut ports);
            if secondary { child.handle(completed(Completion::Calculation9b, record), &mut ports); }
        }
        ports.take();
        assert_eq!(child.handle(control(AcquisitionControl::Stop, record), &mut ports), ChildOutcome::Pending);
        assert!(ports.take().is_empty());
        let completion = if secondary { Completion::Calculation9c } else { Completion::Calculation9b };
        assert_eq!(child.handle(completed(completion, record), &mut ports), ChildOutcome::Complete);
        let traffic = ports.take();
        if secondary {
            assert!(traffic.contains(&Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Selection {
                result: fixture::selection_result().address, shot: Some((fixture::selection_result().shot_record, fixture::selection_result().saved_record)),
            }))));
        } else {
            assert_eq!(traffic, [Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Acquisition(fixture::primary_result().address)))]);
        }
        assert!(!traffic.iter().any(|event| matches!(event, Outbound::Acquisition(AcquisitionTraffic::FocusTable(_) | AcquisitionTraffic::Selection(_, _) | AcquisitionTraffic::Acquisition(_, _)))));
        assert_eq!(ports.acquisition.slot.word, 26);
        assert_eq!(child.handle(control(AcquisitionControl::Start, record), &mut ports), ChildOutcome::Fault);
        assert!(ports.take().is_empty());
        assert_eq!(ports.faults, 0);
    }
}
