use xm1_focus_measurement_round::lanes::Selection;
use xm1_focus_measurement_round::profile::{
    FrameInterval, GridCell, IntervalHistory, IntervalTracking, ProfileMode, ProfileRecords, ProfileSelection,
    PublishedResult, StatisticsInUse, WindowPosition, WindowStatus,
};
use xm1_saclac_interface::round::{GeometryPair, RetainedGeometryValue};
use xm1_saclac_interface::stock::{record_handle_from_a2, record_handle_word};
use xm1_saclac_profile_records::{Bytes, MeasurementSelection};
use xm1_saclac_shared::settings::ProfileHeaders;

use crate::fake::{open, root, Entries};

fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }

fn put(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()) }

#[test]
fn an_invalidated_result_loses_only_its_validity_word_and_only_under_its_grant() {
    for (result, at, cleared) in [
        (PublishedResult::Acquisition, 0x0, 4..8),
        (PublishedResult::Search, 0x84, 0..4),
        (PublishedResult::Auxiliary, 0x180, 12..16),
        (PublishedResult::Source, 0x194, 0..4),
    ] {
        for granted in [true, false] {
            let mut bytes = Bytes::default();
            let record = open(&mut bytes, if granted { &[14, 15, 22, 23] } else { &[] });
            bytes.shared[at..at + 16].fill(0x5a);
            let (_, mut root) = root(bytes, Entries::default());
            root.invalidate(record, result);
            let mut expected = [0x5a; 16];
            if granted {
                expected[cleared.clone()].fill(0);
            }
            assert_eq!(root.records().memory().shared[at..at + 16], expected, "{result:?} granted {granted}");
        }
    }
}

#[test]
fn the_focus_result_is_invalidated_under_the_focus_controllers_record() {
    let mut bytes = Bytes::default();
    let controller = open(&mut bytes, &[25]);
    bytes.shared[0x1ac..0x1ac + 80].fill(0x33);
    let (fake, mut granted) = root(bytes, Entries::default());
    fake.set_word(0x16b_e01c, record_handle_word(controller));
    granted.invalidate_focus_result();
    let history = &granted.records().memory().shared[0x1ac..0x1ac + 80];
    assert_eq!(history[..4], [0; 4]);
    assert!(history[4..].iter().all(|&byte| byte == 0x33));

    let mut bytes = Bytes::default();
    let _ = open(&mut bytes, &[25]);
    bytes.shared[0x1ac..0x1b0].fill(0x33);
    let (fake, mut root) = root(bytes, Entries::default());
    fake.set_word(0x16b_e01c, u32::MAX);
    root.invalidate_focus_result();
    assert_eq!(root.records().memory().shared[0x1ac..0x1b0], [0x33; 4]);
}

#[test]
fn the_interval_history_keeps_the_bytes_after_its_count() {
    let mut bytes = Bytes::default();
    let record = open(&mut bytes, &[14]);
    let history = &mut bytes.shared[0x864..0x864 + 48];
    for index in 0..10 {
        put(history, 4 * index, 100 + index as u32);
    }
    history[40..42].copy_from_slice(&10u16.to_le_bytes());
    history[42..48].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    let (_, mut root) = root(bytes, Entries::default());

    let mut read = root.interval_history();
    assert_eq!(read.intervals().len(), 10);
    assert_eq!(read.intervals()[0], FrameInterval(100));
    read.record(FrameInterval(7));
    root.set_interval_history(record, read);
    let stored = &root.records().memory().shared[0x864..0x864 + 48];
    assert_eq!((word(stored, 0), word(stored, 36)), (101, 7));
    assert_eq!(stored[42..48], [1, 2, 3, 4, 5, 6]);

    root.set_interval_history(record, IntervalHistory::default());
    let stored = &root.records().memory().shared[0x864..0x864 + 48];
    assert!(stored[..42].iter().all(|&byte| byte == 0));
    assert_eq!(stored[42..48], [1, 2, 3, 4, 5, 6]);
    assert_eq!(root.interval_history(), IntervalHistory::default());
}

#[test]
fn a_record_without_the_measurement_grant_writes_nothing_to_the_shared_record() {
    let mut bytes = Bytes::default();
    let record = open(&mut bytes, &[]);
    let (_, mut root) = root(bytes, Entries::default());
    root.set_statistics_in_use(record, StatisticsInUse { primary: true, search: true, auxiliary: true });
    root.store_window_status(record, WindowStatus(2));
    root.set_interval_history(record, IntervalHistory::new([FrameInterval(9); 10], 10));
    assert!(root.records().memory().shared.iter().all(|&byte| byte == 0));
    assert!(!root.grants_measurement_history(record));
}

#[test]
fn the_measurement_record_reads_back_what_the_round_stores() {
    let mut bytes = Bytes::default();
    let record = open(&mut bytes, &[16, 20, 27]);
    bytes.shared[0x13c] = 0b10_0101;
    put(&mut bytes.shared, 0x154, 0x0123_4560);
    put(&mut bytes.shared, 0x14c, 0xaa);
    bytes.shared[0x250..0x252].copy_from_slice(&(-12i16).to_le_bytes());
    bytes.shared[0x710..0x712].copy_from_slice(&0x1234u16.to_le_bytes());
    put(&mut bytes.shared, 0x738, 0xbb);
    let (_, mut root) = root(bytes, Entries::default());

    assert_eq!(root.offered(), Some(Selection { acquisition: true, search: false, auxiliary: true, image: false, source: true }));
    assert!(root.grants_measurement_history(record));
    assert_eq!(root.acquisition_level(), -12);
    root.set_statistics_in_use(record, StatisticsInUse { primary: true, search: false, auxiliary: true });
    assert_eq!(root.statistics_in_use(), StatisticsInUse { primary: true, search: false, auxiliary: true });
    root.detach_capture_buffer(record);
    root.store_window_status(record, WindowStatus(2));
    root.store_window_position(record, WindowPosition([0x0102, 0x0304]));
    let window = root.window();
    assert_eq!((window.status, window.first.0, window.halfword.0, window.second.0), (WindowStatus(2), 0xaa, 0x1234, 0xbb));
    let shared = &root.records().memory().shared;
    assert_eq!((word(shared, 0x154), word(shared, 0x838)), (0, 0x0304_0102));
}

#[test]
fn a_record_that_offers_nothing_has_no_selection() {
    let (_, mut root) = root(Bytes::default(), Entries::default());
    assert_eq!(root.offered(), None);
}

#[test]
fn the_profile_comes_from_the_record_the_handle_selects_and_the_shared_header_entries() {
    let mut bytes = Bytes::default();
    let record = open(&mut bytes, &[0, 4, 13]);
    let profile = &mut bytes.records[0];
    profile[0x3e] = 6;
    profile[0x62] = 1;
    put(profile, 0x20cc, 1);
    put(profile, 0x20d4, 2);
    profile[0xc..0xe].copy_from_slice(&0x0502u16.to_le_bytes());
    put(profile, 0x4, 4);
    put(profile, 0x8, 6);
    profile[0x80] = 0x80;
    put(profile, 0x0, 0x0002_0001);
    profile[0x69] = 1;
    profile[0x2c..0x2e].copy_from_slice(&0x77u16.to_le_bytes());
    put(profile, 0x164, 0x11);
    put(profile, 0x168, 0x22);

    for (header, cell, adjustment) in [
        (0, GridCell { column: 2, row: 5 }, false),
        (2, GridCell { column: 4, row: 6 }, false),
        (5, GridCell { column: 4, row: 6 }, false),
        (6, GridCell { column: 3, row: 3 }, true),
    ] {
        let entries = Entries { selection: MeasurementSelection::Other(9), header_47: header };
        let (_, mut root) = root(bytes.clone(), entries);
        let profile = root.profile(record);
        assert_eq!(profile.selection, ProfileSelection::Other(9));
        assert_eq!(profile.mode, ProfileMode(6));
        assert_eq!(profile.interval_tracking, IntervalTracking::Record);
        assert!(profile.geometry_on_autofocus);
        assert!(profile.profile_cadence);
        assert!(!profile.frame_rate_cadence);
        assert_eq!((profile.focus_cell, profile.takes_search_adjustment), (cell, adjustment), "header {header}");
        assert_eq!(root.mode(record).0, header);
        assert_eq!(root.measurement_selection(record).0, 9);
    }

    let (_, mut root) = root(bytes, Entries::default());
    let geometry = root.geometry(record);
    assert_eq!(geometry.resource, GeometryPair([1, 2]));
    assert_eq!(geometry.converted_profile, RetainedGeometryValue(0x77));
    assert_eq!((geometry.first_parameter, geometry.second_parameter), (RetainedGeometryValue(0x11), RetainedGeometryValue(0x22)));
}

#[test]
fn interval_tracking_is_cleared_by_zero_recorded_by_one_and_kept_otherwise() {
    for (byte, tracking) in [(0, IntervalTracking::Clear), (1, IntervalTracking::Record), (2, IntervalTracking::Keep)] {
        let mut bytes = Bytes::default();
        let record = open(&mut bytes, &[0]);
        bytes.records[0][0x62] = byte;
        let (_, mut root) = root(bytes, Entries::default());
        assert_eq!(root.profile(record).interval_tracking, tracking);
    }
    let (_, mut root) = root(Bytes::default(), Entries::default());
    assert!(!root.profile(record_handle_from_a2(u32::MAX)).geometry_on_autofocus);
}
