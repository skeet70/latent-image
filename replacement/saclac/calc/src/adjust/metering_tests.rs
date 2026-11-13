use std::vec::Vec;

use crate::adjust::metering::*;
use crate::adjust::timing::LevelScale;
use crate::exposure::fakes::World;
use crate::stock::adjust as stock;

/// Weights green 256, red 0, blue 0; scales 128ths at 128; the difference is the value less the
/// reference.
struct Tables;

impl MeteringTables for Tables {
    fn channel_weights(&mut self) -> [u16; 3] { [256, 0, 0] }
    fn feature_bytes(&mut self, _profile: u32) -> [u8; 2] { [128, 128] }
}

impl LevelScale for Tables {
    fn linear(&mut self, _level: i16) -> u32 { 0 }
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16 { value as i16 - reference as i16 }
}

#[derive(Default)]
struct Trace(Vec<u32>);

impl DiagnosticTrace for Trace {
    fn trace(&mut self, point: u32) { self.0.push(point); }
}

fn uniform(green: u32) -> CellSums { CellSums { red: [0; CELLS], green: [green; CELLS], blue: [0; CELLS], second_green: [0; CELLS] } }

#[test]
fn the_mean_level_is_the_summed_luminance_against_the_configured_scale() {
    let mut world = World::default();
    let mut trace = Trace::default();
    let levels = metered_levels(&mut world, &mut Tables, &mut trace, &uniform(1), &MeteringInputs::default());
    assert_eq!(trace.0, [stock::METERING_START, stock::METERING_END]);
    assert_eq!(levels.mean, 3);
    assert_eq!(levels.weighted, 0);
}

#[test]
fn the_weighted_level_averages_the_luminance_by_zone_weight() {
    let mut world = World::default();
    world.property(stock::zone_weight(0), 4);
    let mut sums = uniform(1);
    sums.green[0] = 11;
    let levels = metered_levels(&mut world, &mut Tables, &mut Trace::default(), &sums, &MeteringInputs::default());
    let corner_weighted = (33 * 4 + 3 * 4 * 7) / (4 * 8);
    assert_eq!(stock::zone_weight(0), stock::zone_weight(17));
    assert_eq!(levels.weighted, corner_weighted);
}

#[test]
fn the_levels_add_the_lens_and_reference_offsets_less_the_profile_offset() {
    let mut world = World::default();
    world.lens_levels.insert((7, 3), 20);
    world.reference_offsets.insert(3, 5);
    world.level_offsets.insert(9, 2);
    let inputs = MeteringInputs { level: 100, offset: 10, profile: 9, lens: 7, reference: 3, ..MeteringInputs::default() };
    let levels = metered_levels(&mut world, &mut Tables, &mut Trace::default(), &uniform(1), &inputs);
    assert_eq!(levels.mean, 100 - 10 + 20 - 2 + 5 + 3);
}

#[test]
fn a_lens_level_is_clamped_to_its_bounds_with_a_floor_of_440() {
    let mut world = World::default();
    world.property(stock::lens_level_bounds(false)[0], 60);
    world.property(stock::EXCESS_START, 200);
    let level = |world: &mut World, value: i32| lens_level(world, 7, 3, 0, value, false).level;
    assert_eq!([level(&mut world, 300), level(&mut world, 500), level(&mut world, 700)], [440, 500, 600]);
}

#[test]
fn the_excess_over_the_start_is_taken_up_to_one_step() {
    let mut world = World::default();
    world.property(stock::EXCESS_START, 50);
    world.property(stock::EXCESS_STEP, 3);
    world.property(stock::lens_level_bounds(true)[0], 200);
    world.property(stock::lens_level_bounds(true)[1], 45);
    world.level_offsets.insert(stock::LENS_LEVEL_PROFILE, 4);
    let at = |world: &mut World, value: i32| lens_level(world, 7, 3, 1, value, true);
    assert_eq!(at(&mut world, 500), LensLevel { lens: 7, level: 505, unclipped: 500, profile: 8 });
    assert_eq!(at(&mut world, 520).unclipped, 500);
    assert_eq!(at(&mut world, 600).unclipped, 570);
}
