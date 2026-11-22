use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::selection::*;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    Split(u32),
    Kind(Vec<u8>),
    Supported(u32),
    Number(Vec<u8>, u32),
    Cluster(u32),
    DirectoryPath(u32, u32),
    Find { search: u32, cluster: u32, selected: u32, low: u32, high: u32, tag: u32, page: u32, pattern: Vec<u8> },
    Marked(u32),
    Counted(u32),
    Snapshot,
    Selected(u32),
    Load { search: u32, cluster: u32, selected: u32, low: u32, high: u32, page: u32 },
    Complete(u32),
}

const PATH_AT: u32 = 0x5004;
const CALLBACK_WORD: u32 = 0x380000;
const CONTEXT_BYTES: usize = 0x16c64;

struct Fake {
    context: Option<Vec<u8>>,
    split: RawStatus,
    directory: &'static [u8],
    marker: u32,
    prefix: &'static [u8],
    kind: u32,
    supported: u32,
    number: u32,
    /// Per page: the lookup status, the cluster and the directory path status and text.
    pages: Vec<(u32, RawStatus, u32, RawStatus, &'static [u8])>,
    find: RawStatus,
    position: u32,
    marked: u32,
    counted: Vec<u32>,
    selected: (RawStatus, u32),
    events: Vec<Event>,
}

fn context() -> Vec<u8> {
    vec![0; CONTEXT_BYTES]
}

impl Fake {
    fn new() -> Fake {
        Fake {
            context: Some(context()),
            split: RawStatus(0),
            directory: b"100_FUJI",
            marker: 0,
            prefix: b"B:\\DCIM\\100_FUJI\\",
            kind: 0x10,
            supported: 1,
            number: 0x67,
            pages: Vec::new(),
            find: RawStatus(0),
            position: 7,
            marked: 3,
            counted: Vec::new(),
            selected: (RawStatus(0), 1),
            events: Vec::new(),
        }
    }

    fn put(&mut self, at: u32, value: u32) {
        let context = self.context.as_mut().unwrap();
        context[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_half(&mut self, at: u32, value: u16) {
        let context = self.context.as_mut().unwrap();
        context[at as usize..at as usize + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_byte(&mut self, at: u32, value: u8) {
        self.context.as_mut().unwrap()[at as usize] = value;
    }

    fn get(&self, at: u32) -> u32 {
        let context = self.context.as_ref().unwrap();
        u32::from_le_bytes(context[at as usize..at as usize + 4].try_into().unwrap())
    }

    fn get_half(&self, at: u32) -> u16 {
        let context = self.context.as_ref().unwrap();
        u16::from_le_bytes(context[at as usize..at as usize + 2].try_into().unwrap())
    }

    fn page(&self, page: u32) -> Option<&(u32, RawStatus, u32, RawStatus, &'static [u8])> {
        self.pages.iter().find(|entry| entry.0 == page)
    }

    fn run(&mut self) -> u32 {
        select(self)
    }
}

fn text(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().take_while(|byte| *byte != 0).collect()
}

/// Raw 0 translates to 0 and other raw results to raw + 100.
fn translated(raw: u32) -> u32 {
    if raw == 0 { 0 } else { raw + 100 }
}

impl Port for Fake {
    fn callback(&self) -> Callback {
        Callback(CALLBACK_WORD)
    }
    fn path(&self) -> u32 {
        PATH_AT
    }
    fn context(&self) -> bool {
        self.context.is_some()
    }
    fn word(&self, at: ContextOffset) -> u32 {
        self.get(at.0)
    }
    fn set_word(&mut self, at: ContextOffset, value: u32) {
        self.put(at.0, value)
    }
    fn half(&self, at: ContextOffset) -> u16 {
        self.get_half(at.0)
    }
    fn set_half(&mut self, at: ContextOffset, value: u16) {
        self.put_half(at.0, value)
    }
    fn byte(&self, at: ContextOffset) -> u8 {
        self.context.as_ref().unwrap()[at.0 as usize]
    }
    fn clear(&mut self, buffer: &mut [u8; 32]) {
        buffer.fill(0);
    }
    fn split_path(&mut self, path: u32, frame: &mut Frame) -> RawStatus {
        self.events.push(Event::Split(path));
        frame.directory[..self.directory.len()].copy_from_slice(self.directory);
        frame.pattern.copy_from_slice(b"DSCF3097JPG\0");
        frame.marker = self.marker;
        frame.prefix[..self.prefix.len()].copy_from_slice(self.prefix);
        self.split
    }
    fn extension_kind(&mut self, frame: &Frame) -> ExtensionKind {
        self.events.push(Event::Kind(text(&frame.pattern[8..])));
        ExtensionKind(self.kind)
    }
    fn kind_supported(&mut self, kind: ExtensionKind) -> u32 {
        self.events.push(Event::Supported(kind.0));
        self.supported
    }
    fn directory_number(&mut self, frame: &Frame) -> u32 {
        self.events.push(Event::Number(text(&frame.directory), frame.marker));
        self.number
    }
    fn page_cluster(&mut self, page: Page, frame: &mut Frame) -> RawStatus {
        self.events.push(Event::Cluster(page.0));
        let (status, cluster) = self.page(page.0).map_or((RawStatus(0), 0), |entry| (entry.1, entry.2));
        frame.cluster = cluster;
        status
    }
    fn directory_path(&mut self, frame: &mut Frame) -> RawStatus {
        self.events.push(Event::DirectoryPath(frame.cluster, frame.marker));
        let cluster = frame.cluster;
        let (status, path) = self.pages.iter().find(|entry| entry.2 == cluster).map(|entry| (entry.3, entry.4)).unwrap();
        frame.found[..path.len()].copy_from_slice(path);
        status
    }
    fn paths_differ(&mut self, frame: &Frame) -> bool {
        text(&frame.found) != text(&frame.prefix)
    }
    fn find(&mut self, frame: &mut Frame) -> RawStatus {
        let query = &frame.query;
        assert_eq!(query.pattern, frame.pattern.as_ptr());
        self.events.push(Event::Find {
            search: query.search,
            cluster: query.cluster,
            selected: query.selected,
            low: query.low,
            high: query.high,
            tag: query.tag,
            page: query.page,
            pattern: text(&frame.pattern),
        });
        frame.position = self.position;
        self.find
    }
    fn marked_before(&mut self, position: Position) -> u32 {
        self.events.push(Event::Marked(position.0));
        self.marked
    }
    fn page_counted(&mut self, page: Page) -> u32 {
        self.events.push(Event::Counted(page.0));
        u32::from(self.counted.contains(&page.0))
    }
    fn refresh_snapshot(&mut self) {
        self.events.push(Event::Snapshot);
    }
    fn page_selected(&mut self, page: Page, frame: &mut Frame) -> RawStatus {
        self.events.push(Event::Selected(page.0));
        frame.selected = self.selected.1;
        self.selected.0
    }
    fn load_window(&mut self, frame: &mut Frame) {
        let query = &frame.query;
        self.events.push(Event::Load {
            search: query.search,
            cluster: query.cluster,
            selected: query.selected,
            low: query.low,
            high: query.high,
            page: query.page,
        });
    }
    fn translate(&mut self, raw: RawStatus) -> u32 {
        translated(raw.0)
    }
    fn complete_status(&mut self, callback: Callback, status: u32) {
        assert_eq!(callback, Callback(CALLBACK_WORD));
        self.events.push(Event::Complete(status));
    }
}

const MATCH: &[u8] = b"B:\\DCIM\\100_FUJI\\";
const OTHER: &[u8] = b"B:\\DCIM\\101_FUJI\\";

fn find_on(page: u32, cluster: u32, selected: u32) -> Event {
    Event::Find { search: 1, cluster, selected, low: 0, high: 4999, tag: u32::MAX, page, pattern: b"DSCF3097JPG".to_vec() }
}

#[test]
fn without_context_delivers_the_untranslated_absence() {
    let mut fake = Fake::new();
    fake.context = None;
    assert_eq!(fake.run(), 0);
    assert_eq!(fake.events, vec![Event::Complete(translated(2))]);
}

#[test]
fn a_refused_path_is_delivered_without_a_reload() {
    let mut fake = Fake::new();
    fake.split = RawStatus(4);
    fake.run();
    assert_eq!(fake.events, vec![Event::Split(PATH_AT), Event::Complete(translated(4))]);
}

#[test]
fn an_unknown_extension_reloads_the_window_holding_the_current_position() {
    let mut fake = Fake::new();
    fake.kind = 0;
    fake.put(0x2170, 0x65);
    fake.put(0x2174, 5003);
    fake.pages = vec![(0x65, RawStatus(0), 0x880, RawStatus(0), MATCH)];
    fake.run();
    assert_eq!(
        fake.events,
        vec![
            Event::Split(PATH_AT),
            Event::Kind(b"JPG".to_vec()),
            Event::Cluster(0x65),
            Event::Selected(0x65),
            Event::Load { search: 0, cluster: 0x880, selected: 1, low: 5000, high: 9999, page: 0x65 },
            Event::Complete(translated(4)),
        ]
    );
}

#[test]
fn an_unsupported_kind_skips_the_reload_when_the_current_page_has_no_cluster() {
    let mut fake = Fake::new();
    fake.supported = 0;
    fake.put(0x2170, 0x65);
    fake.run();
    assert_eq!(
        fake.events,
        vec![
            Event::Split(PATH_AT),
            Event::Kind(b"JPG".to_vec()),
            Event::Supported(0x10),
            Event::Cluster(0x65),
            Event::Complete(translated(4)),
        ]
    );
}

#[test]
fn a_known_numbered_directory_becomes_current() {
    let mut fake = Fake::new();
    fake.number = 0x10066;
    fake.put(0x2604 + 4 * 0x66, 1);
    fake.put(0x21a4, 40);
    fake.put(0x21b0, 9);
    fake.put(0x21b4, 2);
    fake.put(0xb2a4 + 4 * 7, 0x5512_3456);
    fake.put_byte(0x2218 + 0x66, 3);
    fake.pages = vec![(0x66, RawStatus(0), 0x900, RawStatus(0), MATCH)];
    for (index, flags) in [0u8, 1, 4, 3, 2, 0, 6, 5, 9].iter().enumerate() {
        fake.put_byte(0x15846 + index as u32, *flags);
    }
    fake.put_half(0x156b6 + 2 * 2, 0x31);
    fake.put_half(0x1270e + 2 * 0x66, 12);
    fake.put_half(0x1270e + 2 * 100, 5);
    fake.put_half(0x1270e + 2 * 101, 6);
    fake.put_half(0x34dc + 2 * 100, 20);
    fake.put_half(0x34dc + 2 * 101, 30);
    fake.counted = vec![101];
    fake.run();
    assert_eq!(
        fake.events,
        vec![
            Event::Split(PATH_AT),
            Event::Kind(b"JPG".to_vec()),
            Event::Supported(0x10),
            Event::Number(b"100_FUJI".to_vec(), 0),
            Event::Cluster(0x66),
            Event::DirectoryPath(0x900, 0),
            find_on(0x66, 0x900, 1),
            Event::Marked(7),
            Event::Counted(0x66),
            Event::Counted(100),
            Event::Counted(101),
            Event::Snapshot,
            Event::Complete(0),
        ]
    );
    assert_eq!(fake.get(0x2174), 7);
    assert_eq!(fake.get(0x2170), 0x66);
    assert_eq!(fake.get(0x2148), 41);
    assert_eq!(fake.get(0x214c), 7);
    assert_eq!(fake.get(0x2158), 9);
    assert_eq!(fake.get(0x215c), 5);
    assert_eq!(fake.get(0x2164), 0x12_3456);
    assert_eq!(fake.get(0x2210), 8);
    assert_eq!(fake.get(0x2160), 57);
    assert_eq!(fake.get(0x2214), 37);
    assert_eq!(fake.get(0x16c60), 0x31);
    assert_eq!(fake.get(0x2150), 12);
    // Positions 0 to 7: flags 0, 1, 3, 0 and 5 count five files from 0xffff.
    assert_eq!(fake.get(0x2154), 4);
    assert_eq!(fake.get_half(0x16bce), 15);
}

#[test]
fn the_directory_range_is_searched_through_its_last_page() {
    let mut fake = Fake::new();
    fake.number = 0x2a;
    fake.put(0x2604 + 4 * 0x2a, 1);
    fake.put(0x218c, 0x64);
    fake.put(0x2190, 0x67);
    fake.put_byte(0x2218 + 0x67, 0xff);
    fake.pages = vec![
        (0x64, RawStatus(9), 0, RawStatus(0), MATCH),
        (0x65, RawStatus(0), 0x901, RawStatus(3), MATCH),
        (0x66, RawStatus(0), 0x902, RawStatus(0), OTHER),
        (0x67, RawStatus(0), 0x903, RawStatus(0), MATCH),
    ];
    fake.run();
    assert_eq!(
        &fake.events[3..10],
        &[
            Event::Number(b"100_FUJI".to_vec(), 0),
            Event::Cluster(0x64),
            Event::Cluster(0x65),
            Event::DirectoryPath(0x901, 0),
            Event::Cluster(0x66),
            Event::DirectoryPath(0x902, 0),
            Event::Cluster(0x67),
        ]
    );
    assert_eq!(fake.events[11], find_on(0x67, 0x903, 1));
    assert_eq!(fake.get(0x2170), 0x67);
    assert_eq!(fake.get(0x2210), 0);
}

#[test]
fn an_exhausted_marked_range_keeps_the_last_lookup_failure() {
    let mut fake = Fake::new();
    fake.marker = 1;
    fake.put(0x2178, 3);
    fake.put(0x217c, 4);
    fake.put(0x2170, 4);
    fake.pages = vec![(3, RawStatus(0), 0x901, RawStatus(0), OTHER), (4, RawStatus(9), 0, RawStatus(0), MATCH)];
    fake.run();
    assert_eq!(fake.events.last(), Some(&Event::Complete(translated(9))));
    assert!(!fake.events.iter().any(|event| matches!(event, Event::Find { .. })));
}

#[test]
fn an_exhausted_range_without_failures_is_not_found() {
    let mut fake = Fake::new();
    fake.put(0x218c, 3);
    fake.put(0x2190, 3);
    fake.run();
    assert_eq!(fake.events.last(), Some(&Event::Complete(translated(7))));
}

#[test]
fn an_empty_range_is_not_found_and_reloads() {
    let mut fake = Fake::new();
    fake.put(0x218c, 5);
    fake.put(0x2190, 4);
    fake.put(0x2170, 0x65);
    fake.pages = vec![(0x65, RawStatus(0), 0x880, RawStatus(0), MATCH)];
    fake.selected = (RawStatus(6), 0);
    fake.run();
    assert_eq!(
        &fake.events[4..],
        &[Event::Cluster(0x65), Event::Selected(0x65), Event::Complete(translated(7))]
    );
}

#[test]
fn a_failed_find_reloads_with_the_found_query_tag() {
    let mut fake = Fake::new();
    fake.number = 0x66;
    fake.put(0x2604 + 4 * 0x66, 1);
    fake.put(0x2170, 0x66);
    fake.put(0x2174, 12);
    fake.find = RawStatus(7);
    fake.pages = vec![(0x66, RawStatus(0), 0x900, RawStatus(0), MATCH)];
    fake.selected = (RawStatus(0), 0);
    fake.run();
    assert_eq!(
        &fake.events[7..],
        &[
            Event::Cluster(0x66),
            Event::Selected(0x66),
            Event::Load { search: 0, cluster: 0x900, selected: 0, low: 0, high: 4999, page: 0x66 },
            Event::Complete(translated(7)),
        ]
    );
    assert_eq!(fake.get(0x2174), 12);
}
