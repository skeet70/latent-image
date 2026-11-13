#![forbid(unsafe_code)]

//! Adjustment-area reads wider than a byte, composed little-endian from the byte port as
//! 0x69fec8, 0x69fef0, 0x69ff24, 0x69ff68 and 0x69ff7c compose them.

pub use xm1_saclac_shared::settings::{AdjustmentKey, Adjustments};

pub const fn key(at: u32) -> AdjustmentKey { AdjustmentKey(at) }

pub fn byte(area: &impl Adjustments, at: u32) -> u8 { area.byte(key(at)) }

pub fn signed_byte(area: &impl Adjustments, at: u32) -> i8 { byte(area, at) as i8 }

pub fn half(area: &impl Adjustments, at: u32) -> u16 { u16::from_le_bytes([byte(area, at), byte(area, at.wrapping_add(1))]) }

pub fn signed_half(area: &impl Adjustments, at: u32) -> i16 { half(area, at) as i16 }

pub fn triple(area: &impl Adjustments, at: u32) -> u32 {
    u32::from_le_bytes([byte(area, at), byte(area, at.wrapping_add(1)), byte(area, at.wrapping_add(2)), 0])
}

pub fn word(area: &impl Adjustments, at: u32) -> u32 {
    u32::from_le_bytes([0, 1, 2, 3].map(|offset| byte(area, at.wrapping_add(offset))))
}
