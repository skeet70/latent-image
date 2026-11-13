use xm1_saclac_shared::settings::{Channel, Channels};

use crate::channel::*;
use crate::curve::curve_value;
use crate::fake::Area;
use crate::numeric::FloatLog;

#[test]
fn the_channel_count_is_the_adjustment_byte_between_one_and_one_hundred() {
    for (byte, count) in [(0, 1), (1, 1), (7, 7), (100, 100), (200, 100)] {
        assert_eq!(super::channel::count(&Area::default().with(0x3e0f80, byte)), count, "{byte}");
    }
}

#[test]
fn channels_outside_the_count_have_no_base_and_no_packed_value() {
    let area = Area::default().with(0x3e0f80, 2).with_half(0x3e21fd + 2, 37).with(0x3e2007 + 3, 1).with(0x3e2007 + 5, 2);
    assert_eq!(base(&area, Channel(1)), 370);
    assert_eq!(base(&area, Channel(2)), 0);
    assert_eq!(base(&area, Channel(-1)), 0);
    assert_eq!(packed(&area, Channel(1)), 0x02_0001);
    assert_eq!(packed(&area, Channel(2)), 0xffff);
    assert!(is_valid(&area, Channel(1)));
    assert!(!is_valid(&area, Channel(2)));
}

#[test]
fn the_shared_layer_sees_the_channels_below_the_configured_count() {
    let area = Area::default().with(0x3e0f80, 3);
    let channels = ConfiguredChannels(&area);
    assert!(channels.is_valid(Channel(2)));
    assert!(!channels.is_valid(Channel(3)));
    assert!(!channels.is_valid(Channel(-1)));
}

#[test]
fn the_packed_value_scales_unless_the_denominator_is_zero() {
    let area = Area::default().with(0x3e0f80, 1).with(0x3e2007, 100).with_half(0x9287, 3);
    assert_eq!(scaled_packed(&area, Channel(0)), 100);
    assert_eq!(scaled_packed(&area.with_half(0x9285, 4), Channel(0)), 75);
}

#[test]
fn a_bias_adds_the_rounded_factor_term_only_when_scale_and_divisor_are_set() {
    let area = Area::default().with_half(0x3e0317 + 4, 50).with_half(0x3e03e0 + 4, 70).with(0x1fc, 3);
    assert_eq!(bias(&area, Channel(2), 0), 50);
    assert_eq!(bias(&area, Channel(2), ALTERNATE_BIAS), 70);
    let area = area.with_half(0x3e2640, 400).with_half(0x3e1b00 + 44, 100);
    // -(3,000,000 / 40,000) = -75, which is -7.5 tenths, rounded to -8.
    assert_eq!(bias(&area, Channel(2), 0), 42);
    assert_eq!(biased_step(&area.with_half(0x3e0572 + 4, 1000), Channel(2), 0), 1042);
}

#[test]
fn classes_split_at_the_three_bounds() {
    let area = Area::default().with(0x3e1ab1, 2).with(0x3e1ab2, 4).with(0x3e1ab3, 6);
    let classes: Vec<u32> = (0..8).map(|channel| channel_class(&area, Channel(channel))).collect();
    assert_eq!(classes, [0, 0, 1, 1, 2, 2, 3, 3]);
    let area = Area::default().with_half(0x3e1ab5, 100).with_half(0x3e1ab7, 200).with_half(0x3e1ab9, 300);
    let classes: Vec<u32> = [0, 100, 101, 200, 201, 300, 301].iter().map(|&value| value_class(&area, value)).collect();
    assert_eq!(classes, [0, 0, 1, 1, 2, 2, 3]);
}

struct NaturalLog;

impl FloatLog for NaturalLog {
    fn ln(&mut self, value: f32) -> f32 { value.ln() }
}

#[test]
fn a_level_is_a_hundred_log2_steps_of_the_base_over_one_hundred() {
    let area = Area::default().with(0x3e0f80, 2).with_half(0x3e21fd, 30).with_half(0x3e21ff, 10);
    assert_eq!(level(&area, &mut NaturalLog, Channel(0), 0, false), 158, "log2 3 is 1.58");
    assert_eq!(level(&area, &mut NaturalLog, Channel(1), 0, false), 0);
    assert_eq!(level(&area, &mut NaturalLog, Channel(5), 0, false), 500, "no base");
    let area = area.with(0x3e1ab1, 1).with(0x3e1ab2, 5).with_half(0x3e1ab5, 10).with_half(0x3e1ab7, 20);
    let area = area.with(0x3e1aa0 + 4 + 1, 3);
    assert_eq!(level(&area, &mut NaturalLog, Channel(1), 11, true), -30, "classes (1, 1) select offset byte 3");
}

#[test]
fn the_first_reaching_byte_search_stops_one_before_the_count() {
    let area = Area::default().with(0x100, 5).with(0x101, 9).with(0x102, 12);
    assert_eq!(first_reaching(&area, 9, 0x100, 4), 1);
    assert_eq!(first_reaching(&area, 12, 0x100, 3), 2, "index 2 is never read");
    assert_eq!(first_reaching(&area, 0, 0x100, 1), 0);
}

#[test]
fn a_curve_value_interpolates_the_summed_points_between_levels() {
    // Row 0's levels are 0, 10, .., 60; level 2's point at index 10 is 4 and level 0's is 0, so
    // odd level 1 interpolates to 2.
    let area = (1..=6).fold(Area::default(), |area, level| area.with_half(0x3f0000 + 2 * (level - 1), 10 * level as u16));
    let area = area.with(0x3e2712, 1).with(0x3f0fa7 + 6, 4);
    assert_eq!(curve_value(&area, 0, 10, 15), 3, "halfway from 2 to 4");
    assert_eq!(curve_value(&area, 0, 11, 15), 0, "no points past index 10");
    assert_eq!(curve_value(&area, 0, 10, 20), 4);
}
