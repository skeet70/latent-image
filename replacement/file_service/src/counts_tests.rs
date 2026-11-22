use std::collections::HashMap;

use crate::completion::RawStatus;
use crate::counts::*;
use crate::requests::Callback;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    Clear,
    Split(u32),
    ExtensionKey(Vec<u8>),
    Byte4(u32),
    Byte5(u32),
    DirectoryNumber(Vec<u8>, u32),
    Cluster(u32),
    ClusterPath(u32, u32),
    Difference(Vec<u8>, Vec<u8>),
    Position(Lookup, Vec<u8>),
    FileNumber(Vec<u8>),
    Store(u32, u32),
    Translate(u32),
    Complete(u32, u32),
}

const PAYLOAD: u32 = 0x5000;
const CALLBACK_WORD: u32 = 0x380000;
const FIRST_AT: u32 = 0x910138;
const SECOND_AT: u32 = 0x91013c;
const PATTERN: &[u8; 12] = b"DSCF3097JPG\0";
const PREFIX: &[u8] = b"DCIM\\100_FUJI\\";

struct Fake {
    callback: u32,
    context: bool,
    entries: HashMap<u16, u32>,
    marked: (u32, u32),
    unmarked: (u32, u32),
    groups: HashMap<u32, u8>,
    starts: HashMap<u8, u32>,
    flags: Vec<u8>,
    page_files: HashMap<u32, u16>,
    directory_files: HashMap<u16, u16>,
    split: u32,
    marker: u32,
    key: u32,
    byte4: u32,
    byte5: u32,
    number: u32,
    /// Per page: the status and cluster 0x713d60 gives.
    clusters: HashMap<u32, (u32, u32)>,
    /// Clusters whose path 0x713db8 finds, and the text it writes.
    paths: HashMap<u32, Vec<u8>>,
    found: (u32, u32),
    file: u32,
    events: Vec<Event>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            callback: CALLBACK_WORD,
            context: true,
            entries: HashMap::from([(0x67, 1)]),
            marked: (300, 302),
            unmarked: (200, 201),
            groups: HashMap::new(),
            starts: HashMap::new(),
            flags: vec![0; 64],
            page_files: HashMap::new(),
            directory_files: HashMap::new(),
            split: 0,
            marker: 0,
            key: 0x77,
            byte4: 1,
            byte5: 0,
            number: 0x10067,
            clusters: HashMap::from([(0x67, (0, 0x900))]),
            paths: HashMap::from([(0x900, PREFIX.to_vec())]),
            found: (0, 5),
            file: 3097,
            events: Vec::new(),
        }
    }

    fn stores(&self) -> Vec<Event> {
        self.events.iter().filter(|event| matches!(event, Event::Store(..))).cloned().collect()
    }

    fn lookups(&self) -> Vec<Lookup> {
        self.events.iter().filter_map(|event| if let Event::Position(lookup, _) = event { Some(*lookup) } else { None }).collect()
    }

    fn pages(&self) -> Vec<u32> {
        self.events.iter().filter_map(|event| if let Event::Cluster(page) = event { Some(*page) } else { None }).collect()
    }

    fn translated(&self) -> Vec<u32> {
        self.events.iter().filter_map(|event| if let Event::Translate(raw) = event { Some(*raw) } else { None }).collect()
    }
}

fn padded(text: &[u8]) -> Vec<u8> {
    let mut bytes = text.to_vec();
    bytes.resize(32, 0);
    bytes
}

/// Raw 0 translates to 0 and other raw results to raw + 100.
fn translated(raw: u32) -> u32 {
    if raw == 0 { 0 } else { raw + 100 }
}

impl Port for Fake {
    fn word(&self, at: Offset) -> u32 {
        match at.0 {
            0 => self.callback,
            0x34 => FIRST_AT,
            0x38 => SECOND_AT,
            other => panic!("unexpected payload word {other:#x}"),
        }
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
    fn directory_entry(&self, directory: Directory) -> u32 {
        self.entries.get(&directory.0).copied().unwrap_or(0)
    }
    fn window(&self, window: Window) -> (Page, Page) {
        let (first, last) = match window {
            Window::Marked => self.marked,
            Window::Unmarked => self.unmarked,
        };
        (Page(first), Page(last))
    }
    fn group(&self, number: FileNumber) -> u8 {
        self.groups.get(&number.0).copied().unwrap_or(0)
    }
    fn group_start(&self, group: u8) -> FileNumber {
        FileNumber(self.starts[&group])
    }
    fn page_flags(&self, index: u32) -> u8 {
        self.flags[index as usize]
    }
    fn page_files(&self, page: Page) -> u16 {
        self.page_files.get(&page.0).copied().unwrap_or(0)
    }
    fn directory_files(&self, directory: Directory) -> u16 {
        self.directory_files.get(&directory.0).copied().unwrap_or(0)
    }
    fn clear(&mut self, buffer: &mut [u8; 32]) {
        *buffer = [0; 32];
        self.events.push(Event::Clear);
    }
    fn split_path(&mut self, path: u32, frame: &mut Frame) -> RawStatus {
        self.events.push(Event::Split(path));
        frame.pattern = *PATTERN;
        frame.directory[..9].copy_from_slice(b"100_FUJI\\");
        frame.prefix[..PREFIX.len()].copy_from_slice(PREFIX);
        frame.marker = self.marker;
        RawStatus(self.split)
    }
    fn extension_key(&mut self, frame: &Frame) -> ExtensionKey {
        self.events.push(Event::ExtensionKey(frame.pattern[8..].to_vec()));
        ExtensionKey(self.key)
    }
    fn extension_byte4(&mut self, key: ExtensionKey) -> u32 {
        self.events.push(Event::Byte4(key.0));
        self.byte4
    }
    fn extension_byte5(&mut self, key: ExtensionKey) -> u32 {
        self.events.push(Event::Byte5(key.0));
        self.byte5
    }
    fn directory_number(&mut self, frame: &Frame) -> u32 {
        self.events.push(Event::DirectoryNumber(frame.directory[..9].to_vec(), frame.marker));
        self.number
    }
    fn reserved_cluster(&mut self, page: Page, frame: &mut Frame) -> RawStatus {
        self.events.push(Event::Cluster(page.0));
        let (status, cluster) = self.clusters.get(&page.0).copied().unwrap_or((0, 0));
        frame.cluster = Cluster(cluster);
        RawStatus(status)
    }
    fn cluster_path(&mut self, frame: &mut Frame) -> u32 {
        self.events.push(Event::ClusterPath(frame.cluster.0, frame.marker));
        match self.paths.get(&frame.cluster.0) {
            Some(text) => {
                frame.query[..text.len()].copy_from_slice(text);
                0
            }
            None => 7,
        }
    }
    fn text_difference(&mut self, frame: &Frame) -> u32 {
        self.events.push(Event::Difference(frame.query.to_vec(), frame.prefix.to_vec()));
        u32::from(frame.query != frame.prefix)
    }
    fn directory_position(&mut self, lookup: Lookup, frame: &mut Frame) -> RawStatus {
        self.events.push(Event::Position(lookup, frame.pattern.to_vec()));
        frame.position = self.found.1;
        RawStatus(self.found.0)
    }
    fn file_number(&mut self, frame: &Frame) -> FileNumber {
        self.events.push(Event::FileNumber(frame.pattern.to_vec()));
        FileNumber(self.file)
    }
    fn translate(&mut self, raw: RawStatus) -> u32 {
        self.events.push(Event::Translate(raw.0));
        translated(raw.0)
    }
    fn complete_status(&mut self, callback: Callback, status: u32) {
        if callback.0 != 0 {
            self.events.push(Event::Complete(callback.0, status));
        }
    }
}

#[test]
fn position_in_a_listed_directory_calls_the_services_in_stock_order() {
    let mut fake = Fake::new();
    fake.directory_files = HashMap::from([(100, 4), (101, 6), (102, 1)]);
    assert_eq!(file_position(&mut fake), 0);
    let lookup = Lookup { cluster: Cluster(0x900), listed: true, page: Page(0x67) };
    assert_eq!(
        fake.events,
        vec![
            Event::Clear,
            Event::Split(PAYLOAD + 4),
            Event::ExtensionKey(b"JPG\0".to_vec()),
            Event::Byte4(0x77),
            Event::DirectoryNumber(b"100_FUJI\\".to_vec(), 0),
            Event::Cluster(0x67),
            Event::Clear,
            Event::ClusterPath(0x900, 0),
            Event::Difference(padded(PREFIX), padded(PREFIX)),
            Event::Position(lookup, PATTERN.to_vec()),
            Event::Store(FIRST_AT, 5 + 4 + 6 + 1),
            Event::Translate(0),
            Event::Complete(CALLBACK_WORD, 0),
        ]
    );
}

#[test]
fn counts_store_the_group_rank_and_the_folded_count() {
    let mut fake = Fake::new();
    fake.byte5 = 0;
    fake.found = (0, 4);
    fake.file = 12;
    fake.groups = HashMap::from([(12, 3), (8, 3), (9, 2), (10, 3), (11, 0)]);
    fake.starts = HashMap::from([(3, 8)]);
    fake.flags[..6].copy_from_slice(&[0, 1, 2, 3, 5, 6]);
    fake.page_files = HashMap::from([(100, 10), (101, 20), (102, 30)]);
    assert_eq!(file_counts(&mut fake), 0);
    assert!(fake.events.contains(&Event::Byte5(0x77)));
    // Of flags 0..=4, bytes 0, 1, 3 and 5 count from -1; pages 100..0x67 add 60.
    assert_eq!(fake.stores(), vec![Event::Store(SECOND_AT, 2), Event::Store(FIRST_AT, 3 + 60)]);
}

#[test]
fn counts_with_no_group_and_position_zero_read_one_flag() {
    let mut fake = Fake::new();
    fake.found = (0, 0);
    fake.number = 100;
    fake.entries = HashMap::from([(100, 1)]);
    fake.clusters = HashMap::from([(100, (0, 0x900))]);
    fake.flags[0] = 1;
    fake.flags[1] = 1;
    fake.page_files = HashMap::from([(100, 10)]);
    file_counts(&mut fake);
    assert_eq!(fake.stores(), vec![Event::Store(SECOND_AT, 0), Event::Store(FIRST_AT, 0)]);
}

#[test]
fn group_rank_is_zero_at_the_group_start() {
    let mut fake = Fake::new();
    fake.groups = HashMap::from([(3097, 5)]);
    fake.starts = HashMap::from([(5, 3097)]);
    file_counts(&mut fake);
    assert_eq!(fake.stores()[0], Event::Store(SECOND_AT, 0));
}

#[test]
fn the_unmarked_window_is_walked_until_a_cluster_path_matches() {
    let mut fake = Fake::new();
    fake.number = 50;
    fake.entries = HashMap::from([(50, 0xffff_ffff)]);
    fake.unmarked = (200, 204);
    fake.clusters = HashMap::from([(200, (3, 0x901)), (201, (0, 0)), (202, (0, 0x902)), (203, (0, 0x903)), (204, (0, 0x904))]);
    fake.paths = HashMap::from([(0x903, b"OTHER\\".to_vec()), (0x904, PREFIX.to_vec())]);
    assert_eq!(file_position(&mut fake), 0);
    assert_eq!(fake.pages(), vec![200, 201, 202, 203, 204]);
    assert_eq!(fake.lookups(), vec![Lookup { cluster: Cluster(0x904), listed: false, page: Page(204) }]);
    assert_eq!(fake.stores(), vec![Event::Store(FIRST_AT, 5)]);
}

#[test]
fn a_listed_directory_below_100_marks_the_lookup() {
    let mut fake = Fake::new();
    fake.number = 50;
    fake.entries = HashMap::from([(50, 1)]);
    fake.clusters = HashMap::from([(200, (0, 0x900))]);
    file_position(&mut fake);
    assert_eq!(fake.lookups(), vec![Lookup { cluster: Cluster(0x900), listed: true, page: Page(200) }]);
}

#[test]
fn the_marker_selects_the_marked_window_and_an_unlisted_lookup() {
    let mut fake = Fake::new();
    fake.marker = 1;
    fake.number = 0x1ff;
    fake.clusters = HashMap::from([(301, (0, 0x900))]);
    file_position(&mut fake);
    assert_eq!(fake.pages(), vec![300, 301]);
    assert!(fake.events.contains(&Event::ClusterPath(0x900, 1)));
    assert_eq!(fake.lookups(), vec![Lookup { cluster: Cluster(0x900), listed: false, page: Page(301) }]);
}

#[test]
fn an_empty_window_is_not_found_without_cluster_calls() {
    let mut fake = Fake::new();
    fake.number = 0;
    fake.unmarked = (5, 4);
    assert_eq!(file_position(&mut fake), translated(7));
    assert!(fake.pages().is_empty());
}

#[test]
fn an_exhausted_window_translates_the_last_cluster_status() {
    let mut fake = Fake::new();
    fake.number = 0;
    fake.clusters = HashMap::from([(200, (0, 0x900)), (201, (9, 0))]);
    fake.paths.clear();
    assert_eq!(file_position(&mut fake), translated(9));
    fake.clusters = HashMap::from([(201, (0, 0))]);
    fake.events.clear();
    assert_eq!(file_position(&mut fake), translated(7));
    assert!(fake.stores().is_empty());
}

#[test]
fn a_failed_lookup_stores_nothing() {
    let mut fake = Fake::new();
    fake.found = (13, 5);
    assert_eq!(file_position(&mut fake), translated(13));
    assert!(fake.stores().is_empty());
    assert_eq!(fake.events.last(), Some(&Event::Complete(CALLBACK_WORD, translated(13))));
}

#[test]
fn refusals_translate_their_raw_status() {
    let cases: Vec<(fn(&mut Fake), u32)> = vec![
        (|fake| fake.context = false, 2),
        (|fake| fake.split = 4, 4),
        (|fake| fake.key = 0, 4),
        (|fake| fake.byte4 = 0, 4),
    ];
    for (change, raw) in cases {
        let mut fake = Fake::new();
        change(&mut fake);
        assert_eq!(file_position(&mut fake), translated(raw));
        assert_eq!(fake.translated(), vec![raw]);
        assert!(fake.lookups().is_empty());
    }
}

#[test]
fn byte5_refuses_counts_but_not_position() {
    let mut fake = Fake::new();
    fake.byte5 = 1;
    assert_eq!(file_counts(&mut fake), 0);
    assert_eq!(fake.translated(), vec![4]);
    assert!(fake.stores().is_empty());
    let mut fake = Fake::new();
    fake.byte5 = 1;
    assert_eq!(file_position(&mut fake), 0);
    assert!(!fake.events.contains(&Event::Byte5(0x77)));
}

#[test]
fn no_callback_is_not_called() {
    let mut fake = Fake::new();
    fake.callback = 0;
    file_position(&mut fake);
    assert!(!fake.events.iter().any(|event| matches!(event, Event::Complete(..))));
}
