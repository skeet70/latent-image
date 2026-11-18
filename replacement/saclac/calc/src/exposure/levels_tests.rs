use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::MeasurementSelection;

use crate::exposure::fakes::{handle, World};
use crate::exposure::levels;
use crate::stock::property;

fn world() -> World {
    let mut world = World::default();
    world.property_word(property::level_step(2), -50i16 as u16);
    world.property_word(property::level_step(3), 300);
    world.property_word(property::TARGET_LEVEL, 640);
    world.set(&group_00::AT_0022, 420);
    world.set(&group_00::AT_0045, 6);
    world.level_offsets.insert(1, 100);
    world
}

#[test]
fn the_record_level_starts_above_the_capture_profile_offset() {
    let mut world = world();
    let levels = levels::start(&mut world, handle(), false);
    assert_eq!((levels.target, levels.measured, levels.unmeasured), (640, 420, false));
    assert_eq!((levels.curve, levels.start, levels.policy), (1, 420, 6));
}

#[test]
fn the_adjustment_path_targets_step_three() {
    let mut world = world();
    assert_eq!(levels::start(&mut world, handle(), true).target, 300);
}

#[test]
fn an_unmeasured_record_starts_from_step_three_and_never_below_the_floor() {
    let mut world = world();
    world.set(&group_00::AT_0022, 0);
    world.level_offsets.insert(1, 380);
    let levels = levels::start(&mut world, handle(), false);
    assert!(levels.unmeasured);
    assert_eq!((levels.measured, levels.start), (0, 380 - 50));
}

#[test]
fn a_record_that_selects_its_own_measurement_measures_at_step_two() {
    let mut world = world();
    world.select(MeasurementSelection::Record);
    world.level_offsets.insert(1, -100);
    let levels = levels::start(&mut world, handle(), false);
    assert_eq!(levels.measured, -50i16 as u16);
    assert_eq!(levels.start, -50);
}

#[test]
fn the_configured_measurement_replaces_the_record_level_and_profile() {
    let mut world = world();
    world.property(property::ADJUSTMENT_FLAGS, 1);
    world.configured_level = 500;
    world.configured_profiles.insert(500, 4);
    world.level_offsets.insert(4, 20);
    let levels = levels::start(&mut world, handle(), false);
    assert_eq!((levels.measured, levels.curve, levels.start), (500, 4, 500));
}
