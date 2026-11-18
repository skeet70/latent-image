use std::collections::BTreeMap;

use crate::adjust::configuration::Configuration;
use crate::adjust::fakes::Lens;
use crate::adjust::lens::{BiasTable, LensConfiguration, LensControl, LensSteps, ReferenceScale, Step, StepRequest};
use crate::adjust::motion::drive::{DriveKind, DriveLimits, DriveMode};
use crate::adjust::scan_drive::*;
use crate::adjust::timing::{CaptureTiming, LevelScale};
use crate::exposure::fakes::{handle, World};
use crate::exposure::ports::{ProfileAccess, Properties, Property};
use crate::profile::Profile;

const LEVEL: i16 = 500;
const SPAN: i32 = 100_000;

/// Drives without ramps at the lens table's rate of 1000 steps a second; the midpoint correction
/// is unscaled and unbiased; a 100 ms exposure at `LEVEL`, six lines from the reference line to the
/// readout start.
struct Bench {
    world: World,
    lens: Lens,
    line_period: Option<u32>,
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl ProfileAccess for Bench {
    type Records = <World as ProfileAccess>::Records;
    type Entries = <World as ProfileAccess>::Entries;
    fn profile(&mut self) -> Profile<'_, Self::Records, Self::Entries> { self.world.profile() }
}

impl LensSteps for Bench {
    fn step(&mut self, request: StepRequest) -> Step { Step { size: request.base, rate: 1000 } }
    fn travel_distance(&mut self, _resource: u32, _index: u32, travel: i16) -> u16 { travel as u16 }
}

impl ReferenceScale for Bench {
    fn reference_scale(&mut self, group: u16, entry: u8) -> u32 { self.lens.reference_scale(group, entry) }
}

impl LensControl for Bench {
    fn position(&mut self) -> u16 { self.lens.position }
    fn travel_margin(&mut self) -> u16 { self.lens.margin }
}

impl LensConfiguration for Bench {
    fn channel_value(&mut self, channel: i32) -> u32 { self.lens.channel_value(channel) }
    fn channel_curve(&mut self, channel: i32) -> u32 { self.lens.channel_curve(channel) }
    fn bias(&mut self, resource: i32, table: BiasTable) -> i16 { self.lens.bias(resource, table) }
    fn biased_position(&mut self, resource: i32, table: BiasTable) -> i16 { self.lens.biased_position(resource, table) }
    fn resource_count(&mut self) -> u8 { self.lens.resource_count() }
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32 { self.lens.selection_word(channel, flags, offset) }
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32 { self.lens.remap(word, channel, mode) }
    fn position_limits(&mut self, key: u32) -> [i16; 2] { self.lens.position_limits(key) }
    fn mapped_value(&mut self, key: u32) -> u32 { self.lens.mapped_value(key) }
}

impl CaptureTiming for Bench {
    fn readout_start(&mut self, _source: u32) -> u32 { 4 }
    fn reference_line(&mut self, _source: u32) -> u16 { 10 }
    fn line_period(&mut self, _source: u32) -> Option<u32> { self.line_period }
}

impl LevelScale for Bench {
    fn linear(&mut self, level: i16) -> u32 { BTreeMap::from([(LEVEL, 1000)]).get(&level).copied().unwrap_or(0) }
    fn log2_difference(&mut self, _value: u32, _reference: u32) -> i16 { 0 }
}

impl DriveLimits for Bench {
    fn first_ramp_speed(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 { 0 }
    fn first_ramp_acceleration(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 { 0 }
    fn second_ramp_acceleration(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 { 0 }
    fn slowest_ramped_speed(&mut self) -> u32 { 0 }
}

/// 100 µs lines, a 40 ms frame interval (scale entry 50 × 50), the lens at 0 within -2000..=2000,
/// and a 5 ms drive delay.
fn bench() -> Bench {
    let mut world = World::default();
    world.property(Property(0x9276), 100);
    world.property(Property(0x9281), 50);
    Bench { world, lens: Lens { limits: [-2000, 2000], ..Lens::default() }, line_period: Some(100) }
}

/// Exposed for 100 lines from line 50, 20 lines apart: areas centred on lines 106, 226, 346 and
/// 466 after the lag.
const AREAS: AreaTiming = AreaTiming { exposure: 100, start: 50, gap: 20, rows: 0, count: 4 };

fn offset(line: i32, period: i32) -> i32 { line * period - SPAN }

/// The drive's position `time` after it starts, at one step a millisecond from 0.
fn at(time: i32) -> i16 { (time / 1000) as i16 }

/// The position halfway through an exposure from `start`.
fn midpoint(start: i32) -> i16 { (at(start) + at(start + SPAN)) / 2 }

fn query(limits: [i16; 2], toward_second: bool) -> ScanQuery {
    let frame = ScanFrame { index: 1, level: LEVEL, resource: 2, scale_group: 50, scale_entry: 50 };
    ScanQuery { frame, monitor: None, contrast: 0, drive_kind: DriveKind(3), toward_second, limits }
}

fn planned(bench: &mut Bench, areas: SampleAreas, limits: [i16; 2]) -> (ScanDrive, ScanPlan) {
    let mut drive = ScanDrive::default();
    drive.set_timing(SampleTiming { level: 0, source: 7, areas });
    let plan = drive.plan_scan(bench, &Configuration::default(), handle(), query(limits, true));
    (drive, plan)
}

#[test]
fn the_travel_runs_to_halfway_through_the_latest_area_and_widens_the_limits_and_the_drive() {
    let mut bench = bench();
    bench.line_period = Some(1000);
    let (_, plan) = planned(&mut bench, SampleAreas::Primary(AREAS), [0, 600]);
    let latest = offset(466, 1000);
    let travel = (latest / 1000 + (latest + SPAN) / 1000) / 2;
    assert_eq!(plan.travel, travel as u16);
    let reach = travel as i16;
    assert_eq!(plan.limits, [-reach, 600 + reach]);
    assert_eq!((plan.window, plan.reverse), ([0, 600 + 2 * reach], false));
}

#[test]
fn a_travel_ending_before_the_frame_starts_or_without_areas_is_zero() {
    let mut bench = bench();
    assert_eq!(planned(&mut bench, SampleAreas::Primary(AREAS), [0, 600]).1.travel, 0);
    bench.line_period = Some(1000);
    assert_eq!(planned(&mut bench, SampleAreas::Primary(AreaTiming { count: 0, ..AREAS }), [0, 600]).1.travel, 0);
}

#[test]
fn the_window_is_clamped_with_the_margin_on_its_starting_side() {
    let mut bench = bench();
    bench.lens = Lens { limits: [20, 500], margin: 10, position: 0, ..Lens::default() };
    let (_, plan) = planned(&mut bench, SampleAreas::Primary(AREAS), [0, 600]);
    assert_eq!(plan.window, [30, 500]);
    bench.lens.position = 300;
    let mut drive = ScanDrive::default();
    let plan = drive.plan_scan(&mut bench, &Configuration::default(), handle(), query([0, 600], false));
    assert_eq!((plan.window, plan.reverse), ([300, 20], true));
}

#[test]
fn a_frame_inside_the_drive_reads_the_centre_primary_areas_target() {
    let mut bench = bench();
    let (drive, _) = planned(&mut bench, SampleAreas::Primary(AREAS), [0, 600]);
    let time = 8 * 40_000 - 5000;
    let corrected = drive.corrected_position(&mut bench, 8);
    assert_eq!(corrected, CorrectedPosition { target: midpoint(time + offset(466, 100)), position: at(time) });
}

#[test]
fn the_secondary_layout_reads_its_first_areas_target() {
    let mut bench = bench();
    let areas = SampleAreas::from_layout(1, AreaTiming::default(), AreaTiming { rows: 4, count: 2, ..AREAS });
    let (drive, _) = planned(&mut bench, areas, [0, 600]);
    let time = 8 * 40_000 - 5000;
    assert_eq!(drive.corrected_position(&mut bench, 8).target, midpoint(time + offset(106, 100)));
    assert_eq!(SampleAreas::from_layout(2, AREAS, AreaTiming::default()), SampleAreas::Primary(AREAS));
}

#[test]
fn a_frame_count_past_a_byte_keeps_the_drive_moving() {
    let mut bench = bench();
    bench.lens.limits = [-30_000, 30_000];
    let (drive, _) = planned(&mut bench, SampleAreas::Primary(AREAS), [0, 20_000]);
    assert_eq!(drive.corrected_position(&mut bench, 300).position, at(300 * 40_000 - 5000));
}

#[test]
fn without_a_line_period_the_areas_are_exposed_at_the_frame_start() {
    let mut bench = bench();
    bench.line_period = None;
    let (drive, _) = planned(&mut bench, SampleAreas::Primary(AREAS), [0, 600]);
    let time = 8 * 40_000 - 5000;
    assert_eq!(drive.corrected_position(&mut bench, 8).target, midpoint(time - SPAN));
}

#[test]
fn area_counts_beyond_the_slots_are_sampled_up_to_the_slots() {
    let mut bench = bench();
    let (drive, _) = planned(&mut bench, SampleAreas::Secondary(AreaTiming { count: 12, rows: 12, ..AREAS }), [0, 600]);
    assert_eq!(drive.corrected_position(&mut bench, 8).target, midpoint(8 * 40_000 - 5000 + offset(106, 100)));
}
