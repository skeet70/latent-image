use xm1_saclac_profile_records::stock::record::{group_00, group_01};
use xm1_saclac_profile_records::stock::shared::group_18;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::motion::readings::{FocusRequest, PreparationInput, RecordReadings, StepLimits};
use crate::adjust::motion::world::{Lists, MotionWorld};
use crate::exposure::fakes::handle;
use crate::stock::motion::{self as stock};
use crate::stock::property;

const LENS: u32 = 7;
const REFERENCE_KEY: u32 = 0x55;

fn input() -> PreparationInput { PreparationInput { record: handle(), lens: LENS, load_test: false, request: FocusRequest::default() } }

fn read(world: &mut MotionWorld, lists: &mut Lists) -> RecordReadings { RecordReadings::read(world, lists, &input()) }

fn limits(world: &mut MotionWorld) -> StepLimits {
    let readings = read(world, &mut Lists::default());
    StepLimits::configured(world, &readings)
}

/// A world whose lens reference offset is 5.
fn offset_world() -> MotionWorld {
    let mut world = MotionWorld::default();
    world.world.set(&group_01::AT_00BC, REFERENCE_KEY);
    world.world.reference_offsets.insert(REFERENCE_KEY, 5);
    world
}

#[test]
fn record_and_list_positions_are_taken_off_the_lens_reference_offset() {
    let mut world = offset_world();
    world.world.set(&group_01::AT_00A8, 100);
    world.world.set(&group_01::AT_0092, -20);
    let mut lists = Lists { area_positions: [40; 10], ..Lists::default() };
    let request = FocusRequest { positions: [10, 20, 30, 40], ..FocusRequest::default() };
    let readings = RecordReadings::read(&mut world, &mut lists, &PreparationInput { request, ..input() });
    assert_eq!(readings.levels[0], 95);
    assert_eq!(readings.levels[3], -25);
    assert_eq!(readings.area_positions, [35; 10]);
    assert_eq!(readings.request.positions, [5, 15, 25, 35]);
}

#[test]
fn an_armed_or_unclassed_record_reads_its_levels_at_the_lens_lowest_step() {
    let mut world = MotionWorld::default();
    world.lowest_steps.insert(LENS, 3);
    let mut limits = [0u8; 64];
    limits[0x38] = 2;
    limits[0x3c] = 9;
    world.world.set_shared(&group_18::AT_00E0, limits);
    world.world.header(SharedHeaderByte::At47, 1);
    let readings = read(&mut world, &mut Lists::default());
    assert_eq!((readings.limit_word, readings.level_step), (2, 9));
    world.world.header(SharedHeaderByte::At63, 1);
    let readings = read(&mut world, &mut Lists::default());
    assert_eq!((readings.limit_word, readings.level_step), (0, 3));
    world.world.header(SharedHeaderByte::At63, 0);
    world.world.header(SharedHeaderByte::At47, 0);
    assert_eq!(read(&mut world, &mut Lists::default()).level_step, 3);
}

#[test]
fn unclassed_rows_scaled_one_by_one_or_nine_by_sixteen_are_scaled_two_by_three() {
    let mut world = MotionWorld::default();
    world.world.set(&group_00::AT_0000, 1 | 1 << 16);
    assert_eq!(read(&mut world, &mut Lists::default()).row_scale, [2, 3]);
    world.world.set(&group_00::AT_0000, 9 | 16 << 16);
    assert_eq!(read(&mut world, &mut Lists::default()).row_scale, [2, 3]);
    world.world.set(&group_00::AT_0000, 9 | 4 << 16);
    assert_eq!(read(&mut world, &mut Lists::default()).row_scale, [9, 4]);
    world.world.header(SharedHeaderByte::At47, 1);
    world.world.set(&group_00::AT_0000, 1 | 1 << 16);
    assert_eq!(read(&mut world, &mut Lists::default()).row_scale, [1, 1]);
}

#[test]
fn the_configured_record_level_follows_the_channels_selection_nibble() {
    let mut world = offset_world();
    for (field, level) in [(&group_01::AT_00A8, 108), (&group_01::AT_0094, 104), (&group_01::AT_0096, 106), (&group_01::AT_0092, 102)] {
        world.world.set(field, level);
    }
    world.property_word(property::level_step(3), 1000);
    world.property(stock::LEVEL_SELECTION, 0x32);
    let readings = read(&mut world, &mut Lists::default());
    assert_eq!(readings.configured_level(&mut world), 1000 + 108 - 5);
    world.world.header(SharedHeaderByte::At46, 1);
    let readings = read(&mut world, &mut Lists::default());
    assert_eq!(readings.configured_level(&mut world), 1000 + 102 - 5);
    world.property(stock::LEVEL_SELECTION, 0x10);
    assert_eq!(readings.configured_level(&mut world), 1000 + 106 - 5);
    world.property(stock::LEVEL_SELECTION, 0x70);
    assert_eq!(readings.configured_level(&mut world), 1000 + 104 - 5);
}

#[test]
fn without_configuration_a_run_may_drive_over_every_step() {
    let mut world = MotionWorld { steps: 40, ..MotionWorld::default() };
    assert_eq!(limits(&mut world), StepLimits { lowest: 0, highest: 39, reach: 0 });
}

#[test]
fn the_lens_range_the_per_lens_table_and_the_cap_narrow_the_steps() {
    let mut world = MotionWorld { steps: 40, ..MotionWorld::default() };
    world.lowest_steps.insert(LENS, 5);
    world.highest_steps.insert(LENS, 30);
    world.property(stock::STEP_LIMITS, 1);
    assert_eq!(limits(&mut world), StepLimits { lowest: 5, highest: 30, reach: 0 });
    world.property(stock::STEP_LIMITS, 5);
    world.property(stock::step_limit_floor(0), 7);
    world.property(stock::step_limit_ceiling(0), 50);
    assert_eq!(limits(&mut world), StepLimits { lowest: 7, highest: 30, reach: 0 });
    world.property(stock::step_limit_ceiling(0), 20);
    assert_eq!(limits(&mut world).highest, 20);
    world.property(stock::STEP_CAP, 0x11);
    world.property(stock::STEP_CAP_VALUE, 12);
    assert_eq!(limits(&mut world).highest, 12);
    world.property(stock::STEP_CAP, 0x10);
    assert_eq!(limits(&mut world).highest, 20, "a cap needs a low nibble too");
}

#[test]
fn the_speed_bound_stops_at_the_highest_step_the_drive_covers_within_one_exposure() {
    let mut world = MotionWorld { steps: 40, top_speed: 100, ..MotionWorld::default() };
    world.property(stock::STEP_LIMITS, 2);
    world.property(stock::FASTEST_DEPTH, 1);
    world.times.insert((stock::SEARCH_PROFILE, 1000, 0), 500_000);
    for step in 20..40 {
        world.depth_positions.insert(step, 10);
    }
    assert_eq!(limits(&mut world).highest, 19, "10 positions take 100 ms, 100 positions a second");
}
