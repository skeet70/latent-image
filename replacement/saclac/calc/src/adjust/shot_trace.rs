//! The shot's traces of its calculation block into the client's dump frame: the selection records
//! of the three selected areas, the pass records, the selected cell, the lens levels and the area
//! grid. A trace with no frame bound writes nothing.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::exposure_limits::{self, LimitPorts, Request, Selection, NO_INDEX};
use crate::exposure::context::Levels;
use crate::exposure::ports::{Lens, ProfileAccess, Properties, StockMemory};
use crate::exposure::settings;
use crate::stock::calculation::{self as block, BlockOffset, Cell, Pass};
use crate::stock::lists;
use crate::stock::property;
use crate::stock::shot_dump::{self as dump, SlotLayout};

/// The shot's calculation block: the bound record, the calc 0x3c02 result and the plan words.
pub trait CalculationBlock {
    fn read_u8(&mut self, at: BlockOffset) -> u8;
    fn read_u16(&mut self, at: BlockOffset) -> u16;
    fn read_u32(&mut self, at: BlockOffset) -> u32;

    fn read_i16(&mut self, at: BlockOffset) -> i16 { self.read_u16(at) as i16 }
}

/// The cells the shot selected, and the level class of a cell.
pub trait SelectedCells {
    /// The cell selected for area `index` of `pass` (0xf68c84 over the selection table).
    fn cell(&mut self, pass: Pass, index: u8) -> Cell;
    /// The level class of a cell (0xef7380).
    fn cell_level(&mut self, cell: Cell) -> u32;
}

/// Which copy of the selected cell, lens levels and area grid a trace writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceSlot {
    First,
    /// The repeated exposure's copy.
    Repeat,
}

impl TraceSlot {
    const fn layout(self) -> SlotLayout {
        match self {
            Self::First => dump::FIRST,
            Self::Repeat => dump::REPEAT,
        }
    }
}

const AREAS: u8 = 3;
/// The area kind that reads a cell's own area rather than a tracked kind's.
const CELL_KIND: u8 = 7;

fn write(memory: &mut impl StockMemory, frame: SharedAddress, offset: u32, word: u32) { memory.write_u32(dump::at(frame, offset), word); }

fn half(block: &mut impl CalculationBlock, at: BlockOffset) -> u32 { i32::from(block.read_i16(at)) as u32 }

fn base_level(lens: &mut impl Lens, reference: u32) -> u32 { i32::from(lens.base_level(reference)) as u32 }

/// The area kind of the plan's entry; with no plan every area is the cell's own.
fn plan_kind(block: &mut impl CalculationBlock, entry: u8) -> u8 {
    if block.read_u32(block::ROW_DATA) == 0 { CELL_KIND } else { block.read_u8(block::plan_kind(entry)) }
}

/// The cell area's two values the selection record carries when a plan was made: its +0xa and
/// 0xf68d84's sum.
fn planned_values(block: &mut impl CalculationBlock, pass: Pass, cell: Cell, index: u8) -> [u32; 2] {
    if block.read_u32(block::ROW_DATA) == 0 {
        return [0; 2];
    }
    let area = block::area(CELL_KIND, pass, cell, index);
    let level = block.read_i16(area.plus(block::AREA_AT_0A));
    let sum = [block::AREA_AT_0C, block::AREA_AT_0E, block::AREA_AT_10, block::AREA_AT_1A]
        .into_iter()
        .fold(0u16, |sum, field| sum.wrapping_add(block.read_u16(area.plus(field))));
    [i32::from(level) as u32, i32::from(sum as i16) as u32]
}

/// The words shared by selection and pass records: an area's levels and reference.
fn area_words(block: &mut impl CalculationBlock, lens: &mut impl Lens, area: BlockOffset) -> [u32; 8] {
    let sum = [block::AREA_AT_0E, block::AREA_AT_0C, block::AREA_AT_10].into_iter().fold(0u32, |sum, field| sum.wrapping_add(half(block, area.plus(field))));
    let reference = block.read_u32(area.plus(block::AREA_REFERENCE));
    [
        half(block, area.plus(block::AREA_AT_00)),
        sum,
        reference,
        base_level(lens, reference),
        half(block, area.plus(block::AREA_AT_0A)),
        half(block, area.plus(block::AREA_POSITION)),
        half(block, area.plus(block::AREA_AT_10)),
        block.read_u32(area.plus(block::AREA_AT_28)),
    ]
}

/// Which plan table and area halfword a selection record reads.
#[derive(Clone, Copy, Debug)]
struct RecordTables {
    first_kind_entry: u8,
    plan: fn(u8) -> BlockOffset,
    tag: BlockOffset,
}

const FIRST_TABLES: RecordTables = RecordTables { first_kind_entry: 0, plan: block::plan_first, tag: block::PLAN_FIRST_TAG };
const SECOND_TABLES: RecordTables = RecordTables { first_kind_entry: 6, plan: block::plan_second, tag: block::PLAN_SECOND_TAG };

/// A selection record of area `index` in its cell, with the area halfword at `area_half`.
fn selection_record(
    block: &mut impl CalculationBlock,
    cells: &mut impl SelectedCells,
    lens: &mut impl Lens,
    tables: RecordTables,
    pass: Pass,
    index: u8,
    area_half: impl Fn(Cell) -> BlockOffset,
) -> [u32; dump::SELECTION_WORDS] {
    let cell = cells.cell(pass, index);
    let planned = planned_values(block, Pass::Plain, cell, index);
    let kind = plan_kind(block, tables.first_kind_entry + index);
    let area = block::area(kind, Pass::Plain, cell, index);
    let row = block::row(Pass::Plain, cell, index);
    let first = half(block, block::cell_first(cell));
    let second = half(block, block::cell_second(cell));
    let own = half(block, area_half(cell));
    let mut record = [0; dump::SELECTION_WORDS];
    record[..8].copy_from_slice(&area_words(block, lens, area));
    record[8] = u32::from(block.read_u8(row.plus(block::ROW_FLAGS)));
    record[9] = half(block, area.plus(block::AREA_AT_48));
    record[10] = block.read_u32(row.plus(block::ROW_DISTANCE));
    record[11] = cells.cell_level(cell);
    record[12] = half(block, area.plus(block::AREA_AT_1A));
    record[13] = half(block, area.plus(block::AREA_AT_0A)).wrapping_add(half(block, area.plus(block::AREA_AT_18)));
    record[14] = record[12].wrapping_add(record[1]);
    record[15] = block.read_u32((tables.plan)(index));
    record[16] = planned[0];
    record[17] = planned[1];
    record[18] = half(block, area.plus(block::AREA_AT_46));
    record[19] = u32::from(kind) | u32::from(block.read_u16(tables.tag)) << 16;
    record[20] = first.wrapping_add(second).wrapping_add(own);
    record[21] = first;
    record[22] = second;
    record[23] = own;
    record
}

/// The record of an area beyond the selection's count.
const fn empty_selection() -> [u32; dump::SELECTION_WORDS] {
    let mut record = [0; dump::SELECTION_WORDS];
    record[19] = CELL_KIND as u32;
    record
}

fn write_record(memory: &mut impl StockMemory, frame: SharedAddress, at: u32, record: &[u32]) {
    for (index, word) in record.iter().enumerate() {
        write(memory, frame, at + 4 * index as u32, *word);
    }
}

fn write_levels(memory: &mut impl StockMemory, frame: SharedAddress, levels: &Levels) {
    write(memory, frame, dump::MEASURED, if levels.unmeasured { 0 } else { i32::from(levels.measured as i16) as u32 });
}

/// The selection records of the selected areas below `count`, an empty record for the rest, each
/// also copied after the three.
fn write_selections(
    ports: &mut (impl StockMemory + Lens),
    block: &mut impl CalculationBlock,
    cells: &mut impl SelectedCells,
    frame: SharedAddress,
    count: u32,
    pass: Pass,
    area_half: impl Fn(Cell, u8) -> BlockOffset,
) {
    for index in 0..AREAS {
        let record = if u32::from(index) < count {
            selection_record(block, cells, ports, FIRST_TABLES, pass, index, |cell| area_half(cell, index))
        } else {
            empty_selection()
        };
        let at = dump::SELECTION_STRIDE * u32::from(index);
        write_record(ports, frame, dump::SELECTIONS + at, &record);
        write_record(ports, frame, dump::SELECTION_COPIES + at, &record);
    }
}

fn write_plan_words(memory: &mut impl StockMemory, block: &mut impl CalculationBlock, frame: SharedAddress) {
    for (offset, at) in [(dump::ROW_DATA, block::ROW_DATA), (dump::SELECTION, block::SELECTION), (dump::MODE, block::MODE)] {
        let word = block.read_u32(at);
        write(memory, frame, offset, word);
    }
}

/// The record of grid pass `pass`'s first area in its selected cell.
fn pass_record(block: &mut impl CalculationBlock, cells: &mut impl SelectedCells, lens: &mut impl Lens, pass: u8) -> [u32; dump::PASS_WORDS] {
    let grid = Pass::Grid(pass);
    let cell = cells.cell(grid, 0);
    let planned = planned_values(block, grid, cell, 0);
    let kind = plan_kind(block, 0);
    let area = block::area(kind, grid, cell, 0);
    let rows = block::row_cell(grid, cell);
    let mut record = [0; dump::PASS_WORDS];
    record[..8].copy_from_slice(&area_words(block, lens, area));
    record[8] = u32::from(block.read_u8(rows.plus(block::ROW_FLAGS)));
    record[9] = half(block, area.plus(block::AREA_AT_48));
    record[10] = block.read_u32(rows.plus(block::ROW_DISTANCE));
    record[11] = block.read_u32(area.plus(block::AREA_AT_1C));
    record[12] = half(block, area.plus(block::AREA_AT_1A));
    record[13] = half(block, area.plus(block::AREA_AT_0A)).wrapping_add(half(block, area.plus(block::AREA_AT_18)));
    record[14] = record[12].wrapping_add(record[1]);
    record[15] = block.read_u32(block::plan_first(0));
    record[16] = planned[0];
    record[17] = planned[1];
    record[18] = half(block, area.plus(block::AREA_AT_46));
    record[19] = u32::from(kind) | u32::from(block.read_u16(block::PLAN_FIRST_TAG)) << 16;
    record[20] = block.read_u32(block::plan_value(grid, cell, 0));
    record[21] = half(block, rows.plus(block::ROW_AT_0A));
    record[22] = half(block, rows.plus(block::ROW_AT_08));
    let upper = half(block, rows.plus(block::ROW_CELL_AT_32));
    record[23] = upper.wrapping_sub(half(block, rows.plus(block::ROW_CELL_AT_30)));
    record[24] = upper;
    record
}

/// The four halfwords of each selected row below `count`, last first.
fn write_rows(memory: &mut impl StockMemory, block: &mut impl CalculationBlock, cells: &mut impl SelectedCells, frame: SharedAddress, pass: Pass, count: u32) {
    for index in (0..AREAS).take_while(|&index| u32::from(index) < count) {
        let row = block::row(Pass::Plain, cells.cell(pass, index), index);
        let at = dump::ROWS + dump::ROW_STRIDE * u32::from(index);
        for (slot, field) in [block::ROW_AT_0E, block::ROW_AT_0C, block::ROW_AT_0A, block::ROW_AT_08].into_iter().enumerate() {
            let word = half(block, row.plus(field));
            write(memory, frame, at + 4 * slot as u32, word);
        }
    }
}

/// The first selected area's level, raised by 500 less the start level.
fn write_area_level(memory: &mut impl StockMemory, block: &mut impl CalculationBlock, cells: &mut impl SelectedCells, frame: SharedAddress, pass: Pass, levels: &Levels) {
    let cell = cells.cell(pass, 0);
    let level = block.read_i16(block::area(CELL_KIND, Pass::Plain, cell, 0).plus(block::AREA_AT_00));
    let raise = 500i16.wrapping_sub(levels.start);
    write(memory, frame, dump::AREA_LEVEL, (i32::from(level) + i32::from(raise)) as u32);
}

/// The selection records of the three selected areas in the mode's pass, the plan words, the
/// pass records and the selected rows (0xf6da20).
pub fn trace_selection(
    ports: &mut (impl StockMemory + Lens),
    block: &mut impl CalculationBlock,
    cells: &mut impl SelectedCells,
    frame: Option<SharedAddress>,
    count: u32,
    levels: &Levels,
) {
    let Some(frame) = frame else { return };
    let pass = Pass::of_word(u32::from(block.read_u8(block::MODE)));
    let mode = block.read_u32(block::MODE);
    write_levels(ports, frame, levels);
    write(ports, frame, dump::START, i32::from(levels.start) as u32);
    write_selections(ports, block, cells, frame, count, pass, |cell, index| block::area_half_of_mode(mode, cell, index));
    write_plan_words(ports, block, frame);
    for pass in 0..AREAS {
        let record = pass_record(block, cells, ports, pass);
        write_record(ports, frame, dump::PASSES + dump::PASS_STRIDE * u32::from(pass), &record);
    }
    write_rows(ports, block, cells, frame, pass, count);
    write_area_level(ports, block, cells, frame, pass, levels);
    let kind = plan_kind(block, 0);
    let word = block.read_u32(block::pass_word(kind, pass, 0));
    write(ports, frame, dump::PASS_WORD, word);
}

/// The selection records of the selected areas below `count` from the second plan table, written
/// over the copies (0xf6e540).
pub fn trace_second_selection(
    ports: &mut (impl StockMemory + Lens),
    block: &mut impl CalculationBlock,
    cells: &mut impl SelectedCells,
    frame: Option<SharedAddress>,
    count: u32,
    levels: &Levels,
) {
    let Some(frame) = frame else { return };
    write_levels(ports, frame, levels);
    let pass = if block.read_u32(block::SELECTION) != 0 { Pass::of_word(u32::from(block.read_u8(block::MODE))) } else { Pass::Plain };
    for index in (0..AREAS).take_while(|&index| u32::from(index) < count) {
        let record = selection_record(block, cells, ports, SECOND_TABLES, pass, index, |cell| block::area_half(pass, cell, index));
        write_record(ports, frame, dump::SELECTION_COPIES + dump::SELECTION_STRIDE * u32::from(index), &record);
    }
}

/// What the plain selection's trace records beside the block.
#[derive(Clone, Copy, Debug)]
pub struct PlainTrace<'a> {
    pub frame: Option<SharedAddress>,
    /// How many areas the selection holds.
    pub count: u32,
    pub levels: &'a Levels,
    pub record: RecordHandle,
    pub lens_reference: u32,
    /// Leaves the four area grids out.
    pub skip_grids: bool,
}

/// The target of the record's unindexed limits for the levels' curve, with no request level.
fn limit_target(ports: &mut (impl LimitPorts + ProfileAccess), record: RecordHandle, levels: &Levels, lens_reference: u32) -> i16 {
    let mut profile = ports.profile();
    let request = Request {
        selection: Selection {
            scene: u32::from(profile.header(record, SharedHeaderByte::At31)),
            flags: profile.read(record, &group_00::AT_002E),
            alternate: profile.read::<u8, _>(record, &group_00::AT_004B) != 0,
            variant: profile.read::<u8, _>(record, &group_00::AT_0066) != 0,
            unmeasured: levels.unmeasured,
            threshold: Selection::threshold_of(profile.read(record, &group_00::AT_0020)),
            auxiliary: profile.header(record, SharedHeaderByte::At46) != 0,
        },
        class: u32::from(profile.read::<u8, _>(record, &group_00::AT_0032)),
        reference: lens_reference,
        policy: levels.policy,
        armed: profile.header(record, SharedHeaderByte::At63) != 0,
        choice: u32::from(profile.read::<u8, _>(record, &group_00::AT_003E)),
        tracking: profile.read(record, &group_00::INTERVAL_TRACKING).0 != 0,
        bank: 0,
        cell: Cell::default(),
        index: NO_INDEX,
        level: i16::MAX,
        profile: u32::from(profile.read::<u8, _>(record, &group_00::AT_0044)),
        fixed_upper: false,
        capture_profile: levels.curve,
        minimum: profile.read(record, &group_00::AT_0024),
    };
    exposure_limits::limits(ports, &request).target
}

/// The record of a pass the plain selection does not trace.
const fn empty_pass() -> [u32; dump::PASS_WORDS] {
    let mut record = [0; dump::PASS_WORDS];
    record[19] = CELL_KIND as u32;
    record
}

/// Each lens step's compensated level at `lens_reference`, the rest of the words zero.
fn write_step_levels(ports: &mut (impl StockMemory + LimitPorts), frame: SharedAddress, lens_reference: u32) {
    let steps = ports.steps();
    for step in 0..steps {
        let level = ports.compensated_level(step, lens_reference);
        write(ports, frame, dump::STEP_LEVELS + 4 * step, i32::from(level) as u32);
    }
    for step in steps..dump::STEP_LEVEL_WORDS {
        write(ports, frame, dump::STEP_LEVELS + 4 * step, 0);
    }
}

/// The plain pass's selection with its limit target, the lens steps' compensated levels and,
/// unless skipped, the plain and grid passes' area grids (0xf6c8fc through 0xf60294).
pub fn trace_plain_selection(
    ports: &mut (impl StockMemory + Lens + LimitPorts + ProfileAccess),
    block: &mut impl CalculationBlock,
    cells: &mut impl SelectedCells,
    trace: &PlainTrace,
) {
    let Some(frame) = trace.frame else { return };
    let levels = trace.levels;
    let target = limit_target(ports, trace.record, levels, trace.lens_reference);
    write(ports, frame, dump::LIMIT, i32::from(target) as u32);
    write_levels(ports, frame, levels);
    write(ports, frame, dump::START, i32::from(levels.start) as u32);
    write_selections(ports, block, cells, frame, trace.count, Pass::Plain, |cell, index| block::area_half(Pass::Plain, cell, index));
    write_plan_words(ports, block, frame);
    for pass in 0..AREAS {
        write_record(ports, frame, dump::PASSES + dump::PASS_STRIDE * u32::from(pass), &empty_pass());
    }
    write_rows(ports, block, cells, frame, Pass::Plain, trace.count);
    for index in trace.count..u32::from(AREAS) {
        write_record(ports, frame, dump::ROWS + dump::ROW_STRIDE * index, &[0; 4]);
    }
    write_step_levels(ports, frame, trace.lens_reference);
    write_area_level(ports, block, cells, frame, Pass::Plain, levels);
    let word = ports.read_u32(lists::CORRECTION_WORD);
    write(ports, frame, dump::LIST_WORD, word);
    if !trace.skip_grids {
        for pass in [Pass::Plain, Pass::Grid(0), Pass::Grid(1), Pass::Grid(2)] {
            write_grid(ports, block, frame, dump::FIRST, pass);
        }
    }
    let kind = plan_kind(block, 0);
    let word = block.read_u32(block::pass_word(kind, Pass::Plain, 0));
    write(ports, frame, dump::PASS_WORD, word);
    let setting = ports.property(property::DUMP_SETTING);
    write(ports, frame, dump::SETTING, setting);
}

/// The selected cell's summary, under the lens levels of `slot` (0xf6d870 for the plain pass,
/// 0xf6e484 for the mode's pass into the first slot).
pub fn trace_selected_cell(
    ports: &mut (impl StockMemory + Lens),
    block: &mut impl CalculationBlock,
    cells: &mut impl SelectedCells,
    frame: Option<SharedAddress>,
    slot: TraceSlot,
    pass: Pass,
) {
    let Some(frame) = frame else { return };
    let cell = cells.cell(pass, 0);
    let summary = block::cell_summary(cell);
    let reference = block.read_u32(summary.plus(block::AREA_REFERENCE));
    let words = [
        half(block, block::RETAINED_HALF),
        reference,
        base_level(ports, reference),
        half(block, summary.plus(block::AREA_AT_0A)),
        half(block, summary.plus(block::AREA_AT_3A)),
        half(block, summary.plus(block::AREA_AT_10)),
        half(block, summary.plus(block::AREA_AT_1A)),
        u32::from(block.read_u16(summary.plus(block::AREA_AT_3E))),
    ];
    write_record(ports, frame, slot.layout().levels + 0x1c, &words);
}

/// The pass the mode word names, as 0xf6e484 reads it.
pub fn mode_pass(block: &mut impl CalculationBlock) -> Pass { Pass::of_word(block.read_u32(block::MODE)) }

/// The result's lens levels; the repeat slot also gets the lens level at `lens_reference` less the
/// result's corrections (0xf6d934).
pub fn trace_lens_levels(
    ports: &mut (impl StockMemory + Lens + ProfileAccess + Properties),
    block: &mut impl CalculationBlock,
    record: RecordHandle,
    frame: Option<SharedAddress>,
    slot: TraceSlot,
    lens_reference: u32,
) {
    let Some(frame) = frame else { return };
    let base = slot.layout().levels;
    let lens = block.read_u32(block::LENS);
    let first = match slot {
        TraceSlot::Repeat => {
            let correction = if ports.profile().header(record, SharedHeaderByte::At63) != 0 {
                settings::level_step(ports, 3)
            } else {
                block.read_i16(block::AT_5A8E)
            };
            let index = ports.calibration_index(lens);
            let level = i32::from(ports.level(index, lens_reference));
            let corrected = [block::AT_40, block::AT_42, block::AT_18, block::AT_22]
                .into_iter()
                .fold(level + i32::from(block.read_i16(block::AT_12)), |level, at| level - i32::from(block.read_i16(at)));
            (corrected + i32::from(correction)) as u32
        }
        TraceSlot::First => half(block, block::AT_63E8),
    };
    let index = ports.calibration_index(lens);
    let words = [
        first,
        index,
        base_level(ports, index),
        half(block, block::AT_12),
        half(block, block::AT_42),
        half(block, block::AT_18),
        half(block, block::AT_22),
    ];
    write_record(ports, frame, base, &words);
}

/// The plain grid's nine cells' halfwords and rows, then its areas (0xf6e91c into the repeat
/// slot).
pub fn trace_area_grid(memory: &mut impl StockMemory, block: &mut impl CalculationBlock, frame: Option<SharedAddress>, slot: TraceSlot) {
    let Some(frame) = frame else { return };
    write_grid(memory, block, frame, slot.layout(), Pass::Plain);
}

const GRID_CELLS: [Cell; 9] = {
    let mut cells = [Cell { column: 0, row: 0 }; 9];
    let mut index = 0;
    while index < 9 {
        cells[index] = Cell { column: (index / 3) as u8, row: (index % 3) as u8 };
        index += 1;
    }
    cells
};

/// One pass's nine cells, then their areas; a grid pass's follow the plain grid's in the slot.
fn write_grid(memory: &mut impl StockMemory, block: &mut impl CalculationBlock, frame: SharedAddress, layout: SlotLayout, pass: Pass) {
    let grid = match pass {
        Pass::Plain => 0,
        Pass::Grid(pass) => 1 + u32::from(pass),
    };
    let cells = layout.cells + 9 * dump::CELL_BYTES * grid;
    let areas = layout.areas + 9 * dump::AREA_BYTES as u32 * grid;
    for cell in GRID_CELLS {
        let rows = block::row_cell(pass, cell);
        let halves = [
            block.read_u16(block::cell_first(cell)),
            block.read_u16(block::cell_second(cell)),
            block.read_u16(block::area_half(pass, cell, 0)),
            u16::from(block.read_u8(rows.plus(block::ROW_FLAGS))),
            block.read_u16(rows.plus(block::ROW_CELL_AT_30)),
            block.read_u16(rows.plus(block::ROW_CELL_AT_32)),
            block.read_u16(rows.plus(0x34)),
            block.read_u16(rows.plus(0x36)),
            block.read_u32(rows.plus(block::ROW_CELL_WORD)) as u16,
        ];
        let at = cells + 0x3c * u32::from(cell.column) + dump::CELL_BYTES * u32::from(cell.row);
        for (slot, value) in halves.into_iter().enumerate() {
            memory.write_u16(dump::at(frame, at + 2 * slot as u32), value);
        }
    }
    for cell in GRID_CELLS {
        for index in 0..AREAS {
            let area = block::area(CELL_KIND, pass, cell, index);
            let at = areas + 0x20 * (3 * u32::from(cell.column) + u32::from(cell.row)) + layout.area_index_stride * u32::from(index);
            write_area_halves(memory, block, frame, at, area, block::plan_value(pass, cell, index));
        }
    }
}

/// An area's fifteen halfwords in the grid; word fields keep their low half.
fn write_area_halves(memory: &mut impl StockMemory, block: &mut impl CalculationBlock, frame: SharedAddress, at: u32, area: BlockOffset, plan: BlockOffset) {
    const HALVES: [(u32, bool); 14] = [
        (block::AREA_AT_00, false),
        (block::AREA_REFERENCE, true),
        (block::AREA_POSITION, false),
        (block::AREA_AT_0A, false),
        (block::AREA_AT_0C, false),
        (block::AREA_AT_0E, false),
        (block::AREA_AT_10, false),
        (block::AREA_AT_28, true),
        (block::AREA_AT_16, false),
        (block::AREA_AT_1A, false),
        (block::AREA_AT_18, false),
        (block::AREA_AT_20, true),
        (block::AREA_AT_48, false),
        (block::AREA_AT_50, true),
    ];
    for (slot, (field, word)) in HALVES.into_iter().enumerate() {
        let value = if word { block.read_u32(area.plus(field)) as u16 } else { block.read_u16(area.plus(field)) };
        memory.write_u16(dump::at(frame, at + 2 * slot as u32), value);
    }
    let value = block.read_u32(plan) as u16;
    memory.write_u16(dump::at(frame, at + 2 * HALVES.len() as u32), value);
}

/// Clears a slot's cells, lens levels and area grid (0xf6eb34).
pub fn clear_slot(memory: &mut impl StockMemory, frame: Option<SharedAddress>, slot: TraceSlot) {
    let Some(frame) = frame else { return };
    let layout = slot.layout();
    clear(memory, frame, layout.cells, 9 * dump::CELL_BYTES as usize);
    clear(memory, frame, layout.levels, dump::LEVEL_BYTES);
    for cell in 0..9 {
        for index in 0..u32::from(AREAS) {
            clear(memory, frame, layout.areas + 0x20 * cell + layout.area_index_stride * index, dump::AREA_BYTES);
        }
    }
}

fn clear(memory: &mut impl StockMemory, frame: SharedAddress, at: u32, bytes: usize) {
    for offset in (0..bytes as u32).step_by(4) {
        write(memory, frame, at + offset, 0);
    }
}
