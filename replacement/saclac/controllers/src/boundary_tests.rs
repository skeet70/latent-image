use crate::stock::{write_diagnostic, DiagnosticRow, DiagnosticSnapshot, DiagnosticState};

#[test]
fn diagnostic_dump_places_all_rows_before_all_states() {
    let mut rows = [DiagnosticRow(0); 32];
    rows[0] = DiagnosticRow(0x1234);
    rows[1] = DiagnosticRow(0x5678);
    rows[31] = DiagnosticRow(0x9abc);
    let mut states = [DiagnosticState(0); 32];
    states[0] = DiagnosticState(0xdef0);
    states[1] = DiagnosticState(0x1357);
    states[31] = DiagnosticState(0x2468);
    let mut output = [0xff; 128];
    write_diagnostic(&DiagnosticSnapshot { rows, states }, &mut output);
    assert_eq!(&output[..4], &[0x34, 0x12, 0x78, 0x56]);
    assert_eq!(&output[4..62], &[0; 58]);
    assert_eq!(&output[62..68], &[0xbc, 0x9a, 0xf0, 0xde, 0x57, 0x13]);
    assert_eq!(&output[68..126], &[0; 58]);
    assert_eq!(&output[126..], &[0x68, 0x24]);
}

#[test]
fn synchronous_lens_progress_keeps_the_waiting_client() {
    use crate::synchronous::Completion::{Finished, Progress};
    use xm1_saclac_interface::CompletionStatus;
    for status in [0, 2] {
        assert_eq!(crate::stock::synchronous_completion(CompletionStatus(status)), Finished);
    }
    for status in [-95, -1, 1, 3, 19, 20, 21] {
        assert_eq!(crate::stock::synchronous_completion(CompletionStatus(status)), Progress);
    }
}

#[test]
fn measurement_feedback_preserves_update_kind_and_restart_completion() {
    use crate::children::{ChildOutcome, MeasurementOutcome, MeasurementUpdate, Restart};
    use xm1_saclac_interface::StatisticsOperation;
    for (word, update, restart, operation) in [
        (5, MeasurementUpdate::Window, Restart::Pending, Some(StatisticsOperation::Request14)),
        (6, MeasurementUpdate::NoPeerRequest, Restart::Pending, None),
        (7, MeasurementUpdate::Source, Restart::Pending, Some(StatisticsOperation::Request41)),
        (8, MeasurementUpdate::Window, Restart::Complete, Some(StatisticsOperation::Request14)),
        (9, MeasurementUpdate::NoPeerRequest, Restart::Complete, None),
        (10, MeasurementUpdate::Source, Restart::Complete, Some(StatisticsOperation::Request41)),
    ] {
        assert_eq!(crate::stock::measurement_outcome(word), ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart }));
        assert_eq!(crate::stock::measurement_update_operation(update), operation);
    }
    assert_eq!(crate::stock::measurement_outcome(1), ChildOutcome::Complete);
    assert_eq!(crate::stock::measurement_outcome(2), ChildOutcome::Pending);
    assert_eq!(crate::stock::measurement_outcome(0), ChildOutcome::Fault);
}
