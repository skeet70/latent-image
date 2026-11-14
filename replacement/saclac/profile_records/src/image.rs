#![forbid(unsafe_code)]

//! A record-shaped value saclac stages before applying it to the selected profile record.

use crate::field::{ChangeBank, ChangeBit, ChangeSet, RecordField, Value};
use crate::stock::layout::{self, RECORD_BYTES};

/// The copied header, including its two change masks (0x75adc0).
pub const HEADER_BYTES: usize = 0x90;

/// A dispatch-scoped header view; the complete record is not copied or zero-filled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header<'a> {
    bytes: &'a [u8; HEADER_BYTES],
}

impl<'a> Header<'a> {
    pub const fn new(bytes: &'a [u8; HEADER_BYTES]) -> Self { Self { bytes } }

    pub const fn bytes(self) -> &'a [u8; HEADER_BYTES] { self.bytes }

    pub(crate) fn get<T: Value, R, W>(self, field: &RecordField<T, R, W>) -> T { T::decode(&self.bytes[field.offset as usize..]) }

    pub(crate) fn changed(self, change: ChangeBit) -> bool {
        let at = layout::change_mask(change.bank) as usize;
        u64::from_le_bytes(self.bytes[at..at + 8].try_into().expect("header change mask")) & change.mask() != 0
    }

    /// Whether this mask holds exactly `changes`, or the other mask is clear (0x75ace4).
    pub fn changes_are(self, changes: ChangeSet) -> bool {
        let other = match changes.bank {
            ChangeBank::First => ChangeBank::Second,
            ChangeBank::Second => ChangeBank::First,
        };
        let mask = |bank| {
            let at = layout::change_mask(bank) as usize;
            u64::from_le_bytes(self.bytes[at..at + 8].try_into().expect("header change mask"))
        };
        mask(changes.bank) == changes.bits || mask(other) == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C, align(4))]
pub struct RecordImage {
    bytes: [u8; RECORD_BYTES],
}

impl Default for RecordImage {
    fn default() -> Self { Self { bytes: [0; RECORD_BYTES] } }
}

impl RecordImage {
    pub fn from_bytes(bytes: [u8; RECORD_BYTES]) -> Self { Self { bytes } }

    pub fn bytes(&self) -> &[u8; RECORD_BYTES] { &self.bytes }

    pub fn header(&self) -> Header<'_> { Header::new(self.bytes[..HEADER_BYTES].try_into().expect("record header")) }

    pub fn get<T: Value, R, W>(&self, field: &RecordField<T, R, W>) -> T { T::decode(&self.bytes[field.offset as usize..]) }

    /// Stores the value and sets the field's change bit, as stock's field writers do on any
    /// record they are given.
    pub fn set<T: Value, R, W>(&mut self, field: &RecordField<T, R, W>, value: T) {
        value.encode(&mut self.bytes[field.offset as usize..]);
        if let Some(change) = field.change {
            let mask = self.mask(change.bank) | change.mask();
            self.set_mask(change.bank, mask);
        }
    }

    pub fn changed(&self, change: ChangeBit) -> bool { self.mask(change.bank) & change.mask() != 0 }


    pub fn clear_changes(&mut self) {
        self.set_mask(ChangeBank::First, 0);
        self.set_mask(ChangeBank::Second, 0);
    }

    fn mask(&self, bank: ChangeBank) -> u64 { u64::from_le_bytes(<[u8; 8]>::decode(&self.bytes[layout::change_mask(bank) as usize..])) }

    fn set_mask(&mut self, bank: ChangeBank, mask: u64) { mask.to_le_bytes().encode(&mut self.bytes[layout::change_mask(bank) as usize..]) }
}
