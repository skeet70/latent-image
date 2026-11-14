use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::{group_27, group_29};
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::motion::exposure::FrameExposure;
use crate::adjust::motion::ports::{AreaClass, AreaGrid, GridArea, SearchBounds, Tracking};
use crate::adjust::motion::prepare::Preparation;
use crate::adjust::motion::preparation::FrameSize;
use crate::adjust::motion::readings::{AreaList, FocusRequest, ListedArea, PreparationInput};
use crate::adjust::motion::search::{ChannelClass, SearchMode};
use crate::adjust::motion::state::MotionState;
use crate::adjust::motion::world::{Lists, MotionWorld};
use crate::exposure::fakes::handle;
use crate::exposure::ports::{Property, StockMemory};
use crate::stock::motion::{self as stock};

const LENS: u32 = 7;
const FRAME: SharedAddress = SharedAddress(0x4000_0000);

fn input(load_test: bool) -> PreparationInput { PreparationInput { record: handle(), lens: LENS, load_test, request: FocusRequest::default() } }

fn prepare_with(world: &mut MotionWorld, lists: &mut Lists, load_test: bool) -> (MotionState, Preparation) { MotionState::prepare(world, lists, &input(load_test), FRAME) }

fn prepare(world: &mut MotionWorld) -> (MotionState, Preparation) { prepare_with(world, &mut Lists::default(), false) }

fn class(world: &mut MotionWorld, class: u8) { world.world.header(SharedHeaderByte::At47, class) }

fn section_word(world: &MotionWorld, offset: u32) -> u32 { world.world.u32_at(SharedAddress(stock::section(FRAME).0 + offset)) }

fn results_word(world: &mut MotionWorld, offset: u32, value: u32) { world.world.write_u32(SharedAddress(stock::results(FRAME).0 + offset), value) }

/// A record the scene treatment's second variant admits: an active subject comparison with a
/// subject listed, class 1 and scene 1.
fn treated(world: &mut MotionWorld) -> Lists {
    world.world.set(&group_00::AT_005D, 1);
    class(world, 1);
    world.world.header(SharedHeaderByte::At31, 1);
    Lists { subjects: 1, ..Lists::default() }
}

/// The selection's frame and the search profile's are the same size.
fn unscaled(world: &mut MotionWorld) {
    for mode in [stock::SELECTION_MODE, stock::SEARCH_PROFILE] {
        world.frames.insert(mode, FrameSize { height: 100, width: 100 });
    }
}

/// Channel classes for any lens below 100.
fn classed(world: &mut MotionWorld) {
    world.property(stock::CHANNEL_CLASSES, 1);
    world.property(stock::LENS_CHANNEL_LIMIT, 100);
}

#[test]
fn the_record_class_and_configuration_choose_the_search_mode() {
    let mut world = MotionWorld::default();
    assert_eq!(prepare(&mut world).0.search_mode(), SearchMode::Area);
    world.property(stock::PLAIN_SEARCH, 1);
    assert_eq!(prepare(&mut world).0.search_mode(), SearchMode::Plain);
    world.property(stock::PLAIN_SEARCH, 0);
    class(&mut world, 7);
    assert_eq!(prepare(&mut world).0.search_mode(), SearchMode::Plain);
    world.property(stock::CHANNEL_SEARCH, 1);
    assert_eq!(prepare(&mut world).0.search_mode(), SearchMode::Channel);
    class(&mut world, 3);
    assert_eq!(prepare(&mut world).0.search_mode(), SearchMode::Plain);
    world.property(stock::LOAD_TEST_AREA, 1);
    assert_eq!(prepare(&mut world).0.search_mode(), SearchMode::Plain);
    assert_eq!(prepare_with(&mut world, &mut Lists::default(), true).0.search_mode(), SearchMode::Area);
    world.property(stock::LOAD_TEST_PLAIN, 1);
    assert_eq!(prepare_with(&mut world, &mut Lists::default(), true).0.search_mode(), SearchMode::Plain);
}

#[test]
fn a_treated_run_exposes_at_its_first_area_and_samples_with_the_treated_settings() {
    let mut world = MotionWorld::default();
    let mut lists = treated(&mut world);
    lists.area_positions = [70; 10];
    for (index, id) in stock::TREATED_PLAN_SETTINGS.into_iter().enumerate() {
        world.property(id, 10 + index as u32);
    }
    let (state, _) = prepare_with(&mut world, &mut lists, false);
    assert_eq!(state.search_mode(), SearchMode::Treated);
    assert_eq!(state.frame_exposure().level, 70);
    assert_eq!(state.search().level_ceiling, 0);
    assert_eq!(state.treated_settings, [10, 11, 12, 13]);
}

#[test]
fn channel_classes_follow_the_configuration_the_record_and_the_bounds_of_both_channels() {
    let mut world = MotionWorld::default();
    class(&mut world, 2);
    assert_eq!(prepare(&mut world).0.search().channel_class, ChannelClass::Off);
    classed(&mut world);
    assert_eq!(prepare(&mut world).0.search().channel_class, ChannelClass::Masked);
    world.property(stock::SECOND_CHANNEL_MASK, 0xff);
    world.bounds.insert(0, SearchBounds { low: -50, high: 100 });
    world.bounds.insert(1, SearchBounds { low: -60, high: 200 });
    let (state, _) = prepare(&mut world);
    assert_eq!(state.search().channel_class, ChannelClass::Wide);
    assert_eq!(state.bounds(), SearchBounds { low: -50, high: 200 });
    world.bounds.insert(1, SearchBounds { low: -60, high: 100 });
    let (state, _) = prepare(&mut world);
    assert_eq!(state.search().channel_class, ChannelClass::Narrow);
    assert_eq!(state.bounds(), SearchBounds { low: -50, high: 100 });
    class(&mut world, 1);
    assert_eq!(prepare(&mut world).0.search().channel_class, ChannelClass::Single);
    class(&mut world, 2);
    world.world.select(MeasurementSelection::Record);
    assert_eq!(prepare(&mut world).0.search().channel_class, ChannelClass::Wide);
}

#[test]
fn an_armed_scene_widens_the_run_only_when_its_bit_is_configured() {
    let mut world = MotionWorld::default();
    class(&mut world, 2);
    classed(&mut world);
    world.world.header(SharedHeaderByte::At63, 1);
    world.world.header(SharedHeaderByte::At31, 14);
    assert_eq!(prepare(&mut world).0.search().channel_class, ChannelClass::Armed);
    world.property(stock::ARMED_SCENE_CHANNELS, 0x20);
    assert_eq!(prepare(&mut world).0.search().channel_class, ChannelClass::Wide);
    world.world.header(SharedHeaderByte::At31, 200);
    assert_eq!(prepare(&mut world).0.search().channel_class, ChannelClass::Single);
}

#[test]
fn the_channel_status_and_frame_delay_follow_the_level_limit_and_the_enabled_channels() {
    let mut world = MotionWorld::default();
    class(&mut world, 2);
    assert_eq!(prepare(&mut world).1.frame_delay, 12);
    world.world.set(&group_00::AT_005F, 1);
    world.property(stock::CHANNEL_LEVEL_LIMIT, 5);
    world.property(stock::CHANNEL_STATUS_MASK, 1);
    world.property(stock::CHANNEL_STATUS_ENABLE, 1);
    let (state, preparation) = prepare(&mut world);
    assert!(state.channel_status());
    assert_eq!(preparation.frame_delay, 0);
    world.property(stock::CHANNEL_STATUS_ENABLE, 0);
    let (state, preparation) = prepare(&mut world);
    assert!(!state.channel_status());
    assert_eq!(preparation.frame_delay, 0);
    world.world.header(SharedHeaderByte::At46, 1);
    world.property(stock::CHANNEL_STATUS_MASK, 2);
    assert_eq!(prepare(&mut world).1.frame_delay, 3);
    world.world.header(SharedHeaderByte::At31, 28);
    assert_eq!(prepare(&mut world).1.frame_delay, 8);
    assert_eq!(prepare_with(&mut world, &mut Lists::default(), true).1.frame_delay, 12);
}

#[test]
fn the_level_ceiling_interpolates_the_measured_contrast_between_the_configured_bounds() {
    let mut world = MotionWorld::default();
    class(&mut world, 3);
    assert_eq!(prepare(&mut world).0.search().level_ceiling, 0);
    world.property(stock::LEVEL_CEILING_DEFAULT, 9);
    assert_eq!(prepare(&mut world).0.search().level_ceiling, 90);
    results_word(&mut world, 0x4, 1);
    world.property_word(stock::LEVEL_CEILING_BAND[0], 2000);
    world.property_word(stock::LEVEL_CEILING_BAND[1], 4000);
    world.property(stock::LEVEL_CEILING_BAND[2], 10);
    world.property(stock::LEVEL_CEILING_BAND[3], 30);
    let cell = 3 * 7 + 3;
    results_word(&mut world, 0x20 + 4 * 5 * cell, 1000);
    assert_eq!(prepare(&mut world).0.search().level_ceiling, 200);
    results_word(&mut world, 0x20 + 4 * 5 * cell, 100_000);
    assert_eq!(prepare(&mut world).0.search().level_ceiling, 300);
    results_word(&mut world, 0x20 + 4 * 5 * cell, 10);
    assert_eq!(prepare(&mut world).0.search().level_ceiling, 100);
}

#[test]
fn the_first_frame_changes_by_the_channel_period_above_two_and_a_half_octaves_up_to_the_limit() {
    let mut world = MotionWorld::default();
    world.channel_rates.insert((LENS, 0), 100);
    world.property_word(stock::CHANGE_FACTORS[0], 100);
    world.property_word(stock::CHANGE_FACTORS[1], 100);
    world.property(stock::CHANGE_LIMIT, 100);
    assert_eq!(prepare(&mut world).0.exposures().first.change, 414);
    world.property(stock::CHANGE_LIMIT, 30);
    assert_eq!(prepare(&mut world).0.exposures().first.change, 300);
    world.world.header(SharedHeaderByte::At46, 2);
    assert_eq!(prepare(&mut world).0.exposures().first.change, 0);
}

#[test]
fn the_second_frame_changes_toward_the_stored_second_channel_exposure() {
    let mut world = MotionWorld::default();
    world.world.set_shared(&group_27::AT_070C, 500);
    world.property(stock::SECOND_CHANGE_SCALE, 50);
    world.property_word(stock::SECOND_CHANGE_LIMIT, 1000);
    assert_eq!(prepare(&mut world).0.frame_exposure().change, 0);
    world.world.set_shared(&group_29::AT_079C, 1);
    let (state, _) = prepare(&mut world);
    assert_eq!(state.frame_exposure().change, 250);
    assert_eq!(section_word(&world, 0x44b20), 250);
    world.property_word(stock::SECOND_CHANGE_LIMIT, 100);
    assert_eq!(prepare(&mut world).0.frame_exposure().change, 100);
}

#[test]
fn the_second_exposure_is_traced_word_by_word() {
    let mut world = MotionWorld::default();
    world.line_periods.insert(stock::SEARCH_PROFILE, 33);
    world.default_time = 4000;
    world.property(stock::FIXED_EXPOSURE, 1);
    let (state, _) = prepare(&mut world);
    let exposure: FrameExposure = *state.frame_exposure();
    let traced: Vec<u32> = (0..15).map(|word| section_word(&world, 0x44b04 + 4 * word)).collect();
    assert_eq!(traced[1], exposure.lens_step);
    assert_eq!(traced[5], 0);
    assert_eq!(traced[13], 33);
    assert_eq!(traced[14], 4000);
    assert_eq!(state.frame_period(), 4000);
}

#[test]
fn a_run_searches_in_full_unless_configured_or_already_focused() {
    let mut world = MotionWorld::default();
    assert_eq!(prepare(&mut world).1.flags(), 3);
    world.property(stock::NO_FULL_SEARCH, 1);
    assert_eq!(prepare(&mut world).1.flags(), 2);
}

#[test]
fn the_statistics_double_their_weights_for_a_high_feature_class_and_keep_the_channel_scales() {
    let mut world = MotionWorld { channel_weights: [10, 20, 30], feature_bytes: [100, 150], ..MotionWorld::default() };
    world.selected_area = GridArea { column: 2, row: 4 };
    class(&mut world, 5);
    let (state, _) = prepare(&mut world);
    let statistics = state.statistics();
    assert_eq!(statistics.channel_weights, [10, 20, 30]);
    assert_eq!(statistics.channel_scales, [100, 128, 150]);
    assert_eq!((statistics.class, statistics.area), (5, GridArea { column: 2, row: 4 }));
    assert_eq!(statistics.trace, SharedAddress(stock::section(FRAME).0 + 0x46bc8));
    world.feature_classes.insert(stock::SEARCH_PROFILE, 9);
    assert_eq!(prepare(&mut world).0.statistics().channel_weights, [10, 40, 60]);
}

#[test]
fn the_search_frames_take_the_first_listed_area_with_its_class() {
    let mut world = MotionWorld::default();
    unscaled(&mut world);
    world.search_frames = [AreaGrid { cell_height: 5, ..AreaGrid::default() }, AreaGrid { columns: 3, ..AreaGrid::default() }];
    let area = ListedArea { x: 1, y: 2, at_1c: 6, ..ListedArea::default() };
    let mut areas = AreaList { count: 1, ..AreaList::default() };
    areas.areas[0] = area;
    let mut lists = Lists { areas, ..Lists::default() };
    let (state, _) = prepare_with(&mut world, &mut lists, false);
    assert_eq!(state.search().frames, world.search_frames);
    assert_eq!(world.search_frame_requests.last().unwrap().area, Some(Tracking { area, class: AreaClass::RowsUpward }));
}

#[test]
fn the_preparation_traces_its_configuration_and_ten_area_records() {
    let mut world = MotionWorld::default();
    unscaled(&mut world);
    world.property(Property(0x9232), 7);
    world.property(Property(0x92bc), 2);
    world.property(Property(0x961c), 0x5a);
    world.property_word(Property(0xa493), 0x1234);
    let mut lists = treated(&mut world);
    lists.area_positions = [30; 10];
    let mut areas = AreaList { count: 2, ..AreaList::default() };
    areas.areas[1] = ListedArea { x: 11, y: 12, width: 13, height: 14, centre_x: 19, centre_y: 20, at_18: 15, at_1c: 16, at_20: 17, at_22: 18, ..ListedArea::default() };
    lists.areas = areas;
    prepare_with(&mut world, &mut lists, false);
    assert_eq!(section_word(&world, 0x44c38), 7);
    assert_eq!(section_word(&world, 0x468e8), 3);
    assert_eq!(section_word(&world, 0x44c18), 0x5_000a);
    assert_eq!(section_word(&world, 0x46998), 0x1234);
    assert_eq!(section_word(&world, 0x46a0c), 2);
    let record: Vec<u32> = (0..11).map(|word| section_word(&world, 0x46a10 + 0x2c + 4 * word)).collect();
    assert_eq!(record, [30, 20, 19, 18, 12, 11, 14, 13, 17, 15, 16]);
}
