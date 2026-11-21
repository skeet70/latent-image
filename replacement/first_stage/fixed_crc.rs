const POLYNOMIAL: u32 = 0xedb8_8320;

const fn entry(mut crc: u32) -> u32 {
    let mut bit = 0;
    while bit < 8 {
        crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { POLYNOMIAL };
        bit += 1;
    }
    crc
}

/// `TABLES[0]` is the byte table; `TABLES[k][i]` is the CRC of byte `i` followed by `k` zero bytes.
const fn tables() -> [[u32; 256]; 4] {
    let mut tables = [[0; 256]; 4];
    let mut index = 0;
    while index < 256 {
        tables[0][index] = entry(index as u32);
        index += 1;
    }
    let mut table = 1;
    while table < 4 {
        index = 0;
        while index < 256 {
            let previous = tables[table - 1][index];
            tables[table][index] = (previous >> 8) ^ tables[0][(previous & 0xff) as usize];
            index += 1;
        }
        table += 1;
    }
    tables
}

const TABLES: [[u32; 256]; 4] = tables();

#[inline(always)]
pub fn update(crc: u32, value: u8) -> u32 {
    (crc >> 8) ^ TABLES[0][((crc as u8) ^ value) as usize]
}

/// `update` over the four bytes of a little-endian word.
#[inline(always)]
pub fn update_word(crc: u32, word: u32) -> u32 {
    let mixed = crc ^ word;
    TABLES[3][(mixed & 0xff) as usize]
        ^ TABLES[2][((mixed >> 8) & 0xff) as usize]
        ^ TABLES[1][((mixed >> 16) & 0xff) as usize]
        ^ TABLES[0][(mixed >> 24) as usize]
}

/// `update` over `length` bytes at word-aligned `address`.
///
/// # Safety
/// `[address, address + length)` is readable.
pub unsafe fn update_range(mut crc: u32, address: usize, length: usize) -> u32 {
    let words = length / 4;
    let mut index = 0;
    while index < words {
        crc = update_word(crc, core::ptr::read_volatile((address as *const u32).add(index)));
        index += 1;
    }
    let mut offset = words * 4;
    while offset < length {
        crc = update(crc, core::ptr::read_volatile((address + offset) as *const u8));
        offset += 1;
    }
    crc
}
