#![forbid(unsafe_code)]

//! X-M1 1.50 addresses of the shot's state.

use crate::absorb::AuxiliaryOutcome;
use crate::grid_boundary::GridSetting;
use crate::state::StateAddress;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, FocusSummary, GridCell, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{ClassifyStage, SelectionList, SelectionSlot};
use xm1_saclac_interface::stock::calculation_result as result;

const fn at(address: u32) -> StateAddress { StateAddress(address) }

/// The calculation block: the bound record, then the calc 0x3c02 result, then the plan words.
pub const BLOCK: StateAddress = at(super::calculation::BLOCK);
pub const RESULT: StateAddress = at(super::calculation::RESULT);
pub const RESULT_BYTES: usize = result::BYTES;
/// The retained result 0xf1d134 saves and 0xf1d0d8 restores.
pub const RETAINED_RESULT: StateAddress = at(super::calculation::RETAINED_RESULT);
/// The working result the absorb sets aside while it exports the retained one (0xf1e580..0xf1e60c).
pub const ASIDE_RESULT: StateAddress = at(0x16a_6b94);
/// The retained result's auxiliary source word (0xf1d5e8).
pub const RETAINED_SOURCE: StateAddress = RETAINED_RESULT.offset(0x63e8);

pub const AREA_COUNT: StateAddress = RESULT.offset(result::AREA_COUNT);
pub const RETAINED_HALF: StateAddress = RESULT.offset(result::RETAINED_HALF);
pub const GRID_SELECTED: StateAddress = BLOCK.offset(0x6410);
pub const SELECTION_MODE: StateAddress = BLOCK.offset(0x6414);
pub const TRACKING: StateAddress = BLOCK.offset(0x6568);
/// The number of tracked kinds the plan names (byte; 0xf1b6fc).
pub const TRACKED_KINDS: StateAddress = BLOCK.offset(0x656c);
pub const AUXILIARY: StateAddress = BLOCK.offset(0x65f0);

const fn bank(stage: ClassifyStage) -> usize {
    match stage {
        ClassifyStage::Absorb | ClassifyStage::RecordLoad => 0,
        ClassifyStage::Round => 1,
        ClassifyStage::CaptureJob => 2,
    }
}

/// An area's subject kind in a stage's bank (0xf1c6cc..0xf1c71c); the first bank's are the plan
/// words the absorb reads (0xf18c9c).
pub const fn subject_kind(stage: ClassifyStage, index: AreaIndex) -> StateAddress {
    BLOCK.offset(0x6570 + 12 * bank(stage) + 4 * index.get() as usize)
}

/// An area's scaled level in a stage's bank, a doubleword.
pub const fn subject_level(stage: ClassifyStage, index: AreaIndex) -> StateAddress {
    BLOCK.offset(0x6598 + 24 * bank(stage) + 8 * index.get() as usize)
}

pub const fn subject_mode(stage: ClassifyStage) -> StateAddress { BLOCK.offset(0x65e0 + 4 * bank(stage)) }

pub fn retained_area(at: AreaAt) -> StateAddress { RETAINED_RESULT.offset(result::area_offset(at)) }

pub fn area(at: AreaAt) -> StateAddress { RESULT.offset(result::area_offset(at)) }

pub fn plan_value(pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress { RESULT.offset(result::plan_value_offset(pass, cell, index)) }

pub fn plan_half(pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress { RESULT.offset(result::plan_half_offset(pass, cell, index)) }

pub fn row(pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress { RESULT.offset(result::row_offset(pass, cell, index)) }

pub fn row_word(pass: ResultPass, cell: GridCell) -> StateAddress { RESULT.offset(result::row_word_offset(pass, cell)) }

pub fn cell_summary(pass: ResultPass, cell: GridCell) -> StateAddress { RESULT.offset(result::cell_summary_offset(pass, cell)) }

pub fn target(kind: TrackedKind, pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress {
    RESULT.offset(result::target_offset(kind, pass, cell, index))
}

/// The word that keeps a tracked kind's target gate open between grid updates (0xf1a104).
pub const fn target_gate(kind: TrackedKind, pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress {
    let kind = kind.get() as usize;
    let place = cell.column as usize * 0x24 + cell.row as usize * 0xc + index.get() as usize * 4;
    match pass_index(pass) {
        None => at(0x16a_690c + (kind * 0x6c + place) as u32),
        Some(pass) => at(0x16a_cfa0 + (kind * 0x144 + pass * 0x6c + place) as u32),
    }
}

pub fn focus_summary(mut word: impl FnMut(StateAddress) -> u32) -> FocusSummary { result::focus_summary(|offset| word(RESULT.offset(offset))) }

pub const fn auxiliary_outcome(word: u32) -> AuxiliaryOutcome { super::absorb::auxiliary_outcome(word) }

pub const fn auxiliary_word(outcome: AuxiliaryOutcome) -> u32 { super::absorb::auxiliary_word(outcome) }

const fn pass_index(pass: ResultPass) -> Option<usize> {
    match pass {
        ResultPass::Plain => None,
        ResultPass::Grid(pass) => Some(pass.index() as usize),
    }
}

/// An area's marker word (0xf1ad58).
pub const fn marker(pass: ResultPass, kind: Option<TrackedKind>, index: AreaIndex) -> StateAddress {
    let index = index.get() as usize;
    match (kind, pass_index(pass)) {
        (Some(kind), Some(pass)) => at(0x16b_3bd0 + (kind.get() as u32) * 0x24 + (pass * 0xc + index * 4) as u32),
        (Some(kind), None) => at(0x16b_3b64 + ((kind.get() as usize * 3 + index) * 4) as u32),
        (None, Some(pass)) => at(0x16b_3bac + ((pass * 3 + index) * 4) as u32),
        (None, None) => at(0x16b_3b58 + (index * 4) as u32),
    }
}

pub const SHOT_CONTEXT: StateAddress = at(0x169_e580);
/// 0xf160d0's word.
pub const REQUEST_WORD: StateAddress = SHOT_CONTEXT.offset(0x1cd0);
/// The selection list (0xf17830's pool word).
pub const SELECTION_LIST: StateAddress = SHOT_CONTEXT.offset(0x1cec);
pub const SELECTION_BYTES: usize = SelectionList::BYTES;
/// The list 0xf17abc and 0xf17ad8 copy.
pub const SAVED_SELECTION: StateAddress = at(0x4171_783c);
/// The list's weights, which a grid reset keeps (0xf1cf90..0xf1cfb8).
pub const SELECTION_WEIGHTS: StateAddress = SELECTION_LIST.offset(0x200);
pub const SELECTION_WEIGHT_BYTES: usize = 12;
/// 0xf17af8's word; mode 7 sets it to 1 after retaining the result.
pub const AUXILIARY_RETAINED: StateAddress = SHOT_CONTEXT.offset(0x1f18);
pub const ELIGIBILITY: StateAddress = SHOT_CONTEXT.offset(0x1f14);
pub const fn area_weight(index: AreaIndex) -> StateAddress { SHOT_CONTEXT.offset(0x1eec + 4 * index.get() as usize) }

/// The two bytes of a selection slot's cell (0xf1b050).
pub const fn cell(slot: SelectionSlot) -> StateAddress { SELECTION_LIST.offset(0x158 + 2 * slot.position()) }

/// A selection slot's class word (0xf1c324).
pub const fn class(slot: SelectionSlot) -> StateAddress { SELECTION_LIST.offset(8 + 4 * slot.position()) }

const EXPOSURE: StateAddress = at(super::calculation::EXPOSURE_CONTEXT);
pub const EXPOSURE_LENS_REFERENCE: StateAddress = EXPOSURE.offset(super::calculation::EXPOSURE_LENS_REFERENCE as usize);
/// The two words 0xf69884 reads through the exposure context's +0x24 (at +8 and +0xc).
pub const EXPOSURE_TARGET_WORD: StateAddress = EXPOSURE.offset(0x2c);
pub const EXPOSURE_FOCUS_MODE: StateAddress = EXPOSURE.offset(0x30);
pub const EXPOSURE_REFINEMENT: StateAddress = EXPOSURE.offset(0x47c);
/// The grid's column and row counts (bytes; 0xf1b844, 0xf1ba60).
pub const EXPOSURE_COLUMNS: StateAddress = EXPOSURE.offset(0x1170);
pub const EXPOSURE_ROWS: StateAddress = EXPOSURE.offset(0x1171);

pub const SAVED_INPUT: StateAddress = at(super::calculation::SAVED_INPUT);
/// 0xf19dd8 and 0xf19de4.
pub const RETAINED_TARGET_WORD: [StateAddress; 2] =
    [at(super::calculation::RETAINED_TARGET_WORD), SAVED_INPUT.offset(super::calculation::INPUT_TARGET_WORD as usize)];
/// 0xf1e878 and 0xf1e884.
pub const RETAINED_TARGET_HALF: [StateAddress; 2] =
    [at(super::calculation::RETAINED_TARGET_HALF), SAVED_INPUT.offset(super::calculation::INPUT_TARGET_HALF as usize)];

/// The configuration entry behind each setting the selection grid reads, and its reader: 0x69f8fc
/// for bytes, 0x69ffe8 for signed bytes, 0x69ff7c for halfwords.
pub const fn grid_setting(setting: GridSetting) -> (u16, SettingWidth) {
    match setting {
        GridSetting::Control => (0xa8, SettingWidth::Byte),
        GridSetting::KindCap => (0x52eb, SettingWidth::Byte),
        GridSetting::KindCapClass => (0x52ec, SettingWidth::Half),
        GridSetting::TargetGate => (0x52ee, SettingWidth::SignedByte),
        // Key table 0xf1ed80.
        GridSetting::KindThreshold { band, step } => (0x52d9 + 3 * band as u16 + step as u16, SettingWidth::Byte),
        // Key table 0xf1edc8.
        GridSetting::EligibilityThreshold { band } => (0x52ef + band as u16, SettingWidth::Byte),
        // Key table 0xf1ed98.
        GridSetting::FocusMargin { entry, step } => (0x62ae + 9 * entry as u16 + step as u16, SettingWidth::SignedByte),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingWidth { Byte, SignedByte, Half }
