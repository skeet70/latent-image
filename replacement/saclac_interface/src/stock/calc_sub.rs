#![forbid(unsafe_code)]

//! X-M1 1.50, decomp rev 55436fc85c0560dd252b0410af6f2427fe198cd3: calcsub's request bytes.

use crate::calc_sub::{AspectRatio, ClassificationRequest, Orientation};
use crate::CalcSubOperation;

/// 0xf49e58 posts to queue 46 (0xf49e78..0xf49e7c) with 0x723cb8's byte as the sender
/// (0xf49e70..0xf49e74). 0xf49dc0 creates queue 46 with block 56 (0xf49dd4..0xf49de4), which
/// carries at most 48 payload bytes.
pub const QUEUE: u8 = 46;
pub const BLOCK_BYTES: u16 = 56;

pub const CLASSIFICATION_BYTES: usize = 48;

pub const fn calc_sub_operation(operation: CalcSubOperation) -> u16 {
    match operation {
        CalcSubOperation::Request00 => 0x3e00,
    }
}

/// 0xf4ebd0..0xf4ebdc posts all twelve words as 0x3e00 through 0xf49e58.
pub fn classification(request: &ClassificationRequest) -> [u8; CLASSIFICATION_BYTES] {
    let mut bytes = [0; CLASSIFICATION_BYTES];
    let mut put = |offset: usize, value: u32| bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    let frame = &request.frame;
    put(0x00, frame.samples.0); // 0xf4eb0c
    put(0x04, frame.plane.0); // 0xf4eb10
    put(0x08, aspect_code(request.aspect)); // 0xf4eb5c
    put(0x0c, orientation_code(request.orientation)); // 0xf4eb84
    put(0x10, frame.levels.0); // 0xf4eb8c
    put(0x14, frame.columns.0); // 0xf4eb94
    put(0x18, frame.rows.0); // 0xf4eb9c
    put(0x1c, frame.average_levels); // 0xf4eba4
    put(0x20, frame.motion_regions); // 0xf4ebac
    put(0x24, frame.region_list.0); // 0xf4ebb4
    put(0x28, u32::from(request.restart)); // 0xf4ebc4
    put(0x2c, request.completion.0); // 0xf4ebcc
    bytes
}

/// 0xf4eb14..0xf4eb58.
const fn aspect_code(aspect: AspectRatio) -> u32 {
    match aspect {
        AspectRatio::Other => 0,
        AspectRatio::ThreeTwo => 1,
        AspectRatio::SixteenNine => 2,
        AspectRatio::Square => 3,
    }
}

/// 0xf4eb60..0xf4eb80.
const fn orientation_code(orientation: Orientation) -> u32 {
    match orientation {
        Orientation::Reading1 => 0,
        Orientation::Reading2 => 1,
        Orientation::Reading3 => 2,
        Orientation::Reading4 => 3,
    }
}
