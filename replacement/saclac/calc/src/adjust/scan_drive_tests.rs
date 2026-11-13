use std::collections::BTreeMap;
use std::vec::Vec;

use crate::adjust::configuration::{Configuration, ControllerFlag};
use crate::adjust::motion::drive::{DriveKind, DriveLimits, DriveMode};
use crate::adjust::motion::sampling::SampleTargets;
use crate::adjust::scan_drive::*;
use crate::adjust::timing::{CaptureTiming, LevelScale};
use crate::exposure::ports::{Properties, Property};

const KIND: DriveKind = DriveKind(5);
const LEVEL: i16 = 500;
const SOURCE: u32 = 9;

/// Drives without ramps, so the lens moves at the requested speed throughout; the midpoint
/// correction is unscaled and unbiased.
struct Bench {
    properties: BTreeMap<u32, u32>,
    linear: BTreeMap<i16, u32>,
    reference_line: u16,
    readout_start: u32,
    line_period: Option<u32>,
    limits: Vec<(DriveKind, DriveMode)>,
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { self.properties.get(&property.0).copied().unwrap_or(0) }
}

impl CaptureTiming for Bench {
    fn readout_start(&mut self, source: u32) -> u32 {
        assert_eq!(source, SOURCE);
        self.readout_start
    }
    fn reference_line(&mut self, source: u32) -> u16 {
        assert_eq!(source, SOURCE);
        self.reference_line
    }
    fn line_period(&mut self, source: u32) -> Option<u32> {
        assert_eq!(source, SOURCE);
        self.line_period
    }
}

impl LevelScale for Bench {
    fn linear(&mut self, level: i16) -> u32 { self.linear.get(&level).copied().unwrap_or(0) }
    fn log2_difference(&mut self, _value: u32, _reference: u32) -> i16 { 0 }
}

impl DriveLimits for Bench {
    fn first_ramp_speed(&mut self, kind: DriveKind, mode: DriveMode) -> u32 {
        self.limits.push((kind, mode));
        0
    }
    fn first_ramp_acceleration(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 { 0 }
    fn second_ramp_acceleration(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 { 0 }
    fn slowest_ramped_speed(&mut self) -> u32 { 0 }
}

/// A 100 ms exposure at `LEVEL`, six lines from the reference line to the readout start and
/// 100 µs lines.
fn bench() -> Bench {
    Bench {
        properties: [(0x9276, 100)].into(),
        linear: [(LEVEL, 1000), (LEVEL + 100, 2000)].into(),
        reference_line: 10,
        readout_start: 4,
        line_period: Some(100),
        limits: Vec::new(),
    }
}

/// Exposed for 100 lines from line 50, 20 lines apart: areas centred on lines 106, 226 and 346
/// after the lag.
const AREAS: AreaTiming = AreaTiming { exposure: 100, start: 50, gap: 20, rows: 0, count: 3 };

const SPAN: i32 = 100_000;

/// Each area's exposure start relative to its frame's.
fn offsets(period: i32) -> [i32; 3] { [106, 226, 346].map(|line| line * period - SPAN) }

/// From 0 to 1000 at 1000 steps a second, starting 5 ms into the scan.
fn planned(bench: &mut Bench, areas: SampleAreas) -> ScanDrive {
    let mut drive = ScanDrive::default();
    drive.set_timing(SampleTiming { level: LEVEL, source: SOURCE, areas });
    drive.plan(bench, &Configuration::default(), ScanLeg { start: 0, end: 1000, reverse: false, start_time: 5000 }, 1000, KIND);
    drive
}

/// The drive's position `time` after it starts.
fn at(time: i32) -> i16 { (time / 1000) as i16 }

/// The position halfway through an exposure from `start`.
fn midpoint(start: i32) -> i16 { (at(start) + at(start + SPAN)) / 2 }

#[test]
fn the_drive_runs_along_the_leg_and_reads_its_limits_for_the_search_mode() {
    let mut bench = bench();
    let drive = planned(&mut bench, SampleAreas::Primary(AREAS));
    assert_eq!(bench.limits, [(KIND, DriveMode { direction: 0, step: 9 })]);
    assert_eq!(drive.position_at_frame(3, 40_000), at(3 * 40_000 - 5000));
    assert_eq!(drive.position_at_frame(0, 40_000), 0);
    assert_eq!(drive.position_at_frame(100, 40_000), 1000);
    let mut configuration = Configuration::default();
    configuration.set_controller_flag(ControllerFlag::First, true);
    let mut drive = drive;
    drive.plan(&mut bench, &configuration, ScanLeg { start: 1000, end: 0, reverse: true, start_time: 0 }, 1000, KIND);
    assert_eq!(bench.limits[1], (KIND, DriveMode { direction: 1, step: 10 }));
    assert_eq!(drive.position_at_frame(1, 250_000), 750);
}

#[test]
fn a_frame_inside_the_drive_targets_each_primary_area_at_the_drives_midpoint_over_its_exposure() {
    let mut bench = bench();
    let drive = planned(&mut bench, SampleAreas::Primary(AREAS));
    let targets = drive.sample_targets(&mut bench, 300, 99, 40_000);
    let time = 300_000;
    let mut expected = SampleTargets::default();
    for (slot, offset) in expected.primary.iter_mut().zip(offsets(100)) {
        *slot = midpoint(time + offset);
    }
    assert_eq!(targets, expected);
}

#[test]
fn a_frame_at_the_drives_start_is_dated_by_its_frame_count() {
    let mut bench = bench();
    let drive = planned(&mut bench, SampleAreas::Primary(AREAS));
    let targets = drive.sample_targets(&mut bench, 0, 3, 40_000);
    let time = 3 * 40_000 - 5000;
    assert_eq!(targets.primary[..3], offsets(100).map(|offset| midpoint(time + offset)));
}

#[test]
fn the_secondary_layout_targets_its_areas_and_fills_its_remaining_slots_with_the_first() {
    let mut bench = bench();
    let areas = AreaTiming { rows: 4, count: 2, ..AREAS };
    let drive = planned(&mut bench, SampleAreas::from_layout(1, AreaTiming::default(), areas));
    let targets = drive.sample_targets(&mut bench, 300, 0, 0);
    let [first, second, _] = offsets(100).map(|offset| midpoint(300_000 + offset));
    assert_eq!(targets.primary, [0; 7]);
    assert_eq!(targets.secondary, [first, second, first, first, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn any_other_layout_word_samples_the_primary_areas() {
    for layout in [0, 2] {
        assert_eq!(SampleAreas::from_layout(layout, AREAS, AreaTiming::default()), SampleAreas::Primary(AREAS));
    }
}

#[test]
fn area_counts_beyond_the_slots_are_sampled_up_to_the_slots() {
    let mut bench = bench();
    let drive = planned(&mut bench, SampleAreas::Primary(AreaTiming { count: 9, ..AREAS }));
    let targets = drive.sample_targets(&mut bench, 300, 0, 0);
    assert!(targets.primary.iter().all(|&target| target != 0));
    let drive = planned(&mut bench, SampleAreas::Secondary(AreaTiming { count: 12, rows: 12, ..AREAS }));
    let targets = drive.sample_targets(&mut bench, 300, 0, 0);
    assert!(targets.secondary.iter().all(|&target| target != 0));
}

#[test]
fn the_travel_runs_to_halfway_through_the_latest_exposed_area_and_takes_the_new_level() {
    let mut bench = bench();
    bench.line_period = Some(1000);
    let mut drive = planned(&mut bench, SampleAreas::Primary(AREAS));
    let latest = offsets(1000)[2];
    assert_eq!(drive.exposure_travel(&mut bench, LEVEL, 1000), ((latest / 1000 + (latest + SPAN) / 1000) / 2) as u16);
    let travel = drive.exposure_travel(&mut bench, LEVEL + 100, 1000);
    let latest = 346_000 - SPAN / 2;
    assert_eq!(travel, ((latest / 1000 + (latest + SPAN / 2) / 1000) / 2) as u16);
    assert_eq!(drive.timing().level, LEVEL + 100);
}

#[test]
fn a_travel_that_ends_before_the_frame_starts_is_zero_and_so_is_one_without_areas() {
    let mut bench = bench();
    let mut drive = planned(&mut bench, SampleAreas::Primary(AREAS));
    assert_eq!(drive.exposure_travel(&mut bench, LEVEL, 1000), 0);
    bench.line_period = Some(1000);
    drive.set_timing(SampleTiming { level: LEVEL, source: SOURCE, areas: SampleAreas::Primary(AreaTiming { count: 0, ..AREAS }) });
    assert_eq!(drive.exposure_travel(&mut bench, LEVEL, 1000), 0);
}

#[test]
fn without_a_line_period_or_a_linear_level_the_areas_are_exposed_at_the_frame_start_or_for_no_time() {
    let mut bench = bench();
    bench.line_period = None;
    let drive = planned(&mut bench, SampleAreas::Primary(AREAS));
    assert_eq!(drive.sample_targets(&mut bench, 300, 0, 0).primary[..3], [midpoint(300_000 - SPAN); 3]);
    bench.line_period = Some(100);
    bench.linear.clear();
    let targets = drive.sample_targets(&mut bench, 300, 0, 0);
    assert_eq!(targets.primary[..3], [106, 226, 346].map(|line| at(300_000 + line * 100)));
}
