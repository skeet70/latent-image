use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::exposure_limits::{LimitQuery, LimitSet};
use crate::exposure::context::ExposureContext;
use crate::exposure::fakes::{handle, World};
use crate::exposure::grid::*;
use crate::exposure::ports::Property;
use crate::stock::calculation::Cell;
use crate::stock::property;

const LENS_REFERENCE: u32 = 0x40;

/// The metered limit is 30 tenths and the lens limit -20; step 3 of the level table is 100 and
/// the lens reference's offset 5.
fn world() -> World {
    let mut world = World::default();
    world.property(property::METERED_DISTANCE_LIMIT, 3);
    world.property(property::LENS_DISTANCE_LIMIT, 0xfe);
    world.property_word(property::level_step(3), 100);
    world.reference_offsets.insert(LENS_REFERENCE, 5);
    world.lens_levels.insert((7, LENS_REFERENCE), 40);
    world
}

fn context(metered_level: i16) -> ExposureContext {
    let mut context = ExposureContext::default();
    context.metered_level = metered_level;
    context
}

/// An area of lens 7 whose lens distance is -30.
fn area(at44: i16) -> AreaReading { AreaReading { at00: 10, reference: 7, level: 20, at44, at48: 20 } }

#[test]
fn the_metered_distance_counts_step_three_and_the_lens_references_offset() {
    let distances = area_distances(&mut world(), &context(-60), LENS_REFERENCE, &area(20));
    assert_eq!((distances.metered, distances.lens), (25, -30));
    assert_eq!((distances.metered_limit, distances.lens_limit), (30, -20));
    assert_eq!((distances.class, distances.code()), (Some(DistanceClass::Within), 1));
}

#[test]
fn a_metered_distance_past_its_limit_classes_the_area_before_its_lens_distance() {
    let mut world = world();
    let at_limit = area_distances(&mut world, &context(-55), LENS_REFERENCE, &area(20));
    assert_eq!((at_limit.metered, at_limit.class), (30, Some(DistanceClass::Within)));
    world.lens_levels.insert((7, LENS_REFERENCE), 0);
    let beyond = area_distances(&mut world, &context(-54), LENS_REFERENCE, &area(20));
    assert_eq!((beyond.lens, beyond.class, beyond.code()), (10, Some(DistanceClass::BeyondMetered), 2));
}

#[test]
fn a_lens_distance_past_its_signed_limit_classes_the_area() {
    let mut world = world();
    world.lens_levels.insert((7, LENS_REFERENCE), 29);
    let distances = area_distances(&mut world, &context(-60), LENS_REFERENCE, &area(20));
    assert_eq!((distances.lens, distances.class, distances.code()), (-19, Some(DistanceClass::BeyondLens), 3));
}

#[test]
fn a_row_left_unflagged_keeps_no_class_and_zeros() {
    let unflagged = AreaDistances::default();
    assert_eq!((unflagged.code(), unflagged.metered, unflagged.lens, unflagged.metered_limit, unflagged.lens_limit), (0, 0, 0, 0, 0));
}

#[test]
fn the_target_reference_is_the_near_step_of_the_records_scene_limits() {
    let mut world = World::default();
    let near_limit = |scene| crate::stock::exposure_limits::near_limit(scene, false, false, false).unwrap();
    world.property_word(near_limit(0), 3);
    world.property_word(near_limit(48), 5);
    world.near_steps.insert(30, 6);
    world.near_steps.insert(50, 8);
    assert_eq!(target_reference(&mut world, &ExposureContext::default(), handle(), Cell::default(), LENS_REFERENCE), 6);
    world.header(SharedHeaderByte::At31, 48);
    assert_eq!(target_reference(&mut world, &ExposureContext::default(), handle(), Cell::default(), LENS_REFERENCE), 8);
}

#[test]
fn the_target_level_is_the_selections_clamped_mode_limit() {
    let mut world = World::default();
    world.property_word(Property(0x68c6), 100);
    world.property_word(Property(0x68c4), 111);
    let limit = LimitQuery { index: 4, policy: 0, armed: false, set: LimitSet::Second, choice: 0, minimum: 0 };
    let query = LevelQuery { scene: 0, flags: 0, alternate: true, variant: true, limit };
    assert_eq!(target_level(&mut world, &query), 100);
    assert_eq!(target_level(&mut world, &LevelQuery { limit: LimitQuery { armed: true, ..limit }, ..query }), 111);
}
