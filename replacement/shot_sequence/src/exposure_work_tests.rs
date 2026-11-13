use crate::boundary::{ClientRef, ExportKind, ExposureLevels, ExposureOffset, Frame, LensReference, MeteringBlock, METERING_BYTES};
use crate::exposure_fakes::ExposureCall;
use crate::exposure_work::ExposureWork;
use crate::leaf_fakes::Fake;
use crate::ports::{Branch, ExposureEffect, ExposureQuery, HalfPressMode, Reply};
use crate::preparation::{HalfPressMask, HalfPressParameters, Prepared};
use crate::subjects::{Subject, SubjectId, Summary};
use std::vec::Vec;
use xm1_saclac_interface::exposure::{Correction, Evaluation, CORRECTION_HALVES};
use xm1_saclac_interface::stock::exposure::{correction_payload, measured_payload, release_payload};
use xm1_saclac_interface::exposure::{MeasuredEntry, ReleaseStatus};
use xm1_saclac_interface::{stock, CommandKind, Completed, Completion, CompletionStatus, RecordHandle};

const REFERENCE: LensReference = LensReference(0x40);

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16a_0000) }

fn completed(completion: Completion, payload: &[u8]) -> Completed<'_> { Completed { completion, parameters: stock::parameters(payload) } }

fn parameters(mode: u32, mask: u32) -> HalfPressParameters {
    HalfPressParameters {
        mode: HalfPressMode(mode),
        option: 9,
        mask: HalfPressMask(mask),
        frame_slot: Some(ClientRef(0x2000)),
        ..HalfPressParameters::default()
    }
}

fn prepared(mode: u32, mask: u32) -> Prepared {
    let mut prepared = Prepared { parameters: Some(parameters(mode, mask)), lens_reference: REFERENCE, refinement_pass: true, ..Prepared::default() };
    prepared.candidates.lists[0].count = 1;
    prepared.candidates.lists[0].entries[0] = Subject { id: SubjectId(77), detail: [1; 100] };
    prepared.candidates.summary = Summary(core::array::from_fn(|index| index as u32 + 1));
    prepared
}

fn begun(mode: u32, mask: u32, fake: &mut Fake) -> (ExposureWork, Prepared) {
    let mut work = ExposureWork::default();
    let mut prepared = prepared(mode, mask);
    work.effect(ExposureEffect::Begin { branch: Branch::HalfPress, record: record() }, &mut prepared, fake);
    (work, prepared)
}

fn correction(fill: impl Fn(usize) -> u16) -> Correction { Correction(core::array::from_fn(fill)) }

#[test]
fn a_half_press_binds_its_parameters_and_candidates_and_prepares_with_the_client_frame() {
    let mut fake = Fake::new();
    fake.frame = Some(Frame(0x8000));
    let (work, _) = begun(3, 0b11, &mut fake);
    let [ExposureCall::Prepare(setup)] = fake.exposure.calls.as_slice() else { panic!("{:?}", fake.exposure.calls) };
    assert_eq!(setup, &work.setup);
    assert_eq!((setup.record, setup.live, setup.mode, setup.mask, setup.option), (Some(record()), false, HalfPressMode(3), HalfPressMask(0b11), 9));
    assert_eq!((setup.lens_reference, setup.refinement_pass, setup.frame), (REFERENCE, true, Some(Frame(0x8000))));
    assert_eq!(setup.selection.0, 130, "the selection comes from the record, the mode and the refinement pass");
    assert_eq!(setup.subjects.entries[0].id, SubjectId(77));
    assert_eq!(setup.summary, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

#[test]
fn mode_seven_binds_without_preparing() {
    let mut fake = Fake::new();
    let (work, _) = begun(7, 1, &mut fake);
    assert!(fake.exposure.calls.is_empty());
    assert_eq!(work.setup.mode, HalfPressMode(7));
}

#[test]
fn the_live_sequence_binds_fixed_values_and_no_subjects() {
    let mut fake = Fake::new();
    let mut work = ExposureWork::default();
    let mut prepared = prepared(3, 0);
    work.effect(ExposureEffect::Begin { branch: Branch::Live, record: record() }, &mut prepared, &mut fake);
    let [ExposureCall::Prepare(setup)] = fake.exposure.calls.as_slice() else { panic!() };
    assert_eq!((setup.live, setup.mode, setup.mask, setup.option, setup.frame), (true, HalfPressMode(4), HalfPressMask(1), 1, None));
    assert_eq!((setup.subjects.count, setup.summary), (0, [0; 10]));
    assert_eq!(setup.lens_reference, REFERENCE);
}

#[test]
fn delivered_results_reach_the_calculation_and_the_queries() {
    let mut fake = Fake::new();
    let (mut work, mut prepared) = begun(0, 3, &mut fake);
    fake.exposure.calls.clear();
    fake.exposure.accepted = true;
    work.receive(completed(Completion::Image4d, &measured_payload(MeasuredEntry(5))));
    work.effect(ExposureEffect::AbsorbStatistics, &mut prepared, &mut fake);
    assert!(work.query(ExposureQuery::StatisticsAccepted, &mut fake));
    work.receive(completed(Completion::Image4d, &measured_payload(MeasuredEntry(6))));
    work.effect(ExposureEffect::AbsorbStatisticsResult, &mut prepared, &mut fake);
    assert_eq!(fake.exposure.calls, [ExposureCall::SelectMeasured(5), ExposureCall::CheckMeasured(5), ExposureCall::AcceptMeasured(5)]);

    for (status, low) in [(0, true), (1, true), (2, false)] {
        work.receive(completed(Completion::Image4c, &release_payload(ReleaseStatus(status))));
        assert_eq!(work.query(ExposureQuery::ReleaseStatusAtMostOne, &mut fake), low);
    }

    let mut payload = [0u8; 16];
    payload[0] = 2;
    payload[8] = 7;
    work.receive(completed(Completion::Calculation8e, &payload));
    work.effect(ExposureEffect::AbsorbCalculation, &mut prepared, &mut fake);
    let Some(ExposureCall::AbsorbEvaluation(Evaluation { entry: 2, levels: [7, 0, 0, 0], .. })) = fake.exposure.calls.last() else { panic!() };
}

#[test]
fn a_correction_without_a_result_repeats_the_last_one() {
    let mut work = ExposureWork::default();
    let first = correction(|index| index as u16);
    work.receive(completed(Completion::Calculation8f, &correction_payload(&first)));
    assert_eq!(work.correction(), &first);
    work.receive(completed(Completion::Calculation8f, &[]));
    assert_eq!(work.correction(), &first);
}

#[test]
fn a_skipped_correction_completes_itself_and_a_sent_one_waits() {
    let mut fake = Fake::new();
    let (mut work, mut prepared) = begun(0, 3, &mut fake);
    fake.exposure.calls.clear();
    work.effect(ExposureEffect::RequestCorrection, &mut prepared, &mut fake);
    assert_eq!(fake.exposure.calls, [ExposureCall::RequestCorrection]);
    fake.exposure.skip_correction = true;
    work.effect(ExposureEffect::RequestCorrection, &mut prepared, &mut fake);
    assert_eq!(fake.exposure.calls.last(), Some(&ExposureCall::SelfComplete(Completion::Calculation8f)));
}

#[test]
fn a_correction_comes_first_unless_a_primary_selection_keeps_its_areas_and_has_evaluations() {
    let cases = [
        (0b11, 0, true, 2, false),
        (0b10, 0, true, 2, true),
        (0b01, 1, true, 2, true),
        (0b01, 0, true, 2, false),
        (0b11, 0, false, 2, true),
        (0b11, 0, true, 0, true),
    ];
    for (mask, mode, primary, count, first) in cases {
        let mut fake = Fake::new();
        let (mut work, _) = begun(mode, mask, &mut fake);
        fake.exposure.primary = primary;
        fake.exposure.area_count = count;
        assert_eq!(work.query(ExposureQuery::CorrectionFirst, &mut fake), first, "mask {mask:#b} mode {mode}");
    }
}

#[test]
fn the_statistics_request_carries_the_record_settings_and_the_bound_subjects() {
    use crate::boundary::Setting;
    let mut fake = Fake::new();
    fake.set(Setting::AreaLayout, 0x0004_0003);
    fake.set(Setting::CaptureVariant, 1);
    fake.set(Setting::SubjectQueryA, 11);
    fake.set(Setting::SubjectQueryB, 12);
    fake.set(Setting::ContextMode, 13);
    fake.set(Setting::StatisticsOption, 14);
    fake.exposure.area_count = 3;
    fake.exposure.recording = true;
    let requests = |mode: u32, mask: u32| {
        let mut fake = Fake { settings: fake.settings.clone(), ..Fake::new() };
        fake.exposure.recording = true;
        let (mut work, mut prepared) = begun(mode, mask, &mut fake);
        work.effect(ExposureEffect::RequestStatistics, &mut prepared, &mut fake);
        fake.exposure.calls
    };
    let calls = requests(1, 0b01);
    let [_, ExposureCall::StartRecording, ExposureCall::RequestStatistics(request, subjects)] = calls.as_slice() else { panic!("{calls:?}") };
    assert_eq!((request.lens_reference, request.calculation_mode, request.subject_query), (REFERENCE.0, 0x21, [11, 12]));
    assert_eq!((request.area_layout, request.context_mode, request.correction_mode, request.statistics_option), (0x0004_0003, 13, 0x31, 14));
    assert!(request.single_area);
    assert_eq!(subjects.entries[0].id, SubjectId(77));

    let calls = requests(2, 0b11);
    let Some(ExposureCall::RequestStatistics(request, _)) = calls.last() else { panic!() };
    assert_eq!((request.area_layout, request.single_area), (0x0003_0002, false));
}

fn concluded(alternate: bool, live: bool) -> (ExposureWork, Prepared, Fake) {
    let mut fake = Fake::new();
    fake.exposure.levels = ExposureLevels { base: -3, reference: 100 };
    fake.exposure.lens_offset = 1000;
    fake.exposure.alternate = alternate;
    fake.exposure.calibration = 5;
    fake.exposure.metering = 0x6d;
    let mut work = ExposureWork::default();
    let mut prepared = prepared(0, 3);
    let branch = if live { Branch::Live } else { Branch::HalfPress };
    work.effect(ExposureEffect::Begin { branch, record: record() }, &mut prepared, &mut fake);
    let result = correction(|index| 200 + index as u16);
    work.receive(completed(Completion::Calculation8f, &correction_payload(&result)));
    fake.exposure.calls.clear();
    work.effect(ExposureEffect::Conclude, &mut prepared, &mut fake);
    (work, prepared, fake)
}

#[test]
fn the_conclusion_writes_the_correction_relative_to_lens_and_reference_then_replies_status_three() {
    let (work, _, fake) = concluded(true, false);
    let lens = 1000 + REFERENCE.0 as i32;
    let relative = |half: usize| lens + 200 + half as i32 - 100;
    let stored = fake.exposure.stored_exposure();
    assert_eq!(stored[0], (0, relative(3)));
    assert_eq!(stored[10], (10, relative(34)), "slot 10 takes the primary level when the variant word is zero");
    assert!(stored.contains(&(20, -3)) && stored.contains(&(21, 103)) && stored.contains(&(22, REFERENCE.0 as i32)) && stored.contains(&(23, 0)));
    assert!(stored.contains(&(13, relative(36))));
    assert!(fake.exposure.has(&ExposureCall::StoreOffset(ExposureOffset::First, (relative(34) + 5) as i16)));
    assert!(fake.exposure.has(&ExposureCall::StoreOffset(ExposureOffset::Third, 237)));
    assert!(fake.exposure.has(&ExposureCall::StoreArea(9, relative(47) as i16)));
    assert_eq!(work.area_values()[0], relative(38) as i16);
    assert!(fake.exposure.has(&ExposureCall::StoreMetering(MeteringBlock([0x6d; METERING_BYTES]))));
    let reply = ExposureCall::Reply(Reply { command: CommandKind::HalfPress, record: Some(record()), status: CompletionStatus(3) });
    assert_eq!(fake.exposure.calls.iter().filter(|call| **call == reply).count(), 1);
    let last_store = fake.exposure.calls.iter().rposition(|call| matches!(call, ExposureCall::StoreMetering(_))).unwrap();
    assert!(fake.exposure.position(|call| *call == reply).unwrap() > last_store);
}

#[test]
fn without_an_alternate_the_alternate_values_and_areas_are_cleared() {
    let (work, _, fake) = concluded(false, false);
    assert!(fake.exposure.stored_exposure().contains(&(13, 0)));
    assert!(fake.exposure.has(&ExposureCall::StoreOffset(ExposureOffset::Second, 0)));
    assert!(fake.exposure.has(&ExposureCall::StoreArea(4, 0)));
    assert_eq!(work.area_values(), [0; 10]);
}

#[test]
fn the_live_conclusion_does_not_reply() {
    let (_, _, fake) = concluded(true, true);
    assert!(!fake.exposure.calls.iter().any(|call| matches!(call, ExposureCall::Reply(_))));
    assert!(!fake.exposure.stored_exposure().is_empty());
}

#[test]
fn the_variant_selects_the_secondary_level_for_slot_ten() {
    let mut fake = Fake::new();
    fake.exposure.variant = 1;
    let mut work = ExposureWork::default();
    let mut prepared = prepared(0, 3);
    work.effect(ExposureEffect::Begin { branch: Branch::HalfPress, record: record() }, &mut prepared, &mut fake);
    work.receive(completed(Completion::Calculation8f, &correction_payload(&correction(|index| index as u16))));
    work.effect(ExposureEffect::Conclude, &mut prepared, &mut fake);
    let stored = fake.exposure.stored_exposure();
    assert_eq!(stored[10], (10, 35 + REFERENCE.0 as i32));
    assert!(stored.contains(&(24, 1)));
}

#[test]
fn an_exporting_calculation_exports_the_current_or_last_correction() {
    for export_last in [false, true] {
        let mut fake = Fake::new();
        fake.frame = Some(Frame(0x9000));
        fake.exposure.exporting = true;
        fake.exposure.export_last = export_last;
        let mut work = ExposureWork::default();
        let mut prepared = prepared(0, 3);
        work.effect(ExposureEffect::Begin { branch: Branch::HalfPress, record: record() }, &mut prepared, &mut fake);
        let last = correction(|_| 1);
        work.receive(completed(Completion::Calculation8f, &correction_payload(&last)));
        work.effect(ExposureEffect::Conclude, &mut prepared, &mut fake);
        let exports: Vec<_> = fake.exposure.calls.iter().filter_map(|call| match call { ExposureCall::Export(frame, _, kind) => Some((*frame, *kind)), _ => None }).collect();
        assert_eq!(exports, [(Some(Frame(0x9000)), ExportKind::Plain)]);
        assert_eq!(fake.exposure.has(&ExposureCall::ClearSelection(Some(Frame(0x9000)), false)), !export_last);
    }
    let mut fake = Fake::new();
    fake.exposure.primary = true;
    let (mut work, mut prepared) = begun(0, 3, &mut fake);
    work.effect(ExposureEffect::Conclude, &mut prepared, &mut fake);
    assert!(fake.exposure.has(&ExposureCall::ClearSelection(None, true)));
    assert!(fake.exposure.has(&ExposureCall::FinishPrimary));
}

#[test]
fn the_conclusion_hands_the_correction_to_the_selection_plan() {
    let mut fake = Fake::new();
    let (mut work, mut prepared) = begun(0, 3, &mut fake);
    prepared.selection_plan.0[4] = 2;
    let mut halves = [0u16; CORRECTION_HALVES];
    halves[48] = 0x5678;
    halves[49] = 0x1234;
    halves[53] = 0xaaaa;
    halves[61] = 0xbbbb;
    work.receive(completed(Completion::Calculation8f, &correction_payload(&Correction(halves))));
    work.effect(ExposureEffect::Conclude, &mut prepared, &mut fake);
    let plan = &prepared.selection_plan.0;
    let word = |offset: usize| u32::from_le_bytes(plan[offset..offset + 4].try_into().unwrap());
    for base in [0x8, 0x5c, 0x68, 0x74] {
        assert_eq!((word(base), word(base + 4), word(base + 8)), (0x1234_5678, 0x1234_5678, 0), "array at {base:#x}");
    }
    assert_eq!(&plan[0x20c..0x216], &[0xaa, 0xaa, 0, 0, 0, 0, 0, 0, 0xbb, 0xbb]);
}
