#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the record fields the search reads and writes.

use crate::contrast::{FocusStatus, RetainedFocus, StageRecord};
use xm1_saclac_controllers::search::LensPosition;

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn half(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// Shared +0x84, 32 bytes; both writers (0xf3c17c, 0xf40300) store 2 at +0x14.
pub fn focus_status(status: FocusStatus) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[0..4].copy_from_slice(&status.active.to_le_bytes());
    bytes[4..8].copy_from_slice(&status.mode.to_le_bytes());
    bytes[8..10].copy_from_slice(&status.position.0.to_le_bytes());
    bytes[12..16].copy_from_slice(&status.snapshot[0].to_le_bytes());
    bytes[16..20].copy_from_slice(&status.snapshot[1].to_le_bytes());
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&status.flag.to_le_bytes());
    bytes[28..32].copy_from_slice(&status.stage.to_le_bytes());
    bytes
}

/// Shared +0x74c, 24 bytes (0xf3c394..0xf3c3ac).
pub fn stage_record(stage: StageRecord) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[0..4].copy_from_slice(&stage.stage.to_le_bytes());
    bytes[4] = stage.first_pass;
    bytes[8..12].copy_from_slice(&stage.words[0].to_le_bytes());
    bytes[12..14].copy_from_slice(&stage.halves[0].to_le_bytes());
    bytes[16..20].copy_from_slice(&stage.words[1].to_le_bytes());
    bytes[20..22].copy_from_slice(&stage.halves[1].to_le_bytes());
    bytes[22] = stage.last;
    bytes
}

/// Shared +0x120, 28 bytes as 0xf3e6f0 reads them.
pub fn retained_focus(bytes: &[u8; 28]) -> RetainedFocus {
    RetainedFocus {
        valid: word(bytes, 0),
        spread: word(bytes, 4),
        position: LensPosition(half(bytes, 8) as i16),
        profile: word(bytes, 16),
    }
}
