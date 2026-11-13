#![forbid(unsafe_code)]

//! The stock state blocks the layer reads and writes, and a byte-array stand-in for them.

use crate::stock::layout::{self, SPAN_BYTES};

/// A block of state stock code outside this layer also reads or writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Block {
    /// Requested, claimed, pending, held and released work bits.
    Changes,
    Controller,
    Parameters,
    Status,
    /// One word per operation a caller asked the controller to keep up to date.
    Requests,
    Counters,
    Work,
    Selection,
    Policy,
    Dump,
    Diagnostics,
    Display,
    GeometryOperation,
    OperationCallbacks,
    RecordMode,
    /// The geometry service's state, which its stop entry 0xeecd94 also writes.
    GeometryService,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Location {
    pub block: Block,
    pub offset: u16,
}

impl Location {
    pub const fn new(block: Block, offset: u16) -> Self { Self { block, offset } }

    pub const fn plus(self, bytes: u16) -> Self { Self { block: self.block, offset: self.offset + bytes } }
}

/// Accesses are naturally sized so stock code reading the same field sees the store stock
/// would make.
pub trait Memory {
    fn read_u8(&self, at: Location) -> u8;
    fn read_u16(&self, at: Location) -> u16;
    fn read_u32(&self, at: Location) -> u32;
    fn write_u8(&mut self, at: Location, value: u8);
    fn write_u16(&mut self, at: Location, value: u16);
    fn write_u32(&mut self, at: Location, value: u32);

    fn read_i16(&self, at: Location) -> i16 { self.read_u16(at) as i16 }

    fn read_bytes(&self, at: Location, into: &mut [u8]) {
        for (index, byte) in into.iter_mut().enumerate() {
            *byte = self.read_u8(at.plus(index as u16));
        }
    }

    fn write_bytes(&mut self, at: Location, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.write_u8(at.plus(index as u16), *byte);
        }
    }

    fn fill(&mut self, at: Location, len: u16, value: u8) {
        for index in 0..len {
            self.write_u8(at.plus(index), value);
        }
    }

    fn set_bits_u8(&mut self, at: Location, bits: u8) { self.write_u8(at, self.read_u8(at) | bits) }

    fn clear_bits_u8(&mut self, at: Location, bits: u8) { self.write_u8(at, self.read_u8(at) & !bits) }

    fn set_bits_u32(&mut self, at: Location, bits: u32) { self.write_u32(at, self.read_u32(at) | bits) }
}

/// Every block as plain little-endian bytes at its stock distance from the first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bytes {
    bytes: [u8; SPAN_BYTES],
}

impl Default for Bytes {
    fn default() -> Self { Self { bytes: [0; SPAN_BYTES] } }
}

impl Bytes {
    fn span(&self, at: Location, len: usize) -> &[u8] { &self.bytes[layout::span_offset(at)..][..len] }

    fn span_mut(&mut self, at: Location, len: usize) -> &mut [u8] { &mut self.bytes[layout::span_offset(at)..][..len] }
}

impl Memory for Bytes {
    fn read_u8(&self, at: Location) -> u8 { self.span(at, 1)[0] }

    fn read_u16(&self, at: Location) -> u16 {
        let span = self.span(at, 2);
        u16::from_le_bytes([span[0], span[1]])
    }

    fn read_u32(&self, at: Location) -> u32 {
        let span = self.span(at, 4);
        u32::from_le_bytes([span[0], span[1], span[2], span[3]])
    }

    fn write_u8(&mut self, at: Location, value: u8) { self.span_mut(at, 1)[0] = value }

    fn write_u16(&mut self, at: Location, value: u16) { self.span_mut(at, 2).copy_from_slice(&value.to_le_bytes()) }

    fn write_u32(&mut self, at: Location, value: u32) { self.span_mut(at, 4).copy_from_slice(&value.to_le_bytes()) }
}
