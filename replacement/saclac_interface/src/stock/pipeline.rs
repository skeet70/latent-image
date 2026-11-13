#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the region block the DCF controller hands the stage control.

use crate::pipeline::FormatPayload;

pub const FORMAT_BLOCK_BYTES: usize = 0x34;

/// The block 0xee2d28 builds on its stack and passes to 0xda027c with flag 4: 0x685b94 reads the
/// three halfwords at +0x24, 0x685dd0 the scales and alignment at +0x2a, 0x65f3d8 the word at
/// +0x30. Bytes 0..0x24, which stock leaves uninitialised, are zero.
pub fn format_block(payload: &FormatPayload) -> [u8; FORMAT_BLOCK_BYTES] {
    let mut bytes = [0; FORMAT_BLOCK_BYTES];
    bytes[0x2a..0x2c].copy_from_slice(&payload.first_scale.to_le_bytes());
    bytes[0x2c..0x2e].copy_from_slice(&payload.second_scale.to_le_bytes());
    bytes[0x2e..0x30].copy_from_slice(&payload.alignment);
    bytes[0x30..0x34].copy_from_slice(&u32::from(payload.remainder_geometry).to_le_bytes());
    bytes
}
