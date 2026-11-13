use crate::exposure::fakes::World;
use crate::exposure::ports::{Properties, Property};
use crate::scene_levels::*;
use crate::stock::scene_levels as stock;
use crate::stock::shared::level_curve;

const PROFILE: u32 = 9;

struct Bench {
    world: World,
    feature_class: u8,
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl FeatureClasses for Bench {
    fn feature_class(&mut self, profile: u32) -> u8 { if profile == PROFILE { self.feature_class } else { 0 } }
}

fn limits(class: u8) -> Bench {
    let mut bench = Bench { world: World::default(), feature_class: class };
    for (table, class_three) in [(0, false), (1000, true)] {
        for band in 0..7u8 {
            for column in 0..4u8 {
                bench.world.property_word(stock::response_limit(class_three, band, column), table + 10 * u16::from(band) + u16::from(column));
            }
        }
    }
    bench
}

#[test]
fn response_limits_are_read_by_level_band_and_reversed_index() {
    let mut bench = limits(1);
    let at = |bench: &mut Bench, level, index| response_limit(bench, PROFILE, level, index);
    assert_eq!([at(&mut bench, 100, 0), at(&mut bench, 99, 1), at(&mut bench, 0, 2), at(&mut bench, -1, 3), at(&mut bench, -100, 7)], [3, 12, 11, 20, 20]);
    assert_eq!([at(&mut bench, -101, 0), at(&mut bench, -201, 0), at(&mut bench, -301, 0), at(&mut bench, -400, 0), at(&mut bench, -401, 0)], [33, 43, 53, 53, 63]);
}

#[test]
fn feature_class_3_reads_its_own_table_and_limits_are_signed() {
    let mut bench = limits(3);
    assert_eq!(response_limit(&mut bench, PROFILE, 150, 3), 1000);
    bench.world.property_word(stock::response_limit(true, 0, 0), 0xff38);
    assert_eq!(response_limit(&mut bench, PROFILE, 150, 3), -200);
}

fn lens(base: i16) -> World {
    let mut world = World::default();
    world.lens_base_levels.insert(4, base);
    world
}

#[test]
fn the_excess_subtracts_the_placement_and_the_step_base_and_clamps_to_200() {
    let mut world = lens(30);
    assert_eq!(excess(&mut world, 500, 400, 4, 10), 80);
    assert_eq!(excess(&mut world, 500, 100, 4, 10), 200);
    assert_eq!(excess(&mut world, -500, 100, 4, 10), -200);
    assert_eq!(excess(&mut world, 32000, -32000, 4, 0), -200, "the difference wraps to 16 bits before the clamp");
}

#[test]
fn only_placed_scenes_47_and_56_have_an_excess() {
    let mut world = lens(0);
    assert_eq!([47, 56, 46].map(|scene| scene_excess(&mut world, scene, 50, Some(20), 4, 0)), [30, 30, 0]);
    assert_eq!(scene_excess(&mut world, 47, 50, None, 4, 0), 0);
}

#[test]
fn a_curve_point_is_moved_by_the_difference_and_points_run_on_across_curves() {
    let mut world = World::default();
    world.put_u16(level_curve(0, 5), 0xfffe);
    world.put_u16(level_curve(1, 3), 40);
    assert_eq!(moved_curve_point(&mut world, 5, 10, 25), 13);
    assert_eq!(moved_curve_point(&mut world, 259, 0, 0), 40);
}

#[test]
fn the_configured_rate_is_three_bytes_scaled_by_the_area() {
    let mut world = World::default();
    for (byte, value) in stock::CONFIGURED_RATE.into_iter().zip([0x34, 0x12, 0x01]) {
        world.property(byte, value);
    }
    let whole = 0x0001_1234;
    assert_eq!(configured_rate(&mut world, 3, 2), ConfiguredRate { whole: 6 * whole, tenths: 6 * (whole / 10), doubled_tenths: 12 * (whole / 10) });
}
