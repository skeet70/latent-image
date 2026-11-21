use std::collections::HashMap;

use crate::requests::Callback;
use crate::volume::*;
use crate::workers::Offset;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    Complete(u32, u32),
    Letter(u8),
    Record(u32),
    Release(u32),
    Switch(u32, u32),
    Other(u32),
    Attach { callback: u32 },
    Reattach,
    Detach,
    Format(u32),
    Reformat(u32),
    CopyDefaults(u32),
    Fill(u32, u32),
    Snapshot,
    Level(u32, u32),
    Prepare,
    Rebuild(u32, u32),
    Select(u32, u32),
    NoVolume,
}

const CALLBACK_WORD: u32 = 0x380000;
const CONTEXT: u32 = 0xa00000;
const SECOND_CONTEXT: u32 = 0xb00000;

struct Fake {
    payload: Vec<u8>,
    context: u32,
    attach_state: u32,
    memory: HashMap<u32, u8>,
    volume: Volume,
    records: [u32; 2],
    others: Vec<Volume>,
    attached_context: u32,
    status: u32,
    events: Vec<Event>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            payload: vec![0; 0x44],
            context: CONTEXT,
            attach_state: 0,
            memory: HashMap::new(),
            volume: Volume(1),
            records: [0, 0],
            others: Vec::new(),
            attached_context: CONTEXT,
            status: 0,
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

    fn put_bytes(&mut self, address: u32, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.memory.insert(address + index as u32, *byte);
        }
    }

    fn at(mut self, field: u32, value: u32) -> Fake {
        self.put_bytes(CONTEXT + field, &value.to_le_bytes());
        self
    }

    fn read(&self, address: u32, bytes: u32) -> u32 {
        (0..bytes).rev().fold(0, |value, index| value << 8 | u32::from(*self.memory.get(&(address + index)).unwrap_or(&0)))
    }

    fn memory_word(&self, field: u32) -> u32 {
        self.read(CONTEXT + field, 4)
    }

    fn memory_half(&self, field: u32) -> u32 {
        self.read(CONTEXT + field, 2)
    }
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
    fn context(&self) -> Context {
        Context(self.context)
    }
    fn set_context(&mut self, context: Context) {
        self.context = context.0;
    }
    fn attach_state(&self) -> u32 {
        self.attach_state
    }
    fn field(&self, context: Context, at: Field) -> u32 {
        self.read(context.0 + at.0, 4)
    }
    fn field_byte(&self, context: Context, at: Field) -> u8 {
        self.read(context.0 + at.0, 1) as u8
    }
    fn set_field(&mut self, context: Context, at: Field, value: u32) {
        self.put_bytes(context.0 + at.0, &value.to_le_bytes());
    }
    fn set_field_half(&mut self, context: Context, at: Field, value: u16) {
        self.put_bytes(context.0 + at.0, &value.to_le_bytes());
    }
    fn set_field_byte(&mut self, context: Context, at: Field, value: u8) {
        self.put_bytes(context.0 + at.0, &[value]);
    }
    fn complete(&mut self, callback: Callback, value: u32) {
        if callback.0 != 0 {
            self.events.push(Event::Complete(callback.0, value));
        }
    }
    fn device_for_letter(&mut self, letter: Letter) -> Volume {
        self.events.push(Event::Letter(letter.0));
        self.volume
    }
    fn volume_record(&mut self, volume: Volume) -> u32 {
        self.events.push(Event::Record(volume.0));
        self.records[volume.0 as usize]
    }
    fn release_volume(&mut self, volume: Volume) -> Residue {
        self.events.push(Event::Release(volume.0));
        Residue(0x7000 + volume.0)
    }
    fn switch_volume(&mut self, from: Volume, to: Volume) -> Residue {
        self.events.push(Event::Switch(from.0, to.0));
        Residue(0x8000 + to.0)
    }
    fn other_volume(&mut self, volume: Volume) -> Volume {
        self.events.push(Event::Other(volume.0));
        self.others.remove(0)
    }
    fn attach(&mut self) -> Residue {
        self.events.push(Event::Attach { callback: self.get(0) });
        self.context = self.attached_context;
        Residue(0x6100)
    }
    fn reattach(&mut self) -> Residue {
        self.events.push(Event::Reattach);
        Residue(0x6200)
    }
    fn detach(&mut self) -> Residue {
        self.events.push(Event::Detach);
        Residue(0x6300)
    }
    fn format(&mut self, volume: Volume) -> u32 {
        self.events.push(Event::Format(volume.0));
        self.status
    }
    fn reformat(&mut self, volume: Volume) -> u32 {
        self.events.push(Event::Reformat(volume.0));
        self.status
    }
    fn copy_defaults(&mut self, context: Context, at: Field) -> Residue {
        self.events.push(Event::CopyDefaults(context.0 + at.0));
        Residue(context.0 + at.0 + 216)
    }
    fn fill_zero(&mut self, context: Context, at: Field, bytes: u32) {
        self.events.push(Event::Fill(context.0 + at.0, bytes));
    }
    fn snapshot_media_state(&mut self) {
        self.events.push(Event::Snapshot);
    }
    fn set_level(&mut self, slot: u32, level: u32) {
        self.events.push(Event::Level(slot, level));
    }
    fn flagged_prepare(&mut self) {
        self.events.push(Event::Prepare);
    }
    fn catalog_rebuild(&mut self, first: u32, second: u32) -> Residue {
        self.events.push(Event::Rebuild(first, second));
        Residue(0x6400)
    }
    fn flagged_select(&mut self, first: u32, second: u32) -> Residue {
        self.events.push(Event::Select(first, second));
        Residue(0x6500)
    }
    fn no_volume_status(&mut self) -> Residue {
        self.events.push(Event::NoVolume);
        Residue(0xffff_ff01)
    }
}

fn attach_request() -> Fake {
    Fake::new().with(0, CALLBACK_WORD).with(4, CONTEXT).with(0x38, u32::from(b'B'))
}

#[test]
fn attach_unknown_letter_hands_the_callback_minus_one() {
    let mut fake = attach_request();
    fake.volume = Volume::NONE;
    assert_eq!(attach(&mut fake), 0);
    assert_eq!(fake.events, vec![Event::Letter(b'B'), Event::Complete(CALLBACK_WORD, u32::MAX), Event::Level(0, 10)]);
    assert_eq!(fake.get(0), CALLBACK_WORD);
}

#[test]
fn attach_without_context_attaches_fresh_and_hands_the_copy_end() {
    let mut fake = attach_request();
    fake.context = 0;
    assert_eq!(attach(&mut fake), 0);
    assert_eq!(
        fake.events,
        vec![
            Event::Letter(b'B'),
            Event::Attach { callback: 0 },
            Event::CopyDefaults(CONTEXT + 0x118),
            Event::Complete(CALLBACK_WORD, CONTEXT + 0x118 + 216),
            Event::Level(0, 10),
        ]
    );
    assert_eq!(fake.memory_word(0x1f0), 9);
    assert_eq!(fake.get(0), 0);
}

#[test]
fn attach_releases_misplaced_slots_and_drops_another_volume_context() {
    let mut fake = attach_request().at(0x114, 0);
    fake.records = [CONTEXT, 0x55];
    assert_eq!(attach(&mut fake), 0);
    assert_eq!(
        fake.events[..6],
        [Event::Letter(b'B'), Event::Record(0), Event::Release(0), Event::Record(1), Event::Release(1), Event::Release(1)]
    );
    assert_eq!(fake.events[6], Event::Attach { callback: 0 });
}

#[test]
fn attach_keeps_slots_already_in_place() {
    let mut fake = attach_request().at(0x114, 1).at(0x2210, 1);
    fake.records = [0x55, CONTEXT];
    attach(&mut fake);
    assert_eq!(
        fake.events,
        vec![
            Event::Letter(b'B'),
            Event::Record(0),
            Event::Record(1),
            Event::Reattach,
            Event::Complete(CALLBACK_WORD, 0x6200),
            Event::Level(0, 10),
        ]
    );
    assert_eq!(fake.get(0), CALLBACK_WORD);
}

#[test]
fn attach_switches_from_another_volume_and_reattaches_flagged() {
    let mut fake = attach_request().with(4, 0x1234).with(0x3c, 1).at(0x114, 0).at(0x2210, 1).at(0x16bd8, 0x10);
    attach(&mut fake);
    assert_eq!(
        fake.events,
        vec![
            Event::Letter(b'B'),
            Event::Record(0),
            Event::Record(1),
            Event::Switch(0, 1),
            Event::Reattach,
            Event::Rebuild(0, 0),
            Event::Complete(CALLBACK_WORD, 0x6400),
            Event::Level(0, 10),
        ]
    );
}

#[test]
fn attach_drops_a_foreign_descriptor_context_and_selects_flagged() {
    let mut fake = attach_request().with(4, 0x1234).with(0x3c, 1).at(0x114, 1).at(0x2210, 1);
    fake.put_bytes(SECOND_CONTEXT + 0x16bd8, &0x11u32.to_le_bytes());
    fake.attached_context = SECOND_CONTEXT;
    attach(&mut fake);
    assert_eq!(
        fake.events,
        vec![
            Event::Letter(b'B'),
            Event::Record(0),
            Event::Record(1),
            Event::Release(1),
            Event::Prepare,
            Event::Attach { callback: 0 },
            Event::CopyDefaults(SECOND_CONTEXT + 0x118),
            Event::Select(0, 1),
            Event::Complete(CALLBACK_WORD, 0x6500),
            Event::Level(0, 10),
        ]
    );
}

#[test]
fn detach_without_context_restores_the_callback_and_hands_it_the_no_volume_status() {
    let mut fake = Fake::new().with(0, CALLBACK_WORD);
    fake.context = 0;
    assert_eq!(detach_all(&mut fake), 0);
    assert_eq!(fake.events, vec![Event::NoVolume, Event::Complete(CALLBACK_WORD, 0xffff_ff01)]);
    assert_eq!(fake.get(0), CALLBACK_WORD);
}

#[test]
fn detach_once_switches_to_the_other_volume() {
    let mut fake = Fake::new().with(0, CALLBACK_WORD).at(0x114, 1).at(0x16c68, 0xffff_ffff);
    fake.others = vec![Volume(0)];
    detach_all(&mut fake);
    assert_eq!(
        fake.events,
        vec![Event::NoVolume, Event::Detach, Event::Release(1), Event::Other(1), Event::Switch(1, 0), Event::Complete(CALLBACK_WORD, 0x8000)]
    );
    assert_eq!(fake.memory_word(0x114), 0);
    assert_eq!(fake.memory_word(0x16c68), 0x0000_ffff);
    assert_eq!(fake.get(0), CALLBACK_WORD);
}

#[test]
fn detach_repeating_stops_when_no_volume_is_left() {
    let mut fake = Fake::new().with(0, CALLBACK_WORD).with(4, 1).at(0x114, 1);
    fake.others = vec![Volume(0), Volume::NONE];
    detach_all(&mut fake);
    assert_eq!(
        fake.events,
        vec![
            Event::NoVolume,
            Event::Detach,
            Event::Release(1),
            Event::Other(1),
            Event::Switch(1, 0),
            Event::Detach,
            Event::Release(0),
            Event::Other(0),
            Event::Complete(CALLBACK_WORD, u32::MAX),
        ]
    );
}

fn format_request() -> Fake {
    Fake::new().with(0, CALLBACK_WORD).with(4, u32::from(b'B'))
}

#[test]
fn format_without_context_delivers_five() {
    let mut fake = format_request();
    fake.context = 0;
    assert_eq!(format_armed(&mut fake), 5);
    assert_eq!(fake.events, vec![Event::Letter(b'B'), Event::Complete(CALLBACK_WORD, 5)]);
    assert_eq!(fake.get(0), 0);
}

#[test]
fn format_failure_delivers_the_status_without_reset() {
    let mut fake = format_request().at(0x2600, 2).at(0x21d0, 7);
    fake.status = 39;
    assert_eq!(format(&mut fake), 39);
    assert_eq!(fake.events, vec![Event::Letter(b'B'), Event::Reformat(1), Event::Complete(CALLBACK_WORD, 39)]);
    assert_eq!(fake.memory_word(0x21d0), 7);
    assert_eq!(fake.memory_word(0x21cc), 0);
}

#[test]
fn armed_format_resets_cursors_and_slot_records() {
    let mut fake = format_request().at(0x2600, 2).at(0x21d0, 7).at(0x21d4, 0x0808_0808).at(0x21d8, 0x0909).at(0x21e0, 7).at(0x21f0, 7).at(0x2214, 3);
    assert_eq!(format_armed(&mut fake), 0);
    assert_eq!(
        fake.events,
        vec![
            Event::Letter(b'B'),
            Event::Format(1),
            Event::Fill(CONTEXT + 0x35a4, 2000),
            Event::Fill(CONTEXT + 0x2604, 4000),
            Event::Snapshot,
            Event::Complete(CALLBACK_WORD, 0),
        ]
    );
    assert_eq!([fake.memory_word(0x21d0), fake.memory_word(0x21d4), fake.memory_half(0x21d8), fake.memory_word(0x21e0)], [0, 0, 0, 0]);
    assert_eq!(fake.memory_word(0x21f0), 7);
    assert_eq!(fake.memory_word(0x2214), 0);
    assert_eq!([fake.memory_word(0x2168), fake.memory_word(0x2180), fake.memory_word(0x217c), fake.memory_word(0x2190)], [100, 100, 1000, 1001]);
    assert_eq!(fake.memory_word(0x21cc), 2);
}

#[test]
fn plain_format_keeps_slot_records() {
    let mut fake = format_request().at(0x2600, 2).at(0x21d0, 7).at(0x2214, 3);
    assert_eq!(format(&mut fake), 0);
    assert_eq!(fake.memory_word(0x21d0), 7);
    assert_eq!(fake.memory_word(0x2214), 3);
    assert_eq!(fake.memory_word(0x21cc), 2);
}
