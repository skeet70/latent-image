#![forbid(unsafe_code)]

//! X-M1 1.50 layout of the capture snapshot the intake reads.

use super::{half, word};
use crate::contrast::LensIdentity;
use crate::intake::{FrameSnapshot, Zone};

/// The bytes 0xfea7ac(1) copies into the intake's frame (0xff6f94).
pub const FRAME_SNAPSHOT_BYTES: usize = 0x5c4;

/// The snapshot as 0xf40944 reads it.
pub fn frame_snapshot(bytes: &[u8; FRAME_SNAPSHOT_BYTES]) -> FrameSnapshot {
    let pair = |offset: usize| [word(bytes, offset), word(bytes, offset + 4)];
    FrameSnapshot {
        status: word(bytes, 0x000),
        windows: [pair(0x004), pair(0x034)],
        size: pair(0x00c),
        extent: pair(0x0dc),
        timing: word(bytes, 0x0ec),
        step: word(bytes, 0x0f8),
        zone: Zone(word(bytes, 0x0f4)),
        levels: [half(bytes, 0x0fc), half(bytes, 0x0fe)],
        adjustment: word(bytes, 0x104),
        level_offset: half(bytes, 0x108) as i16,
        lens: LensIdentity(word(bytes, 0x110) as i32),
        selected: word(bytes, 0x56c),
        frame_mode: word(bytes, 0x570),
        pinned_setting: half(bytes, 0x578),
        pinned: word(bytes, 0x57c),
        pacing: pair(0x580),
        mark: half(bytes, 0x588),
        correction_source: half(bytes, 0x58a) as i16,
        correction_class: word(bytes, 0x58c),
        corrects_position: bytes[0x592],
    }
}
