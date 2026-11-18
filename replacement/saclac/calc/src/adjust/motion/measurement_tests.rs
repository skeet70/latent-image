#![forbid(unsafe_code)]

use xm1_saclac_interface::calc::SharedAddress;
use crate::adjust::motion::measurement::{KindSettings, SearchLimit};
use crate::adjust::motion::plan::{AreaLayout, PlanKind};
use crate::adjust::motion::search::{AreaCell, SearchMode};
use crate::adjust::motion::state::MotionState;
use crate::adjust::motion::world::{blank_state, MotionWorld};
use crate::exposure::ports::{Property, StockMemory};
use crate::stock::measurement::{self as stock, MeasurementRecords};

fn records() -> MeasurementRecords {
    MeasurementRecords { map: SharedAddress(0x2000000), checks: SharedAddress(0x2000400), selection: SharedAddress(0x2000800), peak: SharedAddress(0x2000c00), threshold: SharedAddress(0x2001000), weights: SharedAddress(0x2001400), search: SharedAddress(0x2001800) }
}
fn address(base: SharedAddress, offset: u32) -> SharedAddress { SharedAddress(base.0 + offset) }
fn word(world: &mut MotionWorld, base: SharedAddress, offset: u32) -> u32 { world.read_u32(address(base, offset)) }
fn half(world: &mut MotionWorld, base: SharedAddress, offset: u32) -> u16 { world.read_u16(address(base, offset)) }
fn byte(world: &mut MotionWorld, base: SharedAddress, offset: u32) -> u8 { world.read_u8(address(base, offset)) }
fn state() -> MotionState {
    let mut state = blank_state(SharedAddress(0x1000000), SharedAddress(0x1100000), 1000);
    state.exposures.second.frames = 4;
    state.plan.frame_depth = 150;
    state.plan.frame_travel = 50;
    state.layout.grids[0].columns = 7;
    state.layout.grids[0].rows = 7;
    state
}

#[test]
fn measurement_records_preserve_both_configured_property_bytes() {
    let mut world = MotionWorld::default();
    world.property(Property(0x94cb), 0x34);
    world.property(Property(0x94cc), 0x12);
    state().set_up_measurement(records(), &mut world);
    assert_eq!(word(&mut world, records().checks, 0xa0), 0x1234);
    assert_eq!(word(&mut world, records().search, 0x118), 0x1234);
}

#[test]
fn map_marks_neighbours_but_never_invents_an_invalid_measured_cell() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.search.area = AreaCell { row: 3, column: 3 };
    let result = state.measurement_settings(&mut world);
    assert_eq!(result.measured_cell, Some(24));
    assert_eq!(result.map.0[3][3], 0xfd);
    assert_eq!(result.map.0[3][2], 0xf1);
    assert_eq!(result.map.0[1][1], 0x41);
    assert_eq!(result.map.0[0][0], 1);
    state.search.area.column = 0xff;
    assert_eq!(state.measurement_settings(&mut world).measured_cell, None);
    state.layout.grids[0].columns = 1;
    state.layout.grids[0].rows = 1;
    let result = state.measurement_settings(&mut world);
    assert_eq!(result.map.0[0][0], 0x85);
    assert_eq!(result.map.0[0][1], 0);
    assert_eq!(result.measured_cell, None);
}

#[test]
fn mode_limits_include_the_margin_boundary_and_signed_margin() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.layout.lowest_level = 100;
    world.property(Property(0x9412), 0x1f);
    world.property(Property(0x9420), 2);
    state.exposures.second.level = 80;
    assert_eq!(state.measurement_settings(&mut world).search_limit.words(), [0, 1]);
    state.exposures.second.level = 81;
    assert_eq!(state.measurement_settings(&mut world).search_limit.words(), [0, 3]);
    world.property(Property(0x9420), 0xff);
    state.exposures.second.level = 110;
    assert!(state.measurement_settings(&mut world).search_limit.selected());
    state.search.mode = SearchMode::Channel;
    world.property(Property(0x941b), 4);
    world.property(Property(0x9423), 0xff);
    assert_eq!(state.measurement_settings(&mut world).search_limit, SearchLimit::Channel(true));
    world.property(stock::PLAIN_SEARCH, 1);
    assert_eq!(state.measurement_settings(&mut world).search_limit, SearchLimit::Off);
}

#[test]
fn selection_order_respects_layout_and_kind_specific_configuration() {
    let mut world = MotionWorld::default();
    let mut state = state();
    world.property(Property(0x946e), 7);
    for (layout, expected) in [(AreaLayout::Single, [0, 1, 4]), (AreaLayout::Listed, [0, 4, 4]), (AreaLayout::Grid, [0, 1, 4]), (AreaLayout::GridAlternate, [0, 1, 2])] {
        state.plan.layout = layout;
        assert_eq!(state.measurement_settings(&mut world).step_orders, [expected; 2]);
    }
    state.plan.kind = PlanKind(11);
    world.property(Property(0x92bb), 4);
    world.property(Property(0x92bc), 2);
    world.property(Property(0x941b), 0x20);
    let result = state.measurement_settings(&mut world);
    assert_eq!(result.step_orders, [[2, 4, 4], [1, 4, 4]]);
    assert!(result.selection_limit);
}

#[test]
fn channel_sets_apply_short_cutoff_and_strict_long_exposure_retention() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.search.mode = SearchMode::Channel;
    state.exposures.second.profile = 7;
    world.property(stock::CHANNEL_SETS, 3);
    world.property(stock::CHANNEL_BITS[0], 0xfe);
    world.property(stock::CHANNEL_BITS[1], 0xfe);
    world.property(stock::CHANNEL_CUTOFF, 1);
    world.property(stock::CHANNEL_KEPT, 1);
    world.property(stock::CHANNEL_TIMES[0], 4);
    world.property(stock::CHANNEL_TIMES[1], 5);
    world.times.insert((7, 0, 4), 1001);
    world.times.insert((7, 0, 5), 1000);
    world.property(stock::LEVEL_CHECKS, 1);
    let result = state.set_up_measurement(records(), &mut world);
    assert_eq!(result.channels.unwrap().channels, [true, true, true, true, true, true, true, false, false, false, false, false, false, false]);
    assert!(!result.channels.unwrap().kept);
    assert!(result.level_checks.is_none());
    assert!(result.second_level_checks.is_some());
    assert_eq!(byte(&mut world, records().checks, 0x86), 0);
    assert_eq!(byte(&mut world, records().search, 0xf7), 1);
    state.exposures.second.time = 1001;
    let result = state.measurement_settings(&mut world);
    assert!(result.channels.unwrap().channels[7]);
    assert!(result.channels.unwrap().kept);
}

#[test]
fn row_weights_interpolate_gain_and_keep_byte_offsets_unsigned() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.exposures.second.first_level = 150;
    for (id, value) in stock::ROW_GAIN_LEVELS.into_iter().zip([0, 10, 20, 30]) { world.property(id, value); }
    for (id, value) in stock::ROW_GAINS.into_iter().zip([10, 20, 40, 80]) { world.property(id, value); }
    world.property_word(stock::ROW_WEIGHTS[0], 200);
    world.property(stock::ROW_OFFSETS[0], 200);
    world.property(stock::ROW_NEGATIVE_OFFSETS[0], 255);
    let result = state.set_up_measurement(records(), &mut world);
    assert_eq!(result.row_weights[0][0].weight, 600);
    assert_eq!(word(&mut world, records().weights, 4), 200);
    assert_eq!(word(&mut world, records().weights, 8) as i32, -255);
}

#[test]
fn channel_drive_keeps_plain_settings_and_ceil_rounds_lens_travel() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.search.mode = SearchMode::Channel;
    world.position_rate = 1000;
    world.curve_word = 20;
    world.mapped_step_word = 3;
    world.world.lens_configuration.values.insert(0, 10);
    world.property_word(stock::CHANNEL_DRIVE_BASE, 10);
    world.property(stock::CHANNEL_DRIVE_FACTOR, 2);
    world.property(stock::CHANNEL_DRIVE_LIMIT, 100);
    world.property_word(stock::CHANNEL_TRAVEL_SCALE, 1);
    world.property(stock::CHANNEL_TRAVEL_PERCENT, 4);
    for (id, value) in stock::PLAIN_TRAVEL.into_iter().zip([2, 1, 10]) { world.property(id, value); }
    let result = state.set_up_measurement(records(), &mut world);
    let KindSettings::Channel { drive, plain } = result.kind else { panic!("channel kind"); };
    assert_eq!((drive.frames, drive.step_frames, drive.slope, drive.travel), (100, 98, 128, 4));
    assert_eq!(plain.plain.unwrap().travel, 10);
    assert_eq!(word(&mut world, records().search, 0x13c), 1);
    assert_eq!(half(&mut world, records().search, 0x15e), 10);
    assert_eq!(word(&mut world, records().search, 0x18c), 100);
    assert_eq!(half(&mut world, records().search, 0x198), 4);
    world.property(stock::CHANNEL_TRAVEL_PERCENT, 1);
    let KindSettings::Channel { drive, .. } = state.measurement_settings(&mut world).kind else { panic!("channel kind"); };
    assert_eq!(drive.travel, 3);
    world.world.lens_configuration.values.insert(0, 0);
    let KindSettings::Channel { drive, .. } = state.measurement_settings(&mut world).kind else { panic!("channel kind"); };
    assert_eq!(drive.travel, 0);
}

#[test]
fn a_treated_pass_uses_secondary_thresholds_and_erases_old_channel_settings() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.search.mode = SearchMode::Channel;
    state.set_up_measurement(records(), &mut world);
    state.search.mode = SearchMode::Treated;
    state.thresholds.rows = [[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12], [13, 14, 15]];
    state.depth_settings.halves = [101, 102, 103];
    state.depth_settings.bytes = [11, 12, 13];
    state.treated_settings = [5, 6, 1, 1];
    state.set_up_measurement(records(), &mut world);
    assert_eq!(word(&mut world, records().checks, 0x10), 13);
    assert_eq!(word(&mut world, records().checks, 0x24), 15);
    assert_eq!(half(&mut world, records().search, 0x9e), 101);
    assert_eq!(byte(&mut world, records().search, 0xa4), 13);
    assert_eq!(half(&mut world, records().search, 0xa8), 103);
    assert_eq!(word(&mut world, records().search, 0x13c), 0);
    assert_eq!(half(&mut world, records().search, 0x180), 0);
    assert_eq!(word(&mut world, records().search, 0x1b0), 2);
    assert_eq!(byte(&mut world, records().search, 0x1b4), 5);
}

#[test]
fn plain_travel_uses_half_up_rounding_and_load_test_removes_bounds_and_checks() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.plan.frame_depth = 160;
    state.plan.frame_travel = 1000;
    for (id, value) in stock::PLAIN_TRAVEL.into_iter().zip([1, 0, 255]) { world.property(id, value); }
    let result = state.set_up_measurement(records(), &mut world);
    let KindSettings::Plain(plain) = result.kind else { panic!("plain kind"); };
    assert_eq!(plain.plain.unwrap().travel, 94);
    assert_eq!(half(&mut world, records().search, 0x20c), 0x7fff);
    state.readings.load_test = true;
    world.property(stock::LOAD_TEST_AREA, 3);
    world.property(stock::FIRST_CHECK, 1);
    world.property(stock::LEVEL_CHECKS, 1);
    world.property(stock::CHANNEL_SETS, 3);
    let result = state.set_up_measurement(records(), &mut world);
    let KindSettings::Plain(plain) = result.kind else { panic!("plain load test"); };
    assert_eq!(plain.steps, [2, 6, 6, 6, 6]);
    assert_eq!((result.first_check, result.level_checks, result.channels, result.bounds), (None, None, None, None));
    assert_eq!(half(&mut world, records().search, 0x20c), 0);
    assert_eq!(word(&mut world, records().search, 0x1ec), 0);
}

#[test]
fn ranged_scaling_uses_the_modes_share_and_masked_scene_keeps_its_depths() {
    let mut world = MotionWorld::default();
    let mut state = state();
    state.plan.kind = PlanKind(17);
    state.plan.primary_share = 10_000;
    state.plan.secondary_share = 5000;
    state.exposures.second.change_scale = 20;
    state.exposures.second.gain_scale = 30;
    world.property_word(stock::RANGED_FACTORS[0], 100);
    world.property_word(stock::RANGED_FACTORS[1], 200);
    assert_eq!(state.measurement_settings(&mut world).ranged, Some([600, 1200]));
    state.search.mode = SearchMode::Treated;
    state.readings.scene = stock::MASKED_SCENE;
    for (id, value) in stock::MASKED_DEPTHS.into_iter().zip([1, 2, 3, 4]) { world.property(id, value); }
    let result = state.set_up_measurement(records(), &mut world);
    assert_eq!(result.ranged, Some([300, 600]));
    assert_eq!(result.masked.unwrap().depths, [10, 20, 30, 40]);
    assert_eq!(half(&mut world, records().search, 0x1ba), 24);
    assert_eq!(word(&mut world, records().search, 0x1d0), 40);
}

#[test]
fn null_request_refuses_without_stores_and_real_request_publishes_aliases() {
    let mut world = MotionWorld::default();
    let state = state();
    assert_eq!(stock::configure_request(SharedAddress(0), &state, &mut world), 0x84);
    assert!(world.world.fills.is_empty());
    let request = SharedAddress(0x3000000);
    for (index, value) in records().addresses().iter().enumerate() { world.write_u32(address(request, 8 + index as u32 * 4), value.0); }
    assert_eq!(stock::configure_request(request, &state, &mut world), 0);
    assert_eq!(word(&mut world, request, 0), 0x1000000);
    assert_eq!(word(&mut world, request, 4), 0x1ffec90c);
}
