use crate::writer::{self, callback, kind, Guest, DEBUG, SETTINGS, WRITER};
use std::collections::BTreeMap;

/// Byte-addressed guest memory; unwritten bytes read as zero. Services act on it directly and
/// append to `calls`; frames come from a bump region at `0x3000_0000`.
#[derive(Default)]
struct Sim {
    memory: BTreeMap<u32, u8>,
    calls: Vec<(&'static str, u32, u32, u32)>,
    frames: u32,
}

impl Sim {
    fn put(&mut self, address: u32, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.memory.insert(address + index as u32, *byte);
        }
    }
    fn bytes(&self, address: u32, length: u32) -> Vec<u8> {
        (0..length).map(|index| self.r8(address + index)).collect()
    }
}

impl Guest for Sim {
    fn r8(&self, address: u32) -> u8 {
        *self.memory.get(&address).unwrap_or(&0)
    }
    fn r16(&self, address: u32) -> u16 {
        u16::from_le_bytes([self.r8(address), self.r8(address + 1)])
    }
    fn r32(&self, address: u32) -> u32 {
        u32::from_le_bytes([self.r8(address), self.r8(address + 1), self.r8(address + 2), self.r8(address + 3)])
    }
    fn w8(&mut self, address: u32, value: u8) {
        self.memory.insert(address, value);
    }
    fn w16(&mut self, address: u32, value: u16) {
        self.put(address, &value.to_le_bytes())
    }
    fn w32(&mut self, address: u32, value: u32) {
        self.put(address, &value.to_le_bytes())
    }
    fn copy_bytes(&mut self, destination: u32, source: u32, length: u32) {
        self.calls.push(("copy_bytes", destination, source, length));
        let bytes = self.bytes(source, length);
        self.put(destination, &bytes);
    }
    fn copy_memory(&mut self, destination: u32, source: u32, length: u32) {
        self.calls.push(("memcpy", destination, source, length));
        let bytes = self.bytes(source, length);
        self.put(destination, &bytes);
    }
    fn copy_string(&mut self, destination: u32, source: u32) {
        let length = self.string_length(source) + 1;
        let bytes = self.bytes(source, length);
        self.put(destination, &bytes);
    }
    fn copy_string_bounded(&mut self, destination: u32, source: u32, length: u32) {
        let text = self.string_length(source).min(length);
        let mut bytes = self.bytes(source, text);
        bytes.resize(length as usize, 0);
        self.put(destination, &bytes);
    }
    fn string_length(&mut self, source: u32) -> u32 {
        (0..).find(|index| self.r8(source + index) == 0).unwrap()
    }
    fn fill(&mut self, destination: u32, value: u32, length: u32) {
        self.calls.push(("memset", destination, value, length));
        self.put(destination, &vec![value as u8; length as usize]);
    }
    fn rule_callback(&mut self, callback: u32, source: u32, slot: u32, _: u32, _: u32, entry: u32) {
        match callback {
            0x011c_ed18 => callback::exif_pointer(self, slot, entry),
            0x011c_ed80 => callback::one(self, entry),
            other => panic!("no callback at {other:#x}"),
        }
        let _ = source;
    }
    fn frame<R>(&mut self, body: impl FnOnce(&mut Self, u32) -> R) -> R {
        let base = 0x3000_0000 + 64 * self.frames;
        self.frames += 1;
        let result = body(self, base);
        self.frames -= 1;
        result
    }
}

fn rule(tag: u16, flags: u16, value_type: u16, limit: u16, callback: u32, offset: u32) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0..2].copy_from_slice(&tag.to_le_bytes());
    bytes[2..4].copy_from_slice(&flags.to_le_bytes());
    bytes[4..6].copy_from_slice(&value_type.to_le_bytes());
    bytes[6..8].copy_from_slice(&limit.to_le_bytes());
    bytes[8..12].copy_from_slice(&callback.to_le_bytes());
    bytes[28..32].copy_from_slice(&offset.to_le_bytes());
    bytes
}

const SOURCE: u32 = 0x0050_0000;
const OUTPUT: u32 = 0x0040_0000;
const TYPE_SIZES: [u8; 13] = [1, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 16, 16];

/// A guest with the type-size and mode-mask tables and the Exif table (kind 1) holding `rules`.
fn guest(rules: &[[u8; 32]]) -> Sim {
    let mut sim = Sim::default();
    sim.put(0x011c_2cb8, &TYPE_SIZES);
    sim.put(0x011d_230c, &[1, 0, 2, 0, 4, 0, 8, 0]);
    let table = writer::select_table(kind::EXIF);
    for (index, bytes) in rules.iter().enumerate() {
        sim.put(table + 32 * index as u32, bytes);
    }
    sim
}

#[test]
fn filtered_directory_writes_selected_entries_and_out_of_line_values() {
    let mut sim = guest(&[
        rule(0x829a, 0x3, 5, 1, 0, 0x0),
        rule(0x8827, 0x2, 3, 1, 0, 0x12),
        rule(0x8830, 0x3, 3, 1, 0x011c_ed80, 0),
        rule(0xa433, 0x3, 2, 13, 0, 0x2dc),
    ]);
    sim.put(SOURCE, &[1, 0, 0, 0, 250, 0, 0, 0]);
    sim.put(SOURCE + 0x2dc, b"FUJIFILM\0\0\0\0\0");
    let base = OUTPUT - 8;
    let size = writer::write_filtered_directory(&mut sim, kind::EXIF, SOURCE, OUTPUT, base);
    // Rules 0, 2 and 3 carry mode-0 flag bit 0; rule 1 does not.
    assert_eq!(sim.r16(OUTPUT), 3);
    let entry = |index: u32| OUTPUT + 2 + 12 * index;
    let values = OUTPUT + 2 + 3 * 12 + 4;
    assert_eq!(sim.bytes(entry(0), 8), [0x9a, 0x82, 5, 0, 1, 0, 0, 0]);
    assert_eq!(sim.r32(entry(0) + 8), values - base);
    assert_eq!(sim.bytes(values, 8), [1, 0, 0, 0, 250, 0, 0, 0]);
    assert_eq!(sim.bytes(entry(1), 12), [0x30, 0x88, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(sim.r16(entry(2)), 0xa433);
    assert_eq!(sim.r32(entry(2) + 4), 13);
    assert_eq!(sim.r32(entry(2) + 8), values + 8 - base);
    assert_eq!(sim.bytes(values + 8, 14), b"FUJIFILM\0\0\0\0\0\0");
    assert_eq!(sim.r32(OUTPUT + 2 + 36), 0, "next-IFD word");
    assert_eq!(size, 2 + 36 + 4 + 8 + 14);
    assert_eq!(sim.r32(DEBUG), kind::EXIF);
}

#[test]
fn values_of_four_bytes_or_fewer_stay_in_the_entry_zero_filled() {
    let mut sim = guest(&[rule(0xa300, 0x3, 7, 1, 0, 0x2a8)]);
    sim.put(SOURCE + 0x2a8, &[3, 0xee, 0xee, 0xee]);
    writer::write_filtered_directory(&mut sim, kind::EXIF, SOURCE, OUTPUT, OUTPUT);
    assert_eq!(sim.bytes(OUTPUT + 2 + 8, 4), [3, 0, 0, 0]);
    assert!(sim.calls.contains(&("memset", OUTPUT + 2 + 9, 0, 3)));
}

#[test]
fn selection_follows_the_option_flag_only_when_the_option_is_set() {
    let rules = [rule(0x0001, 0x10, 2, 2, 0, 0x4)];
    let mut sim = guest(&rules);
    writer::write_filtered_directory(&mut sim, kind::EXIF, SOURCE, OUTPUT, OUTPUT);
    assert_eq!(sim.r16(OUTPUT), 0);
    let mut sim = guest(&rules);
    sim.w16(SETTINGS + 12, 1);
    writer::write_filtered_directory(&mut sim, kind::EXIF, SOURCE, OUTPUT, OUTPUT);
    assert_eq!(sim.r16(OUTPUT), 1);
}

#[test]
fn finish_stores_the_app1_length_big_endian_and_the_thumbnail_length() {
    let mut sim = Sim::default();
    sim.w32(WRITER + 0x2c, OUTPUT + 4);
    sim.w32(WRITER + 0x20, OUTPUT + 0x1f0);
    writer::finish_exif(&mut sim, 0x2a14, 0x1d80);
    assert_eq!(sim.bytes(OUTPUT + 4, 2), [0x2a, 0x10]);
    assert_eq!(sim.r32(OUTPUT + 0x1f0), 0x1d80);
}

#[test]
fn gps_dilution_is_written_only_with_a_measurement() {
    let mut sim = Sim::default();
    assert_eq!(writer::should_write_tag(&sim, kind::GPS, 11, SOURCE), 0);
    sim.w32(SOURCE + 0x6c, 1);
    assert_eq!(writer::should_write_tag(&sim, kind::GPS, 11, SOURCE), 1);
    sim.w32(SOURCE + 0x68, 5);
    sim.w32(SOURCE + 0x6c, 0);
    assert_eq!(writer::should_write_tag(&sim, kind::GPS, 11, SOURCE), 1);
}

#[test]
fn maker_face_tags_need_detection_data() {
    let mut sim = Sim::default();
    assert_eq!(writer::should_write_tag(&sim, kind::MAKER, 0x4282, SOURCE), 0);
    sim.w8(SOURCE + 0x1e8, 1);
    sim.w16(SOURCE + 0x1f0, 1);
    sim.w8(SOURCE + 0x1f2, 1);
    sim.w16(SOURCE + 0x58a, 2);
    sim.w32(SOURCE + 0x1d38, 1);
    assert_eq!(writer::should_write_tag(&sim, kind::MAKER, 0x4282, SOURCE), 1);
}
