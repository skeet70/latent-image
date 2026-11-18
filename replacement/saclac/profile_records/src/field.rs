#![forbid(unsafe_code)]

//! Typed field descriptors. Each carries the gate stock's accessor applies, which is not always
//! the gate its group would suggest.

use core::marker::PhantomData;

use crate::handle::AccessGroup;
use crate::memory::{Location, Memory};
use crate::stock::layout::{RECORD_BYTES, SHARED_BYTES};

/// A value with a fixed little-endian width in record memory.
pub trait Value: Copy {
    const BYTES: usize;
    const ALIGN: usize;
    fn load(memory: &impl Memory, at: Location<Self>) -> Self;
    fn store(self, memory: &mut impl Memory, at: Location<Self>);
    fn decode(bytes: &[u8]) -> Self;
    fn encode(self, into: &mut [u8]);
}

macro_rules! unsigned_value {
    ($($type:ty => $load:ident, $store:ident;)*) => {$(
        impl Value for $type {
            const BYTES: usize = core::mem::size_of::<$type>();
            const ALIGN: usize = core::mem::size_of::<$type>();
            fn load(memory: &impl Memory, at: Location<Self>) -> Self { memory.$load(at) }
            fn store(self, memory: &mut impl Memory, at: Location<Self>) { memory.$store(at, self) }
            fn decode(bytes: &[u8]) -> Self {
                let mut raw = [0; core::mem::size_of::<$type>()];
                raw.copy_from_slice(&bytes[..Self::BYTES]);
                <$type>::from_le_bytes(raw)
            }
            fn encode(self, into: &mut [u8]) { into[..Self::BYTES].copy_from_slice(&self.to_le_bytes()) }
        }
    )*};
}

unsigned_value! {
    u8 => read_u8, write_u8;
    u16 => read_u16, write_u16;
    u32 => read_u32, write_u32;
}

macro_rules! signed_value {
    ($($type:ty => $raw:ty;)*) => {$(
        impl Value for $type {
            const BYTES: usize = <$raw as Value>::BYTES;
            const ALIGN: usize = <$raw as Value>::ALIGN;
            fn load(memory: &impl Memory, at: Location<Self>) -> Self { <$raw>::load(memory, at.retype()) as $type }
            fn store(self, memory: &mut impl Memory, at: Location<Self>) { (self as $raw).store(memory, at.retype()) }
            fn decode(bytes: &[u8]) -> Self { <$raw>::decode(bytes) as $type }
            fn encode(self, into: &mut [u8]) { (self as $raw).encode(into) }
        }
    )*};
}

signed_value! {
    i8 => u8;
    i16 => u16;
    i32 => u32;
}

impl<const N: usize> Value for [u8; N] {
    const BYTES: usize = N;
    const ALIGN: usize = 1;

    fn load(memory: &impl Memory, at: Location<Self>) -> Self { memory.read_bytes(at) }

    fn store(self, memory: &mut impl Memory, at: Location<Self>) { memory.write_bytes(at, self) }

    fn decode(bytes: &[u8]) -> Self {
        let mut value = [0; N];
        value.copy_from_slice(&bytes[..N]);
        value
    }

    fn encode(self, into: &mut [u8]) { into[..N].copy_from_slice(&self) }
}

/// Whether `count` values of `T`, `stride` bytes apart from `offset`, lie aligned in a block of
/// `block` bytes.
const fn fits<T: Value>(offset: u16, stride: u16, count: usize, block: usize) -> bool {
    count > 0
        && offset as usize + stride as usize * (count - 1) + T::BYTES <= block
        && offset as usize % T::ALIGN == 0
        && stride as usize % T::ALIGN == 0
}

/// Which of the record's two change masks a field reports its writes in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeBank {
    First,
    Second,
}

/// The bit a record write sets in the written record's change mask.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeBit {
    pub(crate) bank: ChangeBank,
    pub(crate) bit: u8,
}

impl ChangeBit {
    pub(crate) const fn new(bank: ChangeBank, bit: u8) -> Self {
        assert!(bit < 63, "change bit out of range");
        Self { bank, bit }
    }

    pub const fn mask(self) -> u64 { 1 << self.bit }
}

/// Bits of one change mask.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeSet {
    pub(crate) bank: ChangeBank,
    pub(crate) bits: u64,
}

impl ChangeSet {
    pub const fn of(bit: ChangeBit) -> Self { Self { bank: bit.bank, bits: bit.mask() } }

    /// `None` when `bit` reports in the other mask.
    pub const fn with(self, bit: ChangeBit) -> Option<Self> {
        match (self.bank, bit.bank) {
            (ChangeBank::First, ChangeBank::First) | (ChangeBank::Second, ChangeBank::Second) => {
                Some(Self { bank: self.bank, bits: self.bits | bit.mask() })
            }
            _ => None,
        }
    }
}

/// A record read takes its handle's record only when the slot grants the group; otherwise it
/// reads the globally selected record. It is never refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordRead(pub(crate) AccessGroup);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordWrite {
    /// Refused unless the handle may write the group; lands in the handle's record.
    Granted(AccessGroup),
    /// Refused unless the handle may write every listed group.
    GrantedAll(&'static [AccessGroup]),
    /// Never refused; lands where a read of the group would read.
    Selected(AccessGroup),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedRead {
    Open,
    /// Refused unless the handle may write the group.
    Requires(AccessGroup),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedWrite {
    Granted(AccessGroup),
    Open,
}

/// No task-43 accessor reads this field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unread;

/// No task-43 accessor writes this field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unwritten;

/// A field at a fixed offset in whichever record the gate selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordField<T, R = RecordRead, W = RecordWrite> {
    pub(crate) offset: u16,
    pub(crate) read: R,
    pub(crate) write: W,
    pub(crate) change: Option<ChangeBit>,
    value: PhantomData<T>,
}

impl<T: Value, R, W> RecordField<T, R, W> {
    pub(crate) const fn new(offset: u16, read: R, write: W, change: Option<ChangeBit>) -> Self {
        assert!(fits::<T>(offset, 0, 1, RECORD_BYTES), "record field outside the record or misaligned");
        Self { offset, read, write, change, value: PhantomData }
    }
}

impl<T, R, W> RecordField<T, R, W> {
    pub const fn change(&self) -> Option<ChangeBit> { self.change }
}

impl<T: Value> RecordField<T> {
    /// Read and written under one group, the write granted.
    pub(crate) const fn grouped(offset: u16, group: u8, change: Option<ChangeBit>) -> Self {
        let group = AccessGroup::at(group);
        Self::new(offset, RecordRead(group), RecordWrite::Granted(group), change)
    }
}

impl<T, R> RecordField<T, R, RecordWrite> {
    /// The same field with its read gate dropped.
    pub const fn writer(&self) -> RecordField<T, Unread, RecordWrite> {
        RecordField { offset: self.offset, read: Unread, write: self.write, change: self.change, value: PhantomData }
    }
}

/// A field of the shared block, which no handle selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedField<T, R = SharedRead, W = SharedWrite> {
    pub(crate) offset: u16,
    pub(crate) read: R,
    pub(crate) write: W,
    value: PhantomData<T>,
}

impl<T: Value, R, W> SharedField<T, R, W> {
    pub(crate) const fn new(offset: u16, read: R, write: W) -> Self {
        assert!(fits::<T>(offset, 0, 1, SHARED_BYTES), "shared field outside the block or misaligned");
        Self { offset, read, write, value: PhantomData }
    }
}

impl<T: Value> SharedField<T> {
    /// Read openly, written under `group`.
    pub(crate) const fn grouped(offset: u16, group: u8) -> Self {
        Self::new(offset, SharedRead::Open, SharedWrite::Granted(AccessGroup::at(group)))
    }
}

/// An element index below `N`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Index<const N: usize>(u8);

impl<const N: usize> Index<N> {
    pub const fn new(index: usize) -> Option<Self> {
        if index < N { Some(Self(index as u8)) } else { None }
    }

    pub const fn get(self) -> usize { self.0 as usize }
}

/// `N` elements `stride` bytes apart in whichever record the gate selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordArray<T, const N: usize, R = RecordRead, W = RecordWrite> {
    pub(crate) element: RecordField<T, R, W>,
    pub(crate) stride: u16,
}

impl<T: Value, const N: usize, R: Copy, W: Copy> RecordArray<T, N, R, W> {
    pub(crate) const fn new(element: RecordField<T, R, W>, stride: u16) -> Self {
        assert!(fits::<T>(element.offset, stride, N, RECORD_BYTES), "record array outside the record or misaligned");
        Self { element, stride }
    }

    pub const fn at(&self, index: Index<N>) -> RecordField<T, R, W> {
        let element = self.element;
        RecordField { offset: element.offset + self.stride * index.0 as u16, read: element.read, write: element.write, change: element.change, value: PhantomData }
    }
}

/// `N` elements `stride` bytes apart in the shared block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedArray<T, const N: usize, R = SharedRead, W = SharedWrite> {
    pub(crate) element: SharedField<T, R, W>,
    pub(crate) stride: u16,
}

impl<T: Value, const N: usize, R: Copy, W: Copy> SharedArray<T, N, R, W> {
    pub(crate) const fn new(element: SharedField<T, R, W>, stride: u16) -> Self {
        assert!(fits::<T>(element.offset, stride, N, SHARED_BYTES), "shared array outside the block or misaligned");
        Self { element, stride }
    }

    pub const fn at(&self, index: Index<N>) -> SharedField<T, R, W> {
        let element = self.element;
        SharedField { offset: element.offset + self.stride * index.0 as u16, read: element.read, write: element.write, value: PhantomData }
    }
}
