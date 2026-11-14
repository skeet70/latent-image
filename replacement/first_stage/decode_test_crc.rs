//! CRC-32 (IEEE 802.3, reflected). `decode_test_crc_tests.rs` asserts the published check
//! values for "123456789" and "The quick brown fox jumps over the lazy dog".

const POLYNOMIAL: u32 = 0xedb8_8320;

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 { POLYNOMIAL ^ (value >> 1) } else { value >> 1 };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

static TABLE: [u32; 256] = build_table();

#[inline]
pub fn update(crc: u32, byte: u8) -> u32 {
    TABLE[((crc ^ byte as u32) & 0xff) as usize] ^ (crc >> 8)
}

#[inline]
pub fn start() -> u32 {
    0xffff_ffff
}

#[inline]
pub fn finish(crc: u32) -> u32 {
    crc ^ 0xffff_ffff
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = start();
    for &byte in bytes {
        crc = update(crc, byte);
    }
    finish(crc)
}
