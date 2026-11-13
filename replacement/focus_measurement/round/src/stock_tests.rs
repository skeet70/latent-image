use crate::stock::{alternate_search, lane_flags, search_allowed, selection, RecordMode, RecordSelection, SearchModeFlags};

#[test]
fn acquisition_selection_expands_into_both_acquisition_calculations() {
    assert_eq!(lane_flags(selection(RecordSelection(1)).lanes(true)), 0x21);
    assert_eq!(lane_flags(selection(RecordSelection(2)).lanes(true)), 2);
    assert_eq!(lane_flags(selection(RecordSelection(4)).lanes(true)), 4);
    assert_eq!(lane_flags(selection(RecordSelection(8)).lanes(true)), 8);
    assert_eq!(lane_flags(selection(RecordSelection(32)).lanes(true)), 64);
    assert_eq!(lane_flags(selection(RecordSelection(0xffff_ffd0)).lanes(true)), 0);
}

#[test]
fn search_property_blocks_only_search_and_image() {
    let all = selection(RecordSelection(47));
    assert_eq!(lane_flags(all.lanes(search_allowed(0))), 0x6f);
    assert_eq!(lane_flags(all.lanes(search_allowed(1))), 0x65);
    assert_eq!(lane_flags(all.lanes(search_allowed(2))), 0x6f);
}

#[test]
fn alternate_search_obeys_property_precedence_and_record_modes() {
    for (flags, mode, expected) in [
        (0, 0, false), (8, 2, true), (2, 1, true), (2, 0, false),
        (4, 0, true), (4, 1, false), (4, 2, false),
        (6, 0, false), (6, 1, true), (10, 0, false), (12, 1, false),
    ] {
        assert_eq!(alternate_search(SearchModeFlags(flags), RecordMode(mode)), expected);
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
