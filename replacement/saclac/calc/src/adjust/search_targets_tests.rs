use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::fakes::Lens;
use crate::adjust::search_targets::*;
use crate::exposure::fakes::{handle, World};
use crate::stock::adjust as stock;

#[test]
fn the_minimum_step_is_the_larger_of_the_setting_and_the_floor() {
    let mut world = World::default();
    world.property(stock::MINIMUM_STEP, 30);
    assert_eq!((minimum_step(&mut world, 20), minimum_step(&mut world, 40)), (30, 40));
}

/// Factor 50, base channel 1 of value 4, duration 300; channel 2 of value 8, curve 3 and area bias 5.
fn target_setup() -> (World, Lens) {
    let mut world = World::default();
    world.property(stock::TARGET_FACTOR, 50);
    world.property(stock::TARGET_BASE_CHANNEL, 1);
    world.property_word(stock::TARGET_DURATION, 300);
    let lens = Lens { values: [(1, 4), (2, 8)].into(), curves: [(2, 3)].into(), biases: [((2, 0), 5)].into(), ..Lens::default() };
    (world, lens)
}

#[test]
fn the_drive_target_scales_the_amount_through_the_channel() {
    let (mut world, mut lens) = target_setup();
    let mut resources = Lens { flags: 0x40, reference: 7, ..Lens::default() };
    let rate = (1000 * 50 / 100) * (4 * 100) / 8;
    let reach = (300 * rate / 10_000) * (8 * 8);
    let offset = (reach / (3 * 10_000) + 5) as i16;
    let target = drive_target(&mut world, &mut lens, &mut resources, 2, 1000, -100, 1000);
    assert_eq!(lens.selections, [(2, 0x40, offset * 2)]);
    assert_eq!(target, offset * 2 + 7);
}

#[test]
fn the_drive_target_is_held_within_its_bounds() {
    let (mut world, mut lens) = target_setup();
    let mut resources = Lens { reference: 7, ..Lens::default() };
    assert_eq!(drive_target(&mut world, &mut lens, &mut resources, 2, 1000, 500, 1000), 500);
    assert_eq!(drive_target(&mut world, &mut lens, &mut resources, 2, 1000, -100, 10), 10);
}

#[test]
fn a_channel_without_a_value_or_curve_gives_only_its_bias() {
    let (mut world, mut lens) = target_setup();
    lens.values.remove(&2);
    let mut resources = Lens::default();
    assert_eq!(drive_target(&mut world, &mut lens, &mut resources, 2, 1000, -100, 1000), 10);
}

#[test]
fn the_threshold_adds_the_share_and_the_margin() {
    let mut world = World::default();
    world.property(stock::THRESHOLD_MARGIN, 2);
    assert!(below_threshold(&mut world, 151, 50, 100, 60));
    assert!(!below_threshold(&mut world, 150, 50, 100, 60));
}

#[test]
fn a_fall_counts_the_steps_back_to_the_peak() {
    assert_eq!(fall(&[1, 9, 7, 4]), Some(Fall { steps: 2, peak: 9, depth: 5 }));
    assert_eq!(fall(&[9, 7, 4]), Some(Fall { steps: 2, peak: 9, depth: 5 }));
    assert_eq!(fall(&[4, 7]), None);
    assert_eq!(fall(&[4]), None);
    assert_eq!(fall(&[]), None);
}

#[test]
fn a_long_enough_fall_passes_the_peak_whatever_its_depth() {
    let mut world = World::default();
    world.property(stock::PEAK_LENGTH, 2);
    world.property(stock::PEAK_LENGTH_MODE_ONE, 3);
    world.property(stock::THRESHOLD_MARGIN, 100);
    assert_eq!(passed_peak(&mut world, &[1, 9, 8, 7], 0, false), (true, Some(9)));
    assert_eq!(passed_peak(&mut world, &[1, 9, 8, 7], 0, true), (false, Some(9)));
    assert_eq!(passed_peak(&mut world, &[1, 9, 10], 0, false), (false, None));
}

#[test]
fn a_short_fall_passes_when_deeper_than_its_share_and_margin() {
    let mut world = World::default();
    world.property(stock::PEAK_LENGTH, 5);
    world.property(stock::THRESHOLD_MARGIN, 1);
    assert!(passed_peak(&mut world, &[1000, 960], 3, false).0);
    assert!(!passed_peak(&mut world, &[1000, 961], 3, false).0);
}

#[test]
fn a_fall_from_the_peak_needs_both_length_and_depth() {
    let mut world = World::default();
    world.property(stock::FALL_LENGTH, 2);
    world.property(stock::FALL_DEPTH, 3);
    assert!(fell_from_peak(&mut world, &[500, 300, 200]));
    assert!(!fell_from_peak(&mut world, &[500, 300, 201]));
    assert!(!fell_from_peak(&mut world, &[300, 100]));
}

#[test]
fn the_inverse_reference_scale_reads_zero_as_one() {
    let mut lens = Lens::default();
    assert_eq!(inverse_reference_scale(&mut lens, 4, 5), 5_000_000);
    assert_eq!(inverse_reference_scale(&mut lens, 0, 5), 100_000_000);
}

#[test]
fn a_drive_leg_runs_past_one_end_by_the_margin() {
    let mut lens = Lens { position: 77, ..Lens::default() };
    assert_eq!(drive_leg(&mut lens, true, -50, 50, 10), DriveLeg { position: 77, limit: 60, direction: 0 });
    assert_eq!(drive_leg(&mut lens, false, -50, 50, 10), DriveLeg { position: 77, limit: -60, direction: 1 });
}

#[test]
fn narrowed_limits_move_inward() {
    let mut limits = [-50, 50];
    narrow(5, &mut limits);
    assert_eq!(limits, [-45, 45]);
}

#[test]
fn limits_are_clamped_with_the_margin_on_their_far_side() {
    let mut configuration = Lens { limits: [-100, 100], ..Lens::default() };
    let mut control = Lens { margin: 10, ..Lens::default() };
    let mut rising = [-200, 200];
    clamp_limits(&mut configuration, &mut control, 0, &mut rising);
    assert_eq!(rising, [-90, 100]);
    let mut falling = [200, -200];
    clamp_limits(&mut configuration, &mut control, 0, &mut falling);
    assert_eq!(falling, [90, -100]);
}

#[test]
fn zero_stays_zero_when_offset() {
    assert_eq!((offset_unless_zero(0, 5), offset_unless_zero(3, 5), offset_unless_zero(0xffff, 2)), (0, 8, 1));
}

#[test]
fn the_follow_word_carries_the_enable_in_bit_zero() {
    let mut world = World::default();
    world.header(SharedHeaderByte::At31, 12);
    world.set(&group_00::AT_002E, 0x30);
    assert_eq!(scene_and_follow(&mut world, handle(), true), (12, 0x31));
    world.set(&group_00::AT_002E, 0x31);
    assert_eq!(scene_and_follow(&mut world, handle(), false), (12, 0x30));
}

#[test]
fn the_bias_is_interpolated_toward_the_stepped_resource() {
    let mut world = World::default();
    world.property(stock::ascending_step(3), 2);
    let mut lens = Lens { count: 10, ..Lens::default() };
    lens.biases.insert((5, 2), 40);
    lens.biases.insert((3, 2), 10);
    lens.positions.insert((5, 2), 6);
    lens.positions.insert((3, 2), 3);
    assert_eq!(interpolated_bias(&mut world, &mut lens, 3, true, 30), (40 - 30 + 6 * (30 - 10) / 3) as i16);
}

#[test]
fn a_step_past_the_last_resource_moves_the_near_resource_back() {
    let mut world = World::default();
    world.property(stock::ascending_step(8), 2);
    let mut lens = Lens { count: 10, ..Lens::default() };
    lens.biases.insert((9, 2), 40);
    lens.positions.insert((7, 2), 1);
    lens.positions.insert((9, 2), 1);
    assert_eq!(interpolated_bias(&mut world, &mut lens, 8, true, 0), 40);
}

#[test]
fn a_step_below_the_first_resource_starts_from_the_step() {
    let mut world = World::default();
    world.property(stock::descending_step(1), 3);
    let mut lens = Lens::default();
    lens.biases.insert((0, 1), 25);
    lens.positions.insert((3, 1), 2);
    assert_eq!(interpolated_bias(&mut world, &mut lens, 1, false, 5), 20);
}

#[test]
fn a_zero_near_position_interpolates_nothing() {
    let mut world = World::default();
    world.property(stock::descending_step(4), 1);
    let mut lens = Lens::default();
    lens.biases.insert((3, 1), 25);
    lens.positions.insert((3, 1), 9);
    assert_eq!(interpolated_bias(&mut world, &mut lens, 4, false, 5), 20);
}

#[test]
fn listed_scenes_are_admitted_when_enabled() {
    let mut world = World::default();
    assert!(!scene_admitted(&mut world, true, 0, true));
    world.property(stock::SCENE_ADMISSION, 1);
    assert!(scene_admitted(&mut world, false, 57, true));
    assert!(!scene_admitted(&mut world, false, 5, true));
    assert!(scene_admitted(&mut world, true, 5, true));
    assert!(!scene_admitted(&mut world, true, 0, false));
}

#[test]
fn the_extent_threshold_follows_the_mapped_value_band() {
    let mut world = World::default();
    for (index, id) in stock::EXTENT_THRESHOLDS.into_iter().enumerate() {
        world.property(id, 10 + index as u32);
    }
    let mut lens = Lens::default();
    let mut threshold = |mapped| {
        lens.mapped = mapped;
        extent_threshold(&mut world, &mut lens, 0)
    };
    assert_eq!([threshold(399), threshold(400), threshold(559), threshold(560), threshold(799), threshold(800)], [10, 11, 11, 12, 12, 13]);
}
