use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::exposure::{ExposureInputs, FrameExposure, Program, ProgramKind, ProgramStep, PROGRAM_STEPS};
use crate::adjust::motion::readings::{FocusRequest, RecordReadings, StepLimits};
use crate::adjust::motion::world::MotionWorld;
use crate::exposure::fakes::handle;
use crate::stock::motion::{self as stock};
use crate::stock::property;

const LENS: u32 = 7;
const RESULTS: SharedAddress = SharedAddress(0x5000_0000);

fn readings() -> RecordReadings {
    RecordReadings {
        record: handle(),
        measurement_selection: Default::default(),
        scene: 0,
        channel: 0,
        style: 0,
        at_0043: 0,
        class: 0,
        policy: 0,
        lens: LENS,
        limit_word: 0,
        level_step: 0,
        at_0035: 0,
        at_0036: 0,
        at_0038: 0,
        at_20cc: 0,
        at_20c8: 0,
        at_20d0: 0,
        locked: 0,
        row_scale: [0; 2],
        selection_value: 0,
        at_0164: 0,
        at_0168: 0,
        request: FocusRequest::default(),
        confirmation: [0; 8],
        area: Default::default(),
        at_0028: 0,
        levels: [0; 5],
        at_00a4: 0,
        at_00b8: 0,
        measured_level: 0,
        load_test: false,
        at_005f: 0,
        at_005b: 0,
        compares_subjects: 0,
        area_positions: [0; 10],
        armed: 0,
        auxiliary: 0,
        area_words: [0; 2],
        drive_kind: Default::default(),
        reference: 0,
        at_004e: 0,
        at_003c: 0,
        priority: 0,
        at_0039: 0,
        at_003a: 0,
        at_003b: 0,
        designation: [0; 7],
        at_0194: (0, 0),
        record_3e: 0,
        lens_state: 0,
    }
}

fn frame(world: &mut MotionWorld, program: &mut Program, level: i16, change: i16) -> FrameExposure {
    let readings = readings();
    let inputs = ExposureInputs { readings: &readings, limits: StepLimits { lowest: 4, highest: 30, reach: 10 }, level_ceiling: 500, results: RESULTS };
    inputs.frame(world, program, level, change, 20)
}

#[test]
fn the_fixed_exposure_takes_the_configured_level_step_and_profile_and_leaves_the_program() {
    let mut world = MotionWorld { steps: 10, ..MotionWorld::default() };
    world.property(stock::FIXED_EXPOSURE, 1);
    world.property_word(stock::FIXED_LEVEL, 300);
    world.property(stock::FIXED_GAIN_STEP, 3);
    world.property_word(property::level_step(2), 100);
    world.property_word(property::level_step(4), 160);
    world.property(stock::FIXED_STEP, 5);
    world.world.lens_levels.insert((5, LENS), 20);
    world.world.level_offsets.insert(stock::SEARCH_PROFILE, 7);
    world.times.insert((stock::SEARCH_PROFILE, 300, 0), 50);
    world.shortest_times.insert(stock::SEARCH_PROFILE, 80);
    let mut program = [ProgramStep::default(); PROGRAM_STEPS];
    let exposure = frame(&mut world, &mut program, 200, 0);
    assert_eq!(exposure.profile, stock::SEARCH_PROFILE);
    assert_eq!(exposure.lens_step, 5);
    assert_eq!(exposure.exposure_level, 300);
    assert_eq!(exposure.step_offset, 60);
    assert_eq!(exposure.at_06, 300 - 60 + 20 - 7);
    assert_eq!(exposure.change, 253 - 200);
    assert_eq!(exposure.time, 80, "never shorter than the profile's shortest exposure");
    assert_eq!(exposure.frames, 4);
    assert_eq!(exposure.base_level, 100);
    assert_eq!(program, [ProgramStep::default(); PROGRAM_STEPS]);

    world.property(stock::FIXED_STEP, 12);
    assert_eq!(frame(&mut world, &mut program, 200, 0).lens_step, 0, "a step beyond the calibration falls back to step 0");
}

#[test]
fn a_level_above_the_programs_last_step_exposes_at_the_highest_step_capped_at_the_ceiling() {
    let mut world = MotionWorld::default();
    world.property(stock::LEVEL_CEILING, 4);
    world.times.insert((0x22, 40, 0), 1234);
    let mut program = [ProgramStep::default(); PROGRAM_STEPS];
    let exposure = frame(&mut world, &mut program, 50, 0);
    assert_eq!(exposure.program, ProgramKind::Plain);
    assert_eq!(exposure.step, 17);
    assert_eq!(exposure.lens_step, 30);
    assert_eq!(exposure.exposure_level, 40);
    assert_eq!(exposure.time, 1234);
}

#[test]
fn a_level_below_the_first_step_exposes_at_the_reach_raised_to_the_floor() {
    let mut world = MotionWorld::default();
    world.property(stock::level_floor(0), 3);
    world.times.insert((0x22, 30, 0), 77);
    let mut program = [ProgramStep::default(); PROGRAM_STEPS];
    world.world.lens_levels.insert((10, LENS), 100);
    let exposure = frame(&mut world, &mut program, -500, 0);
    assert_eq!(exposure.step, 0);
    assert_eq!(exposure.lens_step, 10);
    assert_eq!(exposure.exposure_level, 30);
    assert_eq!(exposure.time, 77);
}

#[test]
fn the_program_steps_take_the_reach_the_requested_step_and_the_highest_step() {
    let mut world = MotionWorld::default();
    let mut program = [ProgramStep::default(); PROGRAM_STEPS];
    frame(&mut world, &mut program, 0, 0);
    let steps: [(u32, u32); PROGRAM_STEPS] = program.map(|step| (step.low_step, step.high_step));
    assert!(steps[..13].iter().all(|&steps| steps == (10, 10)));
    assert_eq!(steps[13], (20, 10));
    assert_eq!(steps[14], (20, 20));
    assert_eq!(steps[15], (20, 20));
    assert_eq!(steps[16], (30, 20));
    assert_eq!(steps[17], (30, 30));
    world.property(stock::REACH, 1);
    frame(&mut world, &mut program, 0, 0);
    assert_eq!((program[0].low_step, program[0].high_step), (10, 4));
}

#[test]
fn a_configured_gain_program_picks_the_highest_gain_whose_exposure_is_long_enough() {
    let mut world = MotionWorld { default_time: 1, ..MotionWorld::default() };
    world.property(stock::PROGRAM, 2);
    for profile in stock::MODE_PROFILES {
        world.times.insert((profile, stock::LONGEST_LEVEL, 0), 1000);
        world.times.insert((profile, 0, 12), 900);
        world.times.insert((profile, 0, 10), 200_000);
    }
    let mut program = [ProgramStep::default(); PROGRAM_STEPS];
    let exposure = frame(&mut world, &mut program, 0, 0);
    assert_eq!(exposure.program, ProgramKind::Even);
    assert!(program.iter().all(|step| step.gain == 10));
    assert_eq!(exposure.gain, 10);

    world.property(stock::PROGRAM, 3);
    let exposure = frame(&mut world, &mut program, 0, 0);
    assert_eq!(exposure.program, ProgramKind::Odd);
    assert_eq!(exposure.gain, 4, "no odd gain's exposure lasts long enough");
}

#[test]
fn the_program_modes_choose_the_capture_profiles() {
    let mut world = MotionWorld::default();
    world.property(stock::program_mode(0, 17), 2);
    world.property(stock::program_mode(0, 3), 9);
    let mut program = [ProgramStep::default(); PROGRAM_STEPS];
    frame(&mut world, &mut program, 0, 0);
    assert_eq!((program[17].mode, program[17].profile), (2, 0x1f));
    assert_eq!((program[3].mode, program[3].profile), (3, 0x1e), "modes above 3 take mode 3");
    assert_eq!(program[0].profile, 0x22);
}
