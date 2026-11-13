//! The gain request a stock caller hands 0xf4d5bc, as 0xfe170c reads it, and the result bytes
//! it writes back.

use crate::gains::{GainClass, GainRequest, SelectedGains};

pub const REQUEST_BYTES: usize = 64;
pub const RESULT_BYTES: usize = 12;

fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }

fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }

fn triple(bytes: &[u8], at: usize) -> [u16; 3] { core::array::from_fn(|channel| half(bytes, at + 2 * channel)) }

/// The request's fields at the offsets 0xfe170c reads; +0x10, +0x14 and +0x24 are never read.
pub fn request(bytes: &[u8; REQUEST_BYTES]) -> GainRequest {
    GainRequest {
        mode: word(bytes, 0x00),
        variant: word(bytes, 0x04),
        custom_source: word(bytes, 0x08),
        bank: word(bytes, 0x0c),
        fixed: triple(bytes, 0x18),
        preset: word(bytes, 0x20),
        temperature: half(bytes, 0x28),
        column: half(bytes, 0x2a) as i16,
        level: half(bytes, 0x2c) as i16,
        custom: triple(bytes, 0x2e),
        variant_set: word(bytes, 0x34),
        forced: word(bytes, 0x38) != 0,
        variant_subset: word(bytes, 0x3c),
    }
}

/// Writes the gains at +0 and the class at +8, leaving +6 and +7 as they were.
pub fn write_result(result: &mut [u8; RESULT_BYTES], selected: &SelectedGains) {
    for (channel, gain) in selected.gains.iter().enumerate() {
        result[2 * channel..2 * channel + 2].copy_from_slice(&gain.to_le_bytes());
    }
    let GainClass(class) = selected.class;
    result[8..12].copy_from_slice(&class.to_le_bytes());
}
