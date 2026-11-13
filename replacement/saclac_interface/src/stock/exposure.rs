#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the exposure step's statistics request and of the results its completion
//! callbacks deliver. Stock callbacks store the results into shot memory before they post (0xf1eddc,
//! 0xf1ee10, 0xf1fc08); the owned callbacks carry them as the posted payload instead.

use crate::calc::SharedAddress;
use crate::exposure::{Correction, CorrectionRequest, Evaluation, EvaluationRequest, MeasuredEntry, ReleaseStatus, StatisticsRequest, CORRECTION_HALVES, EVALUATION_BYTES};
use crate::stock::StockCallbackWord;

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

pub const EVALUATION_REQUEST_BYTES: usize = 232;

/// 0xf5fd20 posts calc 0x3c00 with the callback, the 100-byte block at context +0xfe0 from byte 4
/// and the 44-byte block at +0x1044 from byte 0x68; stock leaves bytes 0x94.. unwritten
/// (stack). 0xf69224 sets the block's constants and 0xf6a0cc the row's fields.
pub fn evaluation_request(request: &EvaluationRequest, callback: StockCallbackWord) -> [u8; EVALUATION_REQUEST_BYTES] {
    let mut bytes = [0; EVALUATION_REQUEST_BYTES];
    let address = |at: Option<SharedAddress>| at.map_or(0, |at| at.0);
    {
        let mut word = |offset: usize, value: u32| bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        word(0, callback.0);
        word(4, request.row);
        word(4 + 0x10, address(request.row_buffer));
        for (index, rate) in request.rates.iter().enumerate() {
            word(4 + 0x1c + 4 * index, *rate);
        }
        word(4 + 0x2c, u32::from(request.after_reference));
        word(4 + 0x3c, request.direction);
        word(4 + 0x4c, request.column_count);
        word(4 + 0x50, address(request.column_regions));
        word(4 + 0x54, address(request.column_buffer));
        word(4 + 0x60, request.next_profile);
        for (index, curve) in request.level_curves.iter().enumerate() {
            word(0x68 + 4 * index, address(*curve));
        }
        word(0x68 + 0x10, address(request.level_classes));
        word(0x68 + 0x14, address(request.frame));
        for (index, curve) in request.subject_curves.iter().enumerate() {
            word(0x68 + 0x18 + 4 * index, address(*curve));
        }
        word(0x68 + 0x28, address(request.selection_flags));
    }
    let mut half = |offset: usize, value: u16| bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    for (pair, offset) in request.measured.iter().zip([0x04, 0x0c, 0x18]) {
        half(4 + offset, pair[0]);
        half(4 + offset + 2, pair[1]);
    }
    half(4 + 0x08, 16);
    half(4 + 0x0a, 16);
    half(4 + 0x30, request.value);
    half(4 + 0x32, request.lower_value);
    half(4 + 0x34, request.tolerance_above as u16);
    half(4 + 0x36, request.tolerance_below as u16);
    half(4 + 0x38, 250);
    half(4 + 0x3a, request.floor as u16);
    for (index, statistic) in request.statistics.iter().enumerate() {
        half(4 + 0x44 + 2 * index, *statistic);
    }
    for (index, dimension) in request.dimensions.iter().enumerate() {
        half(4 + 0x58 + 2 * index, *dimension);
    }
    bytes
}

pub const CORRECTION_REQUEST_BYTES: usize = 232;
const CORRECTION_BLOCK: usize = 4;
const CORRECTION_TAIL: usize = CORRECTION_BLOCK + 168;

/// 0xf5ff88 posts calc 0x3c01 with the callback, the 168-byte block at context +0x1070 from byte 4
/// and the 12-byte block at +0x1118 from byte 0xac; stock leaves bytes 0xb8.. unwritten (stack).
/// 0xf68db8 fills both blocks; it never writes block bytes 0x0e, 0x45..0x48, 0x5e, 0xa6 and 0xa7,
/// which the preparation 0xf613bc clears.
pub fn correction_request(request: &CorrectionRequest, callback: StockCallbackWord) -> [u8; CORRECTION_REQUEST_BYTES] {
    let mut bytes = [0; CORRECTION_REQUEST_BYTES];
    let address = |at: Option<SharedAddress>| at.map_or(0, |at| at.0);
    bytes[..4].copy_from_slice(&callback.0.to_le_bytes());
    let block = &mut bytes[CORRECTION_BLOCK..CORRECTION_TAIL];
    let mut word = |offset: usize, value: u32| block[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    word(0x00, request.style);
    word(0x04, request.measurement_selection);
    word(0x08, request.selection_kind);
    word(0x10, request.record_kind);
    word(0x14, request.lens_reference);
    word(0x18, request.record_44);
    word(0x1c, request.selection_auxiliary);
    word(0x20, request.focus_class);
    word(0x24, request.record_32);
    word(0x28, request.record_3e);
    word(0x2c, request.variant);
    word(0x30, request.image_word);
    word(0x34, request.rate);
    for (index, list_word) in request.list_words.iter().enumerate() {
        word(0x38 + 4 * index, *list_word);
    }
    for (index, curve) in request.level_curves.iter().enumerate() {
        word(0x48 + 4 * index, address(*curve));
    }
    word(0x60, request.curve_table);
    word(0x68, request.ratio[0]);
    word(0x6c, request.ratio[1]);
    word(0x70, u32::from(request.second_step));
    word(0x74, u32::from(request.treated));
    word(0x78, u32::from(request.first_treatment));
    word(0x7c, address(request.subjects));
    word(0x80, address(request.summary));
    word(0x84, request.list_word);
    for (index, curve) in request.subject_curves.iter().enumerate() {
        word(0x88 + 4 * index, address(*curve));
    }
    word(0x98, request.header_61);
    word(0x9c, request.header_47);
    word(0xa0, u32::from(request.unmeasured));
    let mut half = |offset: usize, value: u16| block[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    half(0x0c, request.selection_flags);
    half(0x58, request.start);
    half(0x5a, request.start_above_target);
    half(0x5c, request.reference_offset);
    half(0x64, request.rate_setting);
    half(0x66, request.reference);
    half(0xa4, request.record_20 as u16);
    block[0x44] = request.list_byte;
    let tail = &mut bytes[CORRECTION_TAIL..CORRECTION_TAIL + 12];
    for (index, at) in [request.buffers[0], request.buffers[1], request.frame].into_iter().enumerate() {
        tail[4 * index..4 * index + 4].copy_from_slice(&address(at).to_le_bytes());
    }
    bytes
}
