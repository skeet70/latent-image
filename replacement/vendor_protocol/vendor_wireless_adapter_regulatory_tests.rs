use super::{RegulatoryInput, RegulatorySelector};
use super::super::{CountryCode, RateBand};

#[test]
fn country_lookup_selects_every_distinct_channel_plan() {
    let input = RegulatoryInput::country([6, 7]);
    let cases = [
        (0x5553, [11, 4, 4, 11, 5, 0]),
        (0x4252, [13, 4, 4, 11, 5, 0]),
        (0x434f, [13, 4, 4, 11, 0, 0]),
        (0x4a50, [13, 1, 4, 4, 11, 0]),
        (0x434e, [13, 0, 4, 11, 5, 0]),
        (0x4154, [13, 0, 4, 0, 5, 0]),
        (0x4155, [11, 4, 4, 11, 4, 0]),
        (0x5255, [13, 4, 4, 11, 4, 0]),
        (0x4944, [13, 0, 0, 0, 4, 0]),
        (0x4341, [11, 4, 4, 5, 5, 5]),
        (0x434c, [13, 4, 4, 0, 5, 0]),
        (0x494c, [13, 4, 4, 0, 0, 0]),
        (0x4a4f, [13, 4, 0, 0, 0, 0]),
        (0x4b52, [13, 4, 4, 8, 5, 0]),
        (0x4555, [13, 4, 4, 11, 7, 0]),
    ];
    for (country, counts) in cases {
        let mut selector = RegulatorySelector::new();
        let selected = selector.select(CountryCode(country), &input);
        assert_eq!(selected.groups.map(|group| group.count), counts);
        assert_eq!(selected.flags, [6, 7]);
    }
}

#[test]
fn japan_channel_fourteen_and_canadian_split_are_preserved() {
    let input = RegulatoryInput::country([0; 2]);
    let mut selector = RegulatorySelector::new();
    let japan = selector.select(CountryCode(0x4a50), &input);
    assert_eq!(japan.groups[1].index, 82);
    assert_eq!(japan.groups[1].band, RateBand::TwoGhz);
    assert_eq!(japan.groups[1].first, 14);
    selector.invalidate();
    let canada = selector.select(CountryCode(0x4341), &input);
    assert_eq!(canada.groups[3].first, 100);
    assert_eq!(canada.groups[4].first, 132);
    assert_eq!(canada.groups[5].first, 149);
}

#[test]
fn eeprom_index_overrides_country_and_invalid_index_falls_back_to_country() {
    let indexed = RegulatoryInput::from_eeprom(1, 3, [[0; 6]; 6], [0; 2]);
    let invalid = RegulatoryInput::from_eeprom(1, 15, [[0; 6]; 6], [0; 2]);
    let mut selector = RegulatorySelector::new();
    let selected = selector.select(CountryCode(0x5553), &indexed);
    assert_eq!(selected.groups[1].first, 14);
    selector.invalidate();
    let selected = selector.select(CountryCode(0x5553), &invalid);
    assert_eq!(selected.groups[0].count, 11);
    assert_eq!(selected.groups[1].first, 36);
}

#[test]
fn unknown_country_and_unknown_eeprom_mode_use_final_profile() {
    let input = RegulatoryInput::from_eeprom(99, 0, [[0; 6]; 6], [0; 2]);
    let mut selector = RegulatorySelector::new();
    let selected = selector.select(CountryCode(0xffff), &input);
    assert_eq!(selected.groups[0].count, 13);
    assert_eq!(selected.groups[4].first, 149);
    assert_eq!(selected.groups[4].count, 7);
}

#[test]
fn cached_selection_survives_country_change_until_invalidation() {
    let input = RegulatoryInput::country([0; 2]);
    let mut selector = RegulatorySelector::new();
    assert_eq!(selector.select(CountryCode(0x5553), &input).groups[0].count, 11);
    assert_eq!(selector.select(CountryCode(0x4a50), &input).groups[1].first, 36);
    selector.invalidate();
    assert_eq!(selector.select(CountryCode(0x4a50), &input).groups[1].first, 14);
}

#[test]
fn custom_eeprom_rows_remain_the_selected_live_configuration() {
    let input = RegulatoryInput::from_eeprom(2, 255, [[99, 2, 8, 40, 3, 0xaa]; 6], [8, 9]);
    let updated = RegulatoryInput::from_eeprom(1, 0, [[88, 7, 9, 50, 4, 0xbb]; 6], [10, 11]);
    let mut selector = RegulatorySelector::new();
    let selected = selector.select(CountryCode(0x5553), &input);
    assert_eq!(selected.groups[0].index, 99);
    assert_eq!(selected.groups[0].first, 40);
    assert_eq!(selected.groups[0].count, 3);
    let selected = selector.select(CountryCode(0x4a50), &updated);
    assert_eq!(selected.groups[0].index, 88);
    assert_eq!(selected.groups[0].band, RateBand::Unsupported(7));
    assert_eq!(selected.flags, [10, 11]);
}
