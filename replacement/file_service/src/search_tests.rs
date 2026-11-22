use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::search::*;
use crate::workers::Offset;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    FindFirst(u32, u32),
    FindNext,
    FindFirstEntry(u32, u32, u32),
    FindNextEntry,
    Rejected(Vec<u8>),
    Divide(u32, u32),
    FormatTime(u32, Vec<u8>, u8),
    Complete(u32, u32),
}

const PAYLOAD: u32 = 0x5000;
const OUTPUT: u32 = 0x6000;
const CALLBACK_WORD: u32 = 0x380000;
const OUTPUT_FILL: u8 = 0xee;

/// One retained search answer: its raw result, the entry it writes and 0x7131a0's verdict.
struct Answer {
    raw: u32,
    entry: [u8; 80],
    rejected: bool,
}

struct Fake {
    payload: Vec<u8>,
    output: Vec<u8>,
    context: bool,
    rounding: u32,
    answers: Vec<Answer>,
    verdict: Option<bool>,
    events: Vec<Event>,
}

fn entry(name: &[u8], short: &[u8; 11], attribute: u8, size: u32) -> [u8; 80] {
    let mut entry = [0; 80];
    entry[..name.len()].copy_from_slice(name);
    entry[0x30..0x3b].copy_from_slice(short);
    entry[0x3b] = attribute;
    entry[0x3d] = 150;
    entry[0x3e..0x42].copy_from_slice(&0x5b75_6400u32.to_le_bytes());
    entry[0x46..0x4a].copy_from_slice(&0x5b76_8a10u32.to_le_bytes());
    entry[0x4c..0x50].copy_from_slice(&size.to_le_bytes());
    entry
}

fn found(name: &[u8], rejected: bool) -> Answer {
    Answer { raw: 0, entry: entry(name, b"DSCF0001JPG", 0x20, 0x12345), rejected }
}

fn failed(raw: u32) -> Answer {
    Answer { raw, entry: entry(b"PARTIAL", b"PARTIAL    ", 0x10, 7), rejected: false }
}

impl Fake {
    fn new(answers: Vec<Answer>) -> Fake {
        let mut payload = vec![0; 0x40];
        payload[0..4].copy_from_slice(&CALLBACK_WORD.to_le_bytes());
        payload[4..8].copy_from_slice(&OUTPUT.to_le_bytes());
        payload[0x38..0x3c].copy_from_slice(&1u32.to_le_bytes());
        Fake { payload, output: vec![OUTPUT_FILL; 0x80], context: true, rounding: 0x8000, answers, verdict: None, events: Vec::new() }
    }

    fn at(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.output[offset..offset + 4].try_into().unwrap())
    }

    fn answer(&mut self, entry: &mut Entry) -> RawStatus {
        let answer = self.answers.remove(0);
        entry.0 = answer.entry;
        self.verdict = Some(answer.rejected);
        RawStatus(answer.raw)
    }
}

fn translated(raw: u32) -> u32 {
    if raw == 0 { 0 } else { raw + 100 }
}

impl Port for Fake {
    fn word(&self, at: Offset) -> u32 {
        u32::from_le_bytes(self.payload[at.0 as usize..at.0 as usize + 4].try_into().unwrap())
    }
    fn address(&self, at: Offset) -> u32 {
        PAYLOAD + at.0
    }
    fn context(&self) -> bool {
        self.context
    }
    fn rounding(&self) -> u32 {
        self.rounding
    }
    fn cleared_entry(&mut self) -> Entry {
        Entry([0; 80])
    }
    fn clear(&mut self, output: Record, length: u32) {
        let start = (output.0 - OUTPUT) as usize;
        self.output[start..start + length as usize].fill(0);
    }
    fn copy(&mut self, output: Record, entry: &Entry, length: u32) {
        let start = (output.0 - OUTPUT) as usize;
        self.output[start..start + length as usize].copy_from_slice(&entry.0[..length as usize]);
    }
    fn set_byte(&mut self, output: Record, at: Field, value: u8) {
        self.output[(output.0 - OUTPUT + at.0) as usize] = value;
    }
    fn set_word(&mut self, output: Record, at: Field, value: u32) {
        let start = (output.0 - OUTPUT + at.0) as usize;
        self.output[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn find_first(&mut self, path: u32, pattern: u32, entry: &mut Entry) -> RawStatus {
        self.events.push(Event::FindFirst(path, pattern));
        self.answer(entry)
    }
    fn find_next(&mut self, entry: &mut Entry) -> RawStatus {
        self.events.push(Event::FindNext);
        self.answer(entry)
    }
    fn find_first_entry(&mut self, path: u32, pattern: u32, entry: &mut Entry, flag: u32) -> RawStatus {
        self.events.push(Event::FindFirstEntry(path, pattern, flag));
        self.answer(entry)
    }
    fn find_next_entry(&mut self, entry: &mut Entry) -> RawStatus {
        self.events.push(Event::FindNextEntry);
        self.answer(entry)
    }
    fn rejected(&mut self, entry: &Entry) -> bool {
        self.events.push(Event::Rejected(entry.0[0x30..0x3b].to_vec()));
        self.verdict.take().unwrap()
    }
    fn little_endian(&mut self, value: u32) -> u32 {
        value
    }
    fn divide(&mut self, dividend: u32, divisor: u32) -> u32 {
        self.events.push(Event::Divide(dividend, divisor));
        dividend / divisor
    }
    fn format_time(&mut self, output: Record, at: Field, entry: &Entry, stamp: EntryOffset, byte: u8) {
        self.events.push(Event::FormatTime(output.0 + at.0, entry.0[stamp.0..stamp.0 + 4].to_vec(), byte));
        let start = (output.0 - OUTPUT + at.0) as usize;
        self.output[start..start + 20].fill(b'T');
    }
    fn translate(&mut self, raw: RawStatus) -> u32 {
        translated(raw.0)
    }
    fn complete_status(&mut self, callback: Callback, status: u32) {
        self.events.push(Event::Complete(callback.0, status));
    }
}

#[test]
fn find_first_copies_the_accepted_entry_and_zeroes_the_rest() {
    let mut fake = Fake::new(vec![found(b"DSCF0001.JPG", false)]);
    assert_eq!(find_first(&mut fake), 0);
    assert_eq!(
        fake.events,
        vec![Event::FindFirst(PAYLOAD + 8, PAYLOAD + 0x28), Event::Rejected(b"DSCF0001JPG".to_vec()), Event::Complete(CALLBACK_WORD, 0)]
    );
    assert_eq!(&fake.output[..12], b"DSCF0001.JPG");
    assert!(fake.output[12..100].iter().all(|&byte| byte == 0));
    assert!(fake.output[100..].iter().all(|&byte| byte == OUTPUT_FILL));
}

#[test]
fn find_first_searches_on_past_rejected_entries() {
    let mut fake = Fake::new(vec![found(b"REJECTED", true), found(b"ACCEPTED", false)]);
    assert_eq!(find_first(&mut fake), 0);
    assert_eq!(
        fake.events,
        vec![
            Event::FindFirst(PAYLOAD + 8, PAYLOAD + 0x28),
            Event::Rejected(b"DSCF0001JPG".to_vec()),
            Event::FindNext,
            Event::Rejected(b"DSCF0001JPG".to_vec()),
            Event::Complete(CALLBACK_WORD, 0),
        ]
    );
    assert_eq!(&fake.output[..9], b"ACCEPTED\0");
}

#[test]
fn a_failed_search_leaves_the_output_zeroed_and_delivers_the_translation() {
    let mut fake = Fake::new(vec![found(b"REJECTED", true), failed(4)]);
    assert_eq!(find_first(&mut fake), 104);
    assert_eq!(fake.events.last(), Some(&Event::Complete(CALLBACK_WORD, 104)));
    assert!(fake.output[..100].iter().all(|&byte| byte == 0));
}

#[test]
fn find_next_starts_with_the_next_search() {
    let mut fake = Fake::new(vec![failed(9)]);
    assert_eq!(find_next(&mut fake), 109);
    assert_eq!(fake.events, vec![Event::FindNext, Event::Complete(CALLBACK_WORD, 109)]);
}

#[test]
fn without_context_no_search_runs_and_the_output_is_untouched() {
    for worker in [find_first::<Fake>, find_next::<Fake>, find_first_entry::<Fake>, find_next_entry::<Fake>] {
        let mut fake = Fake::new(vec![]);
        fake.context = false;
        assert_eq!(worker(&mut fake), 102);
        assert_eq!(fake.events, vec![Event::Complete(CALLBACK_WORD, 102)]);
        assert!(fake.output.iter().all(|&byte| byte == OUTPUT_FILL));
    }
}

#[test]
fn the_entry_record_holds_name_attribute_sizes_and_times() {
    let mut fake = Fake::new(vec![found(b"DSCF0001.JPG", false)]);
    assert_eq!(find_first_entry(&mut fake), 0);
    assert_eq!(
        fake.events,
        vec![
            Event::FindFirstEntry(PAYLOAD + 8, PAYLOAD + 0x28, 1),
            Event::Divide(0x8000 - 1 + 0x12345, 0x8000),
            Event::FormatTime(OUTPUT + 0x3c, 0x5b75_6400u32.to_le_bytes().to_vec(), 150),
            Event::FormatTime(OUTPUT + 0x50, 0x5b76_8a10u32.to_le_bytes().to_vec(), 0),
            Event::Complete(CALLBACK_WORD, 0),
        ]
    );
    assert_eq!(&fake.output[..12], b"DSCF0001.JPG");
    assert_eq!(fake.output[0x30], 0x20);
    assert_eq!(fake.at(0x34), 0x12345);
    assert_eq!(fake.at(0x38), 0x18000);
    assert!(fake.output[0x3c..0x64].iter().all(|&byte| byte == b'T'));
    assert!(fake.output[0x64..].iter().all(|&byte| byte == OUTPUT_FILL));
}

#[test]
fn a_failed_entry_search_writes_no_record() {
    let mut fake = Fake::new(vec![failed(9)]);
    assert_eq!(find_next_entry(&mut fake), 109);
    assert_eq!(fake.events, vec![Event::FindNextEntry, Event::Complete(CALLBACK_WORD, 109)]);
    assert!(fake.output.iter().all(|&byte| byte == OUTPUT_FILL));
}
