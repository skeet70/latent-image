//! Offsets from the client's dump frame base of the shot traces: the selection records, the pass
//! records, the selected cell, the lens levels and the area grids.

use xm1_saclac_interface::calc::SharedAddress;

pub const fn at(frame: SharedAddress, offset: u32) -> SharedAddress { SharedAddress(frame.0 + offset) }

/// The limit word 0xf6c8fc stores.
pub const LIMIT: u32 = 0xf8bc;
/// The measured level (0 when unmeasured) and the start level.
pub const MEASURED: u32 = 0xf8c0;
pub const START: u32 = 0xf8c4;

pub const SELECTION_WORDS: usize = 24;
pub const SELECTION_STRIDE: u32 = 0x60;
/// Three selection records, and the copies after them.
pub const SELECTIONS: u32 = 0xf8c8;
pub const SELECTION_COPIES: u32 = 0xf9e8;

pub const ROW_DATA: u32 = 0xfb08;
pub const SELECTION: u32 = 0xfb0c;
pub const MODE: u32 = 0xfb10;

pub const PASS_WORDS: usize = 25;
pub const PASS_STRIDE: u32 = 100;
/// Three pass records.
pub const PASSES: u32 = 0xfb14;

/// Four halfwords of each selected row, last first.
pub const ROWS: u32 = 0xfc7c;
pub const ROW_STRIDE: u32 = 0x10;

/// The first area's level beside the start level.
pub const AREA_LEVEL: u32 = 0xf88c;
/// The selection lists' correction word (`stock::lists::CORRECTION_WORD`).
pub const LIST_WORD: u32 = 0xe7d0;
/// One compensated level per lens step, the rest of the 31 words zero.
pub const STEP_LEVELS: u32 = 0xfcac;
pub const STEP_LEVEL_WORDS: u32 = 31;
pub const PASS_WORD: u32 = 0x116a0;
/// The configuration value 0xf6c8fc copies (`property::DUMP_SETTING`).
pub const SETTING: u32 = 0x116a4;

/// Where a trace slot's selected cell, lens levels and area grid are.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotLayout {
    /// Fifteen words: the lens levels from +0, the selected cell from +0x1c.
    pub levels: u32,
    /// Nine cells of twenty bytes, by column then row; the first slot follows them with a grid
    /// pass's nine after another.
    pub cells: u32,
    /// Thirty-two bytes per area, by column then row, then index; within an index the first
    /// slot follows the plain areas with each grid pass's nine.
    pub areas: u32,
    pub area_index_stride: u32,
}

pub const FIRST: SlotLayout = SlotLayout { levels: 0xfc40, cells: 0xe7d4, areas: 0xeaa4, area_index_stride: 0x480 };
pub const REPEAT: SlotLayout = SlotLayout { levels: 0x11660, cells: 0x111e4, areas: 0x11298, area_index_stride: 0x120 };

pub const LEVEL_BYTES: usize = 0x3c;
pub const CELL_BYTES: u32 = 20;
pub const AREA_BYTES: usize = 0x20;
