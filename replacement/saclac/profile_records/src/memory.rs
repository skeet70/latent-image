#![forbid(unsafe_code)]

//! The three stock objects the view reads and writes, and a byte-array stand-in for them.

use crate::stock::layout::{RECORD_BYTES, REGISTRY_BYTES, SHARED_BYTES};

/// One of the two profile records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordIndex {
    First,
    Second,
}

impl RecordIndex {
    pub const fn index(self) -> usize {
        match self {
            RecordIndex::First => 0,
            RecordIndex::Second => 1,
        }
    }
}

/// A byte offset into the handle registry, one record or the shared block. The view forms
/// locations only from catalog offsets and bounded element indices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Location {
    Registry(u16),
    Record(RecordIndex, u16),
    Shared(u16),
}

/// Accesses are naturally sized so another task reading the same field sees the store stock
/// would make.
pub trait Memory {
    fn read_u8(&self, at: Location) -> u8;
    fn read_u16(&self, at: Location) -> u16;
    fn read_u32(&self, at: Location) -> u32;
    fn read_bytes(&self, at: Location, into: &mut [u8]);
    fn write_u8(&mut self, at: Location, value: u8);
    fn write_u16(&mut self, at: Location, value: u16);
    fn write_u32(&mut self, at: Location, value: u32);
    fn write_bytes(&mut self, at: Location, bytes: &[u8]);
}

/// The registry, both records and the shared block as plain little-endian bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bytes {
    pub registry: [u8; REGISTRY_BYTES],
    pub records: [[u8; RECORD_BYTES]; 2],
    pub shared: [u8; SHARED_BYTES],
}

impl Default for Bytes {
    fn default() -> Self { Self { registry: [0; REGISTRY_BYTES], records: [[0; RECORD_BYTES]; 2], shared: [0; SHARED_BYTES] } }
}

impl Bytes {
    fn span(&self, at: Location, len: usize) -> &[u8] {
        match at {
            Location::Registry(offset) => &self.registry[offset as usize..][..len],
            Location::Record(record, offset) => &self.records[record.index()][offset as usize..][..len],
            Location::Shared(offset) => &self.shared[offset as usize..][..len],
        }
    }

    fn span_mut(&mut self, at: Location, len: usize) -> &mut [u8] {
        match at {
            Location::Registry(offset) => &mut self.registry[offset as usize..][..len],
            Location::Record(record, offset) => &mut self.records[record.index()][offset as usize..][..len],
            Location::Shared(offset) => &mut self.shared[offset as usize..][..len],
        }
    }
}

impl Memory for Bytes {
    fn read_u8(&self, at: Location) -> u8 { self.span(at, 1)[0] }

    fn read_u16(&self, at: Location) -> u16 {
        let mut bytes = [0; 2];
        bytes.copy_from_slice(self.span(at, 2));
        u16::from_le_bytes(bytes)
    }

    fn read_u32(&self, at: Location) -> u32 {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(self.span(at, 4));
        u32::from_le_bytes(bytes)
    }

    fn read_bytes(&self, at: Location, into: &mut [u8]) { into.copy_from_slice(self.span(at, into.len())) }

    fn write_u8(&mut self, at: Location, value: u8) { self.span_mut(at, 1)[0] = value }

    fn write_u16(&mut self, at: Location, value: u16) { self.span_mut(at, 2).copy_from_slice(&value.to_le_bytes()) }

    fn write_u32(&mut self, at: Location, value: u32) { self.span_mut(at, 4).copy_from_slice(&value.to_le_bytes()) }

    fn write_bytes(&mut self, at: Location, bytes: &[u8]) { self.span_mut(at, bytes.len()).copy_from_slice(bytes) }
}
