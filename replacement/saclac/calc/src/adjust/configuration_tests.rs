use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::{group_14, group_26, group_27};
use xm1_saclac_profile_records::{Bytes, ProfileRecords};

use crate::adjust::configuration::*;
use crate::exposure::fakes::{handle, World};
use crate::exposure::ports::Property;
use crate::stock::adjust as stock;

impl ProfileStore for World {
    fn records(&mut self) -> &mut ProfileRecords<Bytes> { &mut self.records }
}

#[derive(Default)]
struct Pipeline {
    intervals: Vec<(u32, u32)>,
}

impl FrameInterval for Pipeline {
    fn frame_interval(&mut self, timing: u32) -> Option<u32> { self.intervals.iter().find(|(word, _)| *word == timing).map(|(_, interval)| *interval) }
}

#[derive(Default)]
struct Library {
    calls: Vec<&'static str>,
}

impl LibraryDiagnostics for Library {
    fn publish_selection(&mut self, _record: RecordHandle) { self.calls.push("publish"); }
    fn snapshot_diagnostics(&mut self) { self.calls.push("snapshot"); }
    fn reset_selection(&mut self) { self.calls.push("reset selection"); }
    fn reset_diagnostics(&mut self) { self.calls.push("reset diagnostics"); }
}

fn motion(world: &mut World) -> MotionRecord { MotionRecord::decode(world.profile_bytes()) }

trait MotionBytes {
    fn profile_bytes(&mut self) -> [u8; 40];
    fn store_motion(&mut self, motion: &MotionRecord);
}

impl MotionBytes for World {
    fn profile_bytes(&mut self) -> [u8; 40] { self.records.read_shared(handle(), &group_27::AT_07EC).unwrap() }
    fn store_motion(&mut self, motion: &MotionRecord) { self.set_shared(&group_27::AT_07EC, motion.encode()) }
}

fn publish(world: &mut World, kind: u32, speed: u16, area: u32, exposure: u32) {
    let mut record = motion(world);
    record.published_kind = kind;
    record.published_speed = speed;
    record.published_area = area;
    record.published_exposure = exposure;
    world.store_motion(&record);
}

/// Ten cells of the third row changed by 50, the rest unchanged.
fn moved_row() -> GridPair {
    let mut grids = GridPair { current: [100; GRID_CELLS], previous: [100; GRID_CELLS] };
    for cell in 32..42 {
        grids.current[cell] = 150;
    }
    grids
}

fn live_world() -> World {
    let mut world = World::default();
    world.set(&group_00::AT_0000, 1 | 1 << 16);
    world.property(stock::motion_threshold(MotionSource::Live), 40);
    world.property(stock::motion_minimum_cells(MotionSource::Live), 4);
    let mut list = IntervalList::decode([0; 48]);
    list.intervals[0] = 20_000;
    list.intervals[1] = 20_000;
    list.count = 2;
    world.set_shared(&group_14::AT_0864, list.encode());
    world
}

fn measure(configuration: &mut Configuration, world: &mut World, grids: &GridPair, source: MotionSource, previous: &mut Option<Centroid>) {
    configuration.measure_motion(world, &mut Pipeline::default(), handle(), grids, source, previous);
}

#[test]
fn a_history_keeps_the_latest_twenty_most_recent_first() {
    let mut history = History::empty();
    for value in 0..25u16 {
        history.record(value);
    }
    let expected: [Option<u16>; HISTORY] = core::array::from_fn(|index| Some(24 - index as u16));
    assert_eq!(history.entries(), &expected);
}

#[test]
fn a_reset_empties_both_histories_and_resets_the_library() {
    let mut configuration = Configuration::default();
    assert_eq!(configuration.offsets().entries(), &[Some(0); HISTORY]);
    let mut library = Library::default();
    configuration.reset_histories(&mut library);
    assert_eq!(configuration.offsets().entries(), &[None; HISTORY]);
    assert_eq!(configuration.classes().entries(), &[None; HISTORY]);
    assert_eq!(library.calls, ["reset selection", "reset diagnostics"]);
}

#[test]
fn changed_cells_give_their_centre_and_store_the_find() {
    let mut world = live_world();
    let mut configuration = Configuration::default();
    let mut previous = None;
    measure(&mut configuration, &mut world, &moved_row(), MotionSource::Live, &mut previous);
    let centre = Centroid { x: (45 * 20 + 5) / 10, y: (20 * 20 + 5) / 10 };
    assert_eq!(previous, Some(centre));
    let measurement = configuration.measurement();
    assert_eq!((measurement.cells, measurement.found, measurement.centroid, measurement.previous), (10, true, Some(centre), None));
    assert_eq!(measurement.rate, 25);
    assert_eq!((motion(&mut world).found, motion(&mut world).speed), (1, 0));
}

#[test]
fn the_distance_from_the_previous_centre_per_period_is_the_speed() {
    let mut world = live_world();
    let mut configuration = Configuration::default();
    let mut previous = Some(Centroid { x: 90 - 30, y: 40 - 40 });
    measure(&mut configuration, &mut world, &moved_row(), MotionSource::Live, &mut previous);
    assert_eq!(configuration.measurement().speed, ((25 * 50 + 15) / 30) as u16);
    assert_eq!(motion(&mut world).speed, configuration.measurement().speed);
}

#[test]
fn too_few_changed_cells_find_nothing_and_forget_the_centre() {
    let mut world = live_world();
    world.property(stock::motion_minimum_cells(MotionSource::Live), 11);
    let mut configuration = Configuration::default();
    let mut previous = Some(Centroid { x: 1, y: 1 });
    measure(&mut configuration, &mut world, &moved_row(), MotionSource::Live, &mut previous);
    assert_eq!(previous, None);
    assert_eq!((configuration.measurement().found, configuration.measurement().speed), (false, 0));
    assert_eq!(motion(&mut world).found, 0);
}

#[test]
fn the_row_is_scaled_by_the_record_aspect() {
    let mut world = live_world();
    world.set(&group_00::AT_0000, 3 | 2 << 16);
    let mut configuration = Configuration::default();
    let mut previous = None;
    measure(&mut configuration, &mut world, &moved_row(), MotionSource::Live, &mut previous);
    assert_eq!(previous.map(|centre| centre.y), Some((2 * 10 * 3 * 20 + 10) / 20));
}

#[test]
fn the_half_press_takes_its_period_from_the_pipeline() {
    let mut world = live_world();
    world.set_shared(&group_26::AT_0230, 7);
    world.property(stock::motion_threshold(MotionSource::HalfPress), 40);
    let mut configuration = Configuration::default();
    let mut pipeline = Pipeline { intervals: [(7, 4000)].into() };
    configuration.measure_motion(&mut world, &mut pipeline, handle(), &moved_row(), MotionSource::HalfPress, &mut None);
    assert_eq!(configuration.measurement().rate, 250);
    configuration.measure_motion(&mut world, &mut Pipeline::default(), handle(), &moved_row(), MotionSource::HalfPress, &mut None);
    assert_eq!(configuration.measurement().rate, 0);
}

#[test]
fn a_live_interval_count_above_ten_sums_the_ten_intervals() {
    let mut world = live_world();
    let mut list = IntervalList::decode([0; 48]);
    list.intervals = [100_000; INTERVALS];
    list.count = 12;
    world.set_shared(&group_14::AT_0864, list.encode());
    let mut configuration = Configuration::default();
    measure(&mut configuration, &mut world, &moved_row(), MotionSource::Live, &mut None);
    assert_eq!(configuration.measurement().rate, 1);
}

fn classify(configuration: &mut Configuration, world: &mut World, source: MotionSource) -> Library {
    let mut library = Library::default();
    configuration.classify_motion(world, &mut library, handle(), source);
    library
}

#[test]
fn measuring_alone_classes_by_the_find_and_records_the_measured_speed() {
    let mut world = live_world();
    world.property(stock::motion_enable(MotionSource::Live), 2);
    world.property(stock::dominant_window(MotionSource::Live), 1);
    world.property(stock::average_window(MotionSource::Live), 1);
    world.property(stock::DOMINANT_PREFERENCE[0], 1);
    let mut configuration = Configuration::default();
    let mut previous = Some(Centroid { x: 60, y: 0 });
    measure(&mut configuration, &mut world, &moved_row(), MotionSource::Live, &mut previous);
    let library = classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!(library.calls, ["publish"]);
    let level = ((u32::from(configuration.measurement().speed) * 1000 + 160) / 320) as u16;
    assert_eq!((configuration.class(), configuration.dominant(), configuration.offset(), configuration.average()), (MotionClass::One, MotionClass::One, level, level));
    let stored = motion(&mut world);
    assert_eq!((stored.dominant, stored.average, stored.average_scale), (1, level, 30));
}

#[test]
fn classifying_by_kind_alone_takes_the_published_kind_and_level() {
    let mut world = live_world();
    world.property(stock::motion_enable(MotionSource::Live), 1);
    publish(&mut world, 2, 512, 0, 0);
    let mut configuration = Configuration::default();
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!((configuration.class(), configuration.offset()), (MotionClass::Two, 200));
    publish(&mut world, 9, 512, 0, 0);
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!(configuration.class(), MotionClass::Undetermined);
}

#[test]
fn with_both_off_the_class_is_undetermined_and_the_offset_zero() {
    let mut world = live_world();
    let mut configuration = Configuration::default();
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!((configuration.class(), configuration.offset()), (MotionClass::Undetermined, 0));
    assert_eq!(configuration.classes().entries()[0], Some(MotionClass::Undetermined));
}

/// Measured motion of 10 cells and speed 42 with published kind 0, area 600 and no exposure, under
/// moving rules every value lies inside.
fn moving_world(configuration: &mut Configuration) -> World {
    let mut world = live_world();
    world.property(stock::motion_enable(MotionSource::Live), 3);
    let mut previous = Some(Centroid { x: 60, y: 0 });
    measure(configuration, &mut world, &moved_row(), MotionSource::Live, &mut previous);
    assert_eq!(configuration.measurement().speed, 42);
    publish(&mut world, 0, 0, 600, 0);
    let mut record = motion(&mut world);
    record.found = 1;
    world.store_motion(&record);
    for (id, value) in [
        (stock::area_factors(MotionSource::Live)[0], 2),
        (stock::area_factors(MotionSource::Live)[1], 3),
        (stock::MOVING_RULES, 1),
        (stock::MOVING_STILL_EXPOSURE.area[0], 100),
        (stock::MOVING_STILL_EXPOSURE.area[1], 101),
        (stock::MOVING_STILL_EXPOSURE.cells[0], 10),
        (stock::MOVING_STILL_EXPOSURE.cells[1], 11),
        (stock::MOVING_STILL_EXPOSURE.speed[0], 42),
        (stock::MOVING_STILL_EXPOSURE.speed[1], 43),
        (stock::configured_class(1, 0), 2),
    ] {
        world.property(id, value);
    }
    world
}

#[test]
fn a_subject_inside_every_moving_bound_is_class_one() {
    let mut configuration = Configuration::default();
    let mut world = moving_world(&mut configuration);
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!(configuration.class(), MotionClass::One);
}

#[test]
fn a_subject_outside_a_moving_bound_takes_its_configured_class() {
    let outside: [(Property, u32); 4] = [
        (stock::MOVING_STILL_EXPOSURE.area[1], 100),
        (stock::MOVING_STILL_EXPOSURE.cells[0], 11),
        (stock::MOVING_STILL_EXPOSURE.speed[1], 42),
        (stock::MOVING_RULES, 0),
    ];
    for (id, value) in outside {
        let mut configuration = Configuration::default();
        let mut world = moving_world(&mut configuration);
        world.property(id, value);
        classify(&mut configuration, &mut world, MotionSource::Live);
        assert_eq!(configuration.class(), MotionClass::Two, "{id:?}");
    }
}

#[test]
fn an_exposed_subject_is_judged_by_the_second_bounds() {
    let mut configuration = Configuration::default();
    let mut world = moving_world(&mut configuration);
    publish(&mut world, 0, 0, 600, 5);
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!(configuration.class(), MotionClass::Two);
}

#[test]
fn the_published_level_is_recorded_from_the_limit_up() {
    let mut configuration = Configuration::default();
    let mut world = moving_world(&mut configuration);
    publish(&mut world, 0, 1024, 600, 0);
    world.property(stock::offset_limit(MotionSource::Live), 400);
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!(configuration.offset(), 400);
    world.property(stock::offset_limit(MotionSource::Live), 401);
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!(configuration.offset(), ((42 * 1000 + 160) / 320) as u16);
}

#[test]
fn the_dominant_class_needs_a_full_window_and_ties_go_to_the_preference() {
    let mut world = live_world();
    world.property(stock::motion_enable(MotionSource::Live), 1);
    world.property(stock::dominant_window(MotionSource::Live), 4);
    for (index, preference) in [7, 2, 1, 0].into_iter().enumerate() {
        world.property(stock::DOMINANT_PREFERENCE[index], preference);
    }
    let mut configuration = Configuration::default();
    configuration.reset_histories(&mut Library::default());
    let mut dominants = Vec::new();
    for kind in [1, 2, 1, 2, 0] {
        publish(&mut world, kind, 0, 0, 0);
        classify(&mut configuration, &mut world, MotionSource::Live);
        dominants.push(configuration.dominant());
    }
    assert_eq!(dominants, [MotionClass::Undetermined, MotionClass::Undetermined, MotionClass::Undetermined, MotionClass::Two, MotionClass::Two]);
    world.property(stock::DOMINANT_PREFERENCE[1], 9);
    publish(&mut world, 1, 0, 0, 0);
    classify(&mut configuration, &mut world, MotionSource::Live);
    assert_eq!(configuration.dominant(), MotionClass::One);
}

#[test]
fn the_offset_average_needs_a_full_window() {
    let mut world = live_world();
    world.property(stock::motion_enable(MotionSource::Live), 1);
    world.property(stock::average_window(MotionSource::Live), 2);
    let mut configuration = Configuration::default();
    configuration.reset_histories(&mut Library::default());
    let mut averages = Vec::new();
    for speed in [256, 1024, 2048] {
        publish(&mut world, 0, speed, 0, 0);
        classify(&mut configuration, &mut world, MotionSource::Live);
        averages.push(configuration.average());
    }
    assert_eq!(averages, [0, (100 + 400) / 2, (400 + 800) / 2]);
    assert_eq!(motion(&mut world).average, 600);
}

#[test]
fn the_diagnostics_store_every_value_in_order() {
    let mut world = live_world();
    world.property(stock::motion_enable(MotionSource::Live), 2);
    let mut configuration = Configuration::default();
    configuration.reset_histories(&mut Library::default());
    let mut grids = moved_row();
    grids.previous[0] = -2;
    measure(&mut configuration, &mut world, &grids, MotionSource::Live, &mut None);
    classify(&mut configuration, &mut world, MotionSource::Live);
    world.stores.clear();
    let mut library = Library::default();
    configuration.export_diagnostics(&mut world, &mut library, &grids);
    assert_eq!(library.calls, ["snapshot"]);
    assert_eq!(world.stores.len(), 2 * GRID_CELLS + 9 + HISTORY + 2 + HISTORY + 1);
    let word = |world: &World, at: u32| world.u32_at(SharedAddress(at));
    assert_eq!(world.stores[0], (0x5ffe_6f84, 4, 100));
    assert_eq!(world.stores[1], (0x5ffe_7384, 4, u32::MAX - 1));
    assert_eq!(word(&world, 0x5ffe_7784), 11);
    assert_eq!(word(&world, 0x5ffe_7794), u32::MAX);
    assert_eq!(word(&world, 0x5ffe_77a4), 1);
    assert_eq!((word(&world, 0x5ffe_77a8), word(&world, 0x5ffe_77ac)), (1, 100));
    assert_eq!((word(&world, 0x5ffe_7800), word(&world, 0x5ffe_7804)), (0, 0xffff));
    assert_eq!(world.stores.last(), Some(&(0x5ffe_7850, 4, 0)));
}

#[test]
fn clearing_the_interval_list_keeps_its_tail() {
    let mut world = live_world();
    let mut bytes = [7; 48];
    bytes[0x28..0x2a].copy_from_slice(&3u16.to_le_bytes());
    world.set_shared(&group_14::AT_0864, bytes);
    clear_intervals(&mut world, handle());
    let cleared: [u8; 48] = world.records.read_shared(handle(), &group_14::AT_0864).unwrap();
    assert_eq!(&cleared[..0x2a], &[0; 0x2a]);
    assert_eq!(&cleared[0x2a..], &[7; 6]);
}

#[test]
fn the_shared_words_are_read_where_other_tasks_store_them() {
    let mut world = World::default();
    world.put_u16(stock::SHARED_FEATURE, 1);
    assert!(!shared::feature_enabled(&mut world));
    world.property(stock::FEATURE_SETTING, 1);
    assert!(shared::feature_enabled(&mut world));
    world.put_u16(stock::SHARED_MODE, 1);
    assert!(shared::mode_is_one(&mut world));
    shared::store_request(&mut world, 0);
    assert_eq!(world.stores, [(stock::SHARED_REQUEST.0, 4, 0)]);
}

#[test]
fn a_refresh_request_is_kept_unless_suppressed() {
    let mut world = World::default();
    let mut configuration = Configuration::default();
    configuration.request_refresh(&mut world, true);
    assert!(configuration.refresh());
    world.property(stock::REFRESH_SUPPRESSED, 1);
    configuration.request_refresh(&mut world, true);
    assert!(!configuration.refresh());
}
