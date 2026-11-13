#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the calculation step (0xf1d1c0, 0xf1a190, 0xf1d154, 0xf1fd40).

use crate::boundary::LensSnapshot;
use crate::calculation_work::{CalculationProfile, ExposureWord, PlanWords};

/// The exposure context (0x16b3ddc).
pub const EXPOSURE_CONTEXT: u32 = 0x16b_3ddc;
/// Words of the exposure context the calculation binds: two record words, the lens reference
/// and the option (0xf1d220..0xf1d238, 0xf1a1d0..0xf1a1e8).
pub const EXPOSURE_RECORD: [u32; 2] = [0x1c, 0x20];
pub const EXPOSURE_LENS_REFERENCE: u32 = 0x10;
pub const EXPOSURE_OPTION: u32 = 0x18;
/// Cleared by a resumed half press (0xf1d524).
pub const EXPOSURE_REPEAT: u32 = 0x1268;

pub const fn exposure_word(word: ExposureWord) -> u32 {
    match word {
        ExposureWord::Mode => 0x4,
        ExposureWord::Class => 0x8,
        ExposureWord::Profile => 0x14,
        ExposureWord::Evaluation => 0x30,
        ExposureWord::Refinement => 0x47c,
        ExposureWord::Repeat => EXPOSURE_REPEAT,
    }
}

/// The calc 0x3c02 input inside the exposure context (0x16b4f10), 300 bytes; 0xf1fd40 saves it
/// to 0x41717a54 and 0xf1fc5c takes it back.
pub const INPUT: u32 = EXPOSURE_CONTEXT + 0x1134;
pub const INPUT_BYTES: usize = 300;
pub const SAVED_INPUT: u32 = 0x4171_7a54;
/// Input words a half press without a new calculation takes from the retained targets
/// (0xf1d464..0xf1d490): a word at +0xc and a halfword at +0x10.
pub const INPUT_TARGET_WORD: u32 = 0xc;
pub const INPUT_TARGET_HALF: u32 = 0x10;
/// Resume offsets 0xf1d4f8..0xf1d50c write: halfwords at +0x88 and +0x80, a word at +0x8c.
pub const INPUT_RESUME_LEAD: u32 = 0x88;
pub const INPUT_RESUME_LENS_WORD: u32 = 0x8c;
pub const INPUT_RESUME_OFFSET: u32 = 0x80;
/// The retained targets: the word 0xf19634 leaves (0xf19dd8) and the halfword 0xf1e468 leaves
/// (0xf1e878).
pub const RETAINED_TARGET_WORD: u32 = 0x16a_d738;
pub const RETAINED_TARGET_HALF: u32 = 0x16b_3d38;

/// The calculation block (0x16ad740): the bound record, then the calc 0x3c02 result from +4
/// (0x640c bytes, also the retained copy at 0x16a0500), then the plan words.
pub const BLOCK: u32 = 0x16a_d740;
pub const RESULT: u32 = BLOCK + 4;
pub const RESULT_BYTES: usize = 0x640c;
pub const RETAINED_RESULT: u32 = 0x16a_0500;
/// The result word 0xf1d154 copies into the lens snapshot (result +0x63e8).
pub const RESULT_WORD: u32 = RESULT + 0x63e8;
pub const SELECTION: u32 = BLOCK + 0x6410;
/// 4 marks a requested calculation (0xf1d3fc, 0xf1a26c).
pub const REQUEST_MARK: u32 = BLOCK + 0x6414;
pub const REQUESTED: u32 = 4;
/// Plan words 0xf1d23c..0xf1d29c clear and fill: 136 bytes from +0x6568.
pub const PLAN: u32 = BLOCK + 0x6568;
pub const PLAN_BYTES: usize = 136;
const GRID: usize = 8;
const GRID_START: u32 = 7;

/// The 136 plan bytes: the row-data word at +0, the adjustment byte at +4 and the 3x3 grid of
/// start values from +8.
pub fn plan_bytes(plan: PlanWords) -> [u8; PLAN_BYTES] {
    let mut bytes = [0; PLAN_BYTES];
    bytes[0..4].copy_from_slice(&plan.row_data.to_le_bytes());
    bytes[4] = plan.adjustment;
    for cell in 0..9 {
        let offset = GRID + cell * 4;
        bytes[offset..offset + 4].copy_from_slice(&GRID_START.to_le_bytes());
    }
    bytes
}

/// Image-4 accessors behind each profile value (0xf1d22c, 0xf1d2a4, 0xf1d2c4, 0xf1d2d0, 0xf1d300,
/// 0xf1d324, 0xf1d4d4, 0xf1d4e0); 0x75a414 stores the resume offset (0xf1d518).
pub const fn profile_accessor(value: CalculationProfile) -> u32 {
    match value {
        CalculationProfile::LensReference => 0x751b74,
        CalculationProfile::SelectionKind => 0x74e5c0,
        CalculationProfile::RowLimit => 0x74f124,
        CalculationProfile::RowOption => 0x750474,
        CalculationProfile::AdjustmentKind => 0x75060c,
        CalculationProfile::AdjustmentOption => 0x7503ec,
        CalculationProfile::ResumeLead => 0x74f6fc,
        CalculationProfile::ResumeOffset => 0x74f5ec,
    }
}
pub const RESUME_OFFSET_STORE: u32 = 0x75a414;
/// Configuration property 0x65a8 read through 0x69f8fc (0xf1d55c).
pub const REUSE_CONTROL_PROPERTY: u16 = 0x65a8;

/// Lens-snapshot word +0x2c, which the calculation result fills (0xf1d190..0xf1d198).
const SNAPSHOT_RESULT: usize = 0x2c - 36;

pub fn with_result_word(mut snapshot: LensSnapshot, word: u32) -> LensSnapshot {
    snapshot.tail[SNAPSHOT_RESULT..SNAPSHOT_RESULT + 4].copy_from_slice(&word.to_le_bytes());
    snapshot
}

pub fn result_word(snapshot: &LensSnapshot) -> u32 {
    let tail = &snapshot.tail[SNAPSHOT_RESULT..SNAPSHOT_RESULT + 4];
    u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]])
}
