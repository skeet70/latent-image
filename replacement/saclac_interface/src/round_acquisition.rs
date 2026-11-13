#![forbid(unsafe_code)]

//! What the measurement round's acquisition sends to calc1 (0x3d00, 0x3d01) with the records
//! calc1 reads beside them, the result records calc1's completions point at, and the tables and
//! notice it posts to imgdc (0x3f2c, 0x3f2d, 0x3f45).

use crate::calc::SharedAddress;

/// Profile values a 0x3d00 request carries without the round interpreting them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AcquisitionProfile {
    /// Left zero by the S1 acquisition.
    pub halfword: u16,
    pub updated_position: u32,
    pub word: u32,
    pub reference: u16,
    pub selection: u8,
    pub adjustment: u32,
    pub area: [u32; 5],
    pub category: u32,
    pub ratio: [u32; 2],
    pub window: u32,
}

/// The sample values a 0x3d00 request carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AcquisitionSample {
    pub complete: bool,
    pub exposure: u32,
    pub level: u16,
    /// The second level less the gain offset.
    pub reference_level: u16,
    pub gain_step: u32,
    pub offset_level: u16,
    pub adjustment: u32,
    pub timing: u32,
    pub halfword: u16,
    pub extent: [u32; 2],
    pub size: [u32; 2],
    pub origin: [u32; 2],
}

/// Calc1 0x3d00: one evaluation of the primary statistics over the request's grid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AcquisitionRequest {
    pub mode: u32,
    /// No primary result has been collected since the acquisition started.
    pub first: bool,
    pub profile: AcquisitionProfile,
    pub sample: AcquisitionSample,
    /// The sample's level against the decade, the offset and the exposure value.
    pub relative_level: u16,
    pub configuration: u8,
    pub level_category: u32,
    pub correction: u16,
    pub aspect: u16,
    pub decade: u16,
}

/// The configuration values the calc library adds to a 0x3d01 request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionLimits {
    pub step: u16,
    pub lower: u16,
    pub upper: u16,
    pub byte: u8,
}

/// Calc1 0x3d01: one selection over the latest acquisition. Profile values it carries keep the
/// names of their record fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionRequest {
    /// No selection has been collected since the acquisition started.
    pub first: bool,
    pub mode: u32,
    pub halfword: u16,
    pub third_word: u32,
    pub zero_word: u32,
    pub mode_word: u32,
    pub byte: u8,
    pub second_halfword: u16,
    pub lens: u32,
    pub converted_step: u32,
    pub converted_level: u16,
    pub reference: u16,
    pub restore_adjustment: u16,
    pub selection_word: u32,
    pub selection: u32,
    pub exposure: u32,
    pub summary_level: u16,
    pub limits: SelectionLimits,
    pub lens_halfword: u16,
    pub lens_reference: u32,
    pub limit_halfwords: [u16; 2],
    pub gain_step: u32,
    pub unrestored: bool,
    pub correction: u16,
    pub aspect: u16,
    pub decade: u16,
    /// Left zero by the S1 acquisition.
    pub adjustment_class: u8,
    pub eighth_word: u32,
    pub grid_count: u32,
    pub second_word: u32,
    pub adjustment: u32,
    pub signed_word: u16,
    /// Left zero by the S1 acquisition.
    pub seventh_word: u32,
    pub fourth_word: u32,
    pub fifth_word: u32,
    pub focus_word: u32,
    pub word: u32,
    pub rate_mode: u16,
    pub resource_level: u16,
    pub mode_flag: u32,
    pub kind: u32,
    pub ninth_word: u32,
    pub option: u32,
    pub area: [u16; 3],
    pub pair_word: u32,
    pub program_word: u32,
    pub rate_flag: bool,
    pub program: u32,
    pub request: u32,
    pub eligible: u32,
    /// calc1 reads the window and setting records.
    pub window: bool,
    /// calc1 reads the shot's saved reference beside the setting.
    pub shot_copy: bool,
    pub scale_change: bool,
    /// Left zero by the S1 acquisition.
    pub program_changed: u32,
    pub program_flag: u32,
    pub offset_mode: u32,
    /// The S1 acquisition's alone.
    pub s1: SelectionS1,
    pub recording: bool,
}

/// Values only the S1 acquisition's 0x3d01 request carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionS1 {
    pub halfword: u16,
    pub margin: u16,
    pub byte: u8,
    pub position: u32,
}

/// The selection window calc1 reads through a 0x3d01 request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionWindow {
    pub kind: u32,
    pub selection: u32,
    pub updated_position: u32,
    pub position_halfword: u16,
    pub focus_word: u32,
    pub lens: u32,
    pub lens_reference: u32,
    pub mode_rule: u32,
    pub second_word: u32,
    pub mode_word: u32,
    pub request: u32,
    pub clock: u32,
    pub rate: u32,
    pub context: [u32; 3],
    pub context_byte: u8,
    pub position_level: u16,
    pub relative_level: u16,
    pub resource_level: u16,
    pub rate_limit: u32,
    pub rate_mode: u16,
    pub target_level: u16,
    pub ratio: [u32; 2],
    pub converted: bool,
    pub both_modes: u32,
    pub one_mode: u32,
    pub context_word: u32,
    pub change_category: u32,
    pub category: u32,
    pub position_flag: bool,
    pub limit_halfword: u16,
}

/// The selection setting calc1 reads through a 0x3d01 request, as the round builds it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionSetting {
    pub lens: u32,
    pub shot_word: u32,
    pub margin: u16,
    pub category: u32,
    pub one_mode: u32,
    pub context_word: u32,
    pub selection: u32,
    pub updated_position: u32,
    pub position_halfword: u16,
    pub mode_word: u32,
    pub byte: u8,
    pub halfword: u16,
    pub setting_word: u32,
    pub grid_kind: u32,
    pub focus_word: u32,
    pub fourth_word: u32,
    pub request: u32,
    pub rate_mode: u32,
    pub word: u32,
    pub reference: u16,
    pub restore_adjustment: u16,
    pub selection_word: u32,
    pub lens_halfword: u16,
    pub lens_reference: u32,
    pub halfwords: [u16; 3],
    pub eighth_word: u32,
    pub mode_rule: u32,
    pub category_word: u32,
    pub change_category: u32,
    pub second_word: u32,
    pub mode_flag: u32,
    pub rate_flag: bool,
    pub fifth_word: u32,
    pub ninth_word: u32,
    pub profile_flags: [u32; 8],
    pub tenth_word: u32,
    pub lens_limit: u32,
    pub lens_word_index: u32,
    pub kind: u32,
    pub kind_override: u32,
    pub adjustment: u32,
    pub profile_index: u32,
    pub program_word: u32,
    pub restore_position: u32,
    pub hardware_ready: bool,
}

/// What the round writes into a setting the shot restored from its saved selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RestoredSetting {
    pub margin: u16,
    pub reference: u16,
    pub lens_halfword: u16,
    pub lens_reference: u32,
}

/// The levels of the primary result, also published with the record's initial exposure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrimaryLevels {
    pub base: u16,
    pub levels: [u16; 7],
    pub word: u32,
}

/// The levels of the selection result, also published with the record's initial exposure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionLevels {
    pub base: u16,
    pub levels: [u16; 3],
    pub zeroed: [u16; 6],
    pub unsigned: u16,
    pub word: u32,
    pub last: u16,
    pub reference: u16,
    pub tail: [u16; 3],
}

/// The record calc1's 0x3d00 completion points at.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AcquisitionResult {
    pub address: SharedAddress,
    pub levels: PrimaryLevels,
    pub statistics: [SharedAddress; 4],
    pub exposure: u32,
    pub level: u16,
    pub second: u16,
    pub offset: u16,
    pub aspect: u16,
    pub halfword: u16,
    pub gain_offset: u16,
    pub correction: u16,
    pub count: u32,
    /// The statistics and their levels replace the acquisition's.
    pub statistics_valid: bool,
}

/// One of a selection result's two programs: an exposure word and four candidate levels each.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionProgram {
    pub exposure: u32,
    pub levels: [u16; 4],
    pub first: [u16; 4],
    pub second: [u16; 4],
    pub adjustment: [u16; 4],
}

/// The record calc1's 0x3d01 completion points at.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionResult {
    pub address: SharedAddress,
    pub word: u32,
    pub levels: SelectionLevels,
    pub secondary: SelectionProgram,
    pub primary: SelectionProgram,
    pub restore_levels: [u16; 2],
    pub position: u32,
    pub halfwords: [u16; 2],
    pub words: [u32; 3],
    pub table_reference: u16,
    pub table_word: u32,
    pub primary_only: u32,
    pub program_argument: u32,
    pub alternate: u32,
    pub kind: u32,
    pub shot_record: SharedAddress,
    pub saved_record: SharedAddress,
    pub selected: u32,
    pub shot_data: bool,
    pub keeps_program: bool,
    pub adjustment: u32,
    pub program_word: u32,
    pub limit: u8,
    pub program_flag: u32,
    pub tail: [u16; 4],
    pub level_halfword: u16,
    pub position_word: u32,
    pub position_flag: u8,
    pub save: SavedSelection,
}

/// The values a selection result hands the shot's saved selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SavedSelection {
    pub index: u32,
    pub word: u32,
    pub bytes: [u8; 2],
    pub level: i16,
    pub flag: u8,
}

/// The two halfwords the round reads from the record a selection result names for the shot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShotRecordLevels {
    pub first: u16,
    pub last: u16,
}

/// The values the S1 acquisition publishes from the record a selection result saves.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SavedExposure {
    pub exposure: u32,
    pub level: u16,
    pub second: u16,
    pub aspect: u16,
    pub offset: u16,
}

/// imgdc 0x3f2d: the acquisition's lens, mode and position words after a change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChangeNotice {
    pub category: u32,
    pub mode_is_one: u32,
    pub lens: u32,
    pub positions: [u32; 2],
    pub unbounded: u32,
    pub pair: [u32; 2],
    pub kind: u32,
}

/// One candidate of a selection table.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionEntry {
    pub timing: u32,
    pub code: u16,
    pub selected: u8,
    pub gain_step: u32,
    pub exposure: u32,
    pub level: u16,
    pub second: u16,
    pub offset: u16,
    pub word: u32,
    pub reference: u16,
    pub decade: u32,
}

/// imgdc 0x3f2c: the live acquisition's four candidates under each of its two programs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionTable {
    pub primary: [SelectionEntry; 4],
    pub secondary: [SelectionEntry; 4],
    pub levels: [u16; 3],
}

/// imgdc 0x3f45: the S1 acquisition's candidate under each of its two programs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusTable {
    pub primary: SelectionEntry,
    pub secondary: SelectionEntry,
}
