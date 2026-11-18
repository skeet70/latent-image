use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;

use crate::exposure::context::ExposureContext;
use crate::exposure::fakes::World;
use crate::exposure::tables;
use crate::stock::{dump, property, shared};

const FRAME: SharedAddress = SharedAddress(0x4000_0000);
const LEVEL_TABLES: u32 = 0x7000_0000;
const SUBJECT_LEVELS: u32 = 0x7100_0000;
const LEVEL_CLASSES: u32 = 0x7200_0000;

/// A world whose scene is enabled, whose weights are under their limit and whose adjustment
/// window is (-200, 300) around step 2 = 0.
fn world() -> World {
    let mut world = World::default();
    world.property(property::scene_mask(0), 1);
    world.property_word(property::WEIGHT_LIMIT, 1000);
    world.property(property::window(property::WindowSource::Default).0, 50);
    world.property(property::TABLE_FLOOR, 10);
    world.property(property::TABLE_CEILING, 20);
    world.property_word(property::level_step(3), 300);
    world.property(property::DUMP_FIRST, 0x11);
    world.property(property::DUMP_SECOND, 0x22);
    world.set(&group_00::AT_0045, 6);
    world.put_u16(SharedAddress(LEVEL_TABLES), 0);
    world.put_u16(SharedAddress(LEVEL_TABLES + 2), 1000);
    world.put_u16(SharedAddress(LEVEL_TABLES + 4), -2000i16 as u16);
    world.put_u16(SharedAddress(LEVEL_TABLES + 0x600), 7);
    world.put_u16(SharedAddress(SUBJECT_LEVELS), 40);
    world.put_u16(SharedAddress(SUBJECT_LEVELS + 60), 9);
    world.put_u8(SharedAddress(LEVEL_CLASSES), 3);
    world.put_u8(SharedAddress(LEVEL_CLASSES + 1), 4);
    world.put_u8(SharedAddress(LEVEL_CLASSES + 2), 0xfe);
    world
}

/// A valid snapshot at focus level 50 whose adjustment level is 50.
fn context() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.binding.frame = Some(FRAME);
    context.snapshot.valid = 1;
    context.snapshot.subjects.count = 2;
    let focus = &mut context.snapshot.focus;
    focus.level_tables = Some(SharedAddress(LEVEL_TABLES));
    focus.subject_levels = Some(SharedAddress(SUBJECT_LEVELS));
    focus.level_classes = Some(SharedAddress(LEVEL_CLASSES));
    focus.levels = [100, 20, 0, 30, 0];
    focus.weights = [1, 2, 0, 0, 0];
    context
}

#[test]
fn an_adjusting_exposure_clamps_the_record_curves_to_the_configured_range() {
    let mut world = world();
    let mut context = context();
    tables::prepare(&mut world, &mut context);
    assert!(context.outcome.adjusting);
    assert_eq!(context.levels.target, 300);
    assert_eq!(world.u16_at(shared::level_curve(0, 0)), 250);
    assert_eq!(world.u16_at(shared::level_curve(0, 1)), 450);
    assert_eq!(world.u16_at(shared::level_curve(0, 2)), -800i16 as u16);
    assert_eq!(world.u16_at(shared::level_curve(3, 0)), 257);
    assert_eq!(world.u16_at(shared::subject_curve(0, 0)), 290);
    assert_eq!(world.u16_at(shared::subject_curve(3, 0)), 259);
    assert_eq!(context.adjustment.level, 350);
    assert_eq!(context.adjustment.state, 0);
}

#[test]
fn class_bytes_three_and_four_become_one_and_two_and_the_selection_flags_clear() {
    let mut world = world();
    for flag in 0..shared::SELECTION_FLAG_BYTES as u32 {
        world.put_u8(SharedAddress(shared::SELECTION_FLAGS.0 + flag), 0x5a);
    }
    tables::prepare(&mut world, &mut context());
    let class = |point| world.memory.get(&shared::level_class(point).0).copied();
    assert_eq!((class(0), class(1), class(2), class(3)), (Some(1), Some(2), Some(0xfe), Some(0)));
    assert!((0..shared::SELECTION_FLAG_BYTES as u32).all(|flag| world.memory[&(shared::SELECTION_FLAGS.0 + flag)] == 0));
}

#[test]
fn the_dump_frame_records_the_tables_relative_to_the_focus_level() {
    let mut world = world();
    tables::prepare(&mut world, &mut context());
    assert_eq!(world.fills, [(dump::cleared(FRAME).0 .0, 0x1468, 0)]);
    assert_eq!(world.u32_at(dump::configuration(FRAME, 0)), 0x11);
    assert_eq!(world.u32_at(dump::configuration(FRAME, 1)), 0x22);
    assert_eq!(world.u32_at(dump::subjects_present(FRAME)), 1);
    assert_eq!(world.u32_at(dump::level_table(FRAME, 0, 1)), 1450);
    assert_eq!(world.u32_at(dump::level_table(FRAME, 0, 2)) as i32, -1550);
    assert_eq!(world.u32_at(dump::level_class(FRAME, 2)) as i32, -2);
    assert_eq!(world.u32_at(dump::subject_level(FRAME, 0)), 490);
    assert_eq!(world.u32_at(dump::focus_level(FRAME, 3)), 30);
    assert_eq!(world.u32_at(dump::weight(FRAME, 1)), 2);
}

#[test]
fn a_subject_count_past_ten_traces_ten_subject_levels() {
    let mut world = world();
    let mut context = context();
    context.snapshot.subjects.count = 12;
    tables::prepare(&mut world, &mut context);
    assert_eq!(world.u32_at(dump::configuration(FRAME, 0)), 0x11);
    assert_eq!(world.u32_at(dump::configuration(FRAME, 1)), 0x22);
}

#[test]
fn an_invalid_snapshot_does_not_adjust_and_writes_no_curve() {
    let mut world = world();
    let mut context = context();
    context.snapshot.valid = 0;
    tables::prepare(&mut world, &mut context);
    assert!(!context.outcome.adjusting);
    assert_eq!(world.u32_at(dump::level_table(FRAME, 0, 1)), 0);
    assert_eq!(world.u16_at(shared::level_curve(0, 0)), 0);
    assert_eq!(context.levels.target, 0);
}

#[test]
fn a_weight_sum_at_its_limit_or_a_level_outside_the_window_does_not_adjust() {
    let mut world = world();
    world.property_word(property::WEIGHT_LIMIT, 3);
    let mut context = context();
    tables::prepare(&mut world, &mut context);
    assert!(!context.outcome.adjusting);

    let mut world = self::world();
    let mut context = self::context();
    context.snapshot.focus.levels[0] = 400;
    tables::prepare(&mut world, &mut context);
    assert!(!context.outcome.adjusting);
    assert_eq!(world.u16_at(shared::level_curve(0, 0)), 0);
}

#[test]
fn a_disabled_scene_does_not_adjust() {
    let mut world = world();
    world.property(property::scene_mask(0), 0);
    let mut context = context();
    tables::prepare(&mut world, &mut context);
    assert!(!context.outcome.adjusting);
}

#[test]
fn the_adjustment_state_follows_the_fifth_level_and_the_weight_limits() {
    let state = |levels4: i16, third: u16, fourth: u16| {
        let mut world = world();
        world.property_word(property::SECOND_WEIGHT_LIMIT, 10);
        world.property_word(property::THIRD_WEIGHT_LIMIT, 10);
        let mut context = context();
        context.snapshot.focus.levels[4] = levels4;
        context.snapshot.focus.weights[2] = third;
        context.snapshot.focus.weights[3] = fourth;
        tables::prepare(&mut world, &mut context);
        assert!(context.outcome.adjusting);
        context.adjustment.state
    };
    assert_eq!(state(0, 10, 10), 0);
    assert_eq!(state(0, 11, 10), 2);
    assert_eq!(state(0, 10, 11), 1);
    assert_eq!(state(0, 11, 11), 3);
    assert_eq!(state(5, 0, 0), 3);
}
