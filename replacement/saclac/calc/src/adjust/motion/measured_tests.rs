use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::exposure::FrameExposure;
use crate::adjust::motion::measured::MeasuredLevels;
use crate::adjust::motion::state::MotionState;
use crate::adjust::motion::world::{blank_state, MotionWorld};
use crate::exposure::ports::StockMemory;
use crate::stock::motion::{self as stock};
use crate::stock::property;
use crate::stock::shared::level_curve_base;

const SECTION: SharedAddress = SharedAddress(0x0100_0000);
const LENS: u32 = 7;
const STEP_TWO: i16 = 20;
const MARGIN: u32 = 30;

fn section_word(world: &MotionWorld, offset: u32) -> u32 { world.world.u32_at(SharedAddress(SECTION.0 + offset)) }

fn section_words(world: &MotionWorld, offset: u32, count: u32) -> Vec<u32> { (0..count).map(|index| section_word(world, offset + 4 * index)).collect() }

/// Fixed exposures at level step 2 of 20 and a margin of 30.
fn world() -> MotionWorld {
    let mut world = MotionWorld::default();
    world.property_word(property::level_step(2), STEP_TWO as u16);
    world.property(stock::MEASURED_LEVEL_MARGIN, MARGIN);
    world.property(stock::FIXED_EXPOSURE, 1);
    world
}

/// Record levels 100 to 500 in the readings' order; the first exposure at lens step 5, the
/// second at level 200.
fn state() -> MotionState {
    let mut state = blank_state(SECTION, SharedAddress(0x0200_0000), 1000);
    state.readings.lens = LENS;
    state.readings.levels = [100, 200, 300, 400, 500];
    state.exposures.first = FrameExposure { lens_step: 5, profile: stock::SEARCH_PROFILE, level: 150, ..FrameExposure::default() };
    let second = FrameExposure { lens_step: 5, profile: stock::SEARCH_PROFILE, level: 200, at_06: -3, change: 7, step: 2, ..FrameExposure::default() };
    state.exposures.second = second;
    state.exposures.prepared_second = second;
    state
}

/// Every measured level `excess` above its record level after level step 2.
fn measured_above(excess: i16) -> MeasuredLevels {
    MeasuredLevels { levels: [100, 200, 300, 400, 500].map(|level: i16| level + STEP_TWO + excess), curve_level: 0, curve_point: 0 }
}

#[test]
fn a_measured_level_more_than_the_margin_above_the_record_exposes_the_frames_again() {
    let mut world = world();
    let mut state = state();
    world.world.write_u32(stock::reexposure_word(SECTION), 9);
    state.check_measured_levels(&mut world, &measured_above(MARGIN as i16 + 1));
    assert!(state.level_check().reexposed);
    assert_eq!(state.frame_exposure().level, 200 + STEP_TWO + 31);
    assert_eq!(state.exposures().prepared_second.level, 200);
    assert_eq!(section_word(&world, 0x48d38), 1);
    assert_eq!(section_word(&world, 0x44b24), 251);
    assert_eq!(section_word(&world, 0x44b20), 0);
    assert_eq!(world.world.u32_at(stock::reexposure_word(SECTION)), 0);
}

#[test]
fn a_measured_level_within_the_margin_leaves_the_frames_as_prepared() {
    let mut world = world();
    let mut state = state();
    world.world.write_u32(stock::reexposure_word(SECTION), 9);
    state.check_measured_levels(&mut world, &measured_above(MARGIN as i16));
    assert!(!state.level_check().reexposed);
    assert_eq!(*state.frame_exposure(), state.exposures().prepared_second);
    assert_eq!(section_word(&world, 0x48d38), 0);
    assert_eq!(section_word(&world, 0x44b24), 0);
    assert_eq!(world.world.u32_at(stock::reexposure_word(SECTION)), 9);
}

#[test]
fn the_frames_exposed_again_move_to_the_first_exposures_lens_step_up_to_the_ceiling() {
    let mut world = world();
    world.property(stock::FIXED_STEP, 3);
    world.property_word(stock::FIXED_LEVEL, 100);
    world.world.lens_levels.insert((3, LENS), 50);
    world.world.lens_levels.insert((5, LENS), 20);
    world.property(stock::LEVEL_CEILING, 20);
    world.times.insert((stock::SEARCH_PROFILE, 130, 0), 4321);
    let mut state = state();
    state.check_measured_levels(&mut world, &measured_above(40));
    let frames = state.frame_exposure();
    assert_eq!((frames.lens_step, frames.exposure_level, frames.time), (5, 130, 4321));
    world.property(stock::LEVEL_CEILING, 12);
    let mut state = self::state();
    state.check_measured_levels(&mut world, &measured_above(40));
    assert_eq!(state.frame_exposure().exposure_level, 120);
}

/// Only the record level at `index` lies below its measured level by more than the margin.
fn only_above(index: usize) -> MeasuredLevels {
    let mut measured = measured_above(0);
    measured.levels[index] += MARGIN as i16 + 1;
    measured
}

#[test]
fn the_configured_selection_of_the_runs_channel_picks_the_compared_level() {
    let mut world = world();
    world.property(stock::MEASURED_LEVEL_SELECTION, 0x32);
    for (index, reexposed) in [(0, true), (1, false), (2, false)] {
        let mut state = state();
        state.check_measured_levels(&mut world, &only_above(index));
        assert_eq!(state.level_check().reexposed, reexposed, "channel 0, level {index}");
    }
    for (index, reexposed) in [(3, true), (0, false)] {
        let mut state = state();
        state.readings.channel = 1;
        state.check_measured_levels(&mut world, &only_above(index));
        assert_eq!(state.level_check().reexposed, reexposed, "channel 1, level {index}");
    }
    assert_eq!(section_word(&world, 0x48d30), 3);
}

#[test]
fn a_class_two_record_compares_level_0x9c_at_selection_zero() {
    let mut world = world();
    let mut state = state();
    state.readings.class = 2;
    state.check_measured_levels(&mut world, &only_above(4));
    assert!(state.level_check().reexposed);
    let mut state = self::state();
    state.check_measured_levels(&mut world, &only_above(4));
    assert!(!state.level_check().reexposed);
    let mut state = self::state();
    state.check_measured_levels(&mut world, &only_above(1));
    assert!(state.level_check().reexposed);
}

#[test]
fn the_curve_selection_compares_the_record_level_at_the_measured_curve_point() {
    let mut world = world();
    world.property(stock::MEASURED_LEVEL_SELECTION, 4);
    world.world.reference_offsets.insert(0, 40);
    world.world.write_u16(SharedAddress(level_curve_base(0).0 + 2 * 3), 700);
    let mut state = state();
    state.readings.at_00b8 = 100;
    let measured = MeasuredLevels { curve_level: 600 + STEP_TWO + 31, curve_point: 3, ..measured_above(0) };
    state.check_measured_levels(&mut world, &measured);
    assert_eq!(state.level_check().curve_level, 600);
    assert!(state.level_check().reexposed);
    assert_eq!(state.frame_exposure().level, 651);
    let mut state = self::state();
    state.readings.at_00b8 = 100;
    state.check_measured_levels(&mut world, &MeasuredLevels { curve_level: 650, ..measured });
    assert!(!state.level_check().reexposed);
}

#[test]
fn the_check_traces_both_level_sets_and_the_prepared_exposures() {
    let mut world = world();
    world.world.lens_levels.insert((5, LENS), 33);
    world.world.reference_offsets.insert(0, 40);
    world.world.write_u16(SharedAddress(level_curve_base(0).0 + 2 * 6), 90);
    let mut state = state();
    state.readings.at_00b8 = 10;
    let measured = MeasuredLevels { levels: [11, 12, 13, 14, 15], curve_level: 16, curve_point: 6 };
    state.check_measured_levels(&mut world, &measured);
    let raised = |level: i32| (level + i32::from(STEP_TWO)) as u32;
    assert_eq!(section_words(&world, 0x48d08, 5), [raised(200), raised(300), raised(100), raised(400), raised(80)]);
    assert_eq!(section_words(&world, 0x48d1c, 5), [12, 13, 11, 14, 16]);
    assert_eq!(section_words(&world, 0x48cc8, 8), [(-3i32) as u32, 5, 33, 0, 0, 0, 7, 2]);
    assert_eq!(section_words(&world, 0x48ce8, 8), [0, 5, 33, 0, 0, 0, 0, 0]);
    assert_eq!(section_words(&world, 0x48d30, 2), [0, 6]);
}

#[test]
fn calcs_result_carries_the_measured_levels_by_offset() {
    let mut result = [0u8; stock::MEASURED_LEVELS_BYTES];
    for (offset, value) in [(4, 0x0102u16), (6, 0x0304), (8, 0xfffe), (0xa, 0x0007), (0xc, 0x0506), (0xe, 0x0708), (0x10, 0x090a)] {
        result[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(stock::measured_levels(&result), MeasuredLevels { levels: [0x0506, 0x0102, 0x0304, 0x0708, 0x090a], curve_level: -2, curve_point: 7 });
}
