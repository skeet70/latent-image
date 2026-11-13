#![forbid(unsafe_code)]

//! X-M1 1.50 addresses of the shot's state.

use crate::absorb::AuxiliaryOutcome;
use crate::state::StateAddress;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, FocusSummary, GridCell, ResultPass, TrackedKind};
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

pub const AREA_COUNT: StateAddress = RESULT.offset(result::AREA_COUNT);
pub const RETAINED_HALF: StateAddress = RESULT.offset(result::RETAINED_HALF);
pub const GRID_SELECTED: StateAddress = BLOCK.offset(0x6410);
pub const SELECTION_MODE: StateAddress = BLOCK.offset(0x6414);
pub const TRACKING: StateAddress = BLOCK.offset(0x6568);
pub const AUXILIARY: StateAddress = BLOCK.offset(0x65f0);
/// An area's tracked kind, the low byte of a plan word (0xf18c9c).
pub const fn area_kind(index: AreaIndex) -> StateAddress { BLOCK.offset(0x6570 + 4 * index.get() as usize) }

pub fn area(at: AreaAt) -> StateAddress { RESULT.offset(result::area_offset(at)) }

pub fn plan_value(pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress { RESULT.offset(result::plan_value_offset(pass, cell, index)) }

pub fn plan_half(pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress { RESULT.offset(result::plan_half_offset(pass, cell, index)) }

pub fn row(pass: ResultPass, cell: GridCell, index: AreaIndex) -> StateAddress { RESULT.offset(result::row_offset(pass, cell, index)) }

pub fn row_word(pass: ResultPass, cell: GridCell) -> StateAddress { RESULT.offset(result::row_word_offset(pass, cell)) }

pub fn cell_summary(pass: ResultPass, cell: GridCell) -> StateAddress { RESULT.offset(result::cell_summary_offset(pass, cell)) }

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

/// The two bytes (column, row) of an area's grid cell (0xf1b050).
pub const fn cell(kind: Option<TrackedKind>, pass: ResultPass, index: AreaIndex) -> StateAddress {
    let index = index.get() as usize;
    match (kind, pass_index(pass)) {
        (Some(kind), Some(pass)) => at(0x16a_0400 + (kind.get() as usize * 0x12 + pass * 6 + index * 2) as u32),
        (Some(kind), None) => at(0x16a_03ca + ((kind.get() as usize * 3 + index) * 2) as u32),
        (None, Some(pass)) => at(0x16a_03ee + ((pass * 3 + index) * 2) as u32),
        (None, None) => at(0x16a_03c4 + (index * 2) as u32),
    }
}

pub const SHOT_CONTEXT: StateAddress = at(0x169_e580);
/// 0xf17af8's word; mode 7 sets it to 1 after retaining the result.
pub const AUXILIARY_RETAINED: StateAddress = SHOT_CONTEXT.offset(0x1f18);
pub const ELIGIBILITY: StateAddress = SHOT_CONTEXT.offset(0x1f14);
pub const fn area_weight(index: AreaIndex) -> StateAddress { SHOT_CONTEXT.offset(0x1eec + 4 * index.get() as usize) }

const SAVED_INPUT: StateAddress = at(super::calculation::SAVED_INPUT);
/// 0xf19dd8 and 0xf19de4.
pub const RETAINED_TARGET_WORD: [StateAddress; 2] =
    [at(super::calculation::RETAINED_TARGET_WORD), SAVED_INPUT.offset(super::calculation::INPUT_TARGET_WORD as usize)];
/// 0xf1e878 and 0xf1e884.
pub const RETAINED_TARGET_HALF: [StateAddress; 2] =
    [at(super::calculation::RETAINED_TARGET_HALF), SAVED_INPUT.offset(super::calculation::INPUT_TARGET_HALF as usize)];
