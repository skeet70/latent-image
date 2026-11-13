#![forbid(unsafe_code)]

//! The three stock objects the view reads and writes, and a byte-array stand-in for them.

use core::marker::PhantomData;

use crate::field::Value;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Block {
    Registry,
    Record(RecordIndex),
    Shared,
}

impl Block {
    const fn bytes(self) -> usize {
        match self {
            Block::Registry => REGISTRY_BYTES,
            Block::Record(_) => RECORD_BYTES,
            Block::Shared => SHARED_BYTES,
        }
    }
}

/// Where a `T` lies in one block: wholly inside it, at an offset that is a multiple of `T`'s
/// alignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Location<T> {
    block: Block,
    offset: u16,
    value: PhantomData<T>,
}

impl<T: Value> Location<T> {
    pub(crate) const fn new(block: Block, offset: u16) -> Self {
        assert!(offset as usize + T::BYTES <= block.bytes(), "location outside its block");
        assert!(offset as usize % T::ALIGN == 0, "misaligned location");
        Self { block, offset, value: PhantomData }
    }

    /// The same place holding a `U` of the same width.
    pub(crate) const fn retype<U: Value>(self) -> Location<U> { Location::new(self.block, self.offset) }
}

impl<T> Location<T> {
    pub(crate) const fn block(&self) -> Block { self.block }

    pub(crate) const fn offset(&self) -> u16 { self.offset }
}

/// One access of a block copy.
enum Unit {
    Byte,
    Half,
    Word,
}

/// The widest access the offset's alignment and the bytes left allow.
const fn unit(offset: usize, left: usize) -> Unit {
    if offset % 4 == 0 && left >= 4 {
        Unit::Word
    } else if offset % 2 == 0 && left >= 2 {
        Unit::Half
    } else {
        Unit::Byte
    }
}

/// Scalar accesses are single loads and stores of their width. Byte blocks are copied in
/// ascending order, each access the widest `unit` allows.
pub trait Memory {
    fn read_u8(&self, at: Location<u8>) -> u8;
    fn read_u16(&self, at: Location<u16>) -> u16;
    fn read_u32(&self, at: Location<u32>) -> u32;
    fn write_u8(&mut self, at: Location<u8>, value: u8);
    fn write_u16(&mut self, at: Location<u16>, value: u16);
    fn write_u32(&mut self, at: Location<u32>, value: u32);

    fn read_bytes<const N: usize>(&self, at: Location<[u8; N]>) -> [u8; N] {
        let mut bytes = [0; N];
        let mut index = 0;
        while index < N {
            let offset = at.offset + index as u16;
            match unit(offset as usize, N - index) {
                Unit::Word => {
                    bytes[index..index + 4].copy_from_slice(&self.read_u32(Location::new(at.block, offset)).to_le_bytes());
                    index += 4;
                }
                Unit::Half => {
                    bytes[index..index + 2].copy_from_slice(&self.read_u16(Location::new(at.block, offset)).to_le_bytes());
                    index += 2;
                }
                Unit::Byte => {
                    bytes[index] = self.read_u8(Location::new(at.block, offset));
                    index += 1;
                }
            }
        }
        bytes
    }

    fn write_bytes<const N: usize>(&mut self, at: Location<[u8; N]>, bytes: [u8; N]) {
        let mut index = 0;
        while index < N {
            let offset = at.offset + index as u16;
            match unit(offset as usize, N - index) {
                Unit::Word => {
                    self.write_u32(Location::new(at.block, offset), u32::from_le_bytes([bytes[index], bytes[index + 1], bytes[index + 2], bytes[index + 3]]));
                    index += 4;
                }
                Unit::Half => {
                    self.write_u16(Location::new(at.block, offset), u16::from_le_bytes([bytes[index], bytes[index + 1]]));
                    index += 2;
                }
                Unit::Byte => {
                    self.write_u8(Location::new(at.block, offset), bytes[index]);
                    index += 1;
                }
            }
        }
    }
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
    fn span<T: Value>(&self, at: Location<T>) -> &[u8] {
        let block: &[u8] = match at.block {
            Block::Registry => &self.registry,
            Block::Record(record) => &self.records[record.index()],
            Block::Shared => &self.shared,
        };
        &block[at.offset as usize..][..T::BYTES]
    }

    fn span_mut<T: Value>(&mut self, at: Location<T>) -> &mut [u8] {
        let block: &mut [u8] = match at.block {
            Block::Registry => &mut self.registry,
            Block::Record(record) => &mut self.records[record.index()],
            Block::Shared => &mut self.shared,
        };
        &mut block[at.offset as usize..][..T::BYTES]
    }
}

impl Memory for Bytes {
    fn read_u8(&self, at: Location<u8>) -> u8 { self.span(at)[0] }

    fn read_u16(&self, at: Location<u16>) -> u16 { u16::decode(self.span(at)) }

    fn read_u32(&self, at: Location<u32>) -> u32 { u32::decode(self.span(at)) }

    fn write_u8(&mut self, at: Location<u8>, value: u8) { self.span_mut(at)[0] = value }

    fn write_u16(&mut self, at: Location<u16>, value: u16) { value.encode(self.span_mut(at)) }

    fn write_u32(&mut self, at: Location<u32>, value: u32) { value.encode(self.span_mut(at)) }
}
