#![forbid(unsafe_code)]

//! What the shot's exposure step sends to the image task's statistics measurement and what its
//! peers deliver back in the completions 0x4c, 0x4d, 0x8e and 0x8f.

use crate::calc::SharedAddress;

/// Index of the statistics entry the measurement filled; delivered with completion 0x4d.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasuredEntry(pub u8);

/// The measurement's status when it released its buffers; delivered with completion 0x4c.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReleaseStatus(pub u32);

/// The calculation task's evaluation of one statistics entry (calc 0x3c00, completion 0x8e).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Evaluation {
    pub entry: u8,
    pub levels: [u16; 4],
    pub bytes: [u8; EVALUATION_BYTES],
}

pub const EVALUATION_BYTES: usize = 16;
pub const CORRECTION_HALVES: usize = 78;

/// What calc evaluates for one row of the exposure's selection bank (calc 0x3c00).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvaluationRequest {
    pub row: u32,
    /// Halfwords of the row's measurement pairs 0, 2 and 1, each second word first.
    pub measured: [[u16; 2]; 3],
    /// Halfwords of the row's four statistics words.
    pub statistics: [u16; 4],
    pub row_buffer: Option<SharedAddress>,
    /// The rate and its derived values.
    pub rates: [u32; 4],
    /// The row's level is the one after the reference level.
    pub after_reference: bool,
    pub value: u16,
    /// The value of the step one level below the row's; 0x8000 when there is none.
    pub lower_value: u16,
    pub tolerance_above: i16,
    pub tolerance_below: i16,
    /// -900, or -600 for a row of fixed offset 3; it stays from the last request that set it.
    pub floor: i16,
    /// 3 or 4 for a row of fixed offset, else 2 or 1 as the first applied row's level is above
    /// the row's or not, 0 when no row is applied.
    pub direction: u32,
    pub column_count: u32,
    pub column_regions: Option<SharedAddress>,
    pub column_buffer: Option<SharedAddress>,
    pub dimensions: [u16; 3],
    pub next_profile: u32,
    pub level_curves: [Option<SharedAddress>; 4],
    pub level_classes: Option<SharedAddress>,
    pub frame: Option<SharedAddress>,
    pub subject_curves: [Option<SharedAddress>; 4],
    pub selection_flags: Option<SharedAddress>,
}

/// The calculation task's exposure correction (calc 0x3c01, completion 0x8f).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Correction(pub [u16; CORRECTION_HALVES]);

impl Default for Correction {
    fn default() -> Self { Self([0; CORRECTION_HALVES]) }
}

impl Correction {
    pub const fn half(&self, index: usize) -> u16 { self.0[index] }

    /// The word whose low halfword is at `index`.
    pub const fn word(&self, index: usize) -> u32 { self.0[index] as u32 | (self.0[index + 1] as u32) << 16 }
}

/// What calc corrects the exposure from (calc 0x3c01).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CorrectionRequest {
    /// Record byte +0x43 when it is 1 or 2, else 0.
    pub style: u32,
    pub measurement_selection: u32,
    pub selection_kind: u32,
    pub selection_flags: u16,
    /// Record bytes +0x34, +0x44, +0x4b, +0x32, +0x3e and +0x66.
    pub record_kind: u32,
    pub record_44: u32,
    pub focus_class: u32,
    pub record_32: u32,
    pub record_3e: u32,
    pub variant: u32,
    pub lens_reference: u32,
    pub selection_auxiliary: u32,
    /// Image 4's reported word when record byte +0x41 is 1, else 0.
    pub image_word: u32,
    /// 10000 over the subject's drive rate, 1 when there is no first treatment or no rate.
    pub rate: u32,
    /// Words of task 43's selection lists.
    pub list_words: [u32; 3],
    pub list_byte: u8,
    pub list_word: u32,
    pub level_curves: [Option<SharedAddress>; 4],
    pub start: u16,
    pub start_above_target: u16,
    pub reference_offset: u16,
    /// The configuration value calc reads the scene's curves from.
    pub curve_table: u32,
    pub rate_setting: u16,
    /// The evaluation's reference value plus 250, raised by `start_above_target`.
    pub reference: u16,
    pub ratio: [u32; 2],
    /// The measured level is in step 2 of the level table.
    pub second_step: bool,
    pub treated: bool,
    pub first_treatment: bool,
    pub subjects: Option<SharedAddress>,
    pub summary: Option<SharedAddress>,
    pub subject_curves: [Option<SharedAddress>; 4],
    /// Header bytes 0x61 and 0x47.
    pub header_61: u32,
    pub header_47: u32,
    pub unmeasured: bool,
    pub record_20: i16,
    pub buffers: [Option<SharedAddress>; 2],
    pub frame: Option<SharedAddress>,
}

pub const STATISTICS_AREAS_BYTES: usize = 160;

/// The statistics measurement the exposure asks of the image task (imgdc 0x3f1a, completions
/// 0x4d then 0x4c). The subject list it names is staged by the port that sends it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatisticsRequest {
    pub lens_reference: u32,
    pub calculation_mode: u32,
    pub subject_query: [u32; 2],
    /// Upper and lower halfwords.
    pub area_layout: u32,
    pub context_mode: u32,
    pub correction_mode: u32,
    pub statistics_option: u32,
    pub single_area: bool,
    pub areas: [u8; STATISTICS_AREAS_BYTES],
    pub area_count: u8,
    pub scaled: bool,
}
