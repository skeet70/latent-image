//! Offsets in the shot's calculation block (0x16ad740): the bound record word, the calc 0x3c02
//! result from +4, the selection words from +0x6410 and the plan words from +0x6568, as the
//! exposure traces read them. `AT_` names are offsets whose meaning no reader has shown yet.

/// A byte offset into the calculation block.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BlockOffset(pub(crate) u32);

impl BlockOffset {
    pub const fn get(self) -> u32 { self.0 }

    pub const fn plus(self, bytes: u32) -> Self { Self(self.0 + bytes) }
}

/// A grid cell as the shot's selection table names it: column, then row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cell {
    pub column: u8,
    pub row: u8,
}

/// The area set an offset is in: the plain areas or one of the three grid passes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pass {
    Plain,
    Grid(u8),
}

impl Pass {
    /// Pass words below 3 name a grid pass; any other is the plain set.
    pub const fn of_word(word: u32) -> Self { if word < 3 { Self::Grid(word as u8) } else { Self::Plain } }
}

const RESULT: u32 = 4;

/// The selection word (+0x6410) and mode word (+0x6414).
pub const SELECTION: BlockOffset = BlockOffset(0x6410);
pub const MODE: BlockOffset = BlockOffset(0x6414);
/// The plan's row-data word; 0 when no plan was made.
pub const ROW_DATA: BlockOffset = BlockOffset(0x6568);

/// The plan's area kind for entry `entry` (one byte of each of the nine grid words from +0x6570).
pub const fn plan_kind(entry: u8) -> BlockOffset { BlockOffset(0x6570 + 4 * entry as u32) }

/// The plan word of index `index` in the first and second plan tables.
pub const fn plan_first(index: u8) -> BlockOffset { BlockOffset(0x6598 + 8 * index as u32) }
pub const fn plan_second(index: u8) -> BlockOffset { BlockOffset(0x65c8 + 8 * index as u32) }
/// Halfwords the traces pack above the kind.
pub const PLAN_FIRST_TAG: BlockOffset = BlockOffset(0x65e0);
pub const PLAN_SECOND_TAG: BlockOffset = BlockOffset(0x65e8);

/// The halfword the shot retains from the result (result +0x63e6).
pub const RETAINED_HALF: BlockOffset = BlockOffset(RESULT + 0x63e6);
/// The halfword beside it (result +0x63e4).
pub const AT_63E8: BlockOffset = BlockOffset(RESULT + 0x63e4);
/// The result's lens word (result +8).
pub const LENS: BlockOffset = BlockOffset(RESULT + 0x8);
/// Signed halfwords of the result's header.
pub const AT_12: BlockOffset = BlockOffset(RESULT + 0xe);
pub const AT_18: BlockOffset = BlockOffset(RESULT + 0x14);
pub const AT_22: BlockOffset = BlockOffset(RESULT + 0x1e);
pub const AT_40: BlockOffset = BlockOffset(RESULT + 0x3c);
pub const AT_42: BlockOffset = BlockOffset(RESULT + 0x3e);
pub const AT_5A8E: BlockOffset = BlockOffset(RESULT + 0x5a8a);

const fn cell_index(cell: Cell) -> u32 { 3 * cell.column as u32 + cell.row as u32 }

/// An area of the result, as 0xf68bb0 finds it: kinds below 6 are tracked kinds with their own
/// triple; any other kind names the cell's area, column and row clamped to 2.
pub const fn area(kind: u8, pass: Pass, cell: Cell, index: u8) -> BlockOffset {
    let index = 0x54 * index as u32;
    if kind <= 5 {
        let kind = kind as u32;
        match pass {
            Pass::Grid(pass) => BlockOffset(RESULT + 0x3c7c + 0x2f4 * kind + 0xfc * pass as u32 + index),
            Pass::Plain => BlockOffset(RESULT + 0xd04 + 0xfc * kind + index),
        }
    } else {
        let column = if cell.column > 2 { 2 } else { cell.column as u32 };
        let row = if cell.row > 2 { 2 } else { cell.row as u32 };
        let cell = 0x2f4 * column + 0xfc * row + index;
        match pass {
            Pass::Grid(pass) => BlockOffset(RESULT + 0x1684 + 0x8dc * pass as u32 + cell),
            Pass::Plain => BlockOffset(RESULT + 0x5c + cell),
        }
    }
}

/// Fields of an area or a cell summary.
pub const AREA_AT_00: u32 = 0x00;
pub const AREA_REFERENCE: u32 = 0x04;
pub const AREA_POSITION: u32 = 0x08;
pub const AREA_AT_0A: u32 = 0x0a;
pub const AREA_AT_0C: u32 = 0x0c;
pub const AREA_AT_0E: u32 = 0x0e;
pub const AREA_AT_10: u32 = 0x10;
pub const AREA_AT_16: u32 = 0x16;
pub const AREA_AT_18: u32 = 0x18;
pub const AREA_AT_1A: u32 = 0x1a;
pub const AREA_AT_1C: u32 = 0x1c;
pub const AREA_AT_20: u32 = 0x20;
pub const AREA_AT_28: u32 = 0x28;
pub const AREA_AT_3A: u32 = 0x3a;
pub const AREA_AT_3E: u32 = 0x3e;
pub const AREA_AT_46: u32 = 0x46;
pub const AREA_AT_48: u32 = 0x48;
pub const AREA_AT_50: u32 = 0x50;

/// A plain cell's summary (result +0x1390).
pub const fn cell_summary(cell: Cell) -> BlockOffset { BlockOffset(RESULT + 0x1390 + 0xfc * cell.column as u32 + 0x54 * cell.row as u32) }

/// The rows of a cell (result +0x5a54 plain, +0x5cb8 per grid pass): three 16-byte rows, four
/// halfwords at +0x30 and the row word at +0x40.
pub const fn row_cell(pass: Pass, cell: Cell) -> BlockOffset {
    let cell = 0xcc * cell.column as u32 + 0x44 * cell.row as u32;
    match pass {
        Pass::Plain => BlockOffset(RESULT + 0x5a54 + cell),
        Pass::Grid(pass) => BlockOffset(RESULT + 0x5cb8 + 0x264 * pass as u32 + cell),
    }
}

pub const fn row(pass: Pass, cell: Cell, index: u8) -> BlockOffset { row_cell(pass, cell).plus(0x10 * index as u32) }

pub const ROW_FLAGS: u32 = 0x0;
pub const ROW_DISTANCE: u32 = 0x4;
pub const ROW_AT_08: u32 = 0x8;
pub const ROW_AT_0A: u32 = 0xa;
pub const ROW_AT_0C: u32 = 0xc;
pub const ROW_AT_0E: u32 = 0xe;
pub const ROW_CELL_AT_30: u32 = 0x30;
pub const ROW_CELL_AT_32: u32 = 0x32;
pub const ROW_CELL_WORD: u32 = 0x40;

/// An area's plan word (result +0x12ec plain, +0x4e34 per grid pass).
pub const fn plan_value(pass: Pass, cell: Cell, index: u8) -> BlockOffset {
    let at = 0x24 * cell.column as u32 + 0xc * cell.row as u32 + 4 * index as u32;
    match pass {
        Pass::Plain => BlockOffset(RESULT + 0x12ec + at),
        Pass::Grid(pass) => BlockOffset(RESULT + 0x4e34 + 0x6c * pass as u32 + at),
    }
}

/// Two halfwords per cell (result +0x5958 and +0x596a).
pub const fn cell_first(cell: Cell) -> BlockOffset { BlockOffset(RESULT + 0x5958 + 2 * cell_index(cell)) }
pub const fn cell_second(cell: Cell) -> BlockOffset { BlockOffset(RESULT + 0x596a + 2 * cell_index(cell)) }

const fn area_half_at(cell: Cell, index: u8) -> u32 { 2 * (9 * cell.column as u32 + 3 * cell.row as u32 + index as u32) }

/// One halfword per area (result +0x597c plain, +0x59b2 per grid pass).
pub const fn area_half(pass: Pass, cell: Cell, index: u8) -> BlockOffset {
    match pass {
        Pass::Plain => BlockOffset(RESULT + 0x597c + area_half_at(cell, index)),
        Pass::Grid(pass) => area_half_of_mode(pass as u32, cell, index),
    }
}

/// The grid pass halfword at the mode word, unchecked as 0xf6de10 indexes it.
pub const fn area_half_of_mode(mode: u32, cell: Cell, index: u8) -> BlockOffset {
    BlockOffset((RESULT + 0x59b2).wrapping_add(mode.wrapping_mul(0x36)).wrapping_add(area_half_at(cell, index)))
}

/// The selection words 0xf68cfc reads (+0x6418..+0x6568).
pub const fn pass_word(kind: u8, pass: Pass, index: u8) -> BlockOffset {
    let index = 4 * index as u32;
    if kind <= 5 {
        let kind = kind as u32;
        match pass {
            Pass::Grid(pass) => BlockOffset(0x6490 + 36 * kind + 12 * pass as u32 + index),
            Pass::Plain => BlockOffset(0x6424 + 12 * kind + index),
        }
    } else {
        match pass {
            Pass::Grid(pass) => BlockOffset(0x646c + 12 * pass as u32 + index),
            Pass::Plain => BlockOffset(0x6418 + index),
        }
    }
}
