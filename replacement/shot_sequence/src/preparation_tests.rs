use crate::boundary::*;
use crate::leaf_fakes::{Event, Fake, Published};
use crate::ports::HalfPressMode;
use crate::preparation::{FrameHeader, HalfPressMask, HalfPressParameters, Prepared};
use std::vec::Vec;

const REFERENCE: LensReference = LensReference(40);

fn parameters(mode: u32) -> HalfPressParameters {
    HalfPressParameters {
        mode: HalfPressMode(mode),
        option: 9,
        mask: HalfPressMask(0b1000),
        plan_option: 0,
        area_count: Some(ClientRef(0x100)),
        area_selection: 0x55,
        areas: Some(ClientRef(0x200)),
        lens_target: None,
        frame_slot: Some(ClientRef(0x300)),
        lens_reference: REFERENCE,
        ..HalfPressParameters::default()
    }
}

fn result_at(reference: LensReference) -> FocusResult {
    FocusResult { reference, valid: 1, kept: 1, refined: 1, extent: 0, bytes: [0; 132] }
}

fn prepared(parameters: Option<HalfPressParameters>, fake: &mut Fake) -> Prepared {
    let mut prepared = Prepared::default();
    prepared.bind(crate::tests::record());
    let mut context = core::mem::take(&mut fake.context);
    prepared.prepare(parameters, LensReference(77), &mut context, fake);
    fake.context = context;
    prepared
}

#[test]
fn a_mode_6_half_press_leaves_the_lens_and_focus_records_alone() {
    let mut fake = Fake::new();
    fake.frame = Some(Frame(0x8000));
    fake.area_words = std::vec![3, 4];
    let prepared = prepared(Some(parameters(6)), &mut fake);
    for unexpected in [Event::Hold(true), Event::SelectFirstMode, Event::ClearExposureRecord] {
        assert!(!fake.has(&unexpected), "{unexpected:?}");
    }
    assert!(!fake.events.iter().any(|event| matches!(event, Event::StoreLensSnapshot(_) | Event::StoreFocusResult(_) | Event::Dump(..) | Event::Summary(..))));
    assert!(fake.context.plans.is_empty());
    assert!(fake.has(&Event::Announced(Frame(0x8000), FrameHeader { mode: HalfPressMode(6), mask: HalfPressMask(0b1000), option: 9, lens_reference: REFERENCE })));
    let published = fake.published();
    assert!(published.contains(&Published::AreaSelection(0x55)));
    assert!(published.contains(&Published::Area { index: 1, word: 4 }));
    assert_eq!(published.last(), Some(&Published::CaptureLink(0)));
    assert_eq!(prepared.pipeline_gate, 0);
    assert!(prepared.has_work());
    assert!(!prepared.lens_status_pending());
}

#[test]
fn a_half_press_snapshots_the_lens_at_its_reference() {
    let mut fake = Fake::new();
    fake.set_lens(LensReading::Position, (-5i16) as u16 as u32);
    fake.set_lens(LensReading::State, 7);
    fake.set_lens(LensReading::Kind, 2);
    fake.set_lens(LensReading::Reference, 41);
    fake.result = result_at(REFERENCE);
    let prepared = prepared(Some(parameters(0)), &mut fake);
    let current = LensPoint { position: -5, detail: 0, reference: REFERENCE, state: 7, kind: 2 };
    assert_eq!(fake.snapshot.current, current);
    assert_eq!(fake.snapshot.held, current);
    assert_eq!(fake.context.lens_reference, REFERENCE);
    assert_eq!(fake.context.track, Some(41));
    assert_eq!(prepared.pipeline_gate, 2);
    assert!(fake.has(&Event::Hold(true)));
    assert_eq!(fake.context.plans, [(Some(ClientRef(0x100)), false)]);
}

#[test]
fn the_lens_target_takes_the_designated_point_over_the_snapshot() {
    for designated in [None, Some(LensPoint { position: 9, detail: 0, reference: LensReference(1), state: 3, kind: 4 })] {
        let mut fake = Fake::new();
        fake.set_lens(LensReading::Control, 100);
        fake.designated = designated;
        let mut half_press = parameters(0);
        half_press.lens_target = Some(ClientRef(0x400));
        prepared(Some(half_press), &mut fake);
        let point = designated.unwrap_or(fake.snapshot.current);
        assert!(fake.has(&Event::LensTarget { target: ClientRef(0x400), point, step: point.kind as u16 + 1, distance: point.state + 100 }));
    }
}

#[test]
fn results_from_another_lens_reference_are_invalidated() {
    let mut fake = Fake::new();
    fake.result = result_at(LensReference(1));
    fake.confirmation = FocusConfirmation { active: 1, reference: LensReference(1), bytes: [0; 32] };
    fake.set(Setting::PrimarySubjects, 1);
    let _ = prepared(Some(parameters(0)), &mut fake);
    assert_eq!((fake.result.valid, fake.result.kept, fake.result.refined), (0, 0, 1));
    assert_eq!(fake.history.valid, 0);
    assert_eq!(fake.confirmation.active, 0);
    assert!(!fake.events.iter().any(|event| matches!(event, Event::Detect(..))), "subjects are not refreshed for a stale result");
}

#[test]
fn results_at_the_current_reference_are_kept_and_subjects_refreshed() {
    let mut fake = Fake::new();
    fake.result = result_at(REFERENCE);
    fake.confirmation = FocusConfirmation { active: 1, reference: REFERENCE, bytes: [0; 32] };
    fake.set(Setting::PrimarySubjects, 1);
    let _ = prepared(Some(parameters(0)), &mut fake);
    assert!(!fake.events.iter().any(|event| matches!(event, Event::StoreFocusResult(_) | Event::StoreFocusHistory(_) | Event::StoreFocusConfirmation(_))));
    assert!(fake.has(&Event::Detect("primary", 24)));
}

#[test]
fn the_live_branch_prepares_the_lens_without_parameters() {
    let mut fake = Fake::new();
    fake.frame = Some(Frame(0x8000));
    let prepared = prepared(None, &mut fake);
    assert_eq!(fake.snapshot.current.reference, LensReference(77));
    assert_eq!(fake.context.plans, [(None, true)]);
    let published = fake.published();
    assert!(!published.iter().any(|published| matches!(published, Published::AreaSelection(_) | Published::Area { .. } | Published::CaptureFrame(_))));
    assert!(!fake.events.iter().any(|event| matches!(event, Event::Announced(..) | Event::StoreFocusResult(_))));
    assert!(!prepared.has_work());
    assert!(!prepared.lens_status_pending());
}

#[test]
fn mode_2_keeps_the_focus_result_only_for_the_wide_layouts() {
    for (layout, kept) in [((4 << 16) | 3, true), ((3 << 16) | 2, true), ((4 << 16) | 2, false)] {
        let mut fake = Fake::new();
        fake.result = result_at(REFERENCE);
        fake.set(Setting::AreaLayout, layout);
        prepared(Some(parameters(2)), &mut fake);
        assert_eq!(fake.result.valid == 1, kept, "layout {layout:#x}");
    }
}

#[test]
fn an_unrefined_request_drops_the_refined_result() {
    let mut fake = Fake::new();
    fake.result = result_at(REFERENCE);
    fake.set(Setting::CaptureVariant, 1);
    prepared(Some(parameters(1)), &mut fake);
    assert_eq!(fake.result.refined, 0);

    let mut fake = Fake::new();
    fake.result = result_at(REFERENCE);
    let mut keep_areas = parameters(0);
    keep_areas.mask = HalfPressMask(0b10);
    fake.result.extent = 128;
    prepared(Some(keep_areas), &mut fake);
    assert_eq!(fake.result.refined, 1);
    fake.result.extent = 7;
    prepared(Some(keep_areas), &mut fake);
    assert_eq!(fake.result.refined, 0);
}

#[test]
fn the_frame_dumps_follow_the_record_and_adjustment() {
    let mut fake = Fake::new();
    fake.frame = Some(Frame(0x8000));
    fake.set(Setting::LensPolicy, 5);
    fake.set(Setting::ContextMode, 1);
    fake.set(Setting::DumpArgument, 6);
    fake.set_adjustment(Adjustment::StatisticsDump, 0x80);
    fake.calculation_mode = 1;
    prepared(Some(parameters(0)), &mut fake);
    let dumps: Vec<_> = fake.events.iter().filter(|event| matches!(event, Event::Dump(..) | Event::DumpCalculation | Event::Summary(..))).cloned().collect();
    assert_eq!(dumps.len(), 6);
    assert_eq!(dumps[0], Event::Dump(Dump::Statistics, Frame(0x8000), 6));
    assert_eq!(dumps[1], Event::Dump(Dump::Frame, Frame(0x8000), 0));
    assert_eq!(dumps[2], Event::Dump(Dump::Lens, Frame(0x8000), 0));
    assert_eq!(dumps[3], Event::DumpCalculation);
    assert!(matches!(dumps[4], Event::Summary(Frame(0x8000), _)));
    assert_eq!(dumps[5], Event::Dump(Dump::Numbered, Frame(0x8000), 0));
    assert!(fake.published().contains(&Published::CaptureFrame(Some(Frame(0x8000)))));
}

#[test]
fn the_selection_option_0x39_withholds_the_frame() {
    let mut fake = Fake::new();
    fake.frame = Some(Frame(0x8000));
    fake.set(Setting::ContextMode, 1);
    fake.set(Setting::SelectionOption, 0x39);
    prepared(Some(parameters(0)), &mut fake);
    assert!(!fake.published().iter().any(|published| matches!(published, Published::CaptureFrame(_))));
}

#[test]
fn every_preparation_hands_its_mode_to_the_carried_words() {
    let mut fake = Fake::new();
    prepared(Some(parameters(7)), &mut fake);
    prepared(None, &mut fake);
    assert_eq!(fake.context.carried, [Some(7), None]);
}

#[test]
fn the_lens_status_records_the_reading_and_the_change_latch() {
    use crate::dump_boundary::ShotWord;
    let mut fake = Fake::new();
    fake.change = 1;
    fake.set_lens(LensReading::Reference, 12);
    let mut state = crate::state::ShotState::new(crate::state_tests::Memory::default());
    crate::preparation::record_lens(crate::preparation::LensStage::Conclusion, &mut state, &mut fake);
    assert_eq!((state.shot_word(ShotWord::ThirdReference), state.shot_word(ShotWord::MovedLate)), (12, 1));
    assert_eq!(fake.change, 0);
}

#[test]
fn the_record_answers_the_sequence_queries() {
    let mut fake = Fake::new();
    let prepared = Prepared::default();
    assert!(prepared.two_pass_exposure(&mut fake));
    assert!(!prepared.skips_lens_synchronization(&mut fake));
    fake.set(Setting::ContextMode, 1);
    fake.set(Setting::LensPolicy, 3);
    assert!(!prepared.two_pass_exposure(&mut fake));
    assert!(prepared.skips_lens_synchronization(&mut fake));
}
