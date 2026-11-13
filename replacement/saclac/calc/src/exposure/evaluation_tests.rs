use xm1_saclac_interface::calc::{CalcPayload, SharedAddress};
use xm1_saclac_interface::exposure::EvaluationRequest;
use xm1_saclac_interface::stock::StockCallbackWord;

use crate::exposure::context::ExposureContext;
use crate::exposure::evaluation::{reference_value, request_row, set_up};
use crate::exposure::fakes::{placed, Posted, World};
use crate::exposure::ports::InPlace;
use crate::exposure::program::*;
use crate::selection::{BufferSlot, RowIndex, RowPhase};
use crate::stock::adjust::CONFIGURED_FACTOR;
use crate::stock::tables::PROGRAM_PROFILES;
use crate::stock::{dump, property, shared};

const FRAME: SharedAddress = SharedAddress(0x4000_0000);

struct FixedLimits;

impl Limits for FixedLimits {
    fn primary_range(&mut self, _: &PrimaryInputs) -> PrimaryRange { PrimaryRange { upper: 300, near_lens: 1, far_lens: 1, ..PrimaryRange::default() } }
    fn selected_program(&mut self, _: &SelectedInputs) -> u32 { 0 }
    fn selected_lenses(&mut self, _: u32, _: u32) -> (u32, u32) { (1, 1) }
    fn selected_level(&mut self, _: u32, _: u32) -> i16 { 0 }
    fn selected_offset(&mut self, _: u32) -> i16 { 0 }
}

/// A target and a configured measurement of 500 over the program of `program_tests`: steps at
/// 50, 127, 204 and 280.
fn world() -> World {
    let mut world = World::default();
    for (index, profile) in PROGRAM_PROFILES.iter().enumerate() {
        world.profile_levels.insert(*profile, 100 * index as i16);
        world.sensitivity_classes.insert(*profile, index as u8);
    }
    world.property_word(property::PROGRAM_LEVEL_LIMIT, 1000);
    world.property_word(property::TARGET_LEVEL, 500);
    world.property(property::ADJUSTMENT_FLAGS, 1);
    world.configured_level = 500;
    world.property(property::START_MARGIN, 5);
    world.property(property::STEP_LIMIT, 10);
    world.property(property::UPPER_TOLERANCE, 2);
    world.property(property::LOWER_TOLERANCE, 3);
    world.property(property::FIRST_STEP, 255);
    world
}

fn context() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.binding.frame = Some(FRAME);
    context.binding.subjects.count = 12;
    for (index, subject) in context.binding.subjects.entries.iter_mut().enumerate() {
        subject.detail[4..12].copy_from_slice(&[index as u8 + 1; 8]);
    }
    context
}

#[test]
fn the_bank_holds_one_candidate_per_spanned_step_valued_against_the_target() {
    let mut context = context();
    set_up(&mut world(), &mut FixedLimits, &mut context);
    let bank = context.selection;
    assert_eq!((context.span.first, context.span.last), (1, 4));
    assert_eq!((bank.count, bank.first, bank.filled, bank.any_range, bank.highest_below), (4, 1, 4, false, 3));
    assert_eq!(bank.candidates[..4].iter().map(|candidate| (candidate.level, candidate.value)).collect::<std::vec::Vec<_>>(), [(0, 50), (1, 127), (2, 204), (3, 280)]);
    assert_eq!((bank.expected.lowest, bank.expected.highest), (0, 3));
    assert_eq!(context.areas[..4].iter().map(|area| area.level).collect::<std::vec::Vec<_>>(), [500; 4]);
    assert_eq!(context.areas[4], Default::default());
}

#[test]
fn a_configured_first_step_leaves_the_steps_before_it_out_of_the_bank() {
    let mut world = world();
    world.property(property::FIRST_STEP, 3);
    let mut context = context();
    set_up(&mut world, &mut FixedLimits, &mut context);
    let bank = context.selection;
    assert_eq!((bank.count, bank.first, bank.filled, bank.highest_below), (2, 3, 2, 3));
    assert_eq!(bank.candidates[..2].iter().map(|candidate| candidate.level).collect::<std::vec::Vec<_>>(), [2, 3]);
    assert_eq!(context.areas[..2].iter().map(|area| area.profile).collect::<std::vec::Vec<_>>(), [context.program[2].profile, context.program[3].profile]);
}

#[test]
fn every_column_covers_the_listed_subjects_and_keeps_the_bound_count() {
    let mut context = context();
    set_up(&mut world(), &mut FixedLimits, &mut context);
    for column in context.selection.columns {
        assert_eq!(column.count, 12);
        assert_eq!(column.regions[0], [1; 8]);
        assert_eq!(column.regions[9], [10; 8]);
    }
}

#[test]
fn the_requests_carry_the_tolerances_and_the_reference_of_the_highest_candidate() {
    let mut context = context();
    set_up(&mut world(), &mut FixedLimits, &mut context);
    let evaluation = context.evaluation;
    assert_eq!((evaluation.tolerance_above, evaluation.tolerance_below, evaluation.floor, evaluation.frame), (20, -30, -900, Some(FRAME)));
    let correction = context.correction;
    assert_eq!((correction.reference, correction.start, correction.start_above_target), (280 + 250, 500, 0));
    assert_eq!((correction.subjects, correction.summary), (Some(placed(InPlace::BoundSubjects)), Some(placed(InPlace::BoundSummary))));
    assert_eq!((correction.rate, correction.frame), (1, Some(FRAME)));
}

#[test]
fn the_frame_records_the_snapshot_and_how_the_start_was_bound() {
    let mut world = world();
    let mut context = context();
    context.snapshot.valid = 7;
    context.snapshot.focus.pinned = 1;
    set_up(&mut world, &mut FixedLimits, &mut context);
    let words = [0, 1, 2].map(|index| world.u32_at(dump::evaluation(FRAME, index)));
    assert_eq!(words, [7, 1, context.span.bound.code()]);
}

#[test]
fn on_the_adjustment_path_the_reference_is_the_snapshot_lens_level_under_the_level_limit() {
    let mut world = world();
    world.calibration_indices.insert(9, 4);
    world.lens_levels.insert((4, 0x55), 30);
    world.class_offsets.insert(0, 10);
    let mut context = context();
    context.outcome.adjusting = true;
    context.snapshot.focus.lens_detail = 9;
    context.binding.lens_reference = 0x55;
    assert_eq!(reference_value(&mut world, &context), 30 + 1000 - 10);
}

const CALLBACK: StockCallbackWord = StockCallbackWord(0x00f1_ee10);

fn prepared() -> (World, ExposureContext) {
    let mut world = world();
    for (id, value) in CONFIGURED_FACTOR.into_iter().zip([0, 0, 25]) {
        world.property(id, value);
    }
    let mut context = context();
    set_up(&mut world, &mut FixedLimits, &mut context);
    (world, context)
}

fn request(world: &mut World, context: &mut ExposureContext, row: u8) -> EvaluationRequest {
    let mut posted = Posted::default();
    request_row(world, &mut posted, context, RowIndex::new(row).unwrap(), CALLBACK).unwrap();
    match posted.calc[..] {
        [CalcPayload::Evaluation { request, callback }] if callback == CALLBACK => request,
        _ => panic!("one evaluation request with the callback: {:?}", posted.calc),
    }
}

#[test]
fn a_row_request_names_the_row_its_measurement_and_its_column_and_marks_it_requested() {
    let (mut world, mut context) = prepared();
    let measurement = &mut context.selection.rows[2].measurement;
    measurement.pairs = [[0x1_0001, 0x2_0002], [3, 4], [5, 6], [7, 8]];
    measurement.statistics = [9, 10, 11, 0x1_000c];
    let sent = request(&mut world, &mut context, 2);
    assert_eq!((sent.row, sent.measured, sent.statistics), (2, [[2, 1], [6, 5], [4, 3]], [9, 10, 11, 12]));
    assert_eq!((sent.value, sent.next_profile), (204, context.areas[2].next_profile));
    assert_eq!((sent.column_count, sent.column_regions), (12, Some(placed(InPlace::ColumnRegions(2)))));
    let slot = BufferSlot::new(2).unwrap();
    assert_eq!((sent.row_buffer, sent.column_buffer), (Some(shared::row_buffers(slot)[0]), Some(shared::column_buffer(slot))));
    assert_eq!((sent.level_classes, sent.selection_flags), (Some(shared::level_class(0)), Some(shared::SELECTION_FLAGS)));
    assert_eq!((sent.tolerance_above, sent.floor, sent.frame), (20, -900, Some(FRAME)));
    assert_eq!(context.selection.rows[2].phase, RowPhase::Requested);
    assert_eq!(context.evaluation, sent);
}

#[test]
fn the_rates_are_the_configured_factor_and_its_tenths() {
    let (mut world, mut context) = prepared();
    assert_eq!(request(&mut world, &mut context, 0).rates, [25, 2, 4, 2]);
}

#[test]
fn the_lower_value_is_the_candidate_one_level_below_the_row() {
    let (mut world, mut context) = prepared();
    assert_eq!(request(&mut world, &mut context, 2).lower_value, 127);
    assert_eq!(request(&mut world, &mut context, 0).lower_value, 0x8000);
}

#[test]
fn the_row_after_the_reference_level_is_marked_unless_any_range_is_accepted() {
    let (mut world, mut context) = prepared();
    context.selection.reference_level = 1;
    assert!(request(&mut world, &mut context, 2).after_reference);
    assert!(!request(&mut world, &mut context, 1).after_reference);
    context.selection.any_range = true;
    assert!(!request(&mut world, &mut context, 2).after_reference);
}

#[test]
fn a_row_of_fixed_offset_carries_its_state_and_raises_the_floor_for_later_rows() {
    let (mut world, mut context) = prepared();
    context.selection.candidates[1].state = 4;
    context.selection.candidates[2].state = 3;
    let fourth = request(&mut world, &mut context, 1);
    assert_eq!((fourth.direction, fourth.floor), (4, -900));
    let third = request(&mut world, &mut context, 2);
    assert_eq!((third.direction, third.floor), (3, -600));
    assert_eq!(request(&mut world, &mut context, 3).floor, -600);
}

#[test]
fn other_rows_point_toward_the_first_applied_row() {
    let (mut world, mut context) = prepared();
    assert_eq!(request(&mut world, &mut context, 1).direction, 0);
    context.selection.rows[2].phase = RowPhase::Applied;
    assert_eq!(request(&mut world, &mut context, 1).direction, 2);
    assert_eq!(request(&mut world, &mut context, 3).direction, 1);
    context.selection.rows[0].phase = RowPhase::Applied;
    assert_eq!(request(&mut world, &mut context, 3).direction, 1);
    assert_eq!(request(&mut world, &mut context, 0).direction, 1);
}
