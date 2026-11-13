#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the results the half-press pipeline's completion callbacks deliver. Stock
//! callbacks store them into shot memory before they post (0xf205c0, 0xf20594, 0xf2056c,
//! 0xf20380, 0xf20418, 0xf203b4); the owned callbacks carry them as the posted payload instead.

use crate::pipeline::{
    GradationCalculation, RawCalculation, RawCheck, ToneCalculation, ToneProgress, ToneStatus, GRADATION_CALCULATION_BYTES, RAW_CALCULATION_BYTES,
    TONE_CALCULATION_BYTES,
};

fn word(payload: &[u8], offset: usize) -> Option<u32> { Some(u32::from_le_bytes(payload.get(offset..offset + 4)?.try_into().ok()?)) }

/// 0xf205c0 stores its first argument.
pub fn raw_check_payload(check: RawCheck) -> [u8; 4] { check.0.to_le_bytes() }

pub fn raw_check(payload: &[u8]) -> Option<RawCheck> { word(payload, 0).map(RawCheck) }

/// 0xf20594 stores its first argument as the status and the halfword of its second as the level.
pub fn tone_progress_payload(progress: ToneProgress) -> [u8; 6] {
    let mut bytes = [0; 6];
    bytes[..4].copy_from_slice(&progress.status.to_le_bytes());
    bytes[4..].copy_from_slice(&progress.level.to_le_bytes());
    bytes
}

pub fn tone_progress(payload: &[u8]) -> Option<ToneProgress> {
    Some(ToneProgress { status: word(payload, 0)?, level: u16::from_le_bytes(payload.get(4..6)?.try_into().ok()?) })
}

/// 0xf2056c stores its first argument.
pub fn tone_status_payload(status: ToneStatus) -> [u8; 4] { status.0.to_le_bytes() }

pub fn tone_status(payload: &[u8]) -> Option<ToneStatus> { word(payload, 0).map(ToneStatus) }

/// 0xf20380 copies 20 bytes from its argument.
pub fn raw_calculation(payload: &[u8]) -> Option<RawCalculation> { Some(RawCalculation(payload.get(..RAW_CALCULATION_BYTES)?.try_into().ok()?)) }

/// 0xf20418 copies 108 bytes from its argument.
pub fn tone_calculation(payload: &[u8]) -> Option<ToneCalculation> { Some(ToneCalculation(payload.get(..TONE_CALCULATION_BYTES)?.try_into().ok()?)) }

/// 0xf203b4 copies 96 bytes from its argument.
pub fn gradation_calculation(payload: &[u8]) -> Option<GradationCalculation> {
    Some(GradationCalculation(payload.get(..GRADATION_CALCULATION_BYTES)?.try_into().ok()?))
}
