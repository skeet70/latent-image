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

#[test]
fn adjustment_requests_preserve_the_retained_peer_payload() {
    use crate::adjustment::{DriveAmount, DriveScale, LensPosition, LensRequest, Percentage, Selection};
    use crate::stock::{adjustment_payload, LensFinishRoute};
    use xm1_saclac_interface::{stock::StockCallbackWord, LensDcOperation};

    let callback = StockCallbackWord(0x12345678);
    for (request, operation, expected) in [
        (LensRequest::Stop, LensDcOperation::Request15, &[0x12345678, 3, 0][..]),
        (LensRequest::Synchronize, LensDcOperation::Request17, &[0x12345678, 0][..]),
        (LensRequest::Query, LensDcOperation::Request18, &[0x12345678][..]),
        (LensRequest::ConfigureDefault, LensDcOperation::Request16, &[0x12345678, 3, 0, 255][..]),
        (LensRequest::SetValue(DriveAmount(45)), LensDcOperation::Request19, &[0x12345678, 45][..]),
        (LensRequest::ConfigureValue(DriveAmount(67)), LensDcOperation::Request1a, &[0x12345678, 67][..]),
        (LensRequest::Configure { selection: Selection(2), value: DriveAmount(7) },
            LensDcOperation::Request13, &[0x12345678, 2, 7, 1, 0, 10][..]),
        (LensRequest::Move { position: LensPosition(-2), amount: DriveAmount(3), scale: DriveScale(2) },
            LensDcOperation::Request1c, &[0x12345678, 0, 12, 65534, 2, 1, 0, 13][..]),
        (LensRequest::SetPosition { position: LensPosition(9), amount: DriveAmount(8) },
            LensDcOperation::Request1c, &[0x12345678, 1, 8, 9, 2, 1, 0, 13][..]),
        (LensRequest::SetPercentage { position: LensPosition(9), percent: Percentage::saturating(250) },
            LensDcOperation::Request1c, &[0x12345678, 2, 100, 9, 2, 1, 0, 13][..]),
    ] {
        let payload = adjustment_payload(request, callback, LensFinishRoute::Ordinary).unwrap();
        assert_eq!(payload.operation, operation);
        let expected_bytes: std::vec::Vec<_> = expected.iter().flat_map(|word: &u32| word.to_le_bytes()).collect();
        assert_eq!(payload.bytes(), expected_bytes);
    }
    let alternate = adjustment_payload(LensRequest::Configure { selection: Selection(2), value: DriveAmount(7) }, callback, LensFinishRoute::Alternate).unwrap();
    assert_eq!(alternate.operation, LensDcOperation::Request1d);
    assert_eq!(alternate.bytes(), &[0x78, 0x56, 0x34, 0x12, 2, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0]);
    assert!(adjustment_payload(LensRequest::ReserveOnly, callback, LensFinishRoute::Ordinary).is_none());
}

#[test]
fn oversized_adjustment_shifts_produce_zero_without_panicking() {
    use crate::adjustment::{DriveAmount, DriveScale, LensPosition, LensRequest};
    use crate::stock::{adjustment_payload, LensFinishRoute};
    use xm1_saclac_interface::stock::StockCallbackWord;
    let payload = adjustment_payload(LensRequest::Move {
        position: LensPosition(0), amount: DriveAmount(u32::MAX), scale: DriveScale(32),
    }, StockCallbackWord(1), LensFinishRoute::Ordinary).unwrap();
    assert_eq!(&payload.bytes()[8..12], &[0; 4]);
}

#[test]
fn record_start_places_defined_fields_and_completion_in_the_peer_packet() {
    use crate::stock::{record_start_payload, RecordHalfword, RecordMode, RecordSample, RecordStart, RecordWord};
    use xm1_saclac_interface::stock::StockCallbackWord;
    let bytes = record_start_payload(RecordStart {
        channel: RecordWord(8), mode: RecordMode::Second,
        settings: [RecordWord(3), RecordWord(4)],
        range: [RecordHalfword(5), RecordHalfword(6)], source: RecordWord(7),
        sample: RecordSample { leading: RecordWord(9),
            components: [RecordHalfword(10), RecordHalfword(11), RecordHalfword(12), RecordHalfword(13)],
            trailing: RecordWord(14) },
    }, StockCallbackWord(0x12345678));
    assert_eq!(bytes, [
        8,0,0,0, 2,0,0,0, 3,0,0,0, 4,0,0,0,
        5,0,6,0, 0,0,0,0, 7,0,0,0, 100,0,16,0,
        9,0,0,0, 10,0,11,0, 12,0,13,0, 14,0,0,0,
        0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0,
        0x78,0x56,0x34,0x12,
    ]);
}

#[test]
fn record_update_clamps_position_and_duration_before_serializing() {
    use crate::adjustment::{ExposureDuration, ExposureLimits, ExposureMode, LensPosition};
    use crate::stock::{record_update_payload, RecordLimits, RecordUpdate, RecordWord};
    use xm1_saclac_interface::stock::StockCallbackWord;
    let limits = RecordLimits {
        exposure: ExposureLimits { minimum: ExposureDuration(100), maximum: ExposureDuration(1000) },
        minimum_position: LensPosition(-10), maximum_position: LensPosition(20),
    };
    let request = RecordUpdate {
        mode: ExposureMode::Bounded, duration: ExposureDuration(50), value: RecordWord(2),
        position: LensPosition(-100), auxiliary: RecordWord(6),
    };
    assert_eq!(record_update_payload(request, limits, StockCallbackWord(7)), [
        100,0,0,0, 2,0,0,0, 0xf6,0xff,0,0, 6,0,0,0, 7,0,0,0,
    ]);
    let request = RecordUpdate { duration: ExposureDuration(5000), position: LensPosition(100), ..request };
    assert_eq!(record_update_payload(request, limits, StockCallbackWord(7)), [
        0xe8,3,0,0, 2,0,0,0, 20,0,0,0, 6,0,0,0, 7,0,0,0,
    ]);
}

#[test]
fn synchronous_export_boundary_has_one_entry_for_each_recovered_consumer() {
    use crate::stock::{ExportAddress, SynchronousExport, SYNCHRONOUS_EXPORTS};
    assert_eq!(SYNCHRONOUS_EXPORTS.first().unwrap().address, ExportAddress(0xefbe74));
    assert_eq!(SYNCHRONOUS_EXPORTS.last().unwrap().address, ExportAddress(0xefca4c));
    assert!(SYNCHRONOUS_EXPORTS.windows(2).all(|entries| entries[0].address.0 < entries[1].address.0));
    assert_eq!(SYNCHRONOUS_EXPORTS.iter().find(|entry| entry.export == SynchronousExport::StartCaptureRecord).unwrap().argument_words, 14);
}

#[test]
fn lens_client_replies_keep_the_selection_refusal_distinct_from_busy() {
    use crate::adjustment::Reply;
    use xm1_saclac_interface::CompletionStatus;
    assert_eq!(crate::stock::adjustment_reply_status(Reply::Success), CompletionStatus(0));
    assert_eq!(crate::stock::adjustment_reply_status(Reply::Busy), CompletionStatus(-95));
    assert_eq!(crate::stock::adjustment_reply_status(Reply::SelectionOutOfRange), CompletionStatus(-97));
}

#[test]
fn measurement_controls_preserve_geometry_suspend_and_resume_codes() {
    use crate::children::MeasurementControl as C;
    for (control, code) in [(C::Initialize, 0xb7), (C::Stop, 0xb8), (C::Reconfigure, 0xbb), (C::Suspend, 0xb9), (C::Resume, 0xba)] {
        assert_eq!(crate::stock::measurement_control_code(control), code);
    }
}

#[test]
fn the_s1_progress_byte_puts_each_owed_step_in_its_readers_bit() {
    use crate::s1::Waiting;
    use crate::stock::s1_progress;
    let none = Waiting::default();
    assert_eq!(s1_progress(none), 0);
    assert_eq!(s1_progress(Waiting { shot: true, ..none }), 1);
    assert_eq!(s1_progress(Waiting { capture: true, ..none }), 2);
    assert_eq!(s1_progress(Waiting { bridge: true, ..none }), 4);
    assert_eq!(s1_progress(Waiting { release: true, ..none }), 8);
}
