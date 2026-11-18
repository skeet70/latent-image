use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::drive::{DriveKind, DriveMode, DriveProfile, Ramp};
use crate::adjust::motion::fakes::Bench;
use crate::adjust::motion::sampling::*;
use crate::adjust::motion::plan::{AreaLayout, Motion, Plan};
use crate::adjust::motion::state::{MotionState, SampleSchedule};
use crate::adjust::motion::world::blank_state;
use crate::stock::motion::{self as trace, MEASURED_RUN_FRAMES, SAMPLE_CORRECTION_BASE, SAMPLE_CORRECTION_BIAS, SAMPLE_CORRECTION_SCALE, TERMINATOR};

const SECTION: SharedAddress = SharedAddress(0x1000_0000);

/// One step per millisecond from `start` to `end`, cruising throughout.
fn cruise(start: i16, end: i16) -> DriveProfile {
    let distance = (i32::from(end) - i32::from(start)).unsigned_abs();
    DriveProfile {
        start,
        end,
        reverse: end < start,
        second: Ramp { speed: 1000, ..Ramp::default() },
        cruise_distance: distance as u16,
        cruise_duration: distance * 1000,
        ..DriveProfile::default()
    }
}

fn state(profile: DriveProfile, layout: AreaLayout, sampling: SampleSchedule) -> MotionState {
    let mut state = blank_state(SECTION, SharedAddress(0x2000_0000), 10_000);
    state.readings.drive_kind = DriveKind(7);
    MotionState { sampling, plan: Plan { motion: Motion::Continuous, profile, layout, ..Plan::default() }, ..state }
}

fn corrections() -> Bench {
    let mut bench = Bench::default();
    bench.property(SAMPLE_CORRECTION_SCALE, 50);
    bench.property(SAMPLE_CORRECTION_BIAS, 2);
    bench.property(SAMPLE_CORRECTION_BASE, 0xff);
    bench
}

#[test]
fn a_frame_is_read_off_the_drive_from_the_runs_start_time() {
    let mut motion = state(cruise(0, 1000), AreaLayout::Single, SampleSchedule::default());
    motion.plan.start_time = 15_000;
    assert_eq!(motion.position_at_frame(0), 0);
    assert_eq!(motion.position_at_frame(2), 5);
    assert_eq!(motion.position_at_frame(30), 285);
}

#[test]
fn a_target_follows_the_drive_over_its_areas_exposure() {
    let sampling = SampleSchedule::new(2000, &[1000, -3000], &[], 0, 0);
    let motion = state(cruise(0, 1000), AreaLayout::Single, sampling);
    let targets = motion.targets(&mut corrections(), 200, 3);
    // Exposed over 201..203 and 197..199: midpoints 2 and -2 from the position, halved, less
    // the bias of 2, from a base of -1.
    assert_eq!(targets.primary, [198, 196, 0, 0, 0, 0, 0]);
    assert_eq!(targets.secondary, [0; SECONDARY_AREAS]);
}

#[test]
fn a_reverse_drive_adds_its_bias() {
    let sampling = SampleSchedule::new(2000, &[1000], &[], 0, 0);
    let motion = state(cruise(1000, 0), AreaLayout::Single, sampling);
    assert_eq!(motion.targets(&mut corrections(), 800, 3).primary[0], 800);
}

#[test]
fn a_still_run_targets_the_position_itself() {
    let sampling = SampleSchedule::new(2000, &[1000, -3000, 500], &[], 0, 0);
    let mut motion = state(cruise(0, 1000), AreaLayout::Single, sampling);
    motion.plan.motion = Motion::Stepped;
    assert_eq!(motion.targets(&mut corrections(), 200, 3).primary, [200, 200, 200, 0, 0, 0, 0]);
}

#[test]
fn each_sample_traces_its_position_and_drive_time() {
    let mut motion = state(cruise(0, 1000), AreaLayout::Single, SampleSchedule::default());
    let mut bench = corrections();
    motion.targets(&mut bench, 200, 3);
    motion.plan.start_time = 5000;
    motion.targets(&mut bench, -4, 0);
    assert_eq!(
        bench.stores,
        [
            (trace::sample_position(SECTION, 3).0, 200),
            (trace::sample_position(SECTION, 4).0, TERMINATOR),
            (trace::sample_time(SECTION, 3).0, 200_000),
            (trace::sample_time(SECTION, 4).0, TERMINATOR),
            (trace::sample_position(SECTION, 0).0, -4i32 as u32),
            (trace::sample_position(SECTION, 1).0, TERMINATOR),
            (trace::sample_time(SECTION, 0).0, -5000i32 as u32),
            (trace::sample_time(SECTION, 1).0, TERMINATOR),
        ]
    );
}

#[test]
fn listed_secondary_rows_are_sampled_then_repeated_across_the_columns() {
    let sampling = SampleSchedule::new(2000, &[-3000], &[1000], 1, 3);
    let motion = state(cruise(0, 1000), AreaLayout::Listed, sampling);
    let targets = motion.targets(&mut corrections(), 200, 3);
    assert_eq!(targets.secondary, [198, 198, 198, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(targets.words()[..9], [196, 0, 0, 0, 0, 0, 0, 198, 198]);
}

#[test]
fn a_grid_takes_the_centre_primary_target_in_every_area() {
    let sampling = SampleSchedule::new(2000, &[0, 1000, -3000], &[0, 0], 2, 3);
    for layout in [AreaLayout::Grid, AreaLayout::GridAlternate] {
        let mut motion = state(cruise(0, 1000), layout, sampling);
        motion.layout.grids[1].columns = 2;
        motion.layout.grids[1].rows = 3;
        assert_eq!(motion.targets(&mut corrections(), 200, 3).secondary, [198, 198, 198, 198, 198, 198, 0, 0, 0, 0]);
    }
}

/// Still, then speeding up through frame 5, braking from frame 7 and settled by frame 10.
const RUN: [i16; 14] = [100, 100, 100, 101, 103, 107, 113, 119, 125, 129, 131, 132, 132, 132];

fn replanning(measured_frames: u32) -> (MotionState, Bench) {
    let mut motion = state(DriveProfile { start: 100, end: 132, ..DriveProfile::default() }, AreaLayout::Single, SampleSchedule::default());
    motion.plan.start_time = -9;
    motion.plan.first_frame = 2;
    let mut bench = Bench::default();
    bench.property(MEASURED_RUN_FRAMES, measured_frames);
    let earlier = [SpeedStep { frame: 0, speed: 300 }];
    motion.replan_from_run(&mut bench, earlier, SpeedStep { frame: 0, speed: 500 }, &RUN);
    (motion, bench)
}

#[test]
fn a_replanning_drives_at_the_speed_the_run_measured() {
    // 18 steps over the three frames from the turn at frame 5 to the brake at frame 8.
    let (motion, bench) = replanning(3);
    assert_eq!(motion.plan.profile.second.speed, 600);
    assert_eq!(motion.plan.profile.cruise_duration, 53_333);
    assert_eq!(bench.word(trace::drive_planning(SECTION, 3)), Some(600));
    assert_eq!(bench.kinds, [(DriveKind(7), DriveMode::MEASUREMENT); 3]);
}

#[test]
fn a_run_too_short_to_measure_keeps_the_scheduled_speed() {
    let (motion, _) = replanning(5);
    assert_eq!(motion.plan.profile.second.speed, 500);
}

#[test]
fn a_replanning_anchors_the_drive_to_the_first_position_that_moved() {
    // Position 101 is frame 3 of the record, frame 5 of the run, 1.666 ms into the drive.
    let (motion, bench) = replanning(3);
    assert_eq!(motion.plan.start_time, 48_334);
    let traced: Vec<_> = bench.stores.iter().skip(12).copied().collect();
    assert_eq!(
        traced,
        [
            (trace::drive_planning(SECTION, 0).0, 48_334),
            (trace::schedule_frame(SECTION, 0).0, 0),
            (trace::schedule_speed(SECTION, 0).0, 300),
            (trace::schedule_frame(SECTION, 1).0, 0),
            (trace::schedule_speed(SECTION, 1).0, 500),
            (trace::schedule_frame(SECTION, 2).0, TERMINATOR),
            (trace::schedule_speed(SECTION, 2).0, TERMINATOR),
        ]
    );
    assert_eq!(bench.stores[0], (trace::drive_planning(SECTION, 0).0, -9i32 as u32));
}
