use crate::area2::{Area2Record, RECORD_BYTES};
use crate::record_store::{Callbacks, CheckVerdict};

const PROTECTED_BYTES: usize = 0x1ec;
const ROWS: usize = PROTECTED_BYTES / 4;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowParity([u8; 16]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnParity(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Parity { row: RowParity, column: ColumnParity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectedBit { pub byte: usize, pub bit: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Validation { Accepted, Corrected(CorrectedBit), Rejected }

pub fn parity(record: &Area2Record) -> Parity {
    let mut rows = [0; 16];
    let mut column = 0;
    for (index, bytes) in record.0[..PROTECTED_BYTES].chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        column ^= word;
        rows[index / 8] |= ((word.count_ones() & 1) as u8) << (index % 8);
    }
    Parity { row: RowParity(rows), column: ColumnParity(column) }
}
pub fn seal(record: &mut Area2Record) {
    let computed = parity(record);
    record.0[PROTECTED_BYTES..0x1fc].copy_from_slice(&computed.row.0);
    record.0[0x1fc..].copy_from_slice(&computed.column.0.to_le_bytes());
}
pub fn validate(record: &mut Area2Record) -> Validation {
    let computed = parity(record);
    let column = u32::from_le_bytes(record.0[0x1fc..].try_into().unwrap()) ^ computed.column.0;
    let mut row_syndrome = [0; 16];
    for (index, byte) in row_syndrome.iter_mut().enumerate() {
        *byte = record.0[PROTECTED_BYTES + index] ^ computed.row.0[index];
    }
    if column == 0 && row_syndrome.iter().all(|byte| *byte == 0) { return Validation::Accepted; }
    if column.count_ones() != 1 { return Validation::Rejected; }
    let mut row = None;
    for index in 0..ROWS {
        if row_syndrome[index / 8] & (1 << (index % 8)) != 0 {
            if row.replace(index).is_some() { return Validation::Rejected; }
        }
    }
    let Some(row) = row else { return Validation::Rejected; };
    let column = column.trailing_zeros() as usize;
    let corrected = CorrectedBit { byte: row * 4 + column / 8, bit: (column % 8) as u8 };
    record.0[corrected.byte] ^= 1 << corrected.bit;
    Validation::Corrected(corrected)
}

pub struct Area2Codec;
impl Callbacks for Area2Codec {
    fn check(&mut self, bytes: &mut [u8]) -> CheckVerdict {
        let Ok(bytes) = <&mut [u8; RECORD_BYTES]>::try_from(bytes) else { return CheckVerdict::Reject; };
        let record = unsafe { &mut *(bytes as *mut [u8; RECORD_BYTES] as *mut Area2Record) };
        match validate(record) { Validation::Rejected => CheckVerdict::Reject, _ => CheckVerdict::Accept }
    }
    fn seal(&mut self, bytes: &mut [u8]) {
        let bytes: &mut [u8; RECORD_BYTES] = bytes.try_into().unwrap();
        let record = unsafe { &mut *(bytes as *mut [u8; RECORD_BYTES] as *mut Area2Record) };
        seal(record);
    }
}
