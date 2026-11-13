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
    assert_eq!(cadence_property(Work::Image, Mode::ProfileSelected).unwrap().word(), 0x4bce);
    assert_eq!(cadence_property(Work::Image, Mode::AboveThreshold).unwrap().word(), 0x4bcf);
    assert_eq!(cadence_property(Work::Source, Mode::BelowThreshold).unwrap().word(), 0x4bd6);
    assert_eq!(cadence_property(Work::Source, Mode::ProfileSelected), cadence_property(Work::Auxiliary, Mode::ProfileSelected));
    assert_eq!(cadence_property(Work::Window, Mode::Default), None);
    assert_eq!(cadence_property(Work::SecondaryAcquisition, Mode::Default), None);
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

#[test]
fn the_window_request_places_each_field_and_zeroes_the_unwritten_bytes() {
    use crate::pipeline::{GainStep, PipelineSnapshot, PipelineWord, TimingWord};
    use crate::profile::{ProfileMode, ProfileSelection, RetainedHalfword, RetainedWord, WindowRecord, WindowStatus};
    use crate::stock::window_payload;
    use crate::window::WindowRequest;
    use xm1_saclac_interface::calc::PeerCallback;
    use xm1_saclac_interface::stock::calc_payload;
    let request = WindowRequest {
        selection: ProfileSelection::Other(2),
        mode: ProfileMode(6),
        selected: true,
        snapshot: PipelineSnapshot {
            timing: TimingWord(0x0102_0304), flags: 0x55, gain_step: GainStep(1),
            exposure: [PipelineWord(0x1112_1314), PipelineWord(0x99)], levels: [0x2122, 0x3132],
            adjustment: PipelineWord(0x4142_4344),
            ..PipelineSnapshot::default()
        },
        level: 0x5152,
        reference: 0x6162,
        record: WindowRecord { status: WindowStatus(2), first: RetainedWord(0x7172_7374), halfword: RetainedHalfword(0x8182), second: RetainedWord(0x9192_9394) },
        refresh: true,
    };
    let (_, encoded) = calc_payload(&window_payload(request, PeerCallback(0xf3a3f8)));
    let bytes = encoded.bytes();
    assert_eq!(bytes.len(), 80);
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let half = |offset: usize| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
    assert_eq!((word(0), word(4), half(8)), (2, 0x7172_7374, 0x5152));
    assert_eq!((word(12), bytes[16], word(20), half(24), half(26)), (0x0102_0304, 0x55, 0x1112_1314, 0x2122, 0x6162));
    assert_eq!((half(32), word(36), word(40), word(44)), (0x8182, 0x5ffe_c1b4, 4096, 0x4142_4344));
    assert_eq!((word(48), word(52), word(56), word(60), word(64), word(68), word(72), word(76)), (2, 0x9192_9394, 0, 0, 1, 6, 1, 0xf3a3f8));
    for padding in [10..12, 17..20, 28..32, 34..36] { assert!(bytes[padding].iter().all(|&byte| byte == 0)); }
}

#[test]
fn a_snapshot_reads_each_field_at_its_offset() {
    use crate::pipeline::{GainStep, PipelineWord, SnapshotStatus, TimingWord};
    use crate::stock::{pipeline_snapshot, SNAPSHOT_BYTES};
    let mut bytes = [0u8; SNAPSHOT_BYTES];
    let mut put = |offset: usize, value: &[u8]| bytes[offset..offset + value.len()].copy_from_slice(value);
    for (offset, word) in [(0, 5u32), (4, 0x40), (8, 0x80), (0xc, 0xc0), (0x10, 0x100), (0x14, 0x140), (0x18, 0x180), (0x34, 0x340), (0x38, 0x380), (0xdc, 0xdc0), (0xe0, 0xe00), (0xe4, 0xe40), (0xe8, 0xe80), (0xec, 0xec0), (0xf4, 0xf40), (0xf8, 0xf80), (0x104, 0x1040), (0x110, 0x1100)] {
        put(offset, &word.to_le_bytes());
    }
    put(0xf2, &[0x2f]);
    put(0xfc, &0xfc0u16.to_le_bytes());
    put(0xfe, &0xfe0u16.to_le_bytes());
    put(0x100, &0x1000u16.to_le_bytes());
    let snapshot = pipeline_snapshot(&bytes);
    assert!(snapshot.status == SnapshotStatus(5) && snapshot.status.complete());
    assert_eq!((snapshot.timing, snapshot.flags, snapshot.gain_step), (TimingWord(0xec0), 0x2f, GainStep(0xf40)));
    assert_eq!((snapshot.exposure, snapshot.levels, snapshot.adjustment), ([PipelineWord(0xf80), PipelineWord(0x1100)], [0xfc0, 0xfe0], PipelineWord(0x1040)));
    assert_eq!(snapshot.retained, [0x40, 0x80, 0xdc0, 0xe00, 0x340, 0x380].map(PipelineWord));
    assert_eq!((snapshot.size, snapshot.origin, snapshot.extent, snapshot.offset_level), ([0xc0, 0x100].map(PipelineWord), [0x140, 0x180].map(PipelineWord), [0xe40, 0xe80].map(PipelineWord), 0x1000));
}

#[test]
fn a_history_entry_lays_out_banks_levels_exposure_and_carried_words() {
    use crate::history::HistoryEntry;
    use crate::pipeline::{HistoryStatistics, PipelineSnapshot, PipelineWord};
    use crate::stock::{history_entry_bytes, HISTORY_ENTRY_BYTES};
    let mut statistics = HistoryStatistics { word: PipelineWord(0x5145), cell_levels: [0x204, 0x206], ..HistoryStatistics::default() };
    statistics.banks[0][1] = 0x3140;
    statistics.banks[1][63] = 0x4140;
    let snapshot = PipelineSnapshot { levels: [1, 0x20a], retained: [1, 2, 3, 4, 5, 6].map(PipelineWord), ..PipelineSnapshot::default() };
    let bytes = history_entry_bytes(&HistoryEntry::new(&snapshot, statistics, -2));
    assert_eq!(bytes.len(), HISTORY_ENTRY_BYTES);
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let half = |offset: usize| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
    assert_eq!((word(0), word(4 + 63 * 4), word(0x104 + 4)), (1, 0x4140, 0x3140));
    assert_eq!((half(0x204), half(0x206), half(0x208), half(0x20a)), (0x204, 0x206, 0xfffe, 0x20a));
    assert_eq!([0x20c, 0x210, 0x214, 0x218, 0x21c, 0x220].map(word), [1, 2, 3, 4, 5, 6]);
    assert_eq!(word(0x224), 0x5145);
}
