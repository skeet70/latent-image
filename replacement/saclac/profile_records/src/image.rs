#![forbid(unsafe_code)]

//! A record-shaped value saclac stages before applying it to the selected profile record.

use crate::field::{ChangeBank, ChangeBit, RecordField, Value};
use crate::stock::layout::{self, RECORD_BYTES};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordImage {
    bytes: [u8; RECORD_BYTES],
}

impl Default for RecordImage {
    fn default() -> Self { Self { bytes: [0; RECORD_BYTES] } }
}

impl RecordImage {
    pub fn from_bytes(bytes: [u8; RECORD_BYTES]) -> Self { Self { bytes } }

    pub fn bytes(&self) -> &[u8; RECORD_BYTES] { &self.bytes }

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
