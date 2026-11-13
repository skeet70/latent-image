use xm1_saclac_interface::calc::{CalcPayload, SharedAddress};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::exposure::context::ExposureContext;
use crate::exposure::correction::{post, refresh, CorrectionPost};
use crate::exposure::fakes::{placed, Posted, World};
use crate::exposure::levels;
use crate::exposure::ports::InPlace;
use crate::policy::ExposureMode;
use crate::stock::property;
use crate::stock::shared::{level_curve, subject_curve, CURVE_POINTS, LEVEL_CURVES, SUBJECT_POINTS};

const CALLBACK: StockCallbackWord = StockCallbackWord(0x00f1_ee44);

fn level_at(world: &World, at: SharedAddress) -> i16 { world.u16_at(at) as i16 }

fn context() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.binding.mask = 1;
    context.binding.mode = 4;
    context.binding.exposure_mode = ExposureMode(0);
    context
}

fn table_writes(world: &World) -> usize { world.stores.len() }

#[test]
fn a_primary_refresh_raises_every_table_by_the_start_offset_up_to_the_reference() {
    let mut world = World::default();
    world.put_u16(level_curve(2, 7), 100);
    world.put_u16(level_curve(2, 8), 10);
    let mut context = context();
    context.correction.start_above_target = 5;
    context.correction.reference = 103;
    refresh(&mut world, &mut context);
    assert_eq!((level_at(&world, level_curve(2, 7)), level_at(&world, level_curve(2, 8))), (103, 15));
    assert_eq!(level_at(&world, subject_curve(3, 9)), 5);
    assert_eq!(table_writes(&world), LEVEL_CURVES * (CURVE_POINTS + SUBJECT_POINTS));
}

#[test]
fn tables_stay_unwritten_without_a_start_offset_and_in_the_mode_that_keeps_them() {
    let mut world = World::default();
    refresh(&mut world, &mut context());
    let mut kept = context();
    kept.binding.mode = 7;
    kept.correction.start_above_target = 5;
    post(&mut world, &mut Posted::default(), &mut kept, CALLBACK).unwrap();
    assert_eq!(table_writes(&world), 0);
}

/// The adjustment level 1000 raised by a fifth focus level of 50, with near bounds 100 above and
/// below and far bounds 200 either side.
fn window_world(curve_zero: impl Fn(usize) -> i16) -> World {
    let mut world = World::default();
    world.property(property::TABLE_CEILING, 200);
    world.property(property::WINDOW_MARGIN, 3);
    world.property(property::NEAR_ABOVE, 10);
    world.property(property::FAR_ABOVE, 20);
    world.property(property::NEAR_BELOW, (-10i8) as u8 as u32);
    world.property(property::FAR_BELOW, (-20i8) as u8 as u32);
    world.property(property::NEAR_ABOVE_WEIGHT, 1);
    world.property(property::NEAR_BELOW_WEIGHT, 1);
    world.property_word(property::SECOND_WEIGHT_LIMIT, 5);
    world.property_word(property::THIRD_WEIGHT_LIMIT, 5);
    for point in 0..CURVE_POINTS {
        world.put_u16(level_curve(0, point), curve_zero(point) as u16);
    }
    world
}

fn adjusting() -> ExposureContext {
    let mut context = context();
    context.outcome.adjusting = true;
    context.adjustment.level = 1000;
    context.snapshot.focus.levels[4] = 50;
    context
}

#[test]
fn an_adjusting_refresh_clamps_the_tables_to_the_near_bounds_around_the_adjustment_level() {
    let mut world = window_world(|_| 1050);
    world.put_u16(level_curve(1, 3), 2000);
    world.put_u16(subject_curve(2, 0), 100);
    world.put_u16(level_curve(3, 4), 1000);
    refresh(&mut world, &mut adjusting());
    assert_eq!(level_at(&world, level_curve(1, 3)), 1150);
    assert_eq!(level_at(&world, subject_curve(2, 0)), 950);
    assert_eq!(level_at(&world, level_curve(3, 4)), 1000);
    assert!(!world.stores.iter().any(|store| store.0 == level_curve(3, 4).0));
}

#[test]
fn enough_weight_beyond_a_near_bound_moves_that_side_to_the_margin() {
    let mut world = window_world(|point| match point {
        0..=5 => 1200,
        6..=11 => 900,
        _ => 1050,
    });
    world.put_u16(level_curve(1, 0), 2000);
    world.put_u16(level_curve(1, 1), 0);
    refresh(&mut world, &mut adjusting());
    assert_eq!((level_at(&world, level_curve(1, 0)), level_at(&world, level_curve(1, 1))), (1050 + 30 + 250, 1050 - 900 - 30));
}

#[test]
fn the_configured_table_range_bounds_the_window() {
    let mut world = window_world(|point| if point < 6 { 1200 } else { 1050 });
    world.property(property::TABLE_CEILING, 100);
    world.property(property::TABLE_FLOOR, 200);
    world.put_u16(level_curve(1, 0), 2000);
    world.put_u16(level_curve(1, 1), 0);
    refresh(&mut world, &mut adjusting());
    assert_eq!((level_at(&world, level_curve(1, 0)), level_at(&world, level_curve(1, 1))), (1250, 1100));
}

#[test]
fn the_lower_bound_wins_over_an_upper_bound_below_it() {
    let mut world = window_world(|_| 1050);
    world.property(property::TABLE_CEILING, 50);
    world.put_u16(level_curve(1, 0), 2000);
    refresh(&mut world, &mut adjusting());
    assert_eq!(level_at(&world, level_curve(1, 0)), 950);
}

#[test]
fn a_fifth_level_above_99_lowers_the_window_by_100() {
    let mut world = window_world(|_| 1050);
    world.put_u16(level_curve(1, 0), 2000);
    let mut context = adjusting();
    context.snapshot.focus.levels[4] = 150;
    refresh(&mut world, &mut context);
    assert_eq!(level_at(&world, level_curve(1, 0)), 1000 + 150 - 100 + 100);
}

fn carrying_world() -> World {
    let mut world = World::default();
    world.header(SharedHeaderByte::At63, 1);
    world.property_word(property::level_step(2), 100);
    world
}

#[test]
fn a_primary_exposure_leaves_its_tables_for_the_next_exposure_of_another_mode() {
    let mut world = carrying_world();
    world.put_u16(level_curve(1, 9), 321);
    world.put_u16(subject_curve(3, 2), 77);
    let mut context = context();
    context.levels = levels::start(&mut world, context.binding.record_handle(), false);
    refresh(&mut world, &mut context);
    world.put_u16(level_curve(1, 9), 0);
    world.put_u16(subject_curve(3, 2), 0);
    context.binding.exposure_mode = ExposureMode(2);
    refresh(&mut world, &mut context);
    assert_eq!((level_at(&world, level_curve(1, 9)), level_at(&world, subject_curve(3, 2))), (321, 77));
}

#[test]
fn the_next_exposure_raises_the_carried_tables_by_its_start_above_the_saved_start() {
    let mut world = carrying_world();
    world.put_u16(level_curve(0, 0), 10);
    world.put_u16(level_curve(0, 1), 500);
    let mut context = context();
    context.binding.exposure_mode = ExposureMode(2);
    let start = levels::start(&mut world, context.binding.record_handle(), false);
    context.saved.curves[0][0] = 10;
    context.saved.curves[0][1] = 500;
    context.saved.start = start.start - 20;
    refresh(&mut world, &mut context);
    let limit = start.start + 250 - start.target;
    assert_eq!((level_at(&world, level_curve(0, 0)), level_at(&world, level_curve(0, 1))), (30, limit));
    assert_eq!(context.levels, start);
}

#[test]
fn carrying_records_rebuild_treatments_and_name_the_snapshot_subjects_when_adjusting() {
    let mut world = carrying_world();
    world.set(&group_00::AT_005D, 1);
    world.header(SharedHeaderByte::At31, 1);
    world.header(SharedHeaderByte::At47, 1);
    let mut context = context();
    context.binding.subjects.count = 2;
    refresh(&mut world, &mut context);
    assert_eq!((context.outcome.treated, context.outcome.first_treatment), (true, true));
    assert_eq!(context.correction.subjects, Some(placed(InPlace::BoundSubjects)));
    context.outcome.adjusting = true;
    refresh(&mut world, &mut context);
    assert_eq!((context.correction.subjects, context.correction.summary), (Some(placed(InPlace::SnapshotSubjects)), Some(placed(InPlace::SnapshotTables))));
}

#[test]
fn the_correction_is_posted_with_its_request_and_callback() {
    let mut world = World::default();
    let mut posted = Posted::default();
    let mut context = context();
    context.correction.lens_reference = 0x77;
    assert_eq!(post(&mut world, &mut posted, &mut context, CALLBACK), Ok(CorrectionPost::Posted));
    assert_eq!(posted.calc, [CalcPayload::Correction { request: context.correction, callback: CALLBACK }]);
}

#[test]
fn the_correction_is_skipped_without_the_mask_bit_or_for_a_reused_focus_result() {
    let mut world = World::default();
    let mut posted = Posted::default();
    let mut unmasked = context();
    unmasked.binding.mask = 2;
    assert_eq!(post(&mut world, &mut posted, &mut unmasked, CALLBACK), Ok(CorrectionPost::Skipped));
    let mut reused = context();
    reused.outcome.focus_reused = true;
    assert_eq!(post(&mut world, &mut posted, &mut reused, CALLBACK), Ok(CorrectionPost::Skipped));
    assert!(posted.calc.is_empty());
}

#[test]
fn a_reused_focus_result_is_corrected_in_the_adjustment_modes_or_when_configured() {
    let mut world = World::default();
    let mut posted = Posted::default();
    let mut reused = context();
    reused.outcome.focus_reused = true;
    reused.binding.exposure_mode = ExposureMode(3);
    assert_eq!(post(&mut world, &mut posted, &mut reused, CALLBACK), Ok(CorrectionPost::Posted));
    reused.binding.exposure_mode = ExposureMode(0);
    world.property(property::CORRECTION_FLAGS, 2);
    assert_eq!(post(&mut world, &mut posted, &mut reused, CALLBACK), Ok(CorrectionPost::Posted));
    assert_eq!(posted.calc.len(), 2);
}
