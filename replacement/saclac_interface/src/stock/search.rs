#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the lens-bridge request and report.

use crate::search::{BridgePhase, BridgeReport, BridgeRequest, BridgeStatus, LensClass};

pub const BRIDGE_REQUEST_BYTES: usize = 40;

/// The 40 bytes 0xfea634 posts to imgdc 0x3f2e after the completion word (0xf3eab4 stores
/// zero at +0x1c).
pub fn bridge_request(request: &BridgeRequest) -> [u8; BRIDGE_REQUEST_BYTES] {
    let phase: u32 = match request.phase {
        BridgePhase::Direction => 0,
        BridgePhase::PeakSearch => 1,
        BridgePhase::Peak => 2,
        BridgePhase::Return => 3,
        BridgePhase::Focus => 4,
        BridgePhase::FocusDrive => 5,
        BridgePhase::Approach => 6,
        BridgePhase::Monitor => 7,
        BridgePhase::Other => 8,
    };
    let mut bytes = [0; BRIDGE_REQUEST_BYTES];
    let mut put = |offset: usize, value: u32| bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    put(0x00, phase);
    put(0x04, u32::from(request.scanning));
    put(0x10, request.rate);
    put(0x14, u32::from(request.moves));
    put(0x18, request.speed);
    put(0x20, request.lens_mode);
    bytes[0x08..0x0a].copy_from_slice(&request.window[0].to_le_bytes());
    bytes[0x0a..0x0c].copy_from_slice(&request.window[1].to_le_bytes());
    bytes[0x0c..0x0e].copy_from_slice(&request.travel.to_le_bytes());
    bytes
}

/// Completion 0x46's 12 bytes: 0xf3c14c posts the status word, the position halfword at +4
/// and the class word at +8.
pub fn bridge_report(payload: &[u8]) -> Option<BridgeReport> {
    let payload: &[u8; 12] = payload.get(..12)?.try_into().ok()?;
    let word = |offset: usize| u32::from_le_bytes([payload[offset], payload[offset + 1], payload[offset + 2], payload[offset + 3]]);
    let status = match word(0) as i32 {
        0 => BridgeStatus::Moved,
        21 => BridgeStatus::Deferred,
        -94 => BridgeStatus::Fault,
        other => BridgeStatus::Other(other),
    };
    Some(BridgeReport { status, position: i16::from_le_bytes([payload[4], payload[5]]), class: LensClass(word(8)) })
}
