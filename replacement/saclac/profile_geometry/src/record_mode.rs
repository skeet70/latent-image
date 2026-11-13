#![forbid(unsafe_code)]

//! The profile record mode: up to three capture buffers laid out at initialization, and each
//! channel's records, which a finish keeps and an export writes to files and into a frame dump.

use crate::controller::{ProfileGeometry, Refusal, Status, Tables};
use crate::memory::{Location, Memory};
use crate::ports::{ConfigOffset, DumpBuffer, Ports, StockData};
use crate::services::DumpKind;
use crate::stock::layout::{self, record_mode as record, record_mode::snapshot};

const CHANNELS: ConfigOffset = ConfigOffset(0x98c0);
const SETTING_WORD: ConfigOffset = ConfigOffset(0x98c4);
const SETTING_BYTE: ConfigOffset = ConfigOffset(0x98c3);
const RANGE: [ConfigOffset; 2] = [ConfigOffset(0x98c1), ConfigOffset(0x98c2)];
const IDLE: u32 = 1;
const EXPORTING: u32 = 6;
const BUFFER_BASE: u32 = 0x48a6_1000;
const RECORD_SIZE: u32 = 0x24;
const BUFFERS: u16 = 3;
/// The buffer state a finish takes, and the one it leaves.
const FILLED: u32 = 2;
const TAKEN: u32 = 3;
const ALL_RECORDS: u16 = 0xff;
const NO_BUFFER: u32 = 3;

/// A record channel as callers name it. Channels 2 to 5 each have an enable bit in
/// configuration byte 0x98c0, bit `channel - 2`; any other code needs any bit. Channels 2 and 5
/// keep one record taken from a filled buffer, 3 and 4 keep up to 6 and 152 records; 5 is the
/// controllers' S1 statistics channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordChannel(pub u32);

impl RecordChannel {
    pub const SINGLE: Self = Self(2);
    pub const SHORT: Self = Self(3);
    pub const LONG: Self = Self(4);
    pub const STATISTICS: Self = Self(5);
    pub const ANY: Self = Self(7);
    const RECORDING: [Self; 4] = [Self::SINGLE, Self::SHORT, Self::LONG, Self::STATISTICS];

    const fn bit(self) -> Option<u8> {
        match self.0 {
            2..=5 => Some(1 << (self.0 - 2)),
            _ => None,
        }
    }

    const fn valid(self) -> bool { matches!(self.0, 2..=5 | 7) }

    /// The channel's index in the record tables, for channels 2 to 5.
    const fn slot(self) -> Option<usize> {
        match self.0 {
            2..=5 => Some(self.0 as usize - 2),
            _ => None,
        }
    }
}

fn buffer(index: u16) -> Location { record::BUFFERS.plus(record::BUFFER_STRIDE * index) }

fn record_at(slot: usize, index: u16) -> Location { record::RECORDS[slot].0.plus(record::RECORD_STRIDE * index) }

fn kept_at(slot: usize) -> Location { record::KEPT.plus(2 * slot as u16) }

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xef7a90.
    fn channel_enabled(&mut self, channel: RecordChannel) -> bool {
        let bits = self.ports.byte(CHANNELS);
        match channel.bit() {
            Some(bit) => bits & bit != 0,
            None => bits != 0,
        }
    }

    fn record_state(&self) -> u32 { self.memory.read_u32(record::STATE) }

    /// 0xef7d1c.
    pub fn record_mode_available(&mut self, channel: RecordChannel) -> bool { channel.valid() && self.channel_enabled(channel) }

    /// Clears the record mode block and lays out one buffer per enabled capture channel
    /// (0xef7d68).
    ///
    /// Suspected stock defect, kept: the buffers are laid out by the size word at +0x24, which
    /// the clear has just zeroed, so every buffer starts at 0x48a61000.
    pub fn initialize_record_mode(&mut self) -> Status {
        if !self.channel_enabled(RecordChannel::ANY) {
            return Err(Refusal::BadArgument);
        }
        self.memory.fill(record::STATE, record::BYTES, 0);
        let single = !self.channel_enabled(RecordChannel::SINGLE) && !self.channel_enabled(RecordChannel::STATISTICS);
        let count = if single {
            1
        } else if !self.channel_enabled(RecordChannel::SINGLE) || !self.channel_enabled(RecordChannel::STATISTICS) {
            2
        } else {
            3
        };
        let size = self.memory.read_u32(record::BUFFER_BYTES);
        for index in 0..count {
            self.memory.write_u32(buffer(index).plus(4), BUFFER_BASE.wrapping_add(size.wrapping_mul(index as u32)));
        }
        self.memory.write_u8(record::BUFFER_COUNT, count as u8);
        let end = BUFFER_BASE.wrapping_add(size.wrapping_mul(count as u32));
        self.memory.write_u32(record::END_ADDRESS, end);
        self.memory.write_u32(record::START_ADDRESS, end);
        self.memory.write_u32(record::STATE, IDLE);
        Ok(())
    }

    /// 0xef7eb4.
    pub fn begin_record_mode(&mut self, channel: RecordChannel) -> Status {
        if !self.channel_enabled(channel) {
            return Err(Refusal::BadArgument);
        }
        if self.record_state() != IDLE {
            return Err(Refusal::RecordState);
        }
        if !channel.valid() {
            return Err(Refusal::BadArgument);
        }
        self.memory.write_u32(record::STATE, channel.0);
        let Some(slot) = channel.slot() else {
            return Ok(());
        };
        let (records, capacity) = record::RECORDS[slot];
        if matches!(channel, RecordChannel::SINGLE | RecordChannel::STATISTICS) {
            self.memory.write_u32(record::SINGLE_SIZE, RECORD_SIZE);
            self.memory.write_u32(record::FIRST_FREE, NO_BUFFER);
            self.memory.write_u32(record::SECOND_FREE, NO_BUFFER);
            self.memory.fill(records, record::RECORD_STRIDE * capacity, 0);
            self.memory.write_u16(kept_at(slot), 0);
            self.find_free_buffers();
            return Ok(());
        }
        self.memory.write_u32(record::MANY_SIZE, RECORD_SIZE);
        let (cleared, bytes) = record::MANY_CLEARED;
        self.memory.fill(cleared, bytes, 0);
        self.memory.write_u32(record::FIRST_FREE_ADDRESS, 0);
        self.memory.write_u16(record::RECORD_COUNT, 0);
        self.memory.fill(records, record::RECORD_STRIDE * capacity, 0);
        self.memory.write_u16(kept_at(slot), 0);
        if channel == RecordChannel::SHORT {
            self.memory.write_u32(record::RECORDING_ADDRESS, self.memory.read_u32(record::START_ADDRESS));
            self.memory.write_u32(record::SETTING_WORD, 0);
            self.memory.write_u8(record::SETTING_BYTE, 0);
            self.memory.write_u16(record::RANGE[0], 0);
            self.memory.write_u16(record::RANGE[1], ALL_RECORDS);
            return Ok(());
        }
        self.memory.write_u32(record::RECORDING_ADDRESS, self.memory.read_u32(record::END_ADDRESS));
        let count = self.buffer_count();
        let free = (0..count).find(|&index| self.memory.read_u32(buffer(index)) == 0);
        if let Some(index) = free {
            self.memory.write_u32(record::FIRST_FREE_ADDRESS, self.memory.read_u32(buffer(index).plus(4)));
        }
        let word = self.ports.byte(SETTING_WORD);
        self.memory.write_u32(record::SETTING_WORD, word as u32);
        let byte = self.ports.byte(SETTING_BYTE);
        self.memory.write_u8(record::SETTING_BYTE, byte);
        for (at, offset) in record::RANGE.into_iter().zip(RANGE) {
            let index = match self.ports.byte(offset) {
                0 => 0,
                _ => (self.ports.byte(offset) as u16).wrapping_sub(1),
            };
            self.memory.write_u16(at, index);
        }
        Ok(())
    }

    fn buffer_count(&self) -> u16 { self.memory.read_u8(record::BUFFER_COUNT) as u16 }

    fn find_free_buffers(&mut self) {
        let count = self.buffer_count();
        let free = |memory: &M, from: u16| (from..count).find(|&index| memory.read_u32(buffer(index)) == 0);
        let Some(first) = free(&self.memory, 0) else {
            return;
        };
        self.memory.write_u32(record::FIRST_FREE, first as u32);
        if let Some(second) = free(&self.memory, first + 1) {
            self.memory.write_u32(record::SECOND_FREE, second as u32);
        }
    }

    /// Keeps the recording channel's records: a filled buffer for a single-record channel,
    /// the records at or below `limit` for the others (0xef87b0).
    pub fn finish_record_mode(&mut self, limit: u16) -> Status {
        if !self.channel_enabled(RecordChannel::ANY) {
            return Err(Refusal::BadArgument);
        }
        let state = self.record_state();
        if state <= IDLE {
            return Err(Refusal::RecordState);
        }
        match RecordChannel(state) {
            RecordChannel::SINGLE => self.take_filled_buffer(0),
            RecordChannel::STATISTICS => self.take_filled_buffer(3),
            RecordChannel::SHORT => self.keep_records(limit, 1),
            RecordChannel::LONG => self.keep_records(limit, 2),
            _ => {}
        }
        self.memory.write_u32(record::STATE, IDLE);
        Ok(())
    }

    /// Takes the last filled buffer into the slot's record and releases the others the finish
    /// has not taken (0xef788c).
    fn take_filled_buffer(&mut self, slot: usize) {
        let mut taken = None;
        for index in 0..BUFFERS {
            let state = buffer(index);
            match self.memory.read_u32(state) {
                FILLED => {
                    self.memory.write_u32(state, TAKEN);
                    taken = Some(index);
                }
                TAKEN => {}
                _ => self.memory.write_u32(state, 0),
            }
        }
        let Some(index) = taken else {
            self.memory.write_u16(kept_at(slot), 0);
            return;
        };
        self.memory.write_u16(kept_at(slot), 1);
        let (from, to) = (buffer(index), record_at(slot, 0));
        self.memory.write_u32(to.plus(4), self.memory.read_u32(from.plus(4)));
        self.memory.write_u16(to.plus(8), self.memory.read_u16(from.plus(0xc)));
        self.memory.write_u16(to.plus(0xa), self.memory.read_u16(from.plus(0xe)));
        self.memory.write_u16(to, 1);
        self.memory.write_u16(to.plus(0x10), self.memory.read_u16(from.plus(0x10)));
    }

    /// Drops the records whose enable halfword exceeds `limit`, counts the rest and, while the
    /// short channel records, ends the recording area after the last one kept (0xef793c).
    ///
    /// Corrected: a record count that runs past the record mode block reads and writes past it
    /// in stock (0xef7968..0xef7998); owned code stops at the block's end.
    fn keep_records(&mut self, limit: u16, slot: usize) {
        let (records, _) = record::RECORDS[slot];
        let room = (record::BYTES - records.offset) / record::RECORD_STRIDE;
        let count = self.memory.read_u16(record::RECORD_COUNT).min(room);
        let mut kept = 0u16;
        let mut last = 0;
        for index in 0..count {
            let at = record_at(slot, index);
            match self.memory.read_u16(at) {
                enable if enable > limit => self.memory.write_u16(at, 0),
                0 => {}
                _ => {
                    kept = kept.wrapping_add(1);
                    last = index;
                }
            }
        }
        if self.record_state() == RecordChannel::SHORT.0 {
            let at = record_at(slot, last);
            let bytes = (self.memory.read_u16(at.plus(0xa)) as u32).wrapping_mul(self.memory.read_u16(at.plus(8)) as u32) << 1;
            self.memory.write_u32(record::END_ADDRESS, self.memory.read_u32(at.plus(4)).wrapping_add(bytes));
        }
        self.memory.write_u16(kept_at(slot), kept);
    }

    /// Writes each kept record's buffer to a raw dump file, then the record snapshot into
    /// `dump` (0xef8934).
    pub fn export_record_snapshot(&mut self, dump: Option<DumpBuffer>) {
        if self.ports.byte(CHANNELS) == 0 || self.record_state() != IDLE {
            return;
        }
        let Some(dump) = dump else {
            return;
        };
        self.memory.write_u32(record::STATE, EXPORTING);
        for slot in 0..RecordChannel::RECORDING.len() {
            self.write_record_files(slot);
        }
        self.write_snapshot(dump);
        self.memory.write_u32(record::STATE, IDLE);
    }

    /// 0xef7b30.
    fn write_record_files(&mut self, slot: usize) {
        if self.memory.read_u16(kept_at(slot)) == 0 {
            return;
        }
        for index in 0..record::RECORDS[slot].1 {
            let at = record_at(slot, index);
            if self.memory.read_u16(at) == 0 {
                continue;
            }
            let data = StockData(self.memory.read_u32(at.plus(4)));
            let length = (self.memory.read_u16(at.plus(8)) as u32).wrapping_mul(self.memory.read_u16(at.plus(0xa)) as u32) << 1;
            let numbers = StockData(layout::address(at.plus(0xc)) as u32);
            // Stock ignores the writer's status, including its 0x84 for a null address.
            let _ = self.write_dump(DumpKind::Raw, (data.0 != 0).then_some(data), length, Some(numbers));
        }
    }

    /// 0xef8934 from 0xef89e0, with 0xef79e8.
    fn write_snapshot(&mut self, dump: DumpBuffer) {
        let write = |layer: &mut Self, offset: u32, bytes: &[u8]| layer.ports.write_dump(dump, snapshot::OFFSET + offset, bytes);
        const ZEROS: [u8; 64] = [0; 64];
        let mut offset = 0;
        while offset < snapshot::BYTES {
            let length = (snapshot::BYTES - offset).min(ZEROS.len() as u32);
            write(self, offset, &ZEROS[..length as usize]);
            offset += length;
        }
        write(self, snapshot::HEAD_TAG.0, &snapshot::HEAD_TAG.1);
        write(self, snapshot::TAIL_TAG.0, &snapshot::TAIL_TAG.1);
        let enabled = self.ports.byte(CHANNELS) as u32;
        write(self, snapshot::ENABLED, &enabled.to_le_bytes());
        for (pair, at) in snapshot::KEPT.into_iter().enumerate() {
            let first = self.memory.read_u16(kept_at(2 * pair)) as u32 & 0xfff;
            let second = self.memory.read_u16(kept_at(2 * pair + 1)) as u32;
            write(self, at, &(first << 16 | second).to_le_bytes());
        }
        let mut cursor = 0u32;
        for (slot, channel) in RecordChannel::RECORDING.into_iter().enumerate() {
            for index in 0..record::RECORDS[slot].1 {
                let at = record_at(slot, index);
                let enable = self.memory.read_u16(at) as u32;
                if enable == 0 {
                    continue;
                }
                let half = |offset: u16| self.memory.read_u16(at.plus(offset)) as u32;
                let words = [enable | (channel.0 & 0xff) << 16, half(0x10), half(8) | half(0xa) << 16, half(0xe) | half(0xc) << 16];
                for (column, word) in snapshot::COLUMNS.into_iter().zip(words) {
                    write(self, column + 4 * cursor, &word.to_le_bytes());
                }
                cursor += 1;
            }
        }
    }
}
