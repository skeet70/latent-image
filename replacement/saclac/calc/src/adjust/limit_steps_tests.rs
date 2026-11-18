use crate::adjust::exposure_limits::{Request, Selection};
use crate::adjust::fakes::LimitWorld;
use crate::adjust::limit_steps::*;

/// The first parameter set.
const SET: u32 = 0x6125;

/// Scenes 1, 24 and 44 read the first parameter set: an upper level of 600, a threshold level of
/// 100 above step 5 and 50 at or below it, and lens steps 2 to 9 at base level 100 per step.
fn world() -> LimitWorld {
    let mut world = LimitWorld::default();
    for selector in [0x610a, 0x6110, 0x6109] {
        world.set(selector, 1);
    }
    world.set(SET, 10);
    world.set(SET + 1, 5);
    world.set_word(SET + 2, 5);
    world.set(SET + 4, 60);
    world.set(SET + 11, 50);
    for (far, near) in [(0x61e6, 0x621b), (0x61f2, 0x6227), (0x61e4, 0x6219)] {
        world.set_word(far, 9);
        world.set_word(near, 2);
    }
    for step in 0..=9 {
        world.set(0x61c0 + step, 200);
    }
    world.even_base_levels(100);
    world
}

fn inputs(scene: u32, placed_level: i16) -> StepInputs {
    let request = Request { selection: Selection { scene, ..Selection::default() }, index: 4, ..Request::default() };
    StepInputs { request, placed_level, ..StepInputs::default() }
}

#[test]
fn a_measured_level_takes_the_farthest_step_its_threshold_allows() {
    let chosen = choose(&mut world(), &inputs(1, 520));
    assert_eq!((chosen.step, chosen.level, chosen.cap, chosen.correction), (4, 120, 600, 0));
    assert_eq!((chosen.flags, chosen.above, chosen.compensated), (StepFlags(0), false, 28));
}

#[test]
fn a_level_beyond_the_far_or_near_steps_reach_is_flagged() {
    let mut world = world();
    let chosen = choose(&mut world, &inputs(1, 2000));
    assert_eq!((chosen.step, chosen.level, chosen.flags, chosen.above), (9, 600, StepFlags(StepFlags::ABOVE), true));
    let chosen = choose(&mut world, &inputs(1, 100));
    assert_eq!((chosen.step, chosen.level, chosen.flags, chosen.above), (2, 0, StepFlags(StepFlags::BELOW), false));
}

#[test]
fn scene_24_takes_the_far_step_and_holds_the_request_level() {
    let mut world = world();
    let mut pinned = inputs(24, 0);
    pinned.request.level = 300;
    let chosen = choose(&mut world, &pinned);
    assert_eq!((chosen.profile, chosen.step, chosen.level, chosen.cap), (9, 9, 300, 600));
    pinned.request.level = NO_REQUEST_LEVEL;
    assert_eq!(choose(&mut world, &pinned).level, UNREQUESTED_LEVEL);
}

#[test]
fn scenes_44_and_57_walk_from_their_step_and_report_a_short_walk() {
    let mut world = world();
    let walked = |world: &mut LimitWorld, steps| {
        let chosen = choose(world, &StepInputs { steps, ..inputs(44, 720) });
        (chosen.step, chosen.level, chosen.flags)
    };
    assert_eq!(walked(&mut world, 0), (6, 120, StepFlags(0)));
    assert_eq!(walked(&mut world, 2), (4, 320, StepFlags(0)));
    assert_eq!(walked(&mut world, 6), (2, 520, StepFlags(StepFlags::SHORT)));
    assert_eq!(walked(&mut world, -2), (7, 20, StepFlags(StepFlags::SHORT)));
}

#[test]
fn an_unmeasured_level_short_of_the_band_takes_the_near_step_with_a_correction() {
    let mut world = world();
    world.set(0x624e, 40);
    world.set_word(0x68da, 300);
    let mut short = inputs(1, 350);
    short.request.selection.unmeasured = true;
    let chosen = choose(&mut world, &short);
    assert_eq!((chosen.step, chosen.level, chosen.correction, chosen.start, chosen.floor), (2, 350, 200, 100, 300));
    short.request.minimum = 250;
    let chosen = choose(&mut world, &short);
    assert_eq!((chosen.step, chosen.level, chosen.correction, chosen.start), (2, 350, 200, 250));
}
