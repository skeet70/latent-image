//! The media task's directory search workers. Find first and find next copy the found entry's
//! first 48 bytes to the caller's 100-byte output, searching again while 0x7131a0 rejects the
//! entry's 11 bytes at +0x30; the entry-record variants search once and fill the caller's
//! record from the entry. Offsets are into the payloads the posters build, the 80-byte entry
//! the retained search fills and the caller's record.

use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::workers::Offset;

pub const ENTRY_BYTES: usize = 80;

/// The entry a retained search fills.
pub struct Entry(pub [u8; ENTRY_BYTES]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryOffset(pub usize);

/// The guest address of the caller's output record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record(pub u32);

/// A byte offset into the caller's output record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field(pub u32);

impl Entry {
    pub fn byte(&self, at: EntryOffset) -> u8 {
        self.0[at.0]
    }

    pub fn word(&self, at: EntryOffset) -> u32 {
        u32::from_le_bytes([self.0[at.0], self.0[at.0 + 1], self.0[at.0 + 2], self.0[at.0 + 3]])
    }
}

const CALLBACK: Offset = Offset(0);
const OUTPUT: Offset = Offset(4);
const PATH: Offset = Offset(8);
const PATTERN: Offset = Offset(0x28);
/// The word the entry-record first search passes 0x7004b0 last.
const FLAG: Offset = Offset(0x38);

/// Bytes of the find workers' output each pass zeroes.
pub const OUTPUT_BYTES: u32 = 100;
/// Entry bytes copied to the start of the output.
pub const COPIED_BYTES: u32 = 48;
/// The entry bytes at +0x30 0x7131a0 tests.
pub const TESTED: EntryOffset = EntryOffset(0x30);
pub const TESTED_BYTES: u32 = 11;

mod entry {
    use super::EntryOffset;
    pub const ATTRIBUTE: EntryOffset = EntryOffset(0x3b);
    pub const FIRST_TIME_BYTE: EntryOffset = EntryOffset(0x3d);
    pub const FIRST_TIME: EntryOffset = EntryOffset(0x3e);
    pub const SECOND_TIME: EntryOffset = EntryOffset(0x46);
    pub const SIZE: EntryOffset = EntryOffset(0x4c);
}

mod record {
    use super::Field;
    pub const ATTRIBUTE: Field = Field(0x30);
    pub const SIZE: Field = Field(0x34);
    /// The size rounded up to a multiple of the context's word at +0x16bf8.
    pub const ROUNDED: Field = Field(0x38);
    pub const FIRST_TIME: Field = Field(0x3c);
    pub const SECOND_TIME: Field = Field(0x50);
}

const RAW_OK: RawStatus = RawStatus(0);
const RAW_NO_CONTEXT: RawStatus = RawStatus(2);

pub trait Port {
    fn word(&self, at: Offset) -> u32;
    /// The guest address of the payload bytes at `at`.
    fn address(&self, at: Offset) -> u32;
    /// Whether a media-state context is active.
    fn context(&self) -> bool;
    /// The context's word at +0x16bf8.
    fn rounding(&self) -> u32;

    /// An entry zeroed by 0x70e664.
    fn cleared_entry(&mut self) -> Entry;
    /// Zeroes `length` bytes of the record through 0x70e664.
    fn clear(&mut self, output: Record, length: u32);
    /// Copies the entry's first `length` bytes to the record through 0x70e6c4.
    fn copy(&mut self, output: Record, entry: &Entry, length: u32);
    fn set_byte(&mut self, output: Record, at: Field, value: u8);
    fn set_word(&mut self, output: Record, at: Field, value: u32);

    /// 0x7008a4.
    fn find_first(&mut self, path: u32, pattern: u32, entry: &mut Entry) -> RawStatus;
    /// 0x700b10.
    fn find_next(&mut self, entry: &mut Entry) -> RawStatus;
    /// 0x7004b0.
    fn find_first_entry(&mut self, path: u32, pattern: u32, entry: &mut Entry, flag: u32) -> RawStatus;
    /// 0x700768.
    fn find_next_entry(&mut self, entry: &mut Entry) -> RawStatus;
    /// 0x7131a0 over the entry's tested bytes is nonzero.
    fn rejected(&mut self, entry: &Entry) -> bool;
    /// 0x70e650.
    fn little_endian(&mut self, value: u32) -> u32;
    /// 0x4d978.
    fn divide(&mut self, dividend: u32, divisor: u32) -> u32;
    /// 0x71d214(output + at, entry + stamp, byte).
    fn format_time(&mut self, output: Record, at: Field, entry: &Entry, stamp: EntryOffset, byte: u8);

    /// Translates a raw filesystem result, reporting it when the translation reports.
    fn translate(&mut self, raw: RawStatus) -> u32;
    fn complete_status(&mut self, callback: Callback, status: u32);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    First,
    Next,
}

fn output<P: Port>(port: &P) -> Record {
    Record(port.word(OUTPUT))
}

fn finish<P: Port>(port: &mut P, callback: Callback, raw: RawStatus) -> u32 {
    let status = port.translate(raw);
    port.complete_status(callback, status);
    status
}

/// The find first worker at 0x712700.
pub fn find_first<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let raw = if port.context() { search(port, Pass::First) } else { RAW_NO_CONTEXT };
    finish(port, callback, raw)
}

/// The find next worker at 0x7127e0.
pub fn find_next<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let raw = if port.context() { search(port, Pass::Next) } else { RAW_NO_CONTEXT };
    finish(port, callback, raw)
}

fn search<P: Port>(port: &mut P, mut pass: Pass) -> RawStatus {
    loop {
        if !cfg!(file_service_control_search_output_uncleared) {
            let at = output(port);
            port.clear(at, OUTPUT_BYTES);
        }
        let mut entry = port.cleared_entry();
        let raw = match pass {
            Pass::First => {
                let (path, pattern) = (port.address(PATH), port.address(PATTERN));
                port.find_first(path, pattern, &mut entry)
            }
            Pass::Next => port.find_next(&mut entry),
        };
        pass = Pass::Next;
        if raw != RAW_OK {
            return raw;
        }
        if cfg!(file_service_control_search_keeps_rejected) || !port.rejected(&entry) {
            let at = output(port);
            port.copy(at, &entry, COPIED_BYTES);
            return RAW_OK;
        }
    }
}

/// The entry-record first search at 0x71e3a4.
pub fn find_first_entry<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let raw = if port.context() {
        let mut entry = port.cleared_entry();
        let (path, pattern, flag) = (port.address(PATH), port.address(PATTERN), port.word(FLAG));
        let raw = port.find_first_entry(path, pattern, &mut entry, flag);
        if raw == RAW_OK {
            describe(port, &entry);
        }
        raw
    } else {
        RAW_NO_CONTEXT
    };
    finish(port, callback, raw)
}

/// The entry-record next search at 0x71e550.
pub fn find_next_entry<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let raw = if port.context() {
        let mut entry = port.cleared_entry();
        let raw = port.find_next_entry(&mut entry);
        if raw == RAW_OK {
            describe(port, &entry);
        }
        raw
    } else {
        RAW_NO_CONTEXT
    };
    finish(port, callback, raw)
}

fn describe<P: Port>(port: &mut P, entry: &Entry) {
    let at = output(port);
    port.copy(at, entry, COPIED_BYTES);
    let at = output(port);
    port.set_byte(at, record::ATTRIBUTE, entry.byte(entry::ATTRIBUTE));
    let at = output(port);
    let size = port.little_endian(entry.word(entry::SIZE));
    port.set_word(at, record::SIZE, size);
    let unit = port.rounding();
    let at = output(port);
    let size = port.little_endian(entry.word(entry::SIZE));
    let rounded = if cfg!(file_service_control_entry_size_unrounded) {
        size
    } else {
        unit.wrapping_mul(port.divide(unit.wrapping_sub(1).wrapping_add(size), unit))
    };
    port.set_word(at, record::ROUNDED, rounded);
    let at = output(port);
    port.format_time(at, record::FIRST_TIME, entry, entry::FIRST_TIME, entry.byte(entry::FIRST_TIME_BYTE));
    let at = output(port);
    port.format_time(at, record::SECOND_TIME, entry, entry::SECOND_TIME, 0);
}
