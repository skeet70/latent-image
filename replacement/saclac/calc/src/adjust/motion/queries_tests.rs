use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_29;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::motion::drive::DriveKind;
use crate::adjust::motion::fakes::Bench;
use crate::adjust::motion::ports::Ticks;
use crate::adjust::motion::queries::*;
use crate::adjust::motion::state::{Drive, MotionState, SampleSchedule, Timing};
use crate::exposure::fakes::{handle, World};
use crate::stock::motion::{self as trace, PEAK_CHECK, PEAK_THRESHOLD, STEP_SCALE_HIGH, STEP_SCALE_LOW, STEP_SCALE_SELECTOR};

const SECTION: SharedAddress = SharedAddress(0x1000_0000);
const RESULTS: SharedAddress = SharedAddress(0x2000_0000);

fn state() -> MotionState {
    MotionState {
        section: SECTION,
        results: RESULTS,
        prepared_at: Ticks(40),
        drive_kind: DriveKind(0),
        timing: Timing::default(),
        sampling: SampleSchedule::default(),
        drive: Drive { checks_peak: true, ..Drive::default() },
    }
}

#[test]
fn tenths_round_half_away_from_zero() {
    assert_eq!([14, 15, 25, -14, -15, 0, 4].map(round_tenths), [1, 2, 3, -1, -2, 0, 0]);
}

fn channel_world() -> World {
    let mut world = World::default();
    world.set_shared(&group_29::AT_079C, 1);
    world
}

#[test]
fn the_second_channel_needs_the_shared_word_and_an_unrefused_record() {
    let record = handle();
    assert!(second_channel_available(&mut channel_world(), record));
    assert!(!second_channel_available(&mut World::default(), record));
    let refusals: [fn(&mut World); 4] = [
        |w| w.set(&group_00::AT_0067, 1),
        |w| w.set(&group_00::AT_0058, 2),
        |w| w.set(&group_00::AT_003E, 1),
        |w| {
            w.header(SharedHeaderByte::At47, 1);
            w.set(&group_00::AT_0070, 1);
        },
    ];
    for refuse in refusals {
        let mut world = channel_world();
        refuse(&mut world);
        assert!(!second_channel_available(&mut world, record));
    }
    let mut kind_set = channel_world();
    kind_set.header(SharedHeaderByte::At47, 1);
    kind_set.set(&group_00::AT_0067, 1);
    assert!(second_channel_available(&mut kind_set, record));
}

fn scaled(scale: u16) -> Bench {
    let mut bench = Bench { channel_position: 15, side_offset: 5, ..Bench::default() };
    bench.property_word(STEP_SCALE_HIGH, 200);
    bench.property_word(STEP_SCALE_LOW, 100);
    bench.property_word(crate::stock::motion::step_scale(6, 0), scale);
    bench
}

#[test]
fn a_step_offset_shrinks_as_the_kinds_scale_rises_above_the_low_bound() {
    // 50 is 30 steps from the channel position 15 moved 5 to the other side.
    assert_eq!(step_offset(&mut scaled(100), 1, 2, 50, 0, 3, 6), 30);
    assert_eq!(step_offset(&mut scaled(150), 1, 2, 50, 0, 3, 6), 15);
    assert_eq!(step_offset(&mut scaled(250), 1, 2, 50, 0, 3, 6), 0);
    assert_eq!(step_offset(&mut scaled(150), 1, 2, 50, 1, 3, 6), 18);
}

#[test]
fn other_kinds_take_the_selected_scale_and_reversed_bounds_give_none() {
    let mut bench = scaled(100);
    bench.property(STEP_SCALE_SELECTOR, 0x31);
    bench.property_word(crate::stock::motion::step_scale(30, 1), 150);
    assert_eq!(step_offset(&mut bench, 1, 2, 50, 0, 3, 30), 15);
    bench.property(STEP_SCALE_SELECTOR, 0x3);
    bench.property_word(crate::stock::motion::step_scale(30, 0), 100);
    assert_eq!(step_offset(&mut bench, 1, 2, 50, 0, 3, 30), 30);
    bench.property_word(STEP_SCALE_HIGH, 100);
    assert_eq!(step_offset(&mut bench, 1, 2, 50, 0, 3, 6), 0);
}

#[test]
fn the_pipeline_flag_is_read_from_the_dump_frame() {
    let mut bench = Bench::default();
    assert!(!state().pipeline_flag(&mut bench));
    bench.put(trace::pipeline_flag(SECTION), 7);
    assert!(state().pipeline_flag(&mut bench));
}

#[test]
fn the_time_since_preparation_is_traced() {
    let mut bench = Bench { now: 52, ..Bench::default() };
    state().trace_elapsed(&mut bench);
    assert_eq!(bench.stores, [(trace::elapsed(SECTION).0, 120)]);
}

fn measured(area: u32, levels: [u32; 2]) -> Bench {
    let mut bench = Bench::default();
    bench.property(PEAK_CHECK, 1);
    bench.property_word(PEAK_THRESHOLD, 500);
    bench.put(trace::results_present(RESULTS), 1);
    bench.put(trace::result_measured(RESULTS, area), 1);
    let [first, second] = trace::result_levels(RESULTS, area);
    bench.put(first, levels[0]);
    bench.put(second, levels[1]);
    bench
}

#[test]
fn the_peak_is_reached_when_a_measured_area_meets_the_threshold() {
    let mut bench = measured(4, [100, 500]);
    assert!(state().peak_reached(&mut bench));
    assert_eq!(bench.stores, [(trace::results_threshold(RESULTS).0, 500)]);
    assert!(!state().peak_reached(&mut measured(2, [499, 10])));
}

#[test]
fn no_peak_without_the_check_the_window_or_results() {
    let mut unchecked = state();
    unchecked.drive.checks_peak = false;
    let mut bench = measured(0, [900, 900]);
    assert!(!unchecked.peak_reached(&mut bench));
    assert!(bench.stores.is_empty());
    let mut disabled = measured(0, [900, 900]);
    disabled.property(PEAK_CHECK, 0);
    assert!(!state().peak_reached(&mut disabled));
    let mut empty = measured(0, [900, 900]);
    empty.put(trace::results_present(RESULTS), 0);
    assert!(!state().peak_reached(&mut empty));
    assert!(empty.stores.is_empty());
}
