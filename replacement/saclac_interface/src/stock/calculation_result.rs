#![forbid(unsafe_code)]

//! X-M1 1.50 layout of the calc 0x3c02 result: 0x640c bytes the calc task writes at the address the
//! request names (0x16ad744 for the shot's calculation block 0x16ad740). Offsets are from the
//! result's start; strides are the ones 0xf1dd84, 0xf18b94, 0xf1ba88 and 0xf1e468 index with.

use crate::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, CellSummary, FocusSummary, GridCell, ResultPass};

pub const BYTES: usize = 0x640c;

/// Number of areas per cell the result fills (word; 0xf1dddc).
pub const AREA_COUNT: usize = 0x58;
/// The halfword the whole-selection absorb retains (0xf1e86c).
pub const RETAINED_HALF: usize = 0x63e6;

const PLAIN_AREAS: usize = 0x5c;
/// Six kinds of three passes (0xf18dac).
const GRID_TRACKED_AREAS: usize = 0x3c7c;
const TRACKED_AREAS: usize = 0xd04;
const GRID_AREAS: usize = 0x1684;
const CELL_STRIDE: usize = 0xfc;
const COLUMN_STRIDE: usize = 3 * CELL_STRIDE;
const PASS_STRIDE: usize = 3 * COLUMN_STRIDE;

const PLAIN_PLAN_VALUES: usize = 0x12ec;
const GRID_PLAN_VALUES: usize = 0x4e34;
const PLAIN_PLAN_HALVES: usize = 0x1358;
const GRID_PLAN_HALVES: usize = 0x4f78;
const PLAIN_CELLS: usize = 0x1390;
const GRID_CELLS: usize = 0x501c;
const PLAIN_ROWS: usize = 0x5a54;
const GRID_ROWS: usize = 0x5cb8;
const ROW_COLUMN_STRIDE: usize = 0xcc;
const ROW_CELL_STRIDE: usize = 0x44;
const ROW_WORD: usize = 0x40;

/// Focus-summary sources in the order 0xf1ea18..0xf1ea84 stores them; `true` marks a signed
/// halfword, `false` a word.
const FOCUS_SUMMARY: [(usize, bool); 8] = [(0x4, true), (0x8, false), (0xe, true), (0x3e, true), (0x14, true), (0x3c, true), (0x38, false), (0x5954, false)];

fn cell_offset(cell: GridCell) -> usize { usize::from(cell.column) * COLUMN_STRIDE + usize::from(cell.row) * CELL_STRIDE }

fn pass_index(pass: ResultPass) -> Option<usize> {
    match pass {
        ResultPass::Plain => None,
        ResultPass::Grid(pass) => Some(usize::from(pass.index())),
    }
}

pub fn area_offset(at: AreaAt) -> usize {
    match at {
        AreaAt::Cell { pass, cell, index } => {
            let base = match pass_index(pass) {
                None => PLAIN_AREAS,
                Some(pass) => GRID_AREAS + pass * PASS_STRIDE,
            };
            base + cell_offset(cell) + usize::from(index.get()) * AreaResult::BYTES
        }
        AreaAt::Tracked { pass, kind, index } => {
            let base = match pass_index(pass) {
                None => TRACKED_AREAS + usize::from(kind.get()) * CELL_STRIDE,
                Some(pass) => GRID_TRACKED_AREAS + usize::from(kind.get()) * COLUMN_STRIDE + pass * CELL_STRIDE,
            };
            base + usize::from(index.get()) * AreaResult::BYTES
        }
    }
}

/// The plan word of an area (0xf1e0f4; grid 0xf18d40).
pub fn plan_value_offset(pass: ResultPass, cell: GridCell, index: AreaIndex) -> usize {
    let base = pass_index(pass).map_or(PLAIN_PLAN_VALUES, |pass| GRID_PLAN_VALUES + pass * 0x6c);
    base + usize::from(cell.column) * 0x24 + usize::from(cell.row) * 0xc + usize::from(index.get()) * 4
}

/// The plan halfword of an area (0xf1e138; grid 0xf18d5c).
pub fn plan_half_offset(pass: ResultPass, cell: GridCell, index: AreaIndex) -> usize {
    let base = pass_index(pass).map_or(PLAIN_PLAN_HALVES, |pass| GRID_PLAN_HALVES + pass * 0x36);
    base + usize::from(cell.column) * 0x12 + usize::from(cell.row) * 6 + usize::from(index.get()) * 2
}

/// A cell's summary (0xf1e84c; grid 0xf19634's copy source).
pub fn cell_summary_offset(pass: ResultPass, cell: GridCell) -> usize {
    let base = pass_index(pass).map_or(PLAIN_CELLS, |pass| GRID_CELLS + pass * COLUMN_STRIDE);
    base + usize::from(cell.column) * CELL_STRIDE + usize::from(cell.row) * CellSummary::BYTES
}

fn rows_base(pass: ResultPass) -> usize { pass_index(pass).map_or(PLAIN_ROWS, |pass| GRID_ROWS + pass * 0x264) }

fn row_cell_offset(pass: ResultPass, cell: GridCell) -> usize {
    rows_base(pass) + usize::from(cell.column) * ROW_COLUMN_STRIDE + usize::from(cell.row) * ROW_CELL_STRIDE
}

/// The row the distance evaluation writes for an area (0xf1e1bc; grid 0xf1e37c).
pub fn row_offset(pass: ResultPass, cell: GridCell, index: AreaIndex) -> usize {
    row_cell_offset(pass, cell) + usize::from(index.get()) * AreaRow::BYTES
}

/// The word after a cell's rows (0xf1e274; grid in 0xf18b94).
pub fn row_word_offset(pass: ResultPass, cell: GridCell) -> usize { row_cell_offset(pass, cell) + ROW_WORD }

/// `word` reads the little-endian word at a result offset.
pub fn focus_summary(mut word: impl FnMut(usize) -> u32) -> FocusSummary {
    FocusSummary(FOCUS_SUMMARY.map(|(offset, signed)| {
        let value = word(offset);
        if signed { value as u16 as i16 as i32 as u32 } else { value }
    }))
}
