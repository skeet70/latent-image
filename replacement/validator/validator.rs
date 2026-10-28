#![no_std]
//! Area-3 record validator replacement.

pub const RECORD_BYTES: usize = 52;
pub const PROTECTED_BYTES: usize = 44;
pub const ROWS: usize = PROTECTED_BYTES / 4;
pub const COLUMNS: u32 = 32;
pub const ROW_PARITY_BITS: u32 = ROWS as u32;
pub const ROW_PARITY_OFFSET: usize = PROTECTED_BYTES;
pub const COLUMN_PARITY_OFFSET: usize = PROTECTED_BYTES + 4;
pub const ROW_PARITY_MASK: u32 = (1 << ROW_PARITY_BITS) - 1;
pub const ACCEPT: u32 = 0;
pub const REJECT: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowParity(pub u16);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColumnParity(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SingleBit {
    pub row: u8,
    pub column: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Syndrome {
    pub row: RowParity,
    pub column: ColumnParity,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Accepted,
    Corrected(SingleBit),
    Rejected(Syndrome),
}

#[inline]
const fn word(record: &[u8; RECORD_BYTES], row: usize) -> u32 {
    let base = row * 4;
    u32::from_le_bytes([
        record[base],
        record[base + 1],
        record[base + 2],
        record[base + 3],
    ])
}

#[inline]
const fn stored_word(record: &[u8; RECORD_BYTES], offset: usize) -> u32 {
    u32::from_le_bytes([
        record[offset],
        record[offset + 1],
        record[offset + 2],
        record[offset + 3],
    ])
}

pub fn computed_parity(record: &[u8; RECORD_BYTES]) -> Syndrome {
    let mut row = 0u16;
    let mut column = 0u32;
    let mut index = 0;
    while index < ROWS {
        let data = word(record, index);
        column ^= data;
        row |= ((data.count_ones() & 1) as u16) << index;
        index += 1;
    }
    Syndrome {
        row: RowParity(row),
        column: ColumnParity(column),
    }
}

pub fn stored_parity(record: &[u8; RECORD_BYTES]) -> Syndrome {
    Syndrome {
        row: RowParity((stored_word(record, ROW_PARITY_OFFSET) & ROW_PARITY_MASK) as u16),
        column: ColumnParity(stored_word(record, COLUMN_PARITY_OFFSET)),
    }
}

fn flip(record: &mut [u8; RECORD_BYTES], site: SingleBit) {
    let byte = site.row as usize * 4 + site.column as usize / 8;
    record[byte] ^= 1 << (site.column % 8);
}

pub fn validate(record: &mut [u8; RECORD_BYTES]) -> Outcome {
    let computed = computed_parity(record);
    let stored = stored_parity(record);
    let syndrome = Syndrome {
        row: RowParity(computed.row.0 ^ stored.row.0),
        column: ColumnParity(computed.column.0 ^ stored.column.0),
    };
    if syndrome.row.0 == 0 && syndrome.column.0 == 0 {
        return Outcome::Accepted;
    }
    if syndrome.row.0.count_ones() == 1 && syndrome.column.0.count_ones() == 1 {
        let site = SingleBit {
            row: syndrome.row.0.trailing_zeros() as u8,
            column: syndrome.column.0.trailing_zeros() as u8,
        };
        flip(record, site);
        return Outcome::Corrected(site);
    }
    Outcome::Rejected(syndrome)
}

/// Stock ABI: r0 = record buffer, r1 = pointer to the protected-length word.
/// A length other than 0x2c is outside the contract.
#[no_mangle]
pub unsafe extern "C" fn area3_validate(record: *mut u8, protected_length: *const u32) -> u32 {
    if record.is_null() || protected_length.is_null() {
        return REJECT;
    }
    if *protected_length != PROTECTED_BYTES as u32 {
        return REJECT;
    }
    match validate(&mut *(record as *mut [u8; RECORD_BYTES])) {
        Outcome::Accepted | Outcome::Corrected(_) => ACCEPT,
        Outcome::Rejected(_) => REJECT,
    }
}
