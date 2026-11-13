#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the exposure step's statistics request and of the results its completion
//! callbacks deliver. Stock callbacks store the results into shot memory before they post (0xf1eddc,
//! 0xf1ee10, 0xf1fc08); the owned callbacks carry them as the posted payload instead.

use crate::exposure::{Correction, Evaluation, MeasuredEntry, ReleaseStatus, StatisticsRequest, CORRECTION_HALVES, EVALUATION_BYTES};

pub const STATISTICS_REQUEST_BYTES: usize = 220;

/// The block 0xf1f2b0 builds at 0x16b51a8; 0xfea824 stages it at 0x16f0c90 and posts imgdc 0x3f1a
/// with the staged address. `subjects` is the address of the subject list it names (+0xd8).
/// Bytes 0x2a..0x2c and 0xcd..0xd0 are never written by stock and stay zero.
pub fn statistics_request(request: &StatisticsRequest, subjects: u32) -> [u8; STATISTICS_REQUEST_BYTES] {
    let mut bytes = [0; STATISTICS_REQUEST_BYTES];
    let mut put = |offset: usize, value: u32| bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    put(0x00, request.lens_reference);
    put(0x04, request.calculation_mode);
    put(0x08, request.subject_query[0]);
    put(0x0c, request.subject_query[1]);
    put(0x10, request.area_layout);
    put(0x14, request.context_mode);
    put(0x18, request.correction_mode);
    put(0x1c, request.statistics_option);
    put(0x20, u32::from(request.single_area));
    put(0x24, 100 | 100 << 16);
    put(0xd0, u32::from(request.scaled));
    put(0xd4, 1);
    put(0xd8, subjects);
    bytes[0x28..0x2a].copy_from_slice(&16u16.to_le_bytes());
    bytes[0x2c..0xcc].copy_from_slice(&request.areas);
    bytes[0xcc] = request.area_count;
    bytes
}

/// The statistics callback's two outcomes: status 3 posts 0x4d with the entry in its second
/// argument (0xf1ede4..0xf1edf4), any other status posts 0x4c with that status (0xf1edf8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsResult { Measured(MeasuredEntry), Released(ReleaseStatus) }

pub const fn statistics_result(status: u32, entry: u32) -> StatisticsResult {
    if status == 3 { StatisticsResult::Measured(MeasuredEntry(entry as u8)) } else { StatisticsResult::Released(ReleaseStatus(status)) }
}

pub fn measured_payload(entry: MeasuredEntry) -> [u8; 1] { [entry.0] }

pub fn measured(payload: &[u8]) -> Option<MeasuredEntry> { payload.first().map(|entry| MeasuredEntry(*entry)) }

pub fn release_payload(status: ReleaseStatus) -> [u8; 4] { status.0.to_le_bytes() }

pub fn release(payload: &[u8]) -> Option<ReleaseStatus> {
    Some(ReleaseStatus(u32::from_le_bytes(payload.get(..4)?.try_into().ok()?)))
}

/// The 16 bytes 0xf1ee10 copies to 0x16b505c; 0xf5fd9c reads the entry at byte 0 and the levels
/// at bytes 8..16.
pub fn evaluation(payload: &[u8]) -> Option<Evaluation> {
    let bytes: [u8; EVALUATION_BYTES] = payload.get(..EVALUATION_BYTES)?.try_into().ok()?;
    let level = |index: usize| u16::from_le_bytes([bytes[8 + 2 * index], bytes[9 + 2 * index]]);
    Some(Evaluation { entry: bytes[0], levels: [level(0), level(1), level(2), level(3)], bytes })
}

pub const CORRECTION_BYTES: usize = 2 * CORRECTION_HALVES;

/// 0xf1fc08 copies 156 bytes from its argument to 0x16b506c, or the previous correction when the
/// argument is null; an empty payload stands for the null argument.
pub fn correction(payload: &[u8]) -> Option<Correction> {
    let bytes = payload.get(..CORRECTION_BYTES)?;
    let mut halves = [0; CORRECTION_HALVES];
    for (half, pair) in halves.iter_mut().zip(bytes.chunks_exact(2)) { *half = u16::from_le_bytes([pair[0], pair[1]]); }
    Some(Correction(halves))
}

pub fn correction_payload(correction: &Correction) -> [u8; CORRECTION_BYTES] {
    let mut bytes = [0; CORRECTION_BYTES];
    for (pair, half) in bytes.chunks_exact_mut(2).zip(correction.0) { pair.copy_from_slice(&half.to_le_bytes()); }
    bytes
}
