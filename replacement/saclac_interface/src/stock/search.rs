#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the searches' lens-bridge, S1 drive and evaluation requests and reports.

use crate::search::{
    BridgePhase, BridgeReport, BridgeRequest, BridgeStatus, Evaluation, LensClass, ResultAddress, S1DriveRequest, ScanEvaluation,
    SearchRecord,
};

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

/// Completions 0x46 and 0x5a, 12 bytes: 0xf3c14c and 0xf25728 post the status word, the
/// position halfword at +4 and the class word at +8.
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

pub const EVALUATION_BYTES: usize = 112;

/// The result completions 0x89 and 0x9d point at, as 0xf44fec and 0xf267f8 read it.
pub fn evaluation(result: &[u8; EVALUATION_BYTES]) -> Evaluation {
    let word = |index: usize| u32::from_le_bytes([result[4 * index], result[4 * index + 1], result[4 * index + 2], result[4 * index + 3]]);
    let half = |offset: usize| u16::from_le_bytes([result[offset], result[offset + 1]]);
    let pairs = |first: usize| [[word(first), word(first + 1)], [word(first + 2), word(first + 3)], [word(first + 4), word(first + 5)]];
    Evaluation {
        pairs: pairs(0),
        levels: [word(10) as u16, word(11) as u16],
        scan_pairs: pairs(12),
        scan_levels: [word(22) as u16, word(23) as u16],
        reference: word(24),
        settled: word(25),
        flag: word(26),
        focus_values: [half(0x6c), half(0x6e)],
    }
}

/// Completions 0x89 and 0x9d, 8 bytes: 0xf3c104 and 0xf25758 post calc's status word, which
/// neither search reads, then the result's address.
pub fn evaluation_address(payload: &[u8]) -> Option<ResultAddress> {
    let address: [u8; 4] = payload.get(4..8)?.try_into().ok()?;
    Some(ResultAddress(u32::from_le_bytes(address)))
}

pub const SEARCH_RECORD_BYTES: usize = 120;

/// The statistics banks every record names: +0x38, +0x3c, +0x40, then the primary buffer at
/// +0x58 with its length at +0x64 (0xf40a8c..0xf40af8, 0xf27160..0xf271c8).
const STATISTICS_BANKS: [(usize, u32); 5] =
    [(0x38, 0x5ffe_be8c), (0x3c, 0x5ffe_c08c), (0x40, 0x5ffe_e518), (0x58, 0x5ffe_c1b4), (0x64, 0x1000)];

/// The 120 bytes 0xf4c400 copies after the callback word for calc 0x3c09, as 0xf40944 (live)
/// and 0xf26a60 with 0xf2711c (S1) fill them. Neither writes +0x6c or +0x76; the live record
/// never has +0x24 written.
pub fn search_record(record: &SearchRecord) -> [u8; SEARCH_RECORD_BYTES] {
    let mut bytes = [0; SEARCH_RECORD_BYTES];
    let mut put = |offset: usize, value: &[u8]| bytes[offset..offset + value.len()].copy_from_slice(value);
    put(0x00, &u32::from(record.first).to_le_bytes());
    put(0x04, &record.scale[0].to_le_bytes());
    put(0x06, &record.scale[1].to_le_bytes());
    put(0x08, &u32::from(record.settled).to_le_bytes());
    put(0x0c, &u32::from(record.level_matched).to_le_bytes());
    put(0x10, &record.selection.to_le_bytes());
    put(0x14, &record.mode.to_le_bytes());
    put(0x18, &record.ratio[0].to_le_bytes());
    put(0x1c, &record.ratio[1].to_le_bytes());
    put(0x20, &record.range[0].to_le_bytes());
    put(0x22, &record.range[1].to_le_bytes());
    put(0x24, &record.range_offset.to_le_bytes());
    put(0x28, &record.extent[0].to_le_bytes());
    put(0x2c, &record.extent[1].to_le_bytes());
    put(0x30, &record.window[0].to_le_bytes());
    put(0x34, &record.window[1].to_le_bytes());
    for (offset, value) in STATISTICS_BANKS {
        put(offset, &value.to_le_bytes());
    }
    put(0x44, &record.frame_mode.to_le_bytes());
    put(0x48, &u32::from(record.scan).to_le_bytes());
    put(0x4c, &record.profile_class.to_le_bytes());
    put(0x50, &record.resource.to_le_bytes());
    let scan: u32 = match record.scan_evaluation {
        ScanEvaluation::Plain => 0,
        ScanEvaluation::Scan => 1,
        ScanEvaluation::Scanned => 2,
    };
    put(0x54, &scan.to_le_bytes());
    put(0x5c, &record.size[0].to_le_bytes());
    put(0x60, &record.size[1].to_le_bytes());
    put(0x68, &record.timing.to_le_bytes());
    put(0x6e, &record.decade.to_le_bytes());
    put(0x70, &record.correction.to_le_bytes());
    put(0x74, &record.limit.to_le_bytes());
    bytes
}

pub const S1_DRIVE_REQUEST_BYTES: usize = 20;

/// The 20 bytes 0xfeaaec posts to imgdc 0x3f44 after the completion word (0xf25be0 builds
/// them). Bytes 2..4 are never stored and carry stack residue in stock.
pub fn s1_drive_request(request: &S1DriveRequest) -> [u8; S1_DRIVE_REQUEST_BYTES] {
    let mut bytes = [0; S1_DRIVE_REQUEST_BYTES];
    bytes[0..2].copy_from_slice(&request.travel.to_le_bytes());
    bytes[4..8].copy_from_slice(&request.rate.to_le_bytes());
    bytes[8..12].copy_from_slice(&u32::from(request.moves).to_le_bytes());
    bytes[12..16].copy_from_slice(&request.speed.to_le_bytes());
    bytes[16..20].copy_from_slice(&request.lens_mode.to_le_bytes());
    bytes
}
