use std::collections::BTreeMap;

use crate::adjust::peak::*;
use crate::exposure::fakes::World;
use crate::exposure::ports::{Properties, Property};
use crate::stock::rate_curve;

/// Configuration settings and a logarithm table by value.
#[derive(Default)]
struct Fits {
    world: World,
    logs: BTreeMap<u64, u32>,
}

impl Properties for Fits {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl Logarithms for Fits {
    fn centi_log2(&mut self, value: u64) -> Option<u32> { self.logs.get(&value).copied() }
}

fn fits(five_point: bool, parabola_sharpness: u32, fit_sharpness: u32) -> Fits {
    let mut fits = Fits::default();
    fits.world.property(rate_curve::FIVE_POINT_FIT, u32::from(five_point));
    fits.world.property(rate_curve::PARABOLA_SHARPNESS, parabola_sharpness);
    fits.world.property(rate_curve::FIT_SHARPNESS, fit_sharpness);
    fits
}

#[test]
fn a_middle_sample_between_its_neighbours_keeps_its_position() {
    let mut fits = fits(false, 255, 255);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([60, 80, 100], [100, 200, 300])), 200);
}

#[test]
fn three_samples_take_the_vertex_of_their_parabola() {
    let mut fits = fits(false, 255, 255);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([80, 100, 80], [100, 200, 300])), 200);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([60, 100, 80], [100, 200, 300])), 217);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([60, 100, 80], [300, 200, 100])), 183);
}

#[test]
fn a_sharp_peak_takes_the_crossing_of_its_slopes() {
    let mut fits = fits(false, 0, 255);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([60, 100, 80], [100, 200, 300])), 225);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([60, 100, 80], [300, 200, 100])), 175);
}

#[test]
fn positions_that_turn_keep_the_middle_position() {
    let mut fits = fits(false, 255, 255);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([60, 100, 80], [100, 200, 150])), 200);
}

fn logs(fits: &mut Fits, pairs: &[(u64, u32)]) {
    for &(value, log) in pairs {
        fits.logs.insert(value, log);
    }
}

#[test]
fn the_five_point_fit_takes_the_parabola_through_the_logarithms() {
    let mut fits = fits(true, 255, 255);
    logs(&mut fits, &[(1000, 300), (2000, 400), (1500, 350)]);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([1000, 2000, 1500], [100, 200, 300])), 217);
}

#[test]
fn wide_steps_are_rescaled_before_the_fit() {
    let mut fits = fits(true, 255, 255);
    logs(&mut fits, &[(1000, 300), (2000, 400), (1500, 350)]);
    assert_eq!(peak_position(&mut fits, &PeakCurve::from_three([1000, 2000, 1500], [0, 1000, 2000])), 1167);
}

#[test]
fn four_samples_take_the_least_squares_parabola() {
    let mut fits = fits(true, 255, 255);
    let values = [744, 964, 984, 804, 424];
    logs(&mut fits, &values.map(|value| (value as u64, value as u32)));
    let curve = PeakCurve { points: 4, values, positions: [100, 200, 300, 400, 500] };
    assert_eq!(peak_position(&mut fits, &curve), 260);
}

#[test]
fn a_curve_with_no_points_or_a_negative_sample_falls_back() {
    let mut fits = fits(true, 255, 255);
    let curve = PeakCurve { points: 0, ..PeakCurve::from_three([60, 100, 80], [100, 200, 300]) };
    assert_eq!(peak_position(&mut fits, &curve), 200);
    let negative = PeakCurve { values: [-5, 60, 100, 80, 80], ..PeakCurve::from_three([60, 100, 80], [100, 200, 300]) };
    assert_eq!(peak_position(&mut fits, &negative), 217);
}
