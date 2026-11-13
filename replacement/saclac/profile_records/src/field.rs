#![forbid(unsafe_code)]

//! Typed field descriptors. Each carries the gate stock's accessor applies, which is not always
//! the gate its group would suggest.

use core::marker::PhantomData;

use crate::handle::AccessGroup;
use crate::memory::{Location, Memory};

/// A value with a fixed little-endian width in record memory.
pub trait Value: Copy {
    const BYTES: usize;
    fn load(memory: &impl Memory, at: Location) -> Self;
    fn store(self, memory: &mut impl Memory, at: Location);
    fn decode(bytes: &[u8]) -> Self;
    fn encode(self, into: &mut [u8]);
}

macro_rules! scalar_value {
    ($($type:ty => $load:ident, $store:ident, $raw:ty;)*) => {$(
        impl Value for $type {
            const BYTES: usize = core::mem::size_of::<$type>();
            fn load(memory: &impl Memory, at: Location) -> Self { memory.$load(at) as $type }
            fn store(self, memory: &mut impl Memory, at: Location) { memory.$store(at, self as $raw) }
            fn decode(bytes: &[u8]) -> Self {
                let mut raw = [0; core::mem::size_of::<$type>()];
                raw.copy_from_slice(&bytes[..Self::BYTES]);
                <$type>::from_le_bytes(raw)
            }
            fn encode(self, into: &mut [u8]) { into[..Self::BYTES].copy_from_slice(&self.to_le_bytes()) }
        }
    )*};
}

scalar_value! {
    u8 => read_u8, write_u8, u8;
    i8 => read_u8, write_u8, u8;
    u16 => read_u16, write_u16, u16;
    i16 => read_u16, write_u16, u16;
    u32 => read_u32, write_u32, u32;
    i32 => read_u32, write_u32, u32;
}

impl<const N: usize> Value for [u8; N] {
    const BYTES: usize = N;

    fn load(memory: &impl Memory, at: Location) -> Self {
        let mut bytes = [0; N];
        memory.read_bytes(at, &mut bytes);
        bytes
    }

    fn store(self, memory: &mut impl Memory, at: Location) { memory.write_bytes(at, &self) }

    fn decode(bytes: &[u8]) -> Self {
        let mut value = [0; N];
        value.copy_from_slice(&bytes[..N]);
        value
    }

    fn encode(self, into: &mut [u8]) { into[..N].copy_from_slice(&self) }
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

impl<T, R, W> RecordField<T, R, W> {
    pub(crate) const fn new(offset: u16, read: R, write: W, change: Option<ChangeBit>) -> Self {
        Self { offset, read, write, change, value: PhantomData }
    }

    pub const fn change(&self) -> Option<ChangeBit> { self.change }
}

impl<T> RecordField<T> {
    /// Read and written under one group, the write granted.
    pub(crate) const fn grouped(offset: u16, group: u8, change: Option<ChangeBit>) -> Self {
        let group = AccessGroup::at(group);
        Self::new(offset, RecordRead(group), RecordWrite::Granted(group), change)
    }
}

impl<T, R> RecordField<T, R, RecordWrite> {
    /// The same field with its read gate dropped.
    pub const fn writer(&self) -> RecordField<T, Unread, RecordWrite> { RecordField::new(self.offset, Unread, self.write, self.change) }
}

/// A field of the shared block, which no handle selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedField<T, R = SharedRead, W = SharedWrite> {
    pub(crate) offset: u16,
    pub(crate) read: R,
    pub(crate) write: W,
    value: PhantomData<T>,
}

impl<T, R, W> SharedField<T, R, W> {
    pub(crate) const fn new(offset: u16, read: R, write: W) -> Self { Self { offset, read, write, value: PhantomData } }
}

impl<T> SharedField<T> {
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

impl<T: Copy, const N: usize, R: Copy, W: Copy> RecordArray<T, N, R, W> {
    pub(crate) const fn new(element: RecordField<T, R, W>, stride: u16) -> Self { Self { element, stride } }

    pub const fn at(&self, index: Index<N>) -> RecordField<T, R, W> {
        RecordField::new(self.element.offset + self.stride * index.0 as u16, self.element.read, self.element.write, self.element.change)
    }
}

/// `N` elements `stride` bytes apart in the shared block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedArray<T, const N: usize, R = SharedRead, W = SharedWrite> {
    pub(crate) element: SharedField<T, R, W>,
    pub(crate) stride: u16,
}

impl<T: Copy, const N: usize, R: Copy, W: Copy> SharedArray<T, N, R, W> {
    pub(crate) const fn new(element: SharedField<T, R, W>, stride: u16) -> Self { Self { element, stride } }

    pub const fn at(&self, index: Index<N>) -> SharedField<T, R, W> {
        SharedField::new(self.element.offset + self.stride * index.0 as u16, self.element.read, self.element.write)
    }
}
