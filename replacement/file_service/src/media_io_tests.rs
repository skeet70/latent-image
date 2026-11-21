use crate::completion::RawStatus;
use crate::handles::FileIndex;
use crate::media_io::*;
use crate::requests::Callback;
use crate::workers::Offset;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    Complete(u32, u32, Option<u32>),
    Store(u32, u32),
    Done,
    Geometry(u32),
    Flag(u32),
    Units(u32),
    Query(u32, u8),
    Transfer(Direction, u32, Pages),
    Pass(Pattern, Step),
    Read(u32, u32, u32),
    LocalSize(u32),
    LocalRead(u32, u32, u32, u32, u32),
    LocalPosition(u32),
}

const PAYLOAD: u32 = 0x5000;
const CALLBACK_WORD: u32 = 0x380000;
const STATUS_AT: u32 = 0x910138;
const VALUE_AT: u32 = 0x91013c;
const DEVICE: u32 = 7;

struct Fake {
    payload: Vec<u8>,
    context: bool,
    capacity: u32,
    attach_state: u32,
    map: Vec<(u32, u32)>,
    raw: RawStatus,
    geometry: Geometry,
    flag: u32,
    units: u32,
    query: u32,
    passes: Vec<u32>,
    count: u32,
    local: LocalRecord,
    local_size: (u32, u32),
    local_read: u32,
    events: Vec<Event>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            payload: vec![0; 0x40],
            context: true,
            capacity: 0x100,
            attach_state: 0,
            map: vec![(0x41, 3)],
            raw: RawStatus(0),
            geometry: Geometry { low: 0x1234_5000, high: 0 },
            flag: 0,
            units: 0,
            query: 0,
            passes: Vec::new(),
            count: 0,
            local: LocalRecord { mode: 0, handle: 0, cursor: 0, position: 0 },
            local_size: (0, 0),
            local_read: 0,
            events: Vec::new(),
        }
    }

    fn with(mut self, offset: u32, value: u32) -> Fake {
        self.payload[offset as usize..offset as usize + 4].copy_from_slice(&value.to_le_bytes());
        self
    }

    fn get(&self, offset: u32) -> u32 {
        u32::from_le_bytes(self.payload[offset as usize..offset as usize + 4].try_into().unwrap())
    }
}

/// Raw 0 translates to 0 and other raw results to raw + 100.
fn translated(raw: u32) -> u32 {
    if raw == 0 { 0 } else { raw + 100 }
}

impl Port for Fake {
    fn word(&self, at: Offset) -> u32 {
        self.get(at.0)
    }
    fn set_word(&mut self, at: Offset, value: u32) {
        self.payload[at.0 as usize..at.0 as usize + 4].copy_from_slice(&value.to_le_bytes());
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
    fn device(&self) -> Option<Device> {
        self.context.then_some(Device(DEVICE))
    }
    fn capacity(&self) -> Sectors {
        Sectors(self.capacity)
    }
    fn reset_attach_state(&mut self) {
        self.attach_state = u32::MAX;
    }
    fn lookup(&mut self, tag: u32) -> FileIndex {
        FileIndex(self.map.iter().find(|pair| pair.0 == tag).map_or(0, |pair| pair.1))
    }
    fn translate(&mut self, raw: RawStatus) -> u32 {
        translated(raw.0)
    }
    fn complete(&mut self, callback: Callback, status: u32, value: u32) {
        if callback.0 != 0 {
            self.events.push(Event::Complete(callback.0, status, Some(value)));
        }
    }
    fn complete_status(&mut self, callback: Callback, status: u32) {
        if callback.0 != 0 {
            self.events.push(Event::Complete(callback.0, status, None));
        }
    }
    fn done(&mut self) {
        self.events.push(Event::Done);
    }
    fn geometry(&mut self, device: Device) -> Geometry {
        self.events.push(Event::Geometry(device.0));
        self.geometry
    }
    fn geometry_flag(&mut self, device: Device) -> (RawStatus, u32) {
        self.events.push(Event::Flag(device.0));
        (self.raw, self.flag)
    }
    fn geometry_units(&mut self, device: Device) -> u32 {
        self.events.push(Event::Units(device.0));
        self.units
    }
    fn query_path(&mut self, path: u32, flag: u8) -> u32 {
        self.events.push(Event::Query(path, flag));
        self.query
    }
    fn transfer_pages(&mut self, direction: Direction, device: Device, pages: Pages) -> RawStatus {
        self.events.push(Event::Transfer(direction, device.0, pages));
        self.raw
    }
    fn pattern_pass(&mut self, pattern: Pattern, step: Step) -> u32 {
        self.events.push(Event::Pass(pattern, step));
        if self.passes.is_empty() { 0 } else { self.passes.remove(0) }
    }
    fn fs_read(&mut self, index: FileIndex, buffer: u32, length: u32) -> (u32, RawStatus) {
        self.events.push(Event::Read(index.0, buffer, length));
        (self.count, self.raw)
    }
    fn local_record(&mut self) -> LocalRecord {
        self.local
    }
    fn local_size(&mut self, handle: u32) -> (LocalStatus, u32) {
        self.events.push(Event::LocalSize(handle));
        (LocalStatus(self.local_size.0), self.local_size.1)
    }
    fn local_read(&mut self, record: &LocalRecord, buffer: u32, length: u32) -> LocalStatus {
        self.events.push(Event::LocalRead(record.handle, record.cursor, buffer, record.position, length));
        LocalStatus(self.local_read)
    }
    fn local_position(&mut self, position: u32) {
        self.events.push(Event::LocalPosition(position));
    }
}

#[test]
fn capacity_reports_the_geometry_size_when_the_flag_is_clear() {
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    assert_eq!(capacity(&mut port), 0);
    assert_eq!(port.events, vec![Event::Geometry(DEVICE), Event::Flag(DEVICE), Event::Complete(CALLBACK_WORD, 0, Some(0x1234_5000))]);
}

#[test]
fn capacity_with_a_high_geometry_word_reports_all_ones() {
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.geometry.high = 1;
    capacity(&mut port);
    assert_eq!(port.events.last(), Some(&Event::Complete(CALLBACK_WORD, 0, Some(u32::MAX))));
}

#[test]
fn capacity_with_the_flag_set_reports_units_of_0x40000_bytes_below_0x4000_units() {
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.flag = 1;
    port.units = 0x3fff;
    capacity(&mut port);
    assert_eq!(
        port.events,
        vec![Event::Geometry(DEVICE), Event::Flag(DEVICE), Event::Units(DEVICE), Event::Complete(CALLBACK_WORD, 0, Some(0xfffc_0000))]
    );
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.flag = 1;
    port.units = 0x4000;
    capacity(&mut port);
    assert_eq!(port.events.last(), Some(&Event::Complete(CALLBACK_WORD, 0, Some(u32::MAX))));
}

#[test]
fn capacity_failure_and_missing_context_deliver_the_translated_status_and_no_size() {
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.raw = RawStatus(0x102);
    port.flag = 1;
    capacity(&mut port);
    assert_eq!(port.events, vec![Event::Geometry(DEVICE), Event::Flag(DEVICE), Event::Complete(CALLBACK_WORD, translated(0x102), Some(0))]);
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.context = false;
    assert_eq!(capacity(&mut port), 0);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(2), Some(0))]);
}

fn passes_of(repeats: usize) -> Vec<Event> {
    let one = [(0x55, 1), (0xaa, 1), (0xffff, 1), (0x55, 256), (0xaa, 256), (0xffff, 256)];
    (0..repeats).flat_map(|_| one.iter().map(|&(pattern, step)| Event::Pass(Pattern(pattern), Step(step)))).collect()
}

#[test]
fn the_pattern_test_runs_six_passes_per_requested_repeat() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(8, 2);
    assert_eq!(pattern_test(&mut port), 0);
    let mut expected = passes_of(2);
    expected.push(Event::Complete(CALLBACK_WORD, 0, None));
    assert_eq!(port.events, expected);
}

#[test]
fn the_pattern_test_stops_at_the_first_failing_pass_and_translates_its_status() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(8, 3);
    port.passes = vec![0, 0, 0, 0, 0, 0, 0, 8];
    assert_eq!(pattern_test(&mut port), translated(8));
    let mut expected = passes_of(2);
    expected.truncate(8);
    expected.push(Event::Complete(CALLBACK_WORD, translated(8), None));
    assert_eq!(port.events, expected);
}

#[test]
fn the_pattern_test_without_repeats_or_context_runs_no_pass() {
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    assert_eq!(pattern_test(&mut port), 0);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 0, None)]);
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(8, 1);
    port.context = false;
    assert_eq!(pattern_test(&mut port), translated(5));
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(5), None)]);
}

#[test]
fn the_device_query_reports_eleven_for_any_failure() {
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.payload[0x24] = 3;
    assert_eq!(query_device(&mut port), 0);
    assert_eq!(port.events, vec![Event::Query(PAYLOAD + 4, 3), Event::Complete(CALLBACK_WORD, 0, None)]);
    let mut port = Fake::new().with(0, CALLBACK_WORD);
    port.query = 4;
    port.context = false;
    assert_eq!(query_device(&mut port), 11);
    assert_eq!(port.events, vec![Event::Query(PAYLOAD + 4, 0), Event::Complete(CALLBACK_WORD, 11, None)]);
}

fn pages_request(first: u32, count: u32) -> Fake {
    Fake::new().with(0, CALLBACK_WORD).with(4, first).with(8, count).with(0xc, 0xd00000)
}

#[test]
fn a_page_transfer_up_to_the_capacity_reaches_the_device_and_resets_the_attach_state() {
    for direction in [Direction::Read, Direction::Write] {
        let mut port = pages_request(0xfe, 2);
        port.raw = RawStatus(13);
        assert_eq!(transfer(&mut port, direction), translated(13));
        let pages = Pages { first: Sectors(0xfe), count: Sectors(2), buffer: 0xd00000 };
        assert_eq!(port.events, vec![Event::Transfer(direction, DEVICE, pages), Event::Complete(CALLBACK_WORD, translated(13), None)]);
        assert_eq!(port.attach_state, u32::MAX);
    }
}

#[test]
fn a_page_transfer_past_the_capacity_or_of_no_sectors_refuses_with_nineteen() {
    for (first, count) in [(0xff, 2), (0, 0), (0x100, 1)] {
        let mut port = pages_request(first, count);
        assert_eq!(transfer(&mut port, Direction::Read), translated(19));
        assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(19), None)]);
        assert_eq!(port.attach_state, u32::MAX);
    }
}

#[test]
fn a_page_transfer_without_a_context_refuses_with_two() {
    let mut port = pages_request(0, 1).with(0, 0);
    port.context = false;
    assert_eq!(transfer(&mut port, Direction::Write), translated(2));
    assert!(port.events.is_empty());
    assert_eq!(port.attach_state, u32::MAX);
}

fn read_request(blocking: bool) -> Fake {
    let port = Fake::new().with(0, CALLBACK_WORD).with(4, 0x41).with(8, 0x200).with(0xc, 0xd00001);
    if blocking { port.with(0x10, 1).with(0x14, STATUS_AT).with(0x18, VALUE_AT) } else { port }
}

#[test]
fn a_card_read_reads_the_file_index_and_delivers_the_count_to_the_callback_and_the_blocking_caller() {
    let mut port = read_request(true);
    port.count = 0x180;
    port.raw = RawStatus(37);
    assert_eq!(read(&mut port), translated(37));
    assert_eq!(
        port.events,
        vec![
            Event::Read(3, 0xd00001, 0x200),
            Event::Complete(CALLBACK_WORD, translated(37), Some(0x180)),
            Event::Store(STATUS_AT, translated(37)),
            Event::Store(VALUE_AT, 0x180),
            Event::Done,
        ]
    );
    assert_eq!((port.get(0), port.get(4)), (CALLBACK_WORD, 3));
}

#[test]
fn a_card_read_without_a_context_delivers_status_two_and_no_count() {
    let mut port = read_request(false).with(4, 0x99);
    port.context = false;
    assert_eq!(read(&mut port), translated(2));
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(2), Some(0))]);
    assert_eq!(port.get(4), 0);
}

fn local_request(position: u32, size: u32) -> Fake {
    let mut port = read_request(true);
    port.local = LocalRecord { mode: 1, handle: 2, cursor: 9, position };
    port.local_size = (0, size);
    port
}

#[test]
fn a_redirected_read_reads_into_the_even_buffer_advances_the_position_and_signals_nobody() {
    let mut port = local_request(0x100, 0x1000);
    assert_eq!(read(&mut port), 0);
    assert_eq!(
        port.events,
        vec![
            Event::LocalSize(2),
            Event::LocalRead(2, 9, 0xd00002, 0x100, 0x200),
            Event::LocalPosition(0x300),
            Event::Complete(CALLBACK_WORD, 0, Some(0x200)),
        ]
    );
}

#[test]
fn a_redirected_read_past_the_source_end_is_clipped() {
    let mut port = local_request(0xf00, 0x1000);
    read(&mut port);
    assert_eq!(port.events[1], Event::LocalRead(2, 9, 0xd00002, 0xf00, 0x100));
    assert_eq!(port.events[3], Event::Complete(CALLBACK_WORD, 0, Some(0x100)));
}

#[test]
fn a_refused_redirected_read_delivers_five_untranslated() {
    let mut port = local_request(0, 0x1000);
    port.local_size = (u32::MAX - 29, 0);
    assert_eq!(read(&mut port), 5);
    assert_eq!(port.events, vec![Event::LocalSize(2), Event::Complete(CALLBACK_WORD, 5, Some(0x200))]);
    let mut port = local_request(0xf00, 0x1000);
    port.local_read = 1;
    assert_eq!(read(&mut port), 5);
    assert_eq!(port.events.last(), Some(&Event::Complete(CALLBACK_WORD, 5, Some(0x100))));
    assert!(!port.events.iter().any(|event| matches!(event, Event::LocalPosition(_) | Event::Done)));
}
