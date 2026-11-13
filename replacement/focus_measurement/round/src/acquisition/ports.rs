#![forbid(unsafe_code)]

//! What the acquisition reads from and asks of the profile records, the adjustment and calc
//! library layers, the imaging pipeline, the shot sequence and calc1.

use crate::auxiliary::ResourceIndex;
use crate::pipeline::{Exposure, GainStep, Pipeline, PipelineSnapshot, PipelineWord, Statistics, TimingWord};
use crate::record::{Adjustment, Trace};
use xm1_saclac_interface::calc::{AcquisitionGrid, SharedAddress};
use xm1_saclac_interface::round_acquisition::{
    AcquisitionRequest, AcquisitionResult, ChangeNotice, FocusTable, PrimaryLevels, RestoredSetting, SavedExposure,
    SavedSelection, SelectionLevels, SelectionLimits, SelectionRequest, SelectionResult, SelectionSetting,
    SelectionTable, SelectionWindow, ShotRecordLevels,
};
use xm1_saclac_interface::{Completed, RecordHandle};

use super::selection::SelectionMemory;
pub use super::Branch;

/// An adjustment profile an exposure program selection chose; the record keeps its low byte.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Program(pub u32);

/// A statistics buffer calc1 names or the release fills.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsBuffer(pub u32);

/// The profile record values the acquisition reads and does not write.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecordView {
    /// How an initialization starts the exposure.
    pub start: u32,
    pub selection: u32,
    pub s1_mode: u32,
    pub halfword: u16,
    pub mode_word: u32,
    pub byte: u8,
    pub second_halfword: u16,
    pub setting_word: u32,
    pub grid_kind: u32,
    pub focus_word: u32,
    pub window: u32,
    pub seventh_word: u32,
    pub fourth_word: u32,
    pub third_word: u32,
    /// Six asks for the mode-six statistics before an S1 sample.
    pub request: u32,
    pub detail_flag: u32,
    pub word: u32,
    pub reference: u16,
    pub selection_word: u32,
    pub lens_halfword: u16,
    pub lens_reference: u32,
    pub halfwords: [u16; 2],
    pub zero_word: u32,
    pub category: u32,
    pub pair: [u32; 2],
    pub tenth_word: u32,
    pub change_category: u32,
    pub signed_word: u16,
    pub eighth_word: u32,
    pub second_word: u32,
    /// One keeps the exposure record beside the published exposure.
    pub mode_flag: u32,
    pub rate_enabled: u32,
    pub fifth_word: u32,
    pub forced: u32,
    pub ninth_word: u32,
    /// The last is the program flag.
    pub profile_flags: [u32; 8],
    pub lens_word_index: u32,
    pub adjustment: u32,
    pub kind: u32,
    pub kind_override: u32,
    pub pair_word: u32,
    pub reuse_blocked: u32,
    pub restored: u32,
    pub program_word: [u32; 2],
    pub restore_position: u32,
    pub s1_byte: u8,
    pub s1_halfword: u16,
    pub release_halfword: u16,
    pub area: [u32; 5],
    pub mode: u32,
    pub updated_position: u32,
    pub position_halfword: u16,
    pub converted_level: u32,
    pub restore_adjustment: u16,
    pub decade: u16,
    pub profile_index: u32,
    pub ratio: [u32; 2],
    pub lens_limit: u32,
    pub block_reference: u16,
    pub block_word: u32,
    pub s1_exposure: PipelineWord,
    pub s1_level: u16,
    pub s1_second: u16,
    pub s1_timing: TimingWord,
}

impl RecordView {
    pub const fn program_flag(&self) -> u32 { self.profile_flags[7] }
    pub const fn class_profile(&self) -> u32 { self.profile_flags[6] }
    /// The area's first three halfwords.
    pub const fn area_halfwords(&self) -> [u16; 3] {
        [self.area[0] as u16, (self.area[0] >> 16) as u16, self.area[1] as u16]
    }
}

/// The exposure the record publishes, read back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PublishedExposure {
    pub combined: u16,
    pub reference_level: u16,
    pub exposure: PipelineWord,
    pub level: u16,
    pub second: u16,
    pub correction: u16,
    pub aspect: u16,
    pub reference: u16,
    pub offset: u16,
    pub gain_step: GainStep,
    pub lens: ResourceIndex,
    pub timing: TimingWord,
    pub code: u16,
    pub selected: u8,
    pub adjustment: u32,
    pub selection_reference: u16,
    /// The primary levels' second to fifth.
    pub primary_levels: [u16; 4],
}

/// One exposure publication with the values the round derives from it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposurePublication {
    pub exposure: PipelineWord,
    pub level: u16,
    pub second: u16,
    pub reference: u16,
    pub decade: u16,
    pub offset: u16,
    pub correction: u16,
    pub gain_step: GainStep,
    pub timing: TimingWord,
    pub code: u16,
    pub selected: u8,
    pub adjustment: u32,
    /// The level, less the second level and the offset, with the exposure value added.
    pub combined: u16,
    /// The combined level with the reference added, 100 lower under the adjustment.
    pub reference_level: u16,
    pub aspect: u16,
}

/// A change to the exposure record the record keeps under its mode flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExposureRecordUpdate {
    GainStep(GainStep),
    Context { lens: ResourceIndex, index: u32 },
    PrimaryPair([u16; 2]),
    SelectionTail { tail: [u16; 3], last: u16 },
}

/// The record's acquisition block as far as the acquisition reads and writes it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AcquisitionBlock {
    pub lens: ResourceIndex,
    pub collected: u32,
    pub exposure: PipelineWord,
    pub level: u16,
    pub gain_offset: u16,
    pub offset: u16,
    pub aspect: u16,
    pub reference: u16,
    pub halfword: u16,
    pub levels: [u16; 4],
    pub reused: bool,
    pub offsets: StatisticsBuffer,
    pub extra: StatisticsBuffer,
    pub release_halfword: u16,
    pub classes: StatisticsBuffer,
    pub weights: [u16; 4],
    pub release_offset: u16,
    pub copy: StatisticsBuffer,
    pub selected: u32,
    pub selection_reference: u16,
    pub selection_byte: u8,
    pub eligible: u32,
    pub setting_ready: u32,
    pub setting_position: u32,
    pub setting_halfword: u16,
    pub grid_word: u32,
    pub shot_span: u16,
    pub setting_shot_word: u32,
}

/// The record's selection: where the shot's selection stands and the values it settled on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionRecord {
    pub position: u32,
    pub word: u32,
    pub level: u16,
    pub levels: [u16; 4],
    pub target: u16,
    pub limit_level: u16,
    pub reference: u16,
    pub flags: [u32; 4],
    pub limit: u8,
    /// Zero lets any result replace the selection.
    pub tracked: u8,
}

/// The selection slot the record publishes to its readers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionSlot {
    pub word: u32,
    pub level: u16,
    pub kind: u32,
    pub limit_level: u16,
    pub flags: [u32; 2],
    pub levels: [u16; 4],
}

/// What the record's selection hook receives.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionNotice {
    pub kind: u32,
    pub level: i16,
    pub word: u32,
    pub limit_level: i16,
    pub flags: [u32; 2],
    pub recording: u32,
    pub levels: [i16; 4],
    pub position: u32,
}

/// What the record's S1 rate hook receives.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RateNotice {
    pub position: u32,
    pub feature: u16,
    pub levels: [i16; 2],
    pub word: u32,
}

/// A pair of words the record keeps in one of its position slots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionSlot { Reported, Lens, Current, Selected }

/// The rate record's values the acquisition passes on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RateRecord {
    pub word: u32,
    pub level: u16,
}

/// The shot's grid reference: two levels and a word out, four levels in and out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridReference {
    pub level: u16,
    pub second: u16,
    pub word: u32,
    pub levels: [u16; 4],
}

/// The values of the shot's selection defaults the selection window carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShotContext {
    pub word: u32,
    pub words: [u32; 3],
    pub byte: u8,
}

/// The program's choice the record keeps for the selection's alternate program.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgramChoice {
    pub word: u32,
    pub level: u16,
    pub timing: TimingWord,
    pub code: u16,
    pub selected: u8,
}

/// The inputs of an exposure program selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgramInputs {
    pub selection: u32,
    pub adjustment: u32,
    pub kind: u32,
    pub mode: u32,
    pub word: u32,
    pub flag: u32,
    pub program_flag: u32,
    pub option: u32,
    pub policy: bool,
    pub percent: u16,
    pub bytes: [u8; 2],
}

/// The inputs of a program's target exposure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetInputs {
    pub program: Program,
    pub reference: u16,
    pub lens: ResourceIndex,
    pub level: i16,
    pub gain_offset: i16,
    /// Present for a program of the wide target.
    pub lens_reference: Option<u32>,
    pub gain_level: u16,
    pub offset: u16,
}

/// A program's target exposure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgramTarget {
    pub adjustment_flag: u32,
    pub base: u16,
    pub level: u16,
    pub exposure: PipelineWord,
    pub gain_level: u16,
    pub second: u16,
    pub adjustment: u16,
    pub offset: u16,
}

/// The inputs of a program's range selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeInputs {
    pub program: Program,
    pub exposure: PipelineWord,
    pub level: u16,
    pub first: u16,
    pub second: u16,
    pub adjustment: i16,
    pub mode: u32,
    pub argument: u32,
    pub forced: u32,
    pub primary: u32,
    pub lens: u32,
    pub alternate: u32,
    pub request: u32,
}

/// A program's range: the timing it settles on, its levels after the range and the candidate it chose.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Range {
    pub timing: TimingWord,
    pub level: u16,
    pub first: u16,
    pub second: u16,
    pub code: u16,
    pub selected: u8,
}

/// The inputs both of the adjustment layer's mode queries take.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModeInputs {
    pub grid_kind: u32,
    pub grid_byte: u8,
    pub selection: u32,
    pub updated_position: u32,
    pub category: u32,
    pub mode_flag: u32,
    pub change_category: u32,
    pub grid_flag: u8,
}

/// The inputs of the calc library's selection position capture.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PositionInputs {
    pub word: u32,
    pub level: i16,
    pub updated_position: u32,
    pub position_halfword: u16,
    pub mode_word: u32,
    pub byte: u8,
    pub halfword: u16,
}

/// What the capture leaves in the setting that the window reads back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionPosition {
    pub level: u16,
    pub flag: u32,
}

/// What a setting holds when calc1 has finished with it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingEcho {
    pub ready: u32,
    pub updated_position: u32,
    pub position_halfword: u16,
    pub shot_word: u32,
}

/// The inputs the calc library fills a grid from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridInputs {
    pub timing: TimingWord,
    pub lens_word: u32,
    pub positions: [u32; 2],
    pub window: u32,
}

/// The configuration values the acquisition reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AcquisitionSettings {
    /// The grid is the primary snapshot's and the release copies the extra statistics.
    pub snapshot_grid: bool,
    pub rate: u8,
    pub s1_rate: u8,
    pub rate_status: bool,
    pub draws_rate: bool,
    pub gate: bool,
    pub rate_divisor: bool,
    pub suppresses_eligible: bool,
    pub top_steps: u8,
    /// Requests leave trace marks.
    pub traces: bool,
    pub release: ReleaseSettings,
}

/// The release's classification bounds, class weights and offsets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReleaseSettings {
    /// Tens of the lower, lowest, upper and highest bounds about the level.
    pub bounds: [i8; 4],
    /// Weights of classes 1 and 3, then 2 and 4, for each of the two weighted sums of each side.
    pub weights: [[u8; 2]; 4],
    /// For the listed adjustment modes, else for other class profiles, else for class profile 1.
    pub offsets: [i8; 3],
}

/// What the release writes to the shared statistics buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Release<'a> {
    pub classes: &'a [u8; 256],
    pub offsets: &'a [[u16; 256]; 4],
    /// Present when the grid is the primary snapshot's.
    pub snapshot: Option<SnapshotRelease<'a>>,
}

/// What a release under a snapshot grid also writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotRelease<'a> {
    /// The extra statistics, moved by the release offset.
    pub extra: &'a [u16],
    /// Receives a whole copy of the primary statistics buffers a 0x3d00 request names.
    pub copy: StatisticsBuffer,
}

/// The profile records, the layers and the peers the acquisition works through.
pub trait Ports: Pipeline + Exposure + Adjustment + Trace {
    fn selection_memory(&mut self) -> &mut SelectionMemory;
    fn acquisition_settings(&mut self) -> AcquisitionSettings;

    fn record_view(&mut self, record: RecordHandle) -> RecordView;
    fn published_exposure(&mut self, record: RecordHandle) -> PublishedExposure;
    fn publish_exposure(&mut self, record: RecordHandle, publication: &ExposurePublication);
    fn publish_primary_levels(&mut self, record: RecordHandle, levels: &PrimaryLevels);
    fn publish_grid(&mut self, record: RecordHandle, grid: &AcquisitionGrid);
    fn publish_selection_levels(&mut self, record: RecordHandle, levels: &SelectionLevels);
    fn update_exposure_record(&mut self, record: RecordHandle, update: ExposureRecordUpdate);
    fn store_lens(&mut self, record: RecordHandle, lens: ResourceIndex);
    fn store_reference(&mut self, record: RecordHandle, reference: u16);
    fn store_selection_reference(&mut self, record: RecordHandle, reference: u16);
    fn program_kind(&mut self, record: RecordHandle) -> u32;
    fn store_program_kind(&mut self, record: RecordHandle, kind: u32);
    fn stored_program(&mut self, record: RecordHandle) -> Program;
    fn store_program(&mut self, record: RecordHandle, program: Program);
    fn store_program_word(&mut self, record: RecordHandle, word: [u32; 2]);
    fn store_program_choice(&mut self, record: RecordHandle, choice: &ProgramChoice);
    fn acquisition_block(&mut self, record: RecordHandle) -> AcquisitionBlock;
    fn store_acquisition_block(&mut self, record: RecordHandle, block: &AcquisitionBlock);
    fn selection_record(&mut self, record: RecordHandle) -> SelectionRecord;
    fn store_selection_record(&mut self, record: RecordHandle, selection: &SelectionRecord);
    fn selection_slot(&mut self, record: RecordHandle) -> SelectionSlot;
    fn publish_selection_slot(&mut self, record: RecordHandle, slot: &SelectionSlot);
    fn store_slot_word(&mut self, record: RecordHandle, word: u32);
    fn position_pair(&mut self, record: RecordHandle, slot: PositionSlot) -> [u32; 2];
    fn store_position_pair(&mut self, record: RecordHandle, slot: PositionSlot, pair: [u32; 2]);
    fn store_restore_levels(&mut self, record: RecordHandle, reference: u16, levels: [u16; 2]);
    fn store_s1_level(&mut self, record: RecordHandle, level: i16);
    fn clear_grid_selection(&mut self, record: RecordHandle);
    fn acquisition_word(&mut self, record: RecordHandle) -> u32;
    fn store_acquisition_word(&mut self, record: RecordHandle, word: u32);
    fn rate_record(&mut self, record: RecordHandle) -> RateRecord;
    fn store_rate_selection(&mut self, record: RecordHandle, selected: u32);
    fn clear_selection_reuse(&mut self, record: RecordHandle);
    /// Clears the reuse flag of the record the controllers' reply block names.
    fn clear_controller_reuse(&mut self);
    fn notify_selection(&mut self, record: RecordHandle, notice: &SelectionNotice);
    fn notify_rate(&mut self, record: RecordHandle, notice: &RateNotice);

    fn current_lens(&mut self) -> ResourceIndex;
    fn lens_word(&mut self, record: RecordHandle, lens: Option<ResourceIndex>) -> u32;
    fn collapse_index(&mut self, exposure: PipelineWord) -> u32;
    fn mapped_value(&mut self, first: u32, second: u32) -> u32;
    fn lens_position(&mut self, pair: [u32; 2], position: u32) -> u32;
    fn gain_step_for(&mut self, offset: i32) -> GainStep;
    fn acquisition_gain_step(&mut self, adjustment: u32) -> GainStep;
    fn kind_decade(&mut self, kind: u32) -> u32;
    fn change_positions(&mut self, record: RecordHandle) -> [u32; 2];

    fn select_program(&mut self, inputs: &ProgramInputs) -> Program;
    fn program_adjustment(&mut self, program: Program) -> u32;
    fn program_range_limit(&mut self, program: Program) -> u32;
    fn program_policy(&mut self, program: Program) -> u32;
    fn program_secondary_gate(&mut self, program: Program) -> u32;
    fn program_scale(&mut self, program: Program) -> u32;
    fn program_percent(&mut self, program: Program) -> u16;
    fn program_target(&mut self, inputs: &TargetInputs) -> ProgramTarget;
    fn program_range(&mut self, inputs: &RangeInputs) -> Range;
    fn program_lower_bound(&mut self, program: Program, lens: u32) -> u32;
    fn profile_percent(&mut self, program: Program, combined: u16, lens: ResourceIndex, bound: u32) -> u16;

    fn grid_count(&mut self, selection: u32, mode: u32, adjustment: u32, grid_kind: u32) -> u32;
    fn indexed_configuration(&mut self, selection: u32, adjustment: u32, index: u32) -> u8;
    fn level_category(&mut self, level: i16, selection: u32, adjustment: u32) -> u32;
    fn selection_limits(&mut self, selection: u32, adjustment: u32, grid_count: u32, request: u32) -> SelectionLimits;
    fn fill_grid(&mut self, inputs: &GridInputs, grid: &mut AcquisitionGrid);
    fn snapshot_grid(&mut self, statistics: Statistics, grid: &mut AcquisitionGrid);
    fn adjustment_class(&mut self, adjustment: u32, reference: i16) -> u8;
    fn adjustment_option(&mut self) -> u32;
    fn refreshed_position(&mut self) -> u32;
    fn mode_one(&mut self, inputs: &ModeInputs) -> u32;
    fn both_modes(&mut self, inputs: &ModeInputs) -> u32;
    fn five_parameter_adjustment(&mut self, selection: u32, updated_position: u32, mode_flag: u32, modes: u32, grid_word: u32) -> u32;
    fn rate_divisor(&mut self, updated_position: u32, change_category: u32, lens: ResourceIndex, grid_halfword: u16) -> u32;
    fn rate_limit(&mut self, selection: u32, updated_position: u32, position_halfword: u16) -> u32;
    fn rate_mode(&mut self, updated_position: u32, position_halfword: u16, word: u32, mode_flag: u32) -> u16;
    fn selection_mode_category(&mut self, mode_word: u32) -> u32;
    fn offset_position(&mut self, level: i16, reference: u16, flag: u32) -> u16;
    fn clamp_selection_difference(&mut self, mode: u32, lens: ResourceIndex, level: i16, lens_halfword: u16, lens_reference: u32, target: i16) -> i16;
    fn has_valid_offset(&mut self, selection: u32, mode: u32, lens_halfword: i16) -> u32;
    fn allows_unflagged_selection(&mut self, updated_position: u32, halfword: u16) -> bool;
    fn capture_selection_position(&mut self, branch: Branch, inputs: &PositionInputs) -> SelectionPosition;
    fn copy_rate_grid_source(&mut self, buffer: StatisticsBuffer);
    fn rate_pair(&mut self, record: RecordHandle, result: SharedAddress, flag: u32, pair: &mut [u32; 2]);
    fn clear_rate_configuration(&mut self, record: RecordHandle);
    fn rate_flag(&mut self, record: RecordHandle, flag: u32);
    fn draw_rate(&mut self, result: SharedAddress, flag: u32);
    fn draw_rate_status(&mut self, level: u16, word: u32, flag: u32);
    fn rate_position(&mut self, record: RecordHandle) -> (u32, u16);

    fn statistics_buffer(&mut self, buffer: StatisticsBuffer) -> [u16; 256];
    /// The extra statistics the release offsets, `values.len()` of them.
    fn extra_statistics(&mut self, values: &mut [u16]);
    fn store_release(&mut self, release: &Release<'_>);
    fn set_pipeline_reference(&mut self, level: i16);
    fn prepare_s1_sample(&mut self);
    /// The S1 sample the capture keeps for the S1 acquisition.
    fn s1_sample(&mut self) -> PipelineSnapshot;
    fn post_change_notice(&mut self, notice: &ChangeNotice);
    fn post_selection_table(&mut self, table: &SelectionTable);
    fn post_focus_table(&mut self, table: &FocusTable);

    fn shot_reference_level(&mut self) -> u16;
    fn shot_selection_word(&mut self) -> u32;
    fn prepare_selection_defaults(&mut self, record: RecordHandle) -> ShotContext;
    fn restore_selection_setting(&mut self, branch: Branch, lens: ResourceIndex);
    fn copy_reference_backup(&mut self, branch: Branch);
    fn write_saved_settings(&mut self, record: SharedAddress);
    fn copy_selection_to_saved(&mut self, record: SharedAddress);
    fn restore_selection_if_enabled(&mut self, record: RecordHandle, position: u32);
    fn apply_selection_value_change(&mut self);
    fn save_selection(&mut self, position: u32, saved: &SavedSelection, lens_position: u32);
    fn selected_grid_reference(&mut self, record: RecordHandle, reference: GridReference) -> GridReference;

    fn refresh_hardware_state(&mut self);
    fn hardware_ready(&mut self) -> bool;
    fn clock_word(&mut self) -> u32;

    /// The window names the body and tail of `grid` for calc1.
    fn store_selection_window(&mut self, branch: Branch, window: &SelectionWindow, grid: &AcquisitionGrid);
    fn store_selection_setting(&mut self, branch: Branch, setting: &SelectionSetting);
    fn store_restored_setting(&mut self, branch: Branch, setting: &RestoredSetting);
    fn setting_echo(&mut self, branch: Branch) -> SettingEcho;
    fn request_acquisition(&mut self, branch: Branch, request: &AcquisitionRequest, grid: &AcquisitionGrid);
    fn request_selection(&mut self, branch: Branch, request: &SelectionRequest);
    fn acquisition_result(&mut self, completed: Completed<'_>) -> Option<AcquisitionResult>;
    fn selection_result(&mut self, completed: Completed<'_>) -> Option<SelectionResult>;
    fn shot_record_levels(&mut self, record: SharedAddress) -> ShotRecordLevels;
    fn saved_exposure(&mut self, record: SharedAddress) -> SavedExposure;
    fn publish_saved_exposure(&mut self, record: RecordHandle, exposure: &SavedExposure);
    /// Hands calc1's records of a finished result back to calc1.
    fn release_result(&mut self, result: SharedAddress, records: Option<(SharedAddress, SharedAddress)>);
}
