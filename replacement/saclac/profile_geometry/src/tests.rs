use crate::numeric::{self, FloatLog, FractionTables};
use crate::profile::{
    is_paired_mode, normalize_selector, ClassIndex, ExpansionRatio, GeometryClass, ProfileEntry, ProfileHandle, ProfileKind,
    ProfileTables, Profiles, Rectangle, ScalePair,
};
use crate::stock::records::{Parameters, StatusSnapshot};
use crate::stock::tables::{parse_class, parse_entry, parse_expansion, CLASS_BYTES, ENTRY_BYTES};

struct ExactFractions;

impl FractionTables for ExactFractions {
    fn fraction(&self, index: u8) -> u8 { (100.0 * (1.0 + index as f64 / 256.0).log2()) as u8 }
    fn wide_fraction(&self, index: u8) -> u8 { (100.0 * (1.0 + index as f64 / 100.0).log2()) as u8 }
}

#[test]
fn logarithms_of_powers_of_two_are_whole_hundreds() {
    for exponent in 0..32 {
        assert_eq!(numeric::centi_log2(&ExactFractions, 1 << exponent), Some(exponent as u16 * 100));
    }
    for exponent in 0..58 {
        assert_eq!(numeric::centi_log2_wide(&ExactFractions, 1 << exponent), Some(exponent * 100));
    }
}

#[test]
fn logarithms_track_log2_within_two_hundredths() {
    for value in [3u32, 7, 100, 1000, 12_345, 4_000_000, u32::MAX] {
        let expected = 100.0 * (value as f64).log2();
        let got = numeric::centi_log2(&ExactFractions, value).unwrap() as f64;
        assert!((expected - got).abs() <= 2.0, "{value}: {got} for {expected}");
    }
    for value in [3u64, 1000, 1 << 40 | 12345] {
        let expected = 100.0 * (value as f64).log2();
        let got = numeric::centi_log2_wide(&ExactFractions, value).unwrap() as f64;
        assert!((expected - got).abs() <= 2.0, "{value}: {got} for {expected}");
    }
}

#[test]
fn zero_has_no_logarithm_and_sets_the_differences_to_their_limits() {
    assert_eq!(numeric::centi_log2(&ExactFractions, 0), None);
    assert_eq!(numeric::centi_log2_wide(&ExactFractions, 0), None);
    assert_eq!(numeric::scaled_log2(&ExactFractions, 0), None);
    assert_eq!(numeric::log2_difference(&ExactFractions, 0, 8), -1000);
    assert_eq!(numeric::log2_difference(&ExactFractions, 0, 0), -1000);
    assert_eq!(numeric::log2_difference(&ExactFractions, 8, 0), 1000);
    assert_eq!(numeric::log2_offset(&ExactFractions, 0, 5), -1000);
}

#[test]
fn differences_and_offsets_are_in_hundredths() {
    assert_eq!(numeric::log2_difference(&ExactFractions, 1024, 4), 800);
    assert_eq!(numeric::log2_difference(&ExactFractions, 4, 1024), -800);
    assert_eq!(numeric::log2_offset(&ExactFractions, 256, 300), 500);
    assert_eq!(numeric::log2_offset(&ExactFractions, 2, 300), -200);
}

#[test]
fn scaled_logarithm_converts_hundredths_of_log2_to_tenths_of_log10_scale() {
    assert_eq!(numeric::scaled_log2(&ExactFractions, 1024), Some(1000 * 100 / 332));
    assert_eq!(numeric::scaled_log2(&ExactFractions, 1), Some(0));
}

struct StdLog(u32);

impl FloatLog for StdLog {
    fn ln(&mut self, value: f32) -> f32 {
        self.0 += 1;
        value.ln()
    }
}

#[test]
fn log10_divides_two_runtime_logarithms() {
    let mut log = StdLog(0);
    assert!((numeric::log10(&mut log, 1000.0) - 3.0).abs() < 1e-5);
    assert_eq!(log.0, 2);
}

#[test]
fn division_rounds_inexact_quotients_away_from_zero() {
    assert_eq!(numeric::divide_away_from_zero(7, 2), 4);
    assert_eq!(numeric::divide_away_from_zero(-7, 2), -4);
    assert_eq!(numeric::divide_away_from_zero(7, -2), -4);
    assert_eq!(numeric::divide_away_from_zero(-7, -2), 4);
    assert_eq!(numeric::divide_away_from_zero(8, 2), 4);
    assert_eq!(numeric::divide_away_from_zero(-8, 2), -4);
    assert_eq!(numeric::divide_away_from_zero(5, 0), 0);
}

#[test]
fn runtime_quotient_by_zero_is_all_ones_unless_the_numerator_is_zero() {
    assert_eq!(numeric::runtime_quotient(10, 3), 3);
    assert_eq!(numeric::runtime_quotient(10, 0), u64::MAX);
    assert_eq!(numeric::runtime_quotient(0, 0), 0);
}

#[derive(Clone)]
struct Tables {
    entries: Vec<ProfileEntry>,
    classes: Vec<GeometryClass>,
    pair: ScalePair,
}

impl ProfileTables for Tables {
    fn entry(&self, profile: ProfileHandle) -> ProfileEntry { self.entries[profile.0 as usize] }
    fn class(&self, class: ClassIndex) -> GeometryClass { self.classes[class.0 as usize] }
    fn threshold(&self, index: u8) -> u16 {
        assert!(index < 22);
        [5, 6, 10, 15, 20, 24, 25, 28, 30, 40, 46, 50, 60, 70, 80, 100, 120, 160, 200, 240, 320, 400][index as usize]
    }
    fn default_pair(&self) -> ScalePair { self.pair }
    fn expansion(&self) -> ExpansionRatio { ExpansionRatio { numerator: 5, divisor: 9 } }
}

fn entry(class: u32, kind: u8) -> ProfileEntry {
    ProfileEntry { class: ClassIndex(class), kind: ProfileKind(kind), flags: 0, threshold_index: 0, feature: 0, aspect_index: 0 }
}

fn class() -> GeometryClass {
    GeometryClass {
        height: 480,
        width: 640,
        top: 10,
        left: 20,
        crop_height: 101,
        crop_width: 300,
        first_scale: 1,
        second_scale: 1,
        ..GeometryClass::default()
    }
}

fn tables() -> Tables {
    Tables { entries: vec![entry(0, 0), entry(0, 2)], classes: vec![class()], pair: ScalePair { first: 6, second: 6 } }
}

#[test]
fn kind_zero_crop_halves_the_vertical_extent_and_doubles_the_horizontal() {
    let profiles = Profiles::new(tables());
    let full = Rectangle { top: 10, left: 20, width: 300, height: 101 };
    assert_eq!(profiles.crop_rectangle(ProfileHandle(1), false), full);
    assert_eq!(profiles.crop_rectangle(ProfileHandle(0), true), full);
    assert_eq!(profiles.crop_rectangle(ProfileHandle(0), false), Rectangle { top: 5, left: 40, width: 600, height: 51 });
    assert_eq!(profiles.full_rectangle(ProfileHandle(0)), Rectangle { top: 0, left: 0, width: 640, height: 480 });
}

#[test]
fn threshold_index_is_the_highest_threshold_at_or_below_the_value() {
    let profiles = Profiles::new(tables());
    assert_eq!(profiles.threshold_index_for(30), 8);
    assert_eq!(profiles.threshold_index_for(31), 8);
    assert_eq!(profiles.threshold_index_for(399), 20);
    assert_eq!(profiles.threshold_index_for(400), 21);
    assert_eq!(profiles.threshold_index_for(9), 1);
    assert_eq!(profiles.threshold_index_for(0), 1);
}

#[test]
fn a_threshold_index_past_the_table_reads_the_first_threshold() {
    let mut tables = tables();
    tables.entries[0].threshold_index = 22;
    tables.entries[1].threshold_index = 21;
    let profiles = Profiles::new(tables);
    assert_eq!(profiles.selected_threshold(ProfileHandle(0)), 5);
    assert_eq!(profiles.selected_threshold(ProfileHandle(1)), 400);
}

#[test]
fn expanded_ratio_scales_by_four_numerators_over_the_divisor() {
    assert_eq!(Profiles::new(tables()).expanded_ratio(9), 20);
    assert_eq!(Profiles::new(tables()).expanded_ratio(u32::MAX), u32::MAX as u64 * 20 / 9);
}

#[test]
fn adjustment_depends_on_kind_and_for_kind_five_on_flag_zero_and_class() {
    let mut tables = tables();
    tables.classes = vec![class(); 16];
    tables.entries = (0..8).map(|kind| entry(0, kind)).collect();
    tables.entries.push(ProfileEntry { flags: 1, ..entry(0, 5) });
    tables.entries.push(ProfileEntry { flags: 1, ..entry(9, 5) });
    tables.entries.push(ProfileEntry { flags: 1, ..entry(15, 5) });
    let profiles = Profiles::new(tables);
    let adjusted: Vec<bool> = (0..11).map(|handle| profiles.requires_adjustment(ProfileHandle(handle))).collect();
    assert_eq!(adjusted, [false, false, true, true, false, false, true, true, true, false, false]);
}

#[test]
fn scale_pairs_halve_for_unit_scales_and_flags_unless_scaling_is_disabled() {
    let mut tables = tables();
    tables.classes = vec![
        class(),
        GeometryClass { first_scale: 2, ..class() },
        GeometryClass { second_flag: 1, ..class() },
    ];
    tables.entries = vec![entry(0, 1), entry(1, 1), entry(2, 1)];
    let profiles = Profiles::new(tables.clone());
    assert_eq!(profiles.scale_pair(ProfileHandle(0)), ScalePair { first: 6, second: 6 });
    assert_eq!(profiles.scale_pair(ProfileHandle(1)), ScalePair { first: 6, second: 3 });
    assert_eq!(profiles.scale_pair(ProfileHandle(2)), ScalePair { first: 6, second: 6 });
    assert_eq!(profiles.flagged_scale_pair(ProfileHandle(2)), ScalePair { first: 3, second: 6 });
    tables.pair = ScalePair { first: 2, second: 2 };
    let disabled = Profiles::new(tables);
    assert_eq!(disabled.flagged_scale_pair(ProfileHandle(1)), ScalePair { first: 2, second: 2 });
    assert_eq!(disabled.first_pair_scale(ProfileHandle(1)), 1);
}

#[test]
fn first_multiplier_follows_the_class_selection_unless_its_flag_is_set() {
    let mut tables = tables();
    tables.classes = vec![
        GeometryClass { first_scale: 3, scale_selection: 1, ..class() },
        GeometryClass { first_scale: 3, scale_selection: 2, ..class() },
        GeometryClass { first_scale: 3, scale_selection: 7, ..class() },
        GeometryClass { first_scale: 3, scale_selection: 2, first_flag: 1, second_scale: 5, second_flag: 1, ..class() },
    ];
    tables.entries = (0..4).map(|class| entry(class, 1)).collect();
    let profiles = Profiles::new(tables);
    let first: Vec<u16> = (0..4).map(|handle| profiles.scaled_first(ProfileHandle(handle), true)).collect();
    assert_eq!(first, [6, 12, 3, 6]);
    assert_eq!(profiles.scaled_first(ProfileHandle(1), false), 3);
    assert_eq!(profiles.scaled_second(ProfileHandle(3), true), 10);
    assert_eq!(profiles.second_pair_scale(ProfileHandle(0)), 1);
}

#[test]
fn magnification_maps_its_index_to_a_factor() {
    let mut tables = tables();
    tables.classes = (0..8).map(|index| GeometryClass { magnification_index: index, ..class() }).collect();
    tables.entries = (0..8).map(|class| entry(class, 1)).collect();
    let profiles = Profiles::new(tables);
    let factors: Vec<u32> = (0..8).map(|handle| profiles.magnification(ProfileHandle(handle))).collect();
    assert_eq!(factors, [1, 2, 4, 6, 8, 10, 12, 2]);
}

#[test]
fn selectors_outside_five_to_ten_normalize_to_zero() {
    let normalized: Vec<u32> = (0..13).map(normalize_selector).collect();
    assert_eq!(normalized, [0, 0, 0, 0, 0, 5, 6, 7, 8, 9, 10, 0, 0]);
    assert!(is_paired_mode(3) && is_paired_mode(4) && !is_paired_mode(2) && !is_paired_mode(5));
}

#[test]
fn stock_rows_parse_at_their_offsets() {
    let mut row = [0u8; ENTRY_BYTES];
    row[0x4..0x8].copy_from_slice(&7u32.to_le_bytes());
    row[0x8] = 5;
    row[0xa..0xc].copy_from_slice(&0x4011u16.to_le_bytes());
    row[0xc] = 3;
    row[0xd] = 4;
    row[0xe] = 1;
    let entry = parse_entry(&row);
    assert_eq!(entry, ProfileEntry { class: ClassIndex(7), kind: ProfileKind(5), flags: 0x4011, threshold_index: 3, feature: 4, aspect_index: 1 });

    let mut row = [0u8; CLASS_BYTES];
    for (offset, value) in [(0x0, 480u16), (0x2, 640), (0xc, 1), (0xe, 2), (0x10, 3), (0x12, 4)] {
        row[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    row[0x24..0x2f].copy_from_slice(&[11, 12, 13, 14, 15, 16, 17, 18, 19, 0, 20]);
    let class = parse_class(&row);
    assert_eq!(class, GeometryClass {
        height: 480,
        width: 640,
        top: 1,
        left: 2,
        crop_height: 3,
        crop_width: 4,
        first_scale: 11,
        second_scale: 12,
        first_flag: 13,
        second_flag: 14,
        scale_selection: 15,
        attributes: [16, 17],
        kind: 18,
        magnification_index: 19,
        limit: 20,
    });
    assert_eq!(parse_expansion(&[2, 5, 2, 0, 9]), ExpansionRatio { numerator: 5, divisor: 9 });
}

#[test]
fn parameter_fields_change_only_their_own_bytes() {
    let original: [u8; Parameters::BYTES] = core::array::from_fn(|index| index as u8);
    let mut parameters = Parameters::from_bytes(original);
    assert_eq!(parameters.profile(), ProfileHandle(0x0302_0100));
    assert_eq!(parameters.at_04(), 0x0504);
    assert_eq!(parameters.at_44(), 0x44);
    parameters.set_profile(ProfileHandle(7));
    parameters.set_at_28(-2);
    let changed: Vec<usize> = (0..Parameters::BYTES).filter(|&index| parameters.bytes()[index] != original[index]).collect();
    assert_eq!(changed, [0, 1, 2, 3, 0x28, 0x29]);
    assert_eq!(parameters.profile(), ProfileHandle(7));
    assert_eq!(parameters.at_28(), -2);
}

#[test]
fn snapshot_fields_read_back_what_was_set() {
    let mut snapshot = StatusSnapshot::from_bytes([0; StatusSnapshot::BYTES]);
    snapshot.set_at_c4(0xbeef);
    snapshot.set_at_bc(9);
    snapshot.set_at_90(0x1234_5678);
    assert_eq!((snapshot.at_c4(), snapshot.at_bc(), snapshot.at_90()), (0xbeef, 9, 0x1234_5678));
    assert_eq!(snapshot.bytes()[0x90..0x94], [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(snapshot.bytes()[0xc6..], [0, 0]);
}
