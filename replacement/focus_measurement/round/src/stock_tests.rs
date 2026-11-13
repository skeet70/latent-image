use crate::lanes::Selection;
use crate::stock::{alternate_search_rule, availability, lane_flags, profile_selection, search_allowed, selection, ProfileSelectionByte, RecordSelection, SearchModeFlags};

#[test]
fn acquisition_selection_expands_into_both_acquisition_calculations() {
    let lanes = |word| lane_flags(Selection::ALL.shared(selection(RecordSelection(word)), true));
    assert_eq!(lanes(1), 0x21);
    assert_eq!(lanes(2), 2);
    assert_eq!(lanes(4), 4);
    assert_eq!(lanes(8), 8);
    assert_eq!(lanes(32), 64);
    assert_eq!(lanes(0xffff_ffd0), 0);
}

#[test]
fn search_property_blocks_only_search_and_image() {
    let all = selection(RecordSelection(47));
    assert_eq!(lane_flags(Selection::ALL.shared(all, search_allowed(0))), 0x6f);
    assert_eq!(lane_flags(Selection::ALL.shared(all, search_allowed(1))), 0x65);
    assert_eq!(lane_flags(Selection::ALL.shared(all, search_allowed(2))), 0x6f);
}

#[test]
fn only_an_all_zero_selection_word_offers_nothing() {
    assert_eq!(availability(RecordSelection(0)), None);
    assert_eq!(availability(RecordSelection(0x10)), Some(Selection::default()));
}

#[test]
fn alternate_search_obeys_property_precedence_and_profile_selection() {
    for (flags, byte, expected) in [
        (0, 0, false), (8, 2, true), (2, 1, true), (2, 0, false),
        (4, 0, true), (4, 1, false), (4, 2, false),
        (6, 0, false), (6, 1, true), (10, 0, false), (12, 1, false),
    ] {
        assert_eq!(alternate_search_rule(SearchModeFlags(flags)).applies(profile_selection(ProfileSelectionByte(byte))), expected);
    }
}

#[test]
fn cadence_properties_use_mode_specific_keys_and_share_auxiliary_source_policy() {
    use crate::planner::{Mode, Work};
    use crate::stock::cadence_property;
    assert_eq!(cadence_property(Work::PrimaryAcquisition, Mode::Default).unwrap().word(), 0x4bc5);
    assert_eq!(cadence_property(Work::Search, Mode::RecordSelected).unwrap().word(), 0x4bc6);
    assert_eq!(cadence_property(Work::Image, Mode::ResourceSelected).unwrap().word(), 0x4bcd);
    assert_eq!(cadence_property(Work::Source, Mode::BelowThreshold).unwrap().word(), 0x4bd6);
    assert_eq!(cadence_property(Work::Source, Mode::ProfileSelected), cadence_property(Work::Auxiliary, Mode::ProfileSelected));
    assert_eq!(cadence_property(Work::Window, Mode::Default), None);
    assert_eq!(cadence_property(Work::SecondaryAcquisition, Mode::Default), None);
    assert_eq!(cadence_property(Work::Search, Mode::Paused), None);
}

#[test]
fn a_parameter_change_names_its_client_then_the_requested_and_offered_selections() {
    use crate::round::ParameterClient;
    use crate::stock::parameter_change;
    let block: std::vec::Vec<u8> = [0xf0a0u32, 0x21, 0x01].iter().flat_map(|word| word.to_le_bytes()).collect();
    let change = parameter_change(xm1_saclac_interface::stock::parameters(&block));
    assert_eq!(change.client, ParameterClient(xm1_saclac_interface::stock::optional_callback(0xf0a0)));
    assert_eq!(change.requested, Selection { acquisition: true, source: true, ..Selection::default() });
    assert_eq!(change.offered, Selection { acquisition: true, ..Selection::default() });
    let short = parameter_change(xm1_saclac_interface::stock::parameters(&block[..4]));
    assert_eq!((short.requested, short.offered), (Selection::default(), Selection::default()));
}
