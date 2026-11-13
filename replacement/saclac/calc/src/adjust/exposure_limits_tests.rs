use xm1_saclac_profile_records::IntervalTracking;

use crate::adjust::exposure_limits::*;
use crate::adjust::fakes::LimitWorld;
use crate::exposure::program::{PrimaryInputs, PrimaryRange};
use crate::stock::calculation::Cell;

/// The first parameter set.
const SET: u32 = 0x6125;

/// Scene 1 reads the first parameter set, an upper level of 600, targets within -200..=500 and
/// lens steps 2 to 9; each step's level is capped at 2000.
fn world() -> LimitWorld {
    let mut world = LimitWorld::default();
    world.set(0x610a, 1);
    world.set(SET + 4, 60);
    world.set(SET + 11, 50);
    world.set(SET + 12, 0xec);
    world.set_word(0x61e6, 9);
    world.set_word(0x621b, 2);
    for step in 0..=9 {
        world.set(0x61c0 + step, 200);
    }
    world
}

fn request(scene: u32) -> Request { Request { selection: Selection { scene, ..Selection::default() }, reference: 3, index: 4, ..Request::default() } }

fn unmeasured(scene: u32) -> Request {
    let mut request = request(scene);
    request.selection.unmeasured = true;
    request
}

#[test]
fn the_target_is_the_channel_level_less_the_configured_offsets_within_the_set_bounds() {
    let mut world = world();
    world.set(0x61e1, 5);
    world.set(0x61e0, 3);
    let plain = limits(&mut world, &request(1));
    assert_eq!((plain.target, plain.plain_target), (250, 250));
    let mut alternate = request(1);
    alternate.selection.alternate = true;
    assert_eq!(limits(&mut world, &alternate).target, 220);
    let tracking = limits(&mut world, &Request { tracking: true, ..request(1) });
    assert_eq!((tracking.target, tracking.plain_target), (220, 250));
    world.set(SET + 11, 20);
    assert_eq!(limits(&mut world, &request(1)).target, 200);
}

#[test]
fn a_measured_request_bands_at_its_target_without_margin_or_limits() {
    let mut world = world();
    world.set_word(0x68da, 400);
    let read = limits(&mut world, &request(1));
    assert_eq!(read.band, Band { high: read.target, low: read.target });
    assert_eq!((read.margin, read.limit, read.floor), (0, 0, 0));
    assert_eq!((read.far_step, read.near_step, read.upper), (9, 2, 600));
}

#[test]
fn every_scene_keeps_its_levels_steps_and_limits_ordered() {
    let mut world = LimitWorld::default();
    for id in 0x6100..0x6c00 {
        world.set(id, (id * 37 + 11) % 256);
    }
    for scene in 0..60 {
        for flags in [0, 2] {
            for (unmeasured, tracking, bank) in [(false, false, 0), (true, false, 0), (true, true, 1), (true, false, 4)] {
                let mut request = Request { tracking, bank, ..request(scene) };
                request.selection.flags = flags;
                request.selection.unmeasured = unmeasured;
                let read = limits(&mut world, &request);
                assert!(read.lower <= read.target && read.target <= read.upper, "{scene} {read:?}");
                assert!(read.near_step <= read.threshold.step && read.threshold.step <= read.far_step, "{scene} {read:?}");
                assert!(read.target <= read.threshold.at_or_below && read.threshold.at_or_below <= read.threshold.above, "{scene} {read:?}");
                assert!(read.margin <= read.limit && read.limit <= read.floor, "{scene} {read:?}");
            }
        }
    }
}

#[test]
fn a_parameter_bank_reads_the_second_half_of_the_set() {
    let mut world = world();
    world.set(SET + 10, 40);
    let fixed = Request { fixed_upper: true, ..request(1) };
    assert_eq!(limits(&mut world, &fixed).upper, 600);
    assert_eq!(limits(&mut world, &Request { bank: 1, ..fixed }).upper, 400);
}

#[test]
fn a_selector_outside_the_ten_sets_reads_the_first() {
    let mut world = world();
    world.set(0x6132 + 4, 70);
    world.set(0x610a, 2);
    assert_eq!(limits(&mut world, &request(1)).upper, 700);
    for selector in [0, 11, 255] {
        world.set(0x610a, selector);
        assert_eq!(limits(&mut world, &request(1)).upper, 600);
    }
}

#[test]
fn a_flagged_selection_reads_the_settings_of_scene_49() {
    let mut world = world();
    world.set(0x611a, 2);
    world.set(0x6132 + 4, 70);
    let mut flagged = request(1);
    flagged.selection.flags = 2;
    assert_eq!(limits(&mut world, &flagged).upper, 700);
    assert_eq!(limits(&mut world, &request(49)).upper, 700);
}

#[test]
fn a_cell_column_lowers_the_upper_level_below_the_ceiling() {
    let mut world = world();
    world.set(SET + 5, 30);
    assert_eq!(limits(&mut world, &request(1)).upper, 600);
    assert_eq!(limits(&mut world, &Request { cell: Cell { column: 1, row: 0 }, ..request(1) }).upper, 200);
}

#[test]
fn the_threshold_holds_the_band_and_the_target_of_an_unmeasured_selection() {
    let mut world = world();
    world.set(0x61e1, 5);
    world.set(0x611e, 1);
    let mut held = unmeasured(1);
    held.selection.threshold = Selection::threshold_of(120);
    let read = limits(&mut world, &held);
    assert_eq!((read.target, read.band), (120, Band { high: 120, low: 120 }));
    held.selection.scene = 53;
    assert_eq!(limits(&mut world, &held).target, 250);
    held.selection.threshold = Selection::threshold_of(0x7fff);
    held.selection.scene = 1;
    assert_eq!(limits(&mut world, &held).target, 250);
}

#[test]
fn an_indexed_limit_lowers_the_mode_limit_but_not_below_the_minimum() {
    let mut world = world();
    world.set_word(0x68da, 500);
    world.set_word(0x6995 + 2, 300);
    world.set_word(0x1906, 100);
    let selection = Selection { scene: 1, ..Selection::default() };
    let query = LimitQuery { index: 1, policy: 0, armed: false, set: LimitSet::of_stage(7), choice: 0, minimum: 0 };
    assert_eq!(clamped_limit(&mut world, &selection, &query), 300);
    assert_eq!(clamped_limit(&mut world, &selection, &LimitQuery { minimum: 450, ..query }), 350);
    assert_eq!(clamped_limit(&mut world, &selection, &LimitQuery { index: 3, ..query }), 500);
    world.set_word(0x6a2a, 800);
    world.set_word(0x6ae5 + 2, 700);
    assert_eq!(clamped_limit(&mut world, &selection, &LimitQuery { set: LimitSet::of_stage(4), ..query }), 700);
}

#[test]
fn armed_flagged_and_chosen_limits_replace_the_scenes_own() {
    let mut world = world();
    world.set_word(0x68c6, 100);
    world.set_word(0x68c4, 111);
    world.set_word(0x68fa, 222);
    world.set_word(0x1_7890, 333);
    let query = LimitQuery { index: 4, policy: 0, armed: false, set: LimitSet::Second, choice: 0, minimum: 0 };
    let scene = |scene, flags| Selection { scene, flags, ..Selection::default() };
    assert_eq!(clamped_limit(&mut world, &scene(0, 0), &query), 100);
    assert_eq!(clamped_limit(&mut world, &scene(0, 0), &LimitQuery { armed: true, ..query }), 111);
    assert_eq!(clamped_limit(&mut world, &scene(48, 0), &LimitQuery { armed: true, ..query }), 111);
    assert_eq!(clamped_limit(&mut world, &scene(1, 0), &LimitQuery { choice: 13, ..query }), 333);
    assert_eq!(clamped_limit(&mut world, &scene(1, 2), &LimitQuery { choice: 13, ..query }), 222);
}

#[test]
fn scenes_pinned_to_a_level_band_there_and_take_the_response_limits_at_or_below_100() {
    let mut world = world();
    world.set(0x61e1, 30);
    world.set_word(0x68e8, 400);
    let pinned = Request { level: 80, capture_profile: 5, index: 1, ..unmeasured(24) };
    let read = limits(&mut world, &pinned);
    assert_eq!(read.band, Band { high: 80, low: 80 });
    assert_eq!((read.margin, read.limit, read.floor), (59, 59, 62));
    let read = limits(&mut world, &Request { level: 101, ..pinned });
    assert_eq!((read.margin, read.limit, read.floor), (400, 400, 400));
}

#[test]
fn cells_of_level_class_4_or_5_hold_margin_limit_and_floor_at_their_row_level() {
    let mut world = world();
    world.set(SET + 5, 200);
    world.set_word(0x68da, 400);
    let read = limits(&mut world, &Request { cell: Cell { column: 1, row: 2 }, ..unmeasured(1) });
    assert_eq!((read.margin, read.limit, read.floor), (120, 120, 120));
    let read = limits(&mut world, &unmeasured(1));
    assert_eq!((read.limit, read.floor), (400, 400));
}

#[test]
fn the_unselected_measurement_range_takes_the_uncapped_upper_level_and_the_unindexed_limit() {
    let mut world = world();
    world.set(SET + 4, 250);
    world.set_word(0x68da, 400);
    let inputs = PrimaryInputs {
        kind: 1,
        flags: 0,
        lens_reference: 3,
        unmeasured: true,
        policy: 0,
        focus_class: 0,
        variant: 0,
        armed: 0,
        record_3e: 0,
        interval_tracking: IntervalTracking(0),
        record_20: 0x7fff,
        record_44: 0,
        curve: 0,
        step_three: 0,
        auxiliary: 0,
    };
    assert_eq!(primary_range(&mut world, &inputs), PrimaryRange { upper: 2500, lower: 0, far_lens: 9, near_lens: 2, floor: 400 });
}
