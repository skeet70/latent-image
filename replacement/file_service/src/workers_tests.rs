use crate::completion::RawStatus;
use crate::handles::FileIndex;
use crate::requests::Callback;
use crate::workers::*;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    ReleaseTag(u32),
    Unbind(u32),
    Bind(u32),
    Complete(u32, u32, Option<u32>),
    Store(u32, u32),
    Done,
    MakeDirectory(u16, u32),
    Open(Vec<u8>, u8, u32, u32),
    Register(u32),
    Write(u32, u32, u32),
    Seek(u32, u32, u32, u32, u32),
    Close(u32),
    CloseNamed(u32, u32, u32),
    CloseEntry(u32, u8, u32, u32),
    Release(u32),
    CacheRelease(u32),
    Refresh,
    AfterClose(u32),
    Rescan,
}

const PAYLOAD: u32 = 0x5000;
const CALLBACK_WORD: u32 = 0x380000;
const STATUS_AT: u32 = 0x910138;
const VALUE_AT: u32 = 0x91013c;

struct Fake {
    payload: Vec<u8>,
    context: bool,
    modes: [u32; 8],
    kinds: [u32; 8],
    flagged: bool,
    known: bool,
    map: Vec<(u32, u32)>,
    bound: u32,
    raw: RawStatus,
    count: u32,
    slot: u32,
    rescan: u32,
    events: Vec<Event>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            payload: vec![0; 0x60],
            context: true,
            modes: [0; 8],
            kinds: [0; 8],
            flagged: false,
            known: false,
            map: vec![(0x41, 3)],
            bound: 5,
            raw: RawStatus(0),
            count: 0,
            slot: 2,
            rescan: 0x77,
            events: Vec::new(),
        }
    }

    fn with(mut self, offset: u32, value: u32) -> Fake {
        self.put(offset, value);
        self
    }

    fn put(&mut self, offset: u32, value: u32) {
        self.payload[offset as usize..offset as usize + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn get(&self, offset: u32) -> u32 {
        u32::from_le_bytes(self.payload[offset as usize..offset as usize + 4].try_into().unwrap())
    }
}

/// Raw 0 translates to 0 and other raw results to raw + 100, so a test sees which result was
/// translated.
fn translated(raw: u32) -> u32 {
    if raw == 0 { 0 } else { raw + 100 }
}

impl Port for Fake {
    fn word(&self, at: Offset) -> u32 {
        self.get(at.0)
    }
    fn set_word(&mut self, at: Offset, value: u32) {
        self.put(at.0, value)
    }
    fn byte(&self, at: Offset) -> u8 {
        self.payload[at.0 as usize]
    }
    fn address(&self, at: Offset) -> u32 {
        PAYLOAD + at.0
    }
    fn name(&self, at: Offset) -> Name {
        self.payload[at.0 as usize..at.0 as usize + 48].try_into().unwrap()
    }
    fn store(&mut self, address: u32, value: u32) {
        self.events.push(Event::Store(address, value));
    }
    fn context(&self) -> bool {
        self.context
    }
    fn file_mode(&self, index: FileIndex) -> u32 {
        self.modes[index.0 as usize]
    }
    fn set_file_mode(&mut self, index: FileIndex, mode: u32) {
        self.modes[index.0 as usize] = mode;
    }
    fn file_kind(&self, index: FileIndex) -> u32 {
        self.kinds[index.0 as usize]
    }
    fn mark_flagged_close(&mut self) {
        self.flagged = true;
    }
    fn directory_known(&self, _: u16) -> bool {
        self.known
    }
    fn release_tag(&mut self, tag: u32) {
        self.events.push(Event::ReleaseTag(tag));
    }
    fn lookup(&mut self, tag: u32) -> FileIndex {
        FileIndex(self.map.iter().find(|pair| pair.0 == tag).map_or(0, |pair| pair.1))
    }
    fn unbind(&mut self, tag: u32) {
        self.events.push(Event::Unbind(tag));
        self.map.retain(|pair| pair.0 != tag);
    }
    fn bind(&mut self, tag: u32) -> FileIndex {
        self.events.push(Event::Bind(tag));
        self.map.push((tag, self.bound));
        FileIndex(self.bound)
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
    fn parse_path(&mut self, frame: &mut PathFrame, _: u32) -> u32 {
        frame.parsed = 9;
        0
    }
    fn directory_slot(&mut self, parsed: u32) -> u32 {
        assert_eq!(parsed, 9);
        self.slot
    }
    fn split_path(&mut self, _: u32, frame: &mut PathFrame) {
        frame.split_word = 4;
    }
    fn directory_number(&mut self, frame: &PathFrame) -> u32 {
        assert_eq!(frame.split_word, 4);
        0x1_0067
    }
    fn make_directory(&mut self, directory: u16, slot: u32) {
        self.events.push(Event::MakeDirectory(directory, slot));
    }
    fn fs_open(&mut self, name: &Name, mode: OpenMode, index: FileIndex, word: u32) -> RawStatus {
        self.events.push(Event::Open(name[..8].to_vec(), mode.0, index.0, word));
        self.raw
    }
    fn register_written(&mut self, index: FileIndex, _: &Name) {
        self.events.push(Event::Register(index.0));
    }
    fn fs_write(&mut self, index: FileIndex, source: u32, length: u32) -> (u32, RawStatus) {
        self.events.push(Event::Write(index.0, source, length));
        (self.count, self.raw)
    }
    fn fs_seek(&mut self, index: FileIndex, pad: u32, low: u32, high: u32, whence: u32) -> RawStatus {
        self.events.push(Event::Seek(index.0, pad, low, high, whence));
        self.raw
    }
    fn fs_close(&mut self, index: FileIndex) -> RawStatus {
        self.events.push(Event::Close(index.0));
        self.raw
    }
    fn fs_close_named(&mut self, index: FileIndex, word: u32, name: u32) -> RawStatus {
        self.events.push(Event::CloseNamed(index.0, word, name));
        self.raw
    }
    fn fs_close_entry(&mut self, index: FileIndex, attribute: u8, first: u32, second: u32) -> RawStatus {
        self.events.push(Event::CloseEntry(index.0, attribute, first, second));
        self.raw
    }
    fn fs_release(&mut self, index: FileIndex) -> RawStatus {
        self.events.push(Event::Release(index.0));
        self.raw
    }
    fn cache_release(&mut self, index: FileIndex) {
        self.events.push(Event::CacheRelease(index.0));
    }
    fn refresh_media(&mut self) {
        self.events.push(Event::Refresh);
    }
    fn after_written_close(&mut self, index: FileIndex) {
        self.events.push(Event::AfterClose(index.0));
    }
    fn rescan(&mut self) -> u32 {
        self.events.push(Event::Rescan);
        self.rescan
    }
}

fn blocking_write() -> Fake {
    Fake::new().with(0, CALLBACK_WORD).with(4, 0x41).with(8, 0x200).with(0xc, 0xd00000).with(0x10, 1).with(0x14, STATUS_AT).with(0x18, VALUE_AT)
}

#[test]
fn write_hands_the_file_index_source_and_length_to_the_filesystem_and_delivers_the_count() {
    let mut port = blocking_write();
    port.count = 0x180;
    port.raw = RawStatus(16);
    let status = write(&mut port);
    assert_eq!(status, translated(16));
    assert_eq!(
        port.events,
        vec![
            Event::Write(3, 0xd00000, 0x200),
            Event::Complete(CALLBACK_WORD, translated(16), Some(0x180)),
            Event::Store(STATUS_AT, translated(16)),
            Event::Store(VALUE_AT, 0x180),
            Event::Done,
        ]
    );
    assert_eq!((port.get(0), port.get(4), port.get(8)), (0, 3, 0x180));
}

#[test]
fn write_without_a_context_refuses_with_five_and_no_count() {
    let mut port = blocking_write();
    port.context = false;
    assert_eq!(write(&mut port), 5);
    assert_eq!(
        port.events,
        vec![Event::Complete(CALLBACK_WORD, 5, Some(0)), Event::Store(STATUS_AT, 5), Event::Store(VALUE_AT, 0), Event::Done]
    );
    assert_eq!((port.get(0), port.get(4), port.get(8)), (CALLBACK_WORD, 0x41, 0x200));
}

fn open_request(mode: u32) -> Fake {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(0x38, mode).with(0x3c, 0x61).with(0x40, 1).with(0x44, STATUS_AT).with(0x48, VALUE_AT).with(0x4c, 0x9);
    port.payload[4..12].copy_from_slice(b"B:/DCIM/");
    port
}

#[test]
fn a_write_mode_open_creates_the_missing_directory_then_opens_registers_and_delivers_the_tag() {
    let mut port = open_request(1);
    assert_eq!(open(&mut port), translated(0));
    assert_eq!(
        port.events,
        vec![
            Event::MakeDirectory(0x67, 2),
            Event::Bind(0x61),
            Event::Open(b"B:/DCIM/".to_vec(), b'w', 5, 9),
            Event::Register(5),
            Event::Complete(CALLBACK_WORD, translated(0), Some(0x61)),
            Event::Store(STATUS_AT, translated(0)),
            Event::Store(VALUE_AT, 0x61),
            Event::Done,
        ]
    );
    assert_eq!((port.modes[5], port.get(0x3c), port.get(0)), (1, 5, 0));
}

#[test]
fn a_read_open_skips_the_directory_and_registration() {
    let mut port = open_request(0);
    open(&mut port);
    assert_eq!(&port.events[..2], &[Event::Bind(0x61), Event::Open(b"B:/DCIM/".to_vec(), b'r', 5, 9)]);
    assert!(!port.events.iter().any(|event| matches!(event, Event::Register(_) | Event::MakeDirectory(..))));
}

#[test]
fn an_append_open_with_a_known_directory_creates_none_and_does_not_register() {
    let mut port = open_request(2);
    port.known = true;
    open(&mut port);
    assert_eq!(&port.events[..2], &[Event::Bind(0x61), Event::Open(b"B:/DCIM/".to_vec(), b'a', 5, 9)]);
    assert!(!port.events.iter().any(|event| matches!(event, Event::Register(_) | Event::MakeDirectory(..))));
}

#[test]
fn a_failed_open_releases_the_index_pair_and_tag_and_delivers_no_tag() {
    let mut port = open_request(0);
    port.raw = RawStatus(3);
    assert_eq!(open(&mut port), translated(3));
    assert_eq!(
        port.events[2..],
        [
            Event::CacheRelease(5),
            Event::Unbind(0x61),
            Event::ReleaseTag(0x61),
            Event::Complete(CALLBACK_WORD, translated(3), Some(0)),
            Event::Store(STATUS_AT, translated(3)),
            Event::Store(VALUE_AT, 0),
            Event::Done,
        ]
    );
    assert_eq!(port.modes[5], 0);
}

#[test]
fn an_unknown_mode_word_fails_without_a_filesystem_open() {
    let mut port = open_request(4);
    assert_eq!(open(&mut port), translated(19));
    assert!(!port.events.iter().any(|event| matches!(event, Event::Open(..))));
}

fn close_request() -> Fake {
    Fake::new().with(0, CALLBACK_WORD).with(4, 0x41).with(0x20, 1).with(0x24, STATUS_AT)
}

#[test]
fn closing_a_written_file_runs_the_post_close_actions_and_a_rescan_decides_the_status() {
    let mut port = close_request();
    port.modes[3] = 3;
    port.kinds[3] = 0x100;
    assert_eq!(close(&mut port), 0);
    assert_eq!(
        port.events,
        vec![
            Event::ReleaseTag(0x41),
            Event::Unbind(0x41),
            Event::Close(3),
            Event::CacheRelease(3),
            Event::Refresh,
            Event::AfterClose(3),
            Event::Rescan,
            Event::Complete(CALLBACK_WORD, 0x77, None),
            Event::Store(STATUS_AT, 0x77),
            Event::Done,
        ]
    );
    assert!(!port.flagged);
}

#[test]
fn closing_a_written_kind_one_file_marks_the_flagged_close() {
    let mut port = close_request();
    port.modes[3] = 1;
    port.kinds[3] = 1;
    close(&mut port);
    assert!(port.flagged);
}

#[test]
fn closing_an_open_file_with_a_name_uses_the_named_close() {
    let mut port = close_request().with(8, 0);
    port.payload[0xc] = b'X';
    port.modes[3] = 0;
    close(&mut port);
    assert_eq!(port.events[2], Event::Close(3));
    let mut port = close_request().with(8, 0);
    port.payload[0xc] = b'X';
    port.modes[3] = 2;
    close(&mut port);
    assert_eq!(port.events[2], Event::CloseNamed(3, 0, PAYLOAD + 0xc));
}

#[test]
fn a_failed_close_delivers_the_translated_status_without_post_close_actions() {
    let mut port = close_request();
    port.modes[3] = 1;
    port.raw = RawStatus(13);
    close(&mut port);
    assert!(!port.events.iter().any(|event| matches!(event, Event::AfterClose(_))));
    assert!(port.events.contains(&Event::Complete(CALLBACK_WORD, translated(13), None)));
}

#[test]
fn closing_an_entry_of_an_unopened_index_reports_not_open_and_releases_no_cache_index() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(4, 0x41);
    assert_eq!(close_entry(&mut port), 0);
    assert_eq!(
        port.events,
        vec![Event::ReleaseTag(0x41), Event::Unbind(0x41), Event::Refresh, Event::Complete(CALLBACK_WORD, translated(36), None)]
    );
}

#[test]
fn closing_an_entry_hands_the_attribute_and_both_fields() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(4, 0x41);
    port.payload[8] = 0x21;
    port.modes[3] = 2;
    close_entry(&mut port);
    assert_eq!(port.events[2..4], [Event::CloseEntry(3, 0x21, PAYLOAD + 9, PAYLOAD + 0x1d), Event::CacheRelease(3)]);
}

#[test]
fn seek_passes_the_whence_word_before_and_after_the_offset() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(4, 0x41).with(8, 0x1000).with(0xc, 0x2).with(0x10, 1).with(0x14, 1).with(0x18, STATUS_AT);
    port.raw = RawStatus(7);
    assert_eq!(seek(&mut port), translated(7));
    assert_eq!(
        port.events,
        vec![
            Event::Seek(3, 1, 0x1000, 0x2, 1),
            Event::Complete(CALLBACK_WORD, translated(7), None),
            Event::Store(STATUS_AT, translated(7)),
            Event::Done,
        ]
    );
    assert_eq!(port.get(0), CALLBACK_WORD);
}

#[test]
fn release_retires_the_handle_and_releases_the_file() {
    let mut port = Fake::new().with(0, CALLBACK_WORD).with(4, 0x41).with(8, 1).with(0xc, STATUS_AT);
    assert_eq!(release(&mut port), translated(0));
    assert_eq!(
        port.events,
        vec![
            Event::ReleaseTag(0x41),
            Event::Unbind(0x41),
            Event::Release(3),
            Event::CacheRelease(3),
            Event::Complete(CALLBACK_WORD, translated(0), None),
            Event::Store(STATUS_AT, translated(0)),
            Event::Done,
        ]
    );
}

#[test]
fn release_without_a_context_translates_raw_two() {
    let mut port = Fake::new().with(4, 0x41);
    port.context = false;
    assert_eq!(release(&mut port), translated(2));
    assert_eq!(port.events, vec![Event::ReleaseTag(0x41), Event::Unbind(0x41)]);
}
