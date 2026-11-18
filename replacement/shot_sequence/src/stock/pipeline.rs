#![forbid(unsafe_code)]

//! X-M1 1.50 boundary layouts of the half-press pipeline (machine 28, 0xf1fda8..0xf226cc, and
//! machine 20's bind 0xf201f8). The pipeline context 0x16b5428, the scan plan 0x16b52a4 and the
//! frame point 0x16b5288 have no reader outside the pipeline and the finish, and are
//! `PipelineWork` fields. The motion library's blocks the plan names (0x16d7390 and the blocks
//! after it) are read and written through `PlanMemory`.

use crate::boundary::{Frame, LensPoint, LensReference};
use crate::finish_work::{AreaPositions, CorrectionRecord};
use crate::pipeline_work::{
    AreaBlock, CaptureBlock, FocusOutcome, LensBlock, OutcomeHistory, PipelineSetting, PipelineTrace, Plan, PlanBlock, PlanInput, SampleSnapshot,
    ScanBlock, StatisticsRecordValues, SubjectBlock, WorkArea, WorkStore,
};
use crate::ports::PipelinePhase;
use xm1_saclac_interface::pipeline::{ScanRequest, ScanWindow, StatisticsCapture, StatisticsSampling, StatisticsSetup, StatisticsWeights, ToneCalculation};

pub const SUBJECT_BYTES: usize = 0x30;
pub const CAPTURE_BYTES: usize = 0x144;
pub const LENS_BYTES: usize = 0x4e0;
pub const SCAN_BYTES: usize = 0x184;
pub const SNAPSHOT_BYTES: usize = 0xa4;
pub const HISTORY_BYTES: usize = 0x4c;
pub const AREA_BLOCK_BYTES: usize = 0x40;
pub const PLAN_BYTES: usize = 100;
pub const PLAN_INPUT_BYTES: usize = 28;
pub const CORRECTION_BYTES: usize = 28;

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) }
fn put(bytes: &mut [u8], offset: usize, value: u32) { bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
fn put_half(bytes: &mut [u8], offset: usize, value: u16) { bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
fn bytes<const N: usize>(source: &[u8], offset: usize) -> [u8; N] {
    let mut copy = [0; N];
    copy.copy_from_slice(&source[offset..offset + N]);
    copy
}

/// The pipeline plan 0xf4c730 (motion library 0xfa364c) and the shot context 0x169e580 its
/// input names.
pub const PLAN: u32 = 0xf4c730;
pub const SHOT_CONTEXT: u32 = 0x169_e580;

/// The 28-byte input 0xf20628 builds at its frame (0xf206b8..0xf206f4).
pub fn plan_input(input: &PlanInput) -> [u8; PLAN_INPUT_BYTES] {
    let mut bytes = [0; PLAN_INPUT_BYTES];
    put(&mut bytes, 0x00, xm1_saclac_interface::stock::record_handle_word(input.record));
    put(&mut bytes, 0x04, input.lens_reference);
    bytes[0x08] = u8::from(input.load_test_window);
    put(&mut bytes, 0x0c, input.area_count);
    put(&mut bytes, 0x10, input.option);
    put(&mut bytes, 0x14, SHOT_CONTEXT);
    put(&mut bytes, 0x18, input.frame_slot);
    bytes
}

/// The plan's 100-byte answer as 0xf20714..0xf207d0 copies it into the context; words +0x28 and
/// +0x50 are not kept.
pub fn plan(bytes: &[u8; PLAN_BYTES]) -> Plan {
    let block = |offset| PlanBlock(word(bytes, offset));
    Plan {
        frame_flags: crate::pipeline_work::FrameFlags(bytes[0]),
        subject: block(0x04),
        capture: block(0x08),
        lens: block(0x0c),
        scan: block(0x10),
        sample_targets: core::array::from_fn(|index| block(0x14 + 4 * index)),
        image_buffer: word(bytes, 0x2c),
        adjustment: core::array::from_fn(|index| block(0x30 + 4 * index)),
        raw_selection: block(0x4c),
        raw_subject: block(0x54),
        raw_first: block(0x58),
        raw_second: block(0x5c),
        raw_result: block(0x60),
    }
}

/// 0xf208d8 passes the plan's seven adjustment words at +8 of a record whose first eight bytes
/// it never writes.
pub fn adjustment_record(adjustment: &[PlanBlock; 7]) -> [u8; 36] {
    let mut bytes = [0; 36];
    for (index, block) in adjustment.iter().enumerate() { put(&mut bytes, 8 + 4 * index, block.0); }
    bytes
}

/// The work area 0xf20628 keeps at context +0x110: frame +0x116cc with the alias bit cleared
/// (0x740e0c).
pub fn work_area(frame: Frame) -> WorkArea { WorkArea(frame.0.wrapping_add(0x116cc) & !ALIAS) }

/// 0xf2131c flushes the work area (0x740e3c, 0x51c50 bytes) and keeps its aliased view (0x740e24).
pub fn released(area: WorkArea) -> WorkArea { WorkArea(area.0 | ALIAS) }

const ALIAS: u32 = 0x4000_0000;
pub const WORK_BYTES: u32 = 0x5_1c50;

/// Byte stores of one work-area store, in stock's order, as (offset, bytes).
pub fn work_stores(store: WorkStore, mut emit: impl FnMut(u32, &[u8])) {
    match store {
        WorkStore::Phase(phase) => emit(0x5_1c48, &(match phase { PipelinePhase::First => 1u32, PipelinePhase::Second => 2, PipelinePhase::Third => 3 }).to_le_bytes()),
        WorkStore::AdjustmentGate(gated) => emit(0x4_4bc8, &u32::from(gated).to_le_bytes()),
        WorkStore::ToneMark(level) => {
            emit(0x4_5da4, &1u32.to_le_bytes());
            emit(0x4_5da8, &i32::from(level).to_le_bytes());
        }
        WorkStore::Weights(weights) => {
            for (offset, weight) in (0x4_ffe0..).zip(weights) { emit(offset, &[weight]); }
            emit(0x4_ffee, &[0xfd]);
        }
        WorkStore::ChannelWindow { last, count } => emit(0x4_4bf4, &(u32::from(last) | u32::from(count) << 16).to_le_bytes()),
        WorkStore::ChannelMarks { channel, marks } => {
            for (row, mark) in (0..).zip(marks) { emit(0x3_88c4 + 0x100 * row + u32::from(channel), &[mark]); }
        }
    }
}

/// Work word +0x45da4 the tone mark tests (0xf204a4).
pub const WORK_MARK: u32 = 0x4_5da4;

/// Frame words 0xf201f8 writes: the markers at +0 and +0x110, the record's resource and bank at
/// +0x5c and +0x60, the lens position at +0x7c and reference at +0x88; +0x116cc is cleared.
pub const FRAME_MARKER: (u32, u32) = (0, 0x5354_434d);
pub const FRAME_CALIBRATION_MARKER: (u32, u32) = (0x110, 0x4c41_434d);
pub const FRAME_RESOURCE: u32 = 0x5c;
pub const FRAME_BANK: u32 = 0x60;
pub const FRAME_LENS_POSITION: u32 = 0x7c;
pub const FRAME_LENS_REFERENCE: u32 = 0x88;
pub const FRAME_WORK: u32 = 0x1_16cc;

/// The bound frame word 0xf20168 reads for 0xefc9ec, and the restart flag 0xf221b0 clears for
/// 0xf15528.
pub const BOUND_FRAME: u32 = 0x16b_5298;
pub const RESTART_FLAG: u32 = 0x16b_52a0;

/// The capture-sampling entries of the image task: idle 0xfeaa44, sample 0xfeaa24 (with the
/// context's sample block at +0x5c), measurements 0xfeaa34 and snapshot 0xfeaa54.
pub const CAPTURE_IDLE: u32 = 0xfeaa44;
pub const TAKE_SAMPLE: u32 = 0xfeaa24;
pub const CHANNEL_SAMPLES: u32 = 0xfeaa34;
pub const SAMPLE_SNAPSHOT: u32 = 0xfeaa54;

/// The lens block's pointer at +0x55c whose word +0x14 the statistics weights go to.
pub const STATISTICS_TARGET: (u32, u32) = (0x55c, 0x14);
/// The tone-final attribute: byte +5 of the 7-byte entry of the motion library's phase table.
pub const PHASE_TABLE: (u32, u32, u32) = (0xfa_5e04, 7, 5);

pub const fn trace_word(trace: PipelineTrace) -> u32 {
    match trace {
        PipelineTrace::Prepare => 0x0202_0000,
        PipelineTrace::RawRequested => 0x0203_0000,
        PipelineTrace::FrameReleased => 0x1202_0000,
        PipelineTrace::RawConcluded => 0x1203_0000,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingReader { Byte, Halfword, SignedByte }

/// Configuration entries: bytes through 0x69f8fc, the halfword through 0x69fec8 and the signed
/// byte through 0x69fffc.
pub const fn setting_entry(setting: PipelineSetting) -> (SettingReader, u32) {
    use PipelineSetting as S;
    use SettingReader::*;
    match setting {
        S::ChannelStatistics => (Byte, 0x9db2),
        S::WeightDivisor => (Byte, 0x9db3),
        S::Weight(index) => (Byte, 0x9db4 + index as u32),
        S::ToneOffset => (Byte, 0x9425),
        S::ExposureScale => (Byte, 0x96c4),
        S::ChannelThreshold => (Byte, 0x92ff),
        S::FirstToneFinal => (Byte, 0x92cb),
        S::SecondToneFinal => (Byte, 0x94cf),
        S::ChannelsForDecision => (Byte, 0x92b8),
        S::GatedToneFinal => (Byte, 0x92cc),
        S::ToneMask => (Byte, 0x9470),
        S::SelectionOverride => (Byte, 0x96c3),
        S::SubjectKindLimit => (Byte, 0x930c),
        S::Quadrant(index) => (Byte, 0x961c + index as u32),
        S::SelectionTable(index) => (Byte, 0x9671 + index),
        S::DrivingDistance => (Byte, 0x966f),
        S::BoundedSelection => (Byte, 0x944e),
        S::LoadTestTolerance => (Byte, 0x9614),
        S::CorrectionTolerance => (Halfword, 0x3e_0243),
        S::CorrectionBase => (SignedByte, 0x94e1),
        S::FallbackFirst => (Byte, 0x9278),
        S::FallbackSecond => (Byte, 0x3e_2711),
        S::Adjustments => (Byte, 0xa8),
    }
}

/// Load-test registers (0x742088 reads, 0x742098 writes): the published position, the test's
/// target and spread, and the window result.
pub const POSITION_HIGH: u8 = 0x88;
pub const POSITION_LOW: u8 = 0x89;
pub const TARGET_HIGH: u8 = 0x84;
pub const SPREAD_HIGH: u8 = 0x86;
pub const WINDOW_RESULT: u8 = 0x8a;

impl SubjectBlock {
    pub fn kind(&self) -> u8 { self.0[0] }
    pub fn offset(&self) -> i16 { half(&self.0, 0x2) as i16 }
    pub fn horizontal(&self) -> u16 { half(&self.0, 0xa) }
    pub fn vertical(&self) -> u16 { half(&self.0, 0xc) }
    pub fn state(&self) -> u32 { word(&self.0, 0x10) }
    pub fn profile(&self) -> u32 { word(&self.0, 0x14) }
    pub fn scale(&self) -> u32 { word(&self.0, 0x1c) }
    pub fn extent(&self) -> u32 { word(&self.0, 0x28) }
    pub fn mode(&self) -> u8 { self.0[0x2c] }
}

impl CaptureBlock {
    pub fn mode(&self) -> u32 { word(&self.0, 0x4) }
    pub fn phase(&self) -> u32 { word(&self.0, 0x8) }
    pub fn weighting(&self) -> u32 { word(&self.0, 0x18) }
    pub fn reference(&self) -> u32 { word(&self.0, 0x20) }
    pub fn areas(&self) -> [u32; 3] { [word(&self.0, 0x4c), word(&self.0, 0x50), word(&self.0, 0x54)] }
    pub fn area(&self) -> u32 { word(&self.0, 0x48) }
    pub fn restrained(&self) -> u32 { word(&self.0, 0xe0) }
    pub fn tolerance_kind(&self) -> u32 { word(&self.0, 0xf0) }
    pub fn geometry(&self) -> u32 { word(&self.0, 0xf4) }
}

impl LensBlock {
    pub fn kind(&self) -> u32 { word(&self.0, 0x0) }
    pub fn sampling(&self) -> u32 { word(&self.0, 0x58) }
    pub fn mode(&self) -> u32 { word(&self.0, 0x4a8) }
}

impl ScanBlock {
    pub fn start(&self) -> u32 { word(&self.0, 0x4) }
    pub fn phase(&self) -> u32 { word(&self.0, 0x8) }
    pub fn scan_mode(&self) -> u32 { word(&self.0, 0xc) }
    pub fn position(&self) -> i16 { half(&self.0, 0x12) as i16 }
    pub fn position_divisor(&self) -> u16 { half(&self.0, 0x1c) }
    pub fn selection_word(&self, selection: u32) -> u32 { word(&self.0, 0x24 + 4 * selection as usize) }
    pub fn gradation_threshold(&self) -> u32 { word(&self.0, 0x2c) }
    pub fn sampling(&self) -> u32 { word(&self.0, 0x30) }
    pub fn index_base(&self) -> u16 { half(&self.0, 0x90) }
    pub fn decision(&self) -> u32 { word(&self.0, 0x94) }
    pub fn tone_level(&self) -> i16 { half(&self.0, 0x98) as i16 }
    pub fn tone(&self) -> ToneCalculation { ToneCalculation(bytes(&self.0, 0x9c)) }
    pub fn tone_word(&self, word_offset: ToneWord) -> u32 { word(&self.0, 0x9c + word_offset as usize) }
    pub fn class_word(&self, index: u32) -> u32 { word(&self.0, 0xd8 + 4 * index as usize) }
    pub fn focus_position(&self) -> i16 { half(&self.0, 0x10c) as i16 }
    pub fn area_cell(&self) -> u32 { word(&self.0, 0x110) }
    pub fn tone_class(&self) -> u32 { word(&self.0, 0x11c) }
    pub fn area_count(&self) -> u8 { self.0[0x134] }
    pub fn area_position(&self, index: usize) -> i16 { half(&self.0, 0x138 + 8 * index) as i16 }
    pub fn area_index(&self, index: usize) -> u16 { half(&self.0, 0x13c + 8 * index) }
    pub fn area_source(&self) -> [u8; 20] { bytes(&self.0, 0x150) }
    pub fn focus_found(&self) -> bool { self.0[0x168] == 1 }
    pub fn focus_missing(&self) -> u32 { word(&self.0, 0x16c) }
    pub fn area_kind(&self) -> u32 { word(&self.0, 0x170) }
    pub fn tone_limits(&self) -> (u32, u32) { (word(&self.0, 0x174), word(&self.0, 0x178)) }
    pub fn scan_requested(&self) -> u32 { word(&self.0, 0x17c) }
    pub fn selection(&self) -> u32 { word(&self.0, 0x180) }
    /// Six words from +0x34 in the order calc 0x3c04's window takes them (0xf20ee0..0xf20f08).
    pub fn tone_window(&self) -> [u32; 6] { [0x38, 0x34, 0x3c, 0x40, 0x44, 0x48].map(|offset| word(&self.0, offset)) }
}

/// Words of the scan block's tone result, by offset into it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToneWord {
    Value = 0x0,
    Scanned = 0x4,
    Moved = 0x8,
    Driven = 0xc,
    Held = 0x20,
    LowBound = 0x24,
    HighBound = 0x28,
    FirstReached = 0x2c,
    SecondReached = 0x38,
}


/// The scan request 0xf2230c builds from the installed scan plan, the capture and lens blocks
/// (0xf22368..0xf22524), without the selection setting and the correction.
pub fn scan_request(scan: &ScanBlock, capture: &CaptureBlock, lens: &LensBlock) -> ScanRequest {
    let lens_word = |offset| word(&lens.0, offset);
    let window = match word(&scan.0, 0x4c) {
        1 => ScanWindow::Area(bytes(&lens.0, 0x14)),
        mode @ (2 | 3) => ScanWindow::Centred {
            centre: [lens_word(0x4d0).wrapping_add(lens_word(0x4cc) >> 1), lens_word(0x4d4).wrapping_add(lens_word(0x4c8) >> 1)],
            first: [lens_word(0x4c8), lens_word(0x4cc)],
            second: [lens_word(0x4d0), lens_word(0x4d4)],
            held: mode == 3,
        },
        _ => ScanWindow::None,
    };
    ScanRequest {
        start: scan.start(),
        positions: [half(&scan.0, 0x10), half(&scan.0, 0x12), half(&scan.0, 0x16)],
        span: half(&scan.0, 0x14).wrapping_add(1),
        selection_word: scan.selection_word(scan.selection()),
        index_base: scan.index_base(),
        weighting: word(&scan.0, 0x58),
        tolerance_kind: capture.tolerance_kind(),
        window,
        selection_setting: 0,
        windows: bytes(&lens.0, 0x4b0),
        correction: 0,
    }
}

/// The calc 0x3c04 result merged as 0xf20418 does: flags +0x2c and +0x30 stay set once the scan
/// block's tone result has them.
pub fn merged_tone(scan: &ScanBlock, result: &ToneCalculation) -> ToneCalculation {
    let mut merged = *result;
    if word(&scan.0, 0x9c + 0x2c) == 1 { put(&mut merged.0, 0x2c, 1); }
    if word(&scan.0, 0x9c + 0x30) == 1 { put(&mut merged.0, 0x30, 1); }
    merged
}

/// The result words 0xf20418 keeps in the scan block's limits unless they are already 1.
pub fn tone_limits(result: &ToneCalculation) -> (u32, u32) { (word(&result.0, 0x20), word(&result.0, 0x24)) }

/// A scanned tone result asks for the tone mark (0xf20498).
pub fn tone_scanned(result: &ToneCalculation) -> bool { word(&result.0, 0x4) != 0 }

impl SampleSnapshot {
    /// The word of the snapshot's selected entry (index byte +0xa0, entries from +4 every 8 bytes).
    pub fn selected_sample(&self) -> u32 { word(&self.0, 4 + 8 * usize::from(self.0[0xa0])) }
}

/// Tone classes 0..=11 the fallback marks pending (mask 0xd7e, 0xf218c4..0xf218d8).
pub const fn pending_tone(class: u32) -> bool { class <= 11 && 0xd7e & (1 << class) != 0 }

/// The fallback's position-correction answer (0xf4f10c) that marks the outcome's source 1.
pub const CORRECTED_POSITION: u32 = 3;

/// The history 0xf1fda8 keeps (0x7588e8, 0x75890c): +8 valid, then the status, point and
/// selected value of a valid outcome.
pub fn record_outcome(history: &mut OutcomeHistory, outcome: &FocusOutcome) {
    put(&mut history.0, 0x08, u32::from(outcome.valid));
    if !outcome.valid { return; }
    put(&mut history.0, 0x0c, outcome.status);
    history.0[0x10..0x20].copy_from_slice(&point_bytes(&outcome.point));
    put(&mut history.0, 0x20, outcome.selected);
}

pub fn point_bytes(point: &LensPoint) -> [u8; 16] {
    let mut bytes = [0; 16];
    put_half(&mut bytes, 0, point.position as u16);
    put_half(&mut bytes, 2, point.detail);
    put(&mut bytes, 4, point.reference.0);
    put(&mut bytes, 8, point.state);
    put(&mut bytes, 12, point.kind);
    bytes
}

pub fn point(bytes: &[u8; 16]) -> LensPoint {
    LensPoint { position: half(bytes, 0) as i16, detail: half(bytes, 2), reference: LensReference(word(bytes, 4)), state: word(bytes, 8), kind: word(bytes, 12) }
}

/// The correction record 0xf1fda8 stores (0x756970, 28 bytes): enabled, status, point, pending.
pub fn outcome_correction(outcome: &FocusOutcome) -> CorrectionRecord {
    let mut bytes = [0; 64];
    put(&mut bytes, 0x00, 1);
    put(&mut bytes, 0x04, outcome.status);
    bytes[0x08..0x18].copy_from_slice(&point_bytes(&outcome.point));
    put(&mut bytes, 0x18, outcome.pending);
    CorrectionRecord { enabled: 1, pending: outcome.pending, bytes }
}

/// The 64-byte area block 0xf1fda8 passes by value (0x751ec8, 0x751f58, 0x751fe8): the count,
/// 20 source bytes at +4 and each area's grid cell as two words from +0x18.
pub fn area_block(areas: &AreaBlock) -> [u8; AREA_BLOCK_BYTES] {
    let mut bytes = [0; AREA_BLOCK_BYTES];
    bytes[0] = areas.count;
    bytes[4..24].copy_from_slice(&areas.source);
    for (index, (column, row)) in areas.cells.iter().enumerate().take(usize::from(areas.count)) {
        put(&mut bytes, 0x18 + 8 * index, *column);
        put(&mut bytes, 0x1c + 8 * index, *row);
    }
    bytes
}

/// The outcome's positions in the record's area-position layout (0x758980).
pub fn outcome_positions(outcome: &FocusOutcome) -> AreaPositions {
    let mut bytes = [0; 36];
    bytes[0] = outcome.last_position;
    for (index, (position, kind)) in outcome.positions.iter().enumerate() {
        put_half(&mut bytes, 4 + 8 * index, *position as u16);
        put(&mut bytes, 8 + 8 * index, *kind);
    }
    crate::stock::finish::area_positions(&bytes)
}

/// The statistics 0x03 blocks 0xf20b60 fills from the capture, lens and subject blocks and the
/// record (0xf20ca8..0xf20e44).
pub fn statistics_setup(capture: &CaptureBlock, lens: &LensBlock, subject: &SubjectBlock, selection: u32, record: &StatisticsRecordValues, weighted: bool) -> StatisticsSetup {
    let capture_word = |offset| word(&capture.0, offset);
    let lens_word = |offset| word(&lens.0, offset);
    StatisticsSetup {
        mode: if capture.mode() == 1 { 3 } else { 2 },
        weighted,
        selection: [selection, capture_word(0x50), capture_word(0x54)],
        area: record.area,
        levels: [capture_word(0x40), capture_word(0xb8), record.extent, capture_word(0x44)],
        sampling: StatisticsSampling {
            capture: [0x2c, 0x30, 0x34, 0xfc, 0x104, 0x108, 0x10c].map(capture_word),
            lens: [lens_word(0x80), lens_word(0x78)],
            halves: [lens_word(0x7c) as u16, subject.vertical(), subject.horizontal(), half(&lens.0, 0x84)],
            sample: capture_word(0x138),
            flags: [capture.0[0x13c], capture_word(0x140) as u8, record.sample_kind],
        },
        capture: StatisticsCapture {
            words: [capture_word(0x8), capture_word(0x4), capture_word(0x38), record.selection],
            flags: [capture.0[0xbe], record.flags[0], record.flags[1], record.flags[2], record.flags[3]],
            layout: record.layout,
        },
    }
}

/// The weight table arguments 0xf20b60 builds from the subject and lens blocks (0xf20c04..0xf20ca4).
pub fn statistics_weights(weights: [u8; 14], subject: &SubjectBlock, lens: &LensBlock) -> StatisticsWeights {
    let lens_word = |offset| word(&lens.0, offset);
    let shift = u32::from(lens.0[0x4c4] >> 1);
    let first = lens_word(0x4b0);
    let second = lens_word(0x4b4);
    let origin_x = shift.wrapping_mul(u32::from(half(&lens.0, 0x4c0)).wrapping_add(second)).wrapping_add(lens_word(0x4b8).wrapping_sub(lens_word(0x38)));
    let origin_y = shift.wrapping_mul(u32::from(half(&lens.0, 0x4c2)).wrapping_add(first)).wrapping_add(lens_word(0x4bc).wrapping_sub(lens_word(0x3c)));
    StatisticsWeights {
        weights,
        mode: if subject.kind() == 0 { 4 } else { subject.mode() },
        extent: [lens_word(0x44) as u16, lens_word(0x40) as u16],
        origin: [origin_x as u16, origin_y as u16],
        size: [second as u16, first as u16],
    }
}

/// Image-4 record entries 0xf1fda8 stores the outcome through, in its order.
pub const OUTCOME_ENTRIES: [u32; 8] = [0x751de4, 0x7520c4, 0x752178, 0x75222c, 0x7522d4, 0x75236c, 0x752404, 0x751e7c];
pub const AREA_BLOCK_ENTRIES: [u32; 3] = [0x751ec8, 0x751f58, 0x751fe8];
