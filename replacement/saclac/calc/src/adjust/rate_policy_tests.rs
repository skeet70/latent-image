use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_22;

use crate::adjust::fakes::Lens;
use crate::adjust::rate_policy::*;
use crate::exposure::fakes::{handle, World};
use crate::exposure::ports::Property;
use crate::stock::adjust as stock;

#[test]
fn the_configured_factor_is_three_bytes_high_first() {
    let mut world = World::default();
    for (id, value) in stock::CONFIGURED_FACTOR.into_iter().zip([0x01, 0x02, 0x03]) {
        world.property(id, value);
    }
    assert_eq!(scaled_by_configured_factor(&mut world, 2), 2 * 0x01_0203);
}

#[test]
fn rate_kinds_map_to_codes_one_higher() {
    let codes: Vec<u32> = (0..6).map(|kind| RateKind::new(kind).code()).collect();
    assert_eq!(codes, [1, 2, 3, 4, 5, 1]);
    let kinds: Vec<u8> = (0..7).map(|code| RateKind::from_code(code).kind()).collect();
    assert_eq!(kinds, [0, 0, 1, 2, 3, 4, 0]);
}

/// Factor 50, channel 0 of value 4, channel 2 of value 8, steps 5 and span 3.
fn rate_world() -> (World, Lens) {
    let mut world = World::default();
    world.property(stock::TARGET_FACTOR, 50);
    world.property(stock::RATE_STEPS, 5);
    world.property(stock::RATE_SPAN, 3);
    (world, Lens { values: [(0, 4), (2, 8)].into(), ..Lens::default() })
}

#[test]
fn the_drive_rate_scales_the_share_by_the_rounded_period() {
    let (mut world, mut lens) = rate_world();
    let share = 1000 * 50 / 100;
    let rate = 4 * 100 * share / 8;
    let period = (3 * 100_000) / (5 * 50);
    assert_eq!(period, 1200);
    assert_eq!(drive_rate(&mut world, &mut lens, false, 2, 1000), DriveRate { share, rate, scaled: rate * 120 / 10_000 });
}

#[test]
fn a_period_ending_in_five_rounds_away_from_zero() {
    let (mut world, mut lens) = rate_world();
    world.property(stock::RATE_SPAN, 1);
    world.property(stock::RATE_STEPS, 8);
    world.property(stock::TARGET_FACTOR, 100);
    let rate = 4 * 100 * 1000 / 8;
    assert_eq!(drive_rate(&mut world, &mut lens, false, 2, 1000).scaled, rate * 13 / 10_000);
}

#[test]
fn without_steps_the_period_is_ten_and_the_second_factor_applies_when_asked() {
    let (mut world, mut lens) = rate_world();
    world.property(stock::RATE_STEPS, 0);
    world.property(stock::RATE_SECOND_FACTOR, 10);
    let share = 1000 * 10 / 100;
    let rate = 4 * 100 * share / 8;
    assert_eq!(drive_rate(&mut world, &mut lens, true, 2, 1000), DriveRate { share, rate, scaled: rate / 10_000 });
    assert_eq!(drive_rate(&mut world, &mut lens, true, 3, 1000).rate, 0);
}

#[test]
fn the_selector_and_mode_choose_one_of_eight_policies() {
    let one: Vec<u8> = [22, 4, 6, 23, 15, 24, 5, 25, 16, 0].into_iter().map(|mode| rate_policy(true, mode)).collect();
    assert_eq!(one, [4, 4, 4, 5, 5, 6, 6, 7, 7, 2]);
    let other: Vec<u8> = [19, 2, 26, 17, 27, 22].into_iter().map(|mode| rate_policy(false, mode)).collect();
    assert_eq!(other, [1, 1, 1, 1, 1, 0]);
}

#[test]
fn the_policy_setting_is_read_from_its_group() {
    let mut world = World::default();
    world.property(stock::policy_setting(4, false), 7);
    world.property(stock::policy_setting(4, true), 9);
    assert_eq!((policy_setting(&mut world, true, 22, false), policy_setting(&mut world, true, 22, true)), (7, 9));
    assert_eq!(stock::policy_setting(4, true), Property(0x63b4));
}

#[test]
fn rate_settings_come_from_the_policy_row_in_tenths() {
    let mut world = World::default();
    let [byte, first, second] = stock::rate_settings(5, true, false);
    world.property(byte, 3);
    world.property(first, 12);
    world.property(second, 250);
    world.property(stock::RATE_BASE, 0x40);
    assert_eq!(rate_settings(&mut world, true, 23, true, false), RateSettings { byte: 3, first: 120, second: 2500, base: 0x40 });
    assert_eq!(stock::rate_settings(5, true, false), [Property(0x6412), Property(0x63e6), Property(0x63e5)]);
}

#[test]
fn the_level_selection_splits_at_one_hundred() {
    assert_eq!([level_selection(1, 100), level_selection(0, 100), level_selection(1, 99), level_selection(0, 99)], [0, 1, 2, 0]);
}

#[derive(Default)]
struct Curves(Vec<(i32, u8)>);

impl ProfileCurves for Curves {
    fn profile_curve(&mut self, profile: i32, class: u8) -> u32 {
        self.0.push((profile, class));
        u32::from(class) * 100
    }
}

#[test]
fn the_kind_bit_chooses_the_curve_class() {
    let mut curves = Curves::default();
    let classes: Vec<u32> = [(false, 2), (false, 31), (false, 3), (false, 0), (true, 0), (false, 40)].into_iter().map(|(forced, kind)| profile_curve(&mut curves, 7, forced, kind)).collect();
    assert_eq!(classes, [100, 200, 300, 0, 300, 0]);
    assert!(curves.0.iter().all(|(profile, _)| *profile == 7));
}

#[test]
fn the_policy_flag_follows_the_mode_or_the_sensitivity_class() {
    let mut world = World::default();
    world.property(stock::policy_flag_by_mode(6), 1);
    world.property(Property(0x82bc), 5);
    world.sensitivity_classes = BTreeMap::from([(3, 11), (4, 12)]);
    assert_eq!(policy_flag(&mut world, true, 6, 3), 1);
    assert_eq!(policy_flag(&mut world, false, 6, 3), 5);
    assert_eq!(policy_flag(&mut world, false, 6, 4), 0);
}

#[test]
fn an_unlocked_snapshot_of_kinds_eight_to_ten_rotates() {
    let mut world = World::default();
    let mut snapshot: [u8; 20] = core::array::from_fn(|index| index as u8 + 1);
    snapshot[16..].copy_from_slice(&[0; 4]);
    world.set_shared(&group_22::AT_0180, snapshot);
    world.set(&group_00::AT_0034, 9);
    rotate_snapshot(&mut world, handle());
    let rotated: [u8; 20] = world.records.read_shared(handle(), &group_22::AT_0180).unwrap();
    assert_eq!(&rotated[..12], &[7, 8, 9, 10, 11, 12, 7, 8, 9, 10, 11, 12]);

    let mut locked = snapshot;
    locked[16..].copy_from_slice(&[0; 4]);
    locked[19] = 1;
    world.set_shared(&group_22::AT_0180, locked);
    rotate_snapshot(&mut world, handle());
    assert_eq!(world.records.read_shared(handle(), &group_22::AT_0180).unwrap(), locked);
}

#[test]
fn other_kinds_leave_the_snapshot() {
    let mut world = World::default();
    let mut snapshot = [3u8; 20];
    snapshot[16..].copy_from_slice(&[0; 4]);
    snapshot[0] = 9;
    world.set_shared(&group_22::AT_0180, snapshot);
    world.set(&group_00::AT_0034, 11);
    rotate_snapshot(&mut world, handle());
    assert_eq!(world.records.read_shared(handle(), &group_22::AT_0180).unwrap(), snapshot);
}

#[test]
fn the_configuration_words_cover_every_index_once_with_signed_readings_extended() {
    let mut world = World::default();
    world.property(Property(0x520e), 7);
    world.property_word(Property(0x5210), 0x1234);
    world.property_word(Property(0x52ec), 0xfffe);
    world.property(Property(0x52ee), 0x80);
    let words: Vec<(usize, u32)> = configuration_words(&mut world).collect();
    let mut indices: Vec<usize> = words.iter().map(|(index, _)| *index).collect();
    indices.sort_unstable();
    assert_eq!(indices, (0..161).collect::<Vec<_>>());
    let value = |index: usize| words.iter().find(|(at, _)| *at == index).unwrap().1;
    assert_eq!((value(0), value(2), value(147), value(148), value(150)), (7, 0x1234, u32::MAX - 1, 0xffff_ff80, 0));
    assert_eq!(&words[56..58], &[(56, 0), (58, 0)]);
}

#[test]
fn the_primary_scale_is_one_of_eight_settings_in_tenths() {
    let mut world = World::default();
    world.property(Property(0x1346), 30);
    assert_eq!((primary_scale(&mut world, 7), primary_scale(&mut world, 8)), (300, 0));
}

struct Levels;

impl ProfileLevels for Levels {
    fn profile_level(&mut self, profile: u32, level: i32, reference: i32) -> i32 { profile as i32 + level - reference }
}

#[test]
fn the_profile_level_is_compared_with_the_target() {
    assert!(profile_level_is(&mut Levels, 12, 5, 3, 10));
    assert!(!profile_level_is(&mut Levels, 13, 5, 3, 10));
}
