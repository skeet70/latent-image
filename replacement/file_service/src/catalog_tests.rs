use crate::catalog::*;
use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::workers::Offset;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    Complete(u32, u32),
    Store(u32, u32),
    Done,
    Rebuild(bool, u32),
    Fill(u32, u8, u32),
    Find(FindSelectors, u32, u32),
    CompleteRequest,
    Marked(u16),
    Cluster(u16),
    Reserved(u16),
    ClusterPath(u32, u32),
    Scan([u32; 7], u32, u32),
}

const PAYLOAD: u32 = 0x5000;
const CALLBACK_WORD: u32 = 0x380000;
const STATUS_AT: u32 = 0x910138;
const RECORD: u32 = 0xd00000;

struct Fake {
    payload: Vec<u8>,
    context: bool,
    group_count: u8,
    group_mask: Option<u32>,
    mark: u32,
    selection_cleared: bool,
    counts: Vec<(u16, u16)>,
    totals: Vec<(u16, (u32, u32))>,
    raw: RawStatus,
    rebuild_raw: RawStatus,
    marked: RawStatus,
    cluster: u32,
    name: &'static [u8],
    scan_raw: RawStatus,
    scanned: (u32, u32),
    events: Vec<Event>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            payload: vec![0; 0x48],
            context: true,
            group_count: 4,
            group_mask: None,
            mark: 0,
            selection_cleared: false,
            counts: Vec::new(),
            totals: Vec::new(),
            raw: RawStatus(0),
            rebuild_raw: RawStatus(0),
            marked: RawStatus(0),
            cluster: 0x33,
            name: b"B:DSCF3097.JPG",
            scan_raw: RawStatus(0),
            scanned: (0x10, 0x20),
            events: Vec::new(),
        }
    }

    fn with(mut self, offset: u32, value: u32) -> Fake {
        self.payload[offset as usize..offset as usize + 4].copy_from_slice(&value.to_le_bytes());
        self
    }
}

/// Raw 0 translates to 0 and other raw results to raw + 100.
fn translated(raw: u32) -> u32 {
    if raw == 0 { 0 } else { raw + 100 }
}

impl Port for Fake {
    fn word(&self, at: Offset) -> u32 {
        u32::from_le_bytes(self.payload[at.0 as usize..at.0 as usize + 4].try_into().unwrap())
    }
    fn byte(&self, at: Offset) -> u8 {
        self.payload[at.0 as usize]
    }
    fn address(&self, at: Offset) -> u32 {
        PAYLOAD + at.0
    }
    fn store(&mut self, address: u32, value: u32) {
        self.events.push(Event::Store(address, value));
    }
    fn context(&self) -> bool {
        self.context
    }
    fn group_count(&self) -> u8 {
        self.group_count
    }
    fn set_group_mask(&mut self, mask: GroupMask) {
        self.group_mask = Some(mask.0);
    }
    fn catalog_mark(&self) -> CatalogMark {
        CatalogMark(self.mark)
    }
    fn set_catalog_mark(&mut self, mark: CatalogMark) {
        self.mark = mark.0;
    }
    fn clear_selection(&mut self) {
        self.selection_cleared = true;
    }
    fn directory_count(&self, directory: Directory) -> u16 {
        self.counts.iter().find(|entry| entry.0 == directory.0).map_or(0, |entry| entry.1)
    }
    fn directory_totals(&self, directory: Directory) -> (u32, u32) {
        self.totals.iter().find(|entry| entry.0 == directory.0).map_or((0, 0), |entry| entry.1)
    }
    fn translate(&mut self, raw: RawStatus) -> u32 {
        translated(raw.0)
    }
    fn complete_status(&mut self, callback: Callback, status: u32) {
        if callback.0 != 0 {
            self.events.push(Event::Complete(callback.0, status));
        }
    }
    fn done(&mut self) {
        self.events.push(Event::Done);
    }
    fn rebuild_catalog(&mut self, full: bool, mode: RebuildMode) -> RawStatus {
        self.events.push(Event::Rebuild(full, mode.0));
        self.rebuild_raw
    }
    fn fill(&mut self, address: u32, value: u8, length: u32) {
        self.events.push(Event::Fill(address, value, length));
    }
    fn find_entry(&mut self, selectors: FindSelectors, name: u32, record: u32) -> RawStatus {
        self.events.push(Event::Find(selectors, name, record));
        self.raw
    }
    fn complete_request(&mut self) -> RawStatus {
        self.events.push(Event::CompleteRequest);
        self.raw
    }
    fn directory_marked(&mut self, directory: Directory, frame: &mut ResolveFrame) -> RawStatus {
        self.events.push(Event::Marked(directory.0));
        frame.query.marked = 1;
        self.marked
    }
    fn reserved_cluster(&mut self, directory: Directory, frame: &mut ResolveFrame) -> RawStatus {
        self.events.push(Event::Cluster(directory.0));
        frame.query.cluster = self.cluster;
        RawStatus(0)
    }
    fn directory_reserved(&mut self, directory: Directory, frame: &mut ResolveFrame) {
        self.events.push(Event::Reserved(directory.0));
        frame.reserved = 1;
    }
    fn cluster_path(&mut self, frame: &mut ResolveFrame) -> RawStatus {
        self.events.push(Event::ClusterPath(frame.query.cluster, frame.reserved));
        frame.name[..self.name.len()].copy_from_slice(self.name);
        RawStatus(0)
    }
    fn scan_window(&mut self, frame: &mut ResolveFrame) -> RawStatus {
        let query = &frame.query;
        let window = &frame.window;
        self.events.push(Event::Scan(
            [query.start, query.cluster, query.marked, query.first, query.last, query.cursor, query.directory],
            window.position,
            window.name_sum,
        ));
        frame.window.first_total = self.scanned.0;
        frame.window.second_total = self.scanned.1;
        self.scan_raw
    }
}

#[test]
fn a_group_mask_with_a_bit_below_the_count_is_stored_and_rebuilds_the_catalog() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(4, 0x18);
    assert_eq!(select_groups(&mut port), 0);
    assert_eq!(port.group_mask, Some(0x18));
    assert_eq!(port.events, vec![Event::Rebuild(false, 0), Event::Complete(CALLBACK_WORD, 0)]);
}

#[test]
fn a_group_mask_with_bits_only_at_or_above_the_count_is_refused_with_eight() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(4, 0x10);
    assert_eq!(select_groups(&mut port), 8);
    assert_eq!(port.group_mask, None);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 8)]);
}

#[test]
fn a_group_count_of_32_or_more_admits_every_bit() {
    let mut port = Fake::new().with(4, 0x8000_0000);
    port.group_count = 40;
    assert_eq!(select_groups(&mut port), 0);
    assert_eq!(port.group_mask, Some(0x8000_0000));
}

#[test]
fn a_rebuild_passes_the_full_flag_and_mode_and_marks_mode_one() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(8, 1).with(0xc, 1);
    port.rebuild_raw = RawStatus(9);
    assert_eq!(rebuild_catalog(&mut port), 0);
    assert_eq!(port.events, vec![Event::Rebuild(true, 1), Event::Complete(CALLBACK_WORD, translated(9))]);
    assert_eq!(port.mark, 1);
    assert!(port.selection_cleared);
}

#[test]
fn a_rebuild_with_another_mode_and_flag_byte_clears_the_mark() {
    let mut port = Fake::new().with(8, 2).with(0xc, 0x201);
    port.mark = 1;
    rebuild_catalog(&mut port);
    assert_eq!(port.events, vec![Event::Rebuild(true, 2)]);
    assert_eq!(port.mark, 0);
}

#[test]
fn a_rebuild_without_a_context_delivers_raw_two_translated_and_still_clears_the_selection() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(8, 1);
    port.context = false;
    rebuild_catalog(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(2))]);
    assert!(port.selection_cleared);
}

fn find_request(mode: u32, commit: u32) -> Fake {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(4, RECORD).with(8, mode).with(0xc, commit).with(0x10, 3).with(0x14, 0xffff_ffff);
    port.payload[0x18..0x1c].copy_from_slice(b"100_");
    port
}

fn selectors(mode: u32, commit: u32) -> FindSelectors {
    FindSelectors { mode, commit, step: 3, word: 0xffff_ffff }
}

#[test]
fn a_find_clears_the_record_and_passes_the_selectors_name_and_record() {
    let mut port = find_request(1, 0);
    port.raw = RawStatus(4);
    assert_eq!(find_entry(&mut port), 0);
    assert_eq!(
        port.events,
        vec![
            Event::Fill(RECORD, 0, 188),
            Event::Find(selectors(1, 0), PAYLOAD + 0x18, RECORD),
            Event::Complete(CALLBACK_WORD, translated(4)),
        ]
    );
}

#[test]
fn a_find_with_an_empty_name_passes_no_name() {
    let mut port = find_request(1, 0);
    port.payload[0x18] = 0;
    find_entry(&mut port);
    assert_eq!(port.events[1], Event::Find(selectors(1, 0), 0, RECORD));
}

#[test]
fn a_pending_mark_rebuilds_first_and_clears_the_mark() {
    let mut port = find_request(1, 0);
    port.mark = 1;
    find_entry(&mut port);
    assert_eq!(&port.events[..2], &[Event::Rebuild(false, 0), Event::Fill(RECORD, 0, 188)]);
    assert_eq!(port.mark, 0);
}

#[test]
fn a_rewind_with_commit_rebuilds_and_a_failed_rebuild_is_delivered_without_finding() {
    let mut port = find_request(2, 1);
    port.mark = 2;
    port.rebuild_raw = RawStatus(7);
    find_entry(&mut port);
    assert_eq!(port.events, vec![Event::Rebuild(false, 0), Event::Complete(CALLBACK_WORD, translated(7))]);
    assert_eq!(port.mark, 0);
}

#[test]
fn a_find_without_a_context_delivers_raw_two_translated() {
    let mut port = find_request(1, 0);
    port.context = false;
    find_entry(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(2))]);
}

#[test]
fn a_completion_request_delivers_its_translated_result() {
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.raw = RawStatus(13);
    assert_eq!(complete_request(&mut port), 0);
    assert_eq!(port.events, vec![Event::CompleteRequest, Event::Complete(CALLBACK_WORD, translated(13))]);
}

const FIRST_AT: u32 = 0x910300;
const SECOND_AT: u32 = 0x910304;

fn resolve_request(index: u32, blocked: bool) -> Fake {
    let port = Fake::new().with(0, CALLBACK_WORD).with(4, index).with(8, FIRST_AT).with(0xc, SECOND_AT);
    let mut port = if blocked { port.with(0x10, 1).with(0x14, STATUS_AT) } else { port };
    port.counts = vec![(100, 3), (101, 0), (102, 6000), (103, 4)];
    port.totals = vec![(102, (1000, 1)), (103, (5, 7)), (999, (2, 3))];
    port
}

#[test]
fn an_index_resolves_to_its_directory_window_and_stores_the_window_and_later_totals() {
    let mut port = resolve_request(5010, true);
    assert_eq!(resolve_index(&mut port), 0);
    let sum: u32 = b"97.JPG".iter().map(|byte| u32::from(*byte)).sum();
    assert_eq!(
        port.events,
        vec![
            Event::Marked(102),
            Event::Cluster(102),
            Event::Reserved(102),
            Event::ClusterPath(0x33, 1),
            Event::Scan([0, 0x33, 1, 5000, 9999, 0, 102], 5007, sum),
            Event::Store(FIRST_AT, 0x10 + 5 + 2),
            Event::Store(SECOND_AT, 0x20 + 7 + 3),
            Event::Complete(CALLBACK_WORD, 0),
            Event::Store(STATUS_AT, 0),
            Event::Done,
        ]
    );
}

#[test]
fn an_index_past_every_directory_is_refused_with_eight() {
    let mut port = resolve_request(6007, true);
    resolve_index(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 8), Event::Store(STATUS_AT, 8), Event::Done]);
}

#[test]
fn a_name_shorter_than_eight_bytes_is_refused_without_a_scan() {
    let mut port = resolve_request(0, false);
    port.name = b"B:DSC";
    resolve_index(&mut port);
    assert_eq!(port.events.last(), Some(&Event::Complete(CALLBACK_WORD, 8)));
    assert!(!port.events.iter().any(|event| matches!(event, Event::Scan(..) | Event::Store(..))));
}

#[test]
fn a_refused_directory_or_scan_is_refused_with_eight() {
    let mut port = resolve_request(0, false);
    port.marked = RawStatus(1);
    resolve_index(&mut port);
    assert_eq!(port.events, vec![Event::Marked(100), Event::Complete(CALLBACK_WORD, 8)]);

    let mut port = resolve_request(0, false);
    port.scan_raw = RawStatus(3);
    resolve_index(&mut port);
    assert_eq!(port.events.last(), Some(&Event::Complete(CALLBACK_WORD, 8)));
    assert!(!port.events.iter().any(|event| matches!(event, Event::Store(..))));
}

#[test]
fn a_resolution_without_a_context_is_refused_with_eight() {
    let mut port = resolve_request(0, true);
    port.context = false;
    resolve_index(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 8), Event::Store(STATUS_AT, 8), Event::Done]);
}
