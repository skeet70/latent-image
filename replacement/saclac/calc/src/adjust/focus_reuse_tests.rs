use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::focus_reuse::*;
use crate::exposure::context::ExposureContext;
use crate::exposure::fakes::{handle, World};
use crate::stock::{dump, property};

/// A record whose values match `matching()`'s snapshot: armed, scene 12, follow word 0x30, style 3,
/// subjects compared.
fn current() -> World {
    let mut world = World::default();
    world.header(SharedHeaderByte::At63, 1);
    world.header(SharedHeaderByte::At31, 12);
    world.set(&group_00::AT_002E, 0x30);
    world.set(&group_00::AT_003F, 3);
    world.set(&group_00::AT_005D, 1);
    world.property(property::REUSE_TOLERANCE, 3);
    world
}

fn matching() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.binding.mask = 2;
    context.binding.option = 5;
    context.binding.record = Some(handle());
    context.binding.subjects.count = 2;
    context.outcome.adjusting = true;
    let focus = &mut context.snapshot.focus;
    focus.adjustment = [1, 5, 12];
    focus.adjustment_kind = 0x30;
    focus.subject_count = 2;
    focus.limit = 100;
    focus.adjustment_level = 80;
    context
}

fn reuse(world: &mut World, context: &ExposureContext) -> bool { reuse_focus_result(world, context) }

#[test]
fn a_result_matching_the_current_record_is_reused() {
    assert!(reuse(&mut current(), &matching()));
    let mut unarmed = current();
    unarmed.header(SharedHeaderByte::At63, 0);
    unarmed.header(SharedHeaderByte::At31, 0);
    assert!(reuse(&mut unarmed, &matching()));
}

#[test]
fn a_bound_mode_one_without_the_mask_bit_reuses_whatever_the_result() {
    let mut context = ExposureContext::default();
    context.binding.mode = 1;
    assert!(reuse(&mut current(), &context));
    context.binding.mask = 2;
    assert!(!reuse(&mut current(), &context));
}

#[test]
fn each_compared_value_must_match() {
    let changed: [fn(&mut ExposureContext); 8] = [
        |c| c.snapshot.focus.adjustment[0] = 0,
        |c| c.outcome.adjusting = false,
        |c| c.adjustment.state = 1,
        |c| c.snapshot.focus.adjustment[1] = 6,
        |c| c.binding.subjects.count = 3,
        |c| c.snapshot.focus.adjustment[2] = 13,
        |c| c.snapshot.focus.adjustment_kind = 0x31,
        |c| c.snapshot.focus.adjustment_level = 70,
    ];
    for change in changed {
        let mut context = matching();
        change(&mut context);
        assert!(!reuse(&mut current(), &context));
    }
}

#[test]
fn style_six_ignores_the_option_and_unchecked_subjects_are_ignored() {
    let mut world = current();
    world.set(&group_00::AT_003F, 6);
    let mut context = matching();
    context.snapshot.focus.adjustment[1] = 6;
    assert!(reuse(&mut world, &context));
    world.set(&group_00::AT_005D, 0);
    context.binding.subjects.count = 9;
    assert!(reuse(&mut world, &context));
}

#[test]
fn a_style_that_qualifies_for_a_fresh_evaluation_is_not_reused() {
    let mut context = matching();
    context.levels.unmeasured = true;
    let mut world = current();
    world.set(&group_00::AT_003F, 7);
    assert!(!reuse(&mut world, &context));
    world.select(MeasurementSelection::Record);
    assert!(reuse(&mut world, &context));
    assert!(!reuse(&mut current(), &context));
    context.binding.option = 0;
    context.snapshot.focus.adjustment[1] = 0;
    assert!(reuse(&mut current(), &context));
}

#[test]
fn the_tolerance_is_measured_either_side_of_the_limit() {
    let mut context = matching();
    context.snapshot.focus.adjustment_level = 129;
    assert!(reuse(&mut current(), &context));
    context.snapshot.focus.adjustment_level = 130;
    assert!(!reuse(&mut current(), &context));
}

#[test]
fn a_bound_frame_records_the_trace_and_the_tolerance_miss() {
    let frame = SharedAddress(0x0200_0000);
    let mut context = matching();
    context.binding.frame = Some(frame);
    context.snapshot.focus.adjustment_level = 40;
    let mut world = current();
    assert!(!reuse(&mut world, &context));
    let word = |at: SharedAddress, value: u32| (at.0, 4, value);
    assert_eq!(world.stores, [
        word(dump::reuse_result(frame, 0), 1),
        word(dump::reuse_result(frame, 1), 5),
        word(dump::reuse_result(frame, 2), 12),
        word(dump::reuse_result(frame, 3), 0x30),
        word(dump::reuse_result(frame, 4), 2),
        word(dump::reuse_out_of_tolerance(frame), 0),
        word(dump::reuse_level(frame), 540),
        word(dump::reuse_limit(frame), 600),
        word(dump::reuse_tolerance(frame), 3),
        word(dump::reuse_out_of_tolerance(frame), 1),
    ]);
}

#[test]
fn an_unbound_frame_is_not_written() {
    let mut world = current();
    let mut context = matching();
    context.snapshot.focus.adjustment_level = 40;
    reuse(&mut world, &context);
    assert!(world.stores.is_empty());
}
