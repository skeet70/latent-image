use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_22;

use crate::adjust::motion::exposure::{FrameExposure, ProgramKind};
use crate::adjust::motion::ports::{AreaClass, AreaGrid, CropGeometry, DefaultCurve, LensLimits, Rectangle, SearchBounds};
use crate::adjust::motion::search::{AreaCell, SearchMode};
use crate::adjust::motion::state::MotionState;
use crate::adjust::motion::world::{blank_state, MotionWorld};
use crate::stock::adjust::{CONFIGURED_FACTOR, RATE_SECOND_FACTOR, RATE_STEPS, TARGET_FACTOR};
use crate::stock::motion::{self as stock};

const SECTION: SharedAddress = SharedAddress(0x0100_0000);
const LENS: u32 = 7;
const PROFILE: u32 = 0x21;

fn section_word(world: &MotionWorld, offset: u32) -> u32 { world.world.u32_at(SharedAddress(SECTION.0 + offset)) }

fn section_words(world: &MotionWorld, offset: u32, count: u32) -> Vec<u32> { (0..count).map(|index| section_word(world, offset + 4 * index)).collect() }

/// A class 1 plain run of lens 7 whose second exposure, in profile 0x21 at lens step 5, has a
/// linear level of 1000, so that its exposure lasts 100000.
fn state() -> MotionState {
    let mut state = blank_state(SECTION, SharedAddress(0x0200_0000), 1000);
    state.readings.lens = LENS;
    state.readings.class = 1;
    state.exposures.second = FrameExposure { profile: PROFILE, lens_step: 5, ..FrameExposure::default() };
    state
}

/// Sample rows read 3 per line from line 40.
fn timed(world: &mut MotionWorld) {
    world.readout_start = 10;
    world.reference_line = 50;
    world.line_periods.insert(PROFILE, 3);
}

fn lay_out(state: &mut MotionState, world: &mut MotionWorld) { state.lay_out_samples(world) }

fn treated(state: &mut MotionState, kind: u32) {
    state.search.mode = SearchMode::Treated;
    state.areas.count = 1;
    state.areas.areas[0].at_1c = kind;
}

#[test]
fn each_primary_row_starts_its_exposure_before_its_readout() {
    let mut world = MotionWorld::default();
    timed(&mut world);
    world.sample_frames = [AreaGrid { cell_height: 20, top: 100, gap_vertical: 10, rows: 3, columns: 7, ..AreaGrid::default() }, AreaGrid { rows: 2, columns: 1, ..AreaGrid::default() }];
    let mut state = state();
    lay_out(&mut state, &mut world);
    let schedule = state.schedule();
    assert_eq!(schedule.exposure, 100_000);
    assert_eq!(schedule.primary(), [450 - 100_000, 540 - 100_000, 630 - 100_000]);
    assert_eq!(schedule.secondary(), [0, 0]);
    assert_eq!(section_words(&world, 0x44580, 3), [450u32.wrapping_sub(100_000), 540u32.wrapping_sub(100_000), 630u32.wrapping_sub(100_000)]);
    assert_eq!(section_words(&world, 0x4459c, 3), [450, 540, 630]);
    assert_eq!(section_word(&world, 0x4457c), 40);
}

#[test]
fn a_treated_run_schedules_its_secondary_areas_along_its_listed_areas_class() {
    let mut world = MotionWorld::default();
    timed(&mut world);
    let secondary = AreaGrid { cell_height: 40, top: 300, rows: 3, columns: 1, ..AreaGrid::default() };
    world.sample_frames = [AreaGrid::default(), secondary];
    let mut state = state();
    treated(&mut state, 6);
    lay_out(&mut state, &mut world);
    let readouts = [440 * 3, 400 * 3, 360 * 3];
    assert_eq!(state.schedule().secondary(), readouts.map(|readout| readout - 100_000));
    assert_eq!(section_words(&world, 0x445e0, 3), readouts.map(|readout| readout as u32));
    let request = world.sample_frame_requests.last().unwrap();
    assert_eq!(request.tracking.map(|tracking| tracking.class), Some(AreaClass::RowsUpward));
    assert!(request.whole_frame);
    world.sample_frames[1] = AreaGrid { cell_height: 40, top: 300, rows: 1, columns: 3, ..AreaGrid::default() };
    treated(&mut state, 3);
    lay_out(&mut state, &mut world);
    assert_eq!(section_words(&world, 0x445e0, 3), [360 * 3; 3]);
    assert_eq!(state.schedule().secondary(), [360 * 3 - 100_000]);
}

#[test]
fn an_area_run_samples_one_scaled_cell_around_its_area() {
    let mut world = MotionWorld::default();
    let primary = AreaGrid { cell_height: 30, cell_width: 40, left: 100, top: 200, gap_horizontal: 4, gap_vertical: 6, columns: 7, rows: 7, at_16: [1, 2] };
    world.sample_frames = [primary, AreaGrid::default()];
    world.cell_steps = [4, 6];
    world.crop = CropGeometry { crop: Rectangle { left: 0, top: 0, height: 1000, width: 1000 }, ..CropGeometry::default() };
    let mut state = state();
    state.readings.class = 2;
    state.search.mode = SearchMode::Area;
    state.search.area = AreaCell { column: 2, row: 1 };
    state.search.area_scale = 50;
    lay_out(&mut state, &mut world);
    let expected = AreaGrid { cell_height: 12, cell_width: 18, left: 199, top: 245, gap_horizontal: 0, gap_vertical: 0, columns: 1, rows: 1, at_16: [1, 2] };
    assert_eq!(state.layout().grids[0], expected);
    assert!(!world.sample_frame_requests[0].whole_frame);
    world.crop.crop.width = 10;
    state.search.area = AreaCell { column: 0, row: 0 };
    lay_out(&mut state, &mut world);
    let cell = state.layout().grids[0];
    assert_eq!((cell.cell_width, cell.left, cell.top), (10, 115, 209));
}

#[test]
fn a_class_0_run_samples_its_area_in_the_crops_eighths_unless_its_scene_masks_the_second_channel() {
    let mut world = MotionWorld::default();
    let primary = AreaGrid { cell_height: 30, cell_width: 40, columns: 7, rows: 7, ..AreaGrid::default() };
    world.sample_frames = [primary, AreaGrid::default()];
    world.crop = CropGeometry { crop: Rectangle { left: 80, top: 40, height: 800, width: 1600 }, ..CropGeometry::default() };
    let mut state = state();
    state.readings.class = 0;
    state.search.area = AreaCell { column: 3, row: 3 };
    state.search.area_scale = 100;
    lay_out(&mut state, &mut world);
    let cell = state.layout().grids[0];
    assert_eq!((cell.left, cell.top, cell.cell_width, cell.cell_height), (860, 425, 40, 30));
    state.readings.scene = stock::MASKED_SCENE;
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().grids[0], primary);
}

#[test]
fn the_drive_starts_at_its_channels_start_moved_to_the_lens_step_within_the_bounds() {
    let mut world = MotionWorld::default();
    world.channel_starts.insert(1, 30);
    world.bounds.insert(1, SearchBounds { low: -10, high: 20 });
    let mut state = state();
    state.readings.channel = 1;
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().start_position, 20);
    assert_eq!(state.bounds(), SearchBounds { low: -10, high: 20 });
    world.channel_starts.insert(1, -40);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().start_position, -10);
}

#[test]
fn a_channel_0_run_of_a_listed_scene_starts_at_channel_0s_table_position() {
    let mut world = MotionWorld::default();
    world.channel_starts.insert(0, 30);
    world.table_positions.insert(0, [12, 0]);
    world.bounds.insert(0, SearchBounds { low: -100, high: 100 });
    let mut state = state();
    state.readings.scene = 5;
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().start_position, 17);
    state.readings.scene = 1;
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().start_position, 35);
}

/// A drive rate of 50 for 500 of lens 7: a period of 2000, at position 15.
fn rated(world: &mut MotionWorld, state: &mut MotionState) {
    world.property(TARGET_FACTOR, 100);
    world.property(RATE_STEPS, 0);
    world.world.lens_configuration.values.insert(0, 1000);
    world.world.lens_configuration.values.insert(LENS as i32, 100);
    world.period_positions.insert(2000, 15);
    world.bounds.insert(0, SearchBounds { low: -10, high: 20 });
    state.areas.areas[0].at_22 = 500;
}

#[test]
fn a_treated_run_ends_its_window_where_its_areas_rate_drives_and_traces_it() {
    let mut world = MotionWorld::default();
    let mut state = state();
    treated(&mut state, 0);
    rated(&mut world, &mut state);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().treated_position, 15);
    assert_eq!(section_words(&world, 0x469e0, 9), [500, 500, 500_000, 1000, LENS, 100, 50, 2000, 15]);
    assert_ne!(state.layout().start_position, 15);
    world.property(stock::TREATED_WINDOW_START, 1);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().start_position, 15);
    world.property(stock::TREATED_WINDOW_START, 0x10);
    state.readings.auxiliary = 1;
    world.property(RATE_SECOND_FACTOR, 100);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().start_position, 15);
}

#[test]
fn an_untreated_run_has_no_window() {
    let mut world = MotionWorld::default();
    let mut state = state();
    rated(&mut world, &mut state);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().treated_position, 0);
    assert_eq!(section_words(&world, 0x469e0, 9), [0; 9]);
}

#[test]
fn the_exposure_ratio_is_held_between_its_floor_and_its_ceilings() {
    let mut world = MotionWorld::default();
    world.world.lens_levels.insert((5, LENS), 5);
    let mut state = state();
    state.exposures.second = FrameExposure { exposure_level: 100, profile_offset: 10, at_0c: 20, change: 30, ..state.exposures.second };
    world.property(stock::RATIO_CEILING, 200);
    world.property(stock::RATIO_LEVEL, 150);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().exposure_ratio, 1045);
    world.property(stock::RATIO_FLOOR, 120);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().exposure_ratio, 1200);
    world.property(stock::RATIO_FLOOR, 0);
    world.property(stock::RATIO_WITHOUT_CHANGE, 1);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().exposure_ratio, 1075);
    world.property(stock::RATIO_CEILING, 100);
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().exposure_ratio, 1000);
}

#[test]
fn the_default_curve_takes_the_records_stored_weights_and_profile_factors() {
    let mut world = MotionWorld::default();
    world.channel_weights = [7, 8, 9];
    world.default_curve = DefaultCurve { points: [10, 20, 30], limit: 77 };
    world.world.set(&group_00::AT_0049, 1);
    world.world.set(&group_00::AT_004A, 2);
    world.world.set(&group_00::AT_003E, 3);
    let mut state = state();
    state.readings.levels[0] = -4;
    lay_out(&mut state, &mut world);
    assert_eq!(state.layout().curve, [13, 21, 32]);
    assert_eq!(state.layout().curve_limit, 77);
    let request = world.curve_requests.last().unwrap();
    assert_eq!((request.weights, request.lens, request.level), ([7, 8, 9], LENS, -4));
    let mut stored = [0u8; 20];
    stored[..6].copy_from_slice(&[1, 0, 2, 0, 3, 0]);
    stored[14] = 1;
    world.world.set_shared(&group_22::AT_0180, stored);
    lay_out(&mut state, &mut world);
    assert_eq!(world.curve_requests.last().unwrap().weights, [1, 2, 3]);
}

#[test]
fn the_layout_keeps_the_second_exposures_positions_weights_and_rates() {
    let mut world = MotionWorld::default();
    world.table_positions.insert(5, [3, 4]);
    world.lens_limits = LensLimits { low: -7, high: 9 };
    world.channel_weights = [10, 20, 30];
    world.feature_classes.insert(PROFILE, 9);
    world.property(stock::program_setting(ProgramKind::Even.index(), 2), 9);
    world.property(CONFIGURED_FACTOR[2], 3);
    world.sample_frames = [AreaGrid { cell_height: 3, cell_width: 4, ..AreaGrid::default() }, AreaGrid::default()];
    let mut state = state();
    state.exposures.second.at_06 = -2;
    state.exposures.second.step = 2;
    state.exposures.second.program = ProgramKind::Even;
    state.program[0].bottom = 100;
    state.program[0].second_top = 25;
    lay_out(&mut state, &mut world);
    let layout = state.layout();
    assert_eq!(layout.channel_positions, [3, 4]);
    assert_eq!(layout.low_limit, -7);
    let query = world.limit_queries.last().unwrap();
    assert!(query.with_margins && query.channel == 0 && query.step == 5);
    assert_eq!(layout.exposure_at_06, -2);
    assert_eq!(layout.weights, [10, 40, 60]);
    assert_eq!(layout.step_setting, 5);
    assert_eq!(layout.cell_rate, 72);
    assert_eq!(layout.lowest_level, 125);
}

#[test]
fn the_layout_traces_its_grid_settings_grids_and_bounds() {
    let mut world = MotionWorld::default();
    world.property(stock::SAMPLE_GRID_PERCENT[0], 80);
    world.property(stock::SAMPLE_GRID_PERCENT[1], 70);
    world.property(stock::sample_row_percent(2), 9);
    world.property(stock::sample_column_percent(2), 11);
    let grid = AreaGrid { cell_height: 1, cell_width: 2, left: 3, top: 4, gap_horizontal: 5, gap_vertical: 6, columns: 7, rows: 8, at_16: [0; 2] };
    world.sample_frames = [grid, grid];
    world.bounds.insert(0, SearchBounds { low: -2, high: 300 });
    let mut state = state();
    state.exposures.second.step = 2;
    state.search.frames = [grid, grid];
    lay_out(&mut state, &mut world);
    assert_eq!(section_words(&world, 0x44c28, 4), [80, 70, 9, 11]);
    let packed = [3 << 16 | 4, 2 << 16 | 1, 5 << 16 | 6, 7 << 16 | 8];
    let expected = [packed[0], packed[1], packed[2], packed[3], packed[0], packed[1], packed[3], packed[0], packed[1], packed[2], packed[3], packed[0], packed[1], packed[3]];
    assert_eq!(section_words(&world, 0x44c40, 14), expected);
    assert_eq!(section_word(&world, 0x44bfc), 0xfffe_012c);
    assert_eq!(section_word(&world, 0x44c00), 0xfffe_012c);
    let request = world.sample_frame_requests.last().unwrap();
    assert_eq!((request.grid.height_percent, request.grid.row_percent, request.grid.bands, request.lens_step), (80, 9, 7, 5));
}

#[test]
fn a_treated_layout_traces_its_crop_and_tracked_grid_against_the_window() {
    let mut world = MotionWorld::default();
    world.crop = CropGeometry {
        window: Rectangle { left: 10, top: 20, height: 600, width: 800 },
        crop: Rectangle { left: 15, top: 30, height: 500, width: 700 },
    };
    let tracked = AreaGrid { cell_height: 40, cell_width: 50, left: 110, top: 220, columns: 1, rows: 3, ..AreaGrid::default() };
    world.sample_frames = [AreaGrid::default(), tracked];
    let mut state = state();
    treated(&mut state, 0);
    lay_out(&mut state, &mut world);
    assert_eq!(section_words(&world, 0x469b4, 10), [800, 600, 5, 10, 100, 200, 50, 40, 1, 3]);
    assert_eq!(state.layout().crop, world.crop);
}
