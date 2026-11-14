use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_41;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::exposure_limits::NO_INDEX;
use crate::adjust::profile_mode::*;
use crate::exposure::fakes::{handle, World};
use crate::exposure::ports::Property;
use crate::stock::calculation::Cell;
use crate::stock::profile_mode::{CellTable, CELL_TABLE_BYTES};

/// Every limit reads lens steps 2 to 9, base level 100 per step, and image 4 is ready. A measured
/// selection's limits have target and limit 0, so its first level is the near step's base, 200.
fn world() -> World {
    let mut world = World::default();
    for limit in [0, u16::MAX] {
        world.near_steps.insert(limit, 2);
        world.far_steps.insert(limit, 9);
    }
    for step in 0..=9 {
        world.lens_base_levels.insert(step, 100 * step as i16);
    }
    world.device_ready = true;
    world
}

fn selection(scene: u32, style: u8, position: i16) -> FlagInputs {
    FlagInputs { scene, style, index: NO_INDEX, positions: Positions { position, spread: Spread::default() }, ..FlagInputs::default() }
}

/// Scene 1 unmeasured with its mode limit at 30 tenths.
fn unmeasured(world: &mut World, position: i16) -> FlagInputs {
    world.property_word(Property(0x68da), 30);
    FlagInputs { unmeasured: true, ..selection(1, 0, position) }
}

fn levels(world: &mut World, inputs: &FlagInputs) -> LimitLevels { limit_levels(world, inputs, FlagStyle::Selection).0 }

fn flags(world: &mut World, inputs: &FlagInputs) -> u8 { limit_levels(world, inputs, FlagStyle::Selection).1 .0 }

#[test]
fn a_measured_selection_has_one_level_on_the_near_step_or_the_records_step() {
    let mut world = world();
    assert_eq!(levels(&mut world, &selection(1, 0, 0)), LimitLevels { first: 200, second: None });
    assert_eq!(levels(&mut world, &FlagInputs { profile: 6, ..selection(46, 0, 0) }).first, 600);
}

#[test]
fn an_unmeasured_selection_holds_its_limit_less_the_offset_and_has_a_second_level() {
    let mut world = world();
    world.property(Property(0x6580), 3);
    let inputs = FlagInputs { limit_offset: 10, ..unmeasured(&mut world, 0) };
    assert_eq!(levels(&mut world, &inputs), LimitLevels { first: 180, second: Some(210) });
    let inputs = FlagInputs { limit_offset: 40, ..inputs };
    assert_eq!(levels(&mut world, &inputs), LimitLevels { first: 200, second: Some(230) });
}

#[test]
fn scenes_17_and_30_raise_the_second_level_by_their_own_setting() {
    let mut world = world();
    world.property(Property(0x6580), 3);
    world.property(Property(0x6581), 5);
    world.property(Property(0x6582), 7);
    for (scene, limit, second) in [(17, 0x68e6, 220), (30, 0x68f4, 240)] {
        world.property_word(Property(limit), 30);
        let inputs = FlagInputs { unmeasured: true, ..selection(scene, 0, 0) };
        assert_eq!(levels(&mut world, &inputs), LimitLevels { first: 170, second: Some(second) });
    }
}

#[test]
fn an_auxiliary_byte_leaves_no_second_level() {
    let mut world = world();
    let inputs = FlagInputs { auxiliary: 1, ..unmeasured(&mut world, 0) };
    assert_eq!(levels(&mut world, &inputs).second, None);
}

#[test]
fn each_style_flags_a_position_below_and_at_the_first_level() {
    let mut world = world();
    for (style, below, at) in [(0, 0x01, 0x00), (1, 0x81, 0x00), (2, 0x01, 0x02), (3, 0x81, 0x82), (4, 0x04, 0x02), (5, 0x84, 0x82), (6, 0x00, 0x00), (7, 0x41, 0x42)] {
        assert_eq!(flags(&mut world, &selection(1, style, 199)), below, "style {style} below");
        assert_eq!(flags(&mut world, &selection(1, style, 200)), at, "style {style} at");
    }
}

#[test]
fn styles_2_3_and_7_flag_the_own_lower_level_below_scenes_45_and_55() {
    let mut world = world();
    for scene in [45, 55] {
        assert_eq!(flags(&mut world, &selection(scene, 2, 199)), LimitFlags::OWN_LOWER);
    }
}

#[test]
fn an_armed_selection_lowers_scenes_11_to_15_and_raises_the_listed_scenes() {
    let mut world = world();
    let armed = |scene, style, position| FlagInputs { armed: true, ..selection(scene, style, position) };
    assert_eq!(flags(&mut world, &armed(11, 0, 199)), LimitFlags::OWN_LOWER);
    assert_eq!(flags(&mut world, &selection(11, 0, 199)), LimitFlags::BELOW);
    assert_eq!(flags(&mut world, &armed(14, 2, 199)), LimitFlags::OWN_LOWER);
    assert_eq!(flags(&mut world, &armed(13, 0, 199)), LimitFlags::BELOW);
    assert_eq!(flags(&mut world, &armed(4, 0, 200)), LimitFlags::AT_OR_ABOVE);
    assert_eq!(flags(&mut world, &selection(4, 0, 200)), 0);
}

#[test]
fn plain_flags_ignore_the_scene_style_and_armed_byte() {
    let mut world = world();
    let inputs = FlagInputs { armed: true, ..selection(14, 7, 199) };
    assert_eq!(limit_levels(&mut world, &inputs, FlagStyle::Plain).1, LimitFlags(LimitFlags::BELOW));
    assert_eq!(limit_levels(&mut world, &FlagInputs { positions: Positions { position: 200, spread: Spread::default() }, ..inputs }, FlagStyle::Plain).1, LimitFlags(0));
}

#[test]
fn a_configured_spread_past_its_margin_is_flagged() {
    let mut world = world();
    let spread = |position| FlagInputs { positions: Positions { position, spread: Spread::Configured { from: 100, to: 250 } }, ..selection(1, 0, 0) };
    world.property(Property(0x657e), 10);
    assert_eq!(flags(&mut world, &spread(200)), LimitFlags::SPREAD);
    world.property(Property(0x657e), 15);
    assert_eq!(flags(&mut world, &spread(200)), 0);
    assert_eq!(flags(&mut world, &spread(199)), LimitFlags::BELOW);
}

#[test]
fn a_second_level_above_the_position_holds_the_spread_to_its_own_margin() {
    let mut world = world();
    world.property(Property(0x657e), 1);
    world.property(Property(0x657f), 20);
    let base = unmeasured(&mut world, 0);
    let spread = |position| FlagInputs { positions: Positions { position, spread: Spread::Configured { from: 100, to: 250 } }, ..base };
    assert_eq!(levels(&mut world, &spread(0)), LimitLevels { first: 170, second: Some(170) });
    assert_eq!(flags(&mut world, &spread(170)), LimitFlags::SPREAD);
    world.property(Property(0x6580), 5);
    assert_eq!(flags(&mut world, &spread(170)), 0);
}

#[test]
fn a_bounded_spread_ends_at_the_position_when_configured() {
    let mut world = world();
    world.property(Property(0x746c), 20);
    let bounded = FlagInputs { positions: Positions { position: 250, spread: Spread::Bounded { from: 100, to: 400 } }, ..selection(1, 0, 0) };
    assert_eq!(flags(&mut world, &bounded), LimitFlags::SPREAD);
    world.property(Property(0x746b), 1);
    assert_eq!(flags(&mut world, &bounded), 0);
}

#[test]
fn the_limit_flags_are_off_while_configured_off_or_the_selection_is_disabled() {
    let mut world = world();
    let below = selection(1, 0, 199);
    assert_eq!(limit_flags(&mut world, &below), LimitFlags(LimitFlags::BELOW));
    assert_eq!(limit_flags(&mut world, &FlagInputs { auxiliary: 2, ..below }), LimitFlags(0));
    assert_eq!(limit_flags(&mut world, &FlagInputs { measurement_selection: MeasurementSelection::Record, ..below }), LimitFlags(0));
    assert_eq!(limit_flags(&mut world, &FlagInputs { readiness: Readiness::NotReady, ..below }), LimitFlags(0));
    world.device_ready = false;
    assert_eq!(limit_flags(&mut world, &below), LimitFlags(0));
    assert_eq!(limit_flags(&mut world, &FlagInputs { readiness: Readiness::Ready, ..below }), LimitFlags(LimitFlags::BELOW));
    let styled = FlagInputs { style: 7, auxiliary: 2, measurement_selection: MeasurementSelection::Record, ..below };
    assert_eq!(limit_flags(&mut world, &styled), LimitFlags(0x41));
    world.property(Property(0xa8), 8);
    assert_eq!(limit_flags(&mut world, &styled), LimitFlags(0));
}

#[test]
fn limit_index_2_drops_a_spread_but_not_a_position_below() {
    let mut world = world();
    world.property(Property(0x657e), 10);
    let spread = FlagInputs { index: 2, positions: Positions { position: 200, spread: Spread::Configured { from: 100, to: 250 } }, ..selection(1, 0, 0) };
    assert_eq!(limit_flags(&mut world, &FlagInputs { index: NO_INDEX, ..spread }), LimitFlags(LimitFlags::SPREAD));
    assert_eq!(limit_flags(&mut world, &spread), LimitFlags(0));
    assert_eq!(limit_flags(&mut world, &FlagInputs { index: 2, ..selection(1, 0, 199) }), LimitFlags(LimitFlags::BELOW));
}

#[test]
fn a_mode_reads_its_cell_from_row_seven_by_rate() {
    let mut bytes = [0; CELL_TABLE_BYTES];
    bytes[392..394].copy_from_slice(&[3, 4]);
    bytes[344..346].copy_from_slice(&[5, 6]);
    let table = CellTable(bytes);
    assert_eq!(table.mode_cell(1), Cell { column: 3, row: 4 });
    assert_eq!(table.mode_cell(5), Cell { column: 5, row: 6 });
}

/// Scene 1 measured at 300, its positions on a base of 50 less 30.
fn mode_world(first_position: i16) -> World {
    let mut world = world();
    world.header(SharedHeaderByte::At31, 1);
    world.set(&group_00::AT_0022, 300);
    world.set(&group_00::AT_001C, 30);
    world.set_shared(&group_41::AT_0972, 50);
    world.set_shared(&group_41::AT_096C, first_position);
    world
}

fn mode_step_of(world: &mut World) -> u32 {
    let cells = CellTable([0; CELL_TABLE_BYTES]);
    mode_step(world, &ModeInputs { record: handle(), rate: 4, cells: &cells, lens_reference: 0 })
}

#[test]
fn a_mode_takes_the_farthest_step_its_first_position_reaches() {
    assert_eq!(mode_step_of(&mut mode_world(480)), 5);
    assert_eq!(mode_step_of(&mut mode_world(730)), 7);
    assert_eq!(mode_step_of(&mut mode_world(2000)), 9);
}

#[test]
fn an_enabled_scene_adjustment_reaches_from_the_measured_levels_step() {
    let mut world = mode_world(730);
    world.property(Property(0x6594), 1);
    assert_eq!(mode_step_of(&mut world), 8);
}
