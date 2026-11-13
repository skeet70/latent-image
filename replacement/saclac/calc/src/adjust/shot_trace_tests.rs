use std::collections::BTreeMap;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::shot_trace::*;
use crate::exposure::context::Levels;
use crate::exposure::fakes::{self, World};
use crate::stock::calculation::{BlockOffset, Cell, Pass};

const FRAME: SharedAddress = SharedAddress(0x4000_0000);

#[derive(Default)]
struct Block {
    bytes: BTreeMap<u32, u8>,
}

impl Block {
    fn put(&mut self, at: u32, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.bytes.insert(at + index as u32, *byte);
        }
    }

    fn put_u16(&mut self, at: u32, value: u16) { self.put(at, &value.to_le_bytes()); }

    fn put_u32(&mut self, at: u32, value: u32) { self.put(at, &value.to_le_bytes()); }
}

impl CalculationBlock for Block {
    fn read_u8(&mut self, at: BlockOffset) -> u8 { self.bytes.get(&at.get()).copied().unwrap_or(0) }
    fn read_u16(&mut self, at: BlockOffset) -> u16 { u16::from_le_bytes([self.read_u8(at), self.read_u8(at.plus(1))]) }
    fn read_u32(&mut self, at: BlockOffset) -> u32 { u32::from(self.read_u16(at)) | u32::from(self.read_u16(at.plus(2))) << 16 }
}

/// Selects cell (1, 2) for every index of the plain pass and cell (2, 0) for every grid pass.
struct Cells;

const PLAIN_CELL: Cell = Cell { column: 1, row: 2 };
const GRID_CELL: Cell = Cell { column: 2, row: 0 };

impl SelectedCells for Cells {
    fn cell(&mut self, pass: Pass, _index: u8) -> Cell {
        match pass {
            Pass::Plain => PLAIN_CELL,
            Pass::Grid(_) => GRID_CELL,
        }
    }

    fn cell_level(&mut self, cell: Cell) -> u32 { u32::from(cell.column + cell.row) + 1 }
}

fn frame_word(world: &World, offset: u32) -> u32 { world.u32_at(SharedAddress(FRAME.0 + offset)) }

/// The plain area of `PLAIN_CELL` at `index`, from the result at block +4.
fn plain_area(index: u32) -> u32 { 4 + 0x5c + 0x2f4 + 2 * 0xfc + 0x54 * index }

#[test]
fn no_frame_no_stores() {
    let mut world = World::default();
    let mut block = Block::default();
    trace_selection(&mut world, &mut block, &mut Cells, None, 3, &Levels::default());
    let levels = Levels::default();
    trace_plain_selection(&mut world, &mut block, &mut Cells, &PlainTrace { frame: None, count: 3, levels: &levels, record: fakes::handle(), lens_reference: 0, skip_grids: false });
    trace_selected_cell(&mut world, &mut block, &mut Cells, None, TraceSlot::First, Pass::Plain);
    clear_slot(&mut world, None, TraceSlot::Repeat);
    assert!(world.stores.is_empty());
}

#[test]
fn the_selected_cell_summary_follows_the_lens_levels() {
    let mut world = World::default();
    world.lens_base_levels.insert(0x77, -12);
    let mut block = Block::default();
    let summary = 4 + 0x1390 + 0xfc + 2 * 0x54;
    block.put_u32(summary + 4, 0x77);
    block.put_u16(summary + 0xa, (-3i16) as u16);
    block.put_u16(summary + 0x3e, 0xfffe);
    block.put_u16(4 + 0x63e6, 41);
    trace_selected_cell(&mut world, &mut block, &mut Cells, Some(FRAME), TraceSlot::Repeat, Pass::Plain);
    let words: std::vec::Vec<u32> = (0..8).map(|index| frame_word(&world, 0x11660 + 0x1c + 4 * index)).collect();
    assert_eq!(words, [41, 0x77, -12i32 as u32, -3i32 as u32, 0, 0, 0, 0xfffe]);
}

#[test]
fn selected_areas_below_the_count_get_records_and_the_rest_empty_ones() {
    let mut world = World::default();
    world.lens_base_levels.insert(0x55, 7);
    let mut block = Block::default();
    block.put_u32(0x6414, 4);
    block.put_u32(0x6568, 1);
    block.put(0x6570, &[3]);
    block.put_u16(0x65e0, 0x0102);
    for index in 0..3 {
        let tracked = 4 + 0xd04 + 3 * 0xfc + 0x54 * index;
        block.put_u32(tracked + 4, 0x55);
        block.put_u16(tracked + 0xc, 10);
        block.put_u16(tracked + 0xe, 20);
        block.put_u16(tracked + 0x10, 30);
    }
    trace_selection(&mut world, &mut block, &mut Cells, Some(FRAME), 1, &Levels::default());
    let record = |base: u32| -> std::vec::Vec<u32> { (0..24).map(|index| frame_word(&world, base + 4 * index)).collect() };
    let first = record(0xf8c8);
    assert_eq!(first[1], 60);
    assert_eq!(first[2], 0x55);
    assert_eq!(first[3], 7);
    assert_eq!(first[6], 30);
    assert_eq!(first[11], 4);
    assert_eq!(first[19], 3 | 0x0102 << 16);
    assert_eq!(record(0xf9e8), first);
    let mut empty = [0u32; 24];
    empty[19] = 7;
    assert_eq!(record(0xf8c8 + 0x60), empty);
    assert_eq!(record(0xf9e8 + 0xc0), empty);
}

#[test]
fn without_a_plan_the_cell_area_is_traced() {
    let mut world = World::default();
    let mut block = Block::default();
    block.put_u32(0x6414, 4);
    block.put_u16(plain_area(0), 900);
    block.put_u16(plain_area(0) + 0x48, (-4i16) as u16);
    trace_selection(&mut world, &mut block, &mut Cells, Some(FRAME), 3, &Levels { start: 450, ..Levels::default() });
    assert_eq!(frame_word(&world, 0xf8c8), 900);
    assert_eq!(frame_word(&world, 0xf8c8 + 0x24), -4i32 as u32);
    assert_eq!(frame_word(&world, 0xf8c8 + 0x4c), 7);
    assert_eq!(frame_word(&world, 0xf88c), 950);
    assert_eq!(frame_word(&world, 0xf8c4), 450);
}

#[test]
fn the_repeat_slot_takes_the_lens_level_less_the_results_corrections() {
    let mut world = World::default();
    world.calibration_indices.insert(0x10, 4);
    world.lens_levels.insert((4, 0x99), 200);
    let mut block = Block::default();
    block.put_u32(0xc, 0x10);
    block.put_u16(0x12, 30);
    block.put_u16(0x40, 5);
    block.put_u16(0x42, 6);
    block.put_u16(0x18, 7);
    block.put_u16(0x22, 8);
    block.put_u16(0x5a8e, (-2i16) as u16);
    trace_lens_levels(&mut world, &mut block, fakes::handle(), Some(FRAME), TraceSlot::Repeat, 0x99);
    assert_eq!(frame_word(&world, 0x11660), (200 + 30 - 5 - 6 - 7 - 8 - 2) as u32);
    assert_eq!(frame_word(&world, 0x11664), 4);
    assert_eq!(frame_word(&world, 0x11670), 6);
}

#[test]
fn an_armed_record_corrects_by_the_third_level_step() {
    let mut world = World::default();
    world.header(SharedHeaderByte::At63, 1);
    world.property_word(crate::stock::property::level_step(3), 100);
    let mut block = Block::default();
    trace_lens_levels(&mut world, &mut block, fakes::handle(), Some(FRAME), TraceSlot::Repeat, 0);
    assert_eq!(frame_word(&world, 0x11660), 100);
}

#[test]
fn the_area_grid_keeps_each_cells_halfwords_and_areas() {
    let mut world = World::default();
    let mut block = Block::default();
    block.put_u16(4 + 0x5958 + 2 * 5, 0x1111);
    block.put(4 + 0x5a54 + 0xcc + 2 * 0x44, &[9]);
    block.put_u32(plain_area(2) + 4, 0x0003_0004);
    block.put_u32(4 + 0x12ec + 0x24 + 2 * 0xc + 8, 0x0001_0002);
    trace_area_grid(&mut world, &mut block, Some(FRAME), TraceSlot::Repeat);
    let cell = 0x111e4 + 0x3c + 2 * 20;
    assert_eq!(world.u16_at(SharedAddress(FRAME.0 + cell)), 0x1111);
    assert_eq!(world.u16_at(SharedAddress(FRAME.0 + cell + 6)), 9);
    let area = 0x11298 + 0x20 * 5 + 0x120 * 2;
    assert_eq!(world.u16_at(SharedAddress(FRAME.0 + area + 2)), 4);
    assert_eq!(world.u16_at(SharedAddress(FRAME.0 + area + 28)), 2);
}

#[test]
fn clearing_a_slot_zeroes_its_levels_cells_and_areas() {
    let mut world = World::default();
    clear_slot(&mut world, Some(FRAME), TraceSlot::First);
    let cleared: std::collections::BTreeSet<u32> = world.stores.iter().map(|&(at, _, _)| at - FRAME.0).collect();
    assert!(cleared.contains(&0xe7d4) && cleared.contains(&(0xe7d4 + 0xb0)));
    assert!(cleared.contains(&0xfc40) && cleared.contains(&(0xfc40 + 0x38)));
    assert!(cleared.contains(&(0xeaa4 + 0x20 * 8 + 0x480 * 2 + 0x1c)));
    assert!(world.stores.iter().all(|&(_, _, value)| value == 0));
}

/// The index in `world.stores` of the first store in `[from, to)` of the frame.
fn first_store(world: &World, from: u32, to: u32) -> usize {
    world.stores.iter().position(|&(at, _, _)| (FRAME.0 + from..FRAME.0 + to).contains(&at)).unwrap()
}

/// The index in `world.stores` of the last store in `[from, to)` of the frame.
fn last_store(world: &World, from: u32, to: u32) -> usize {
    world.stores.iter().rposition(|&(at, _, _)| (FRAME.0 + from..FRAME.0 + to).contains(&at)).unwrap()
}

fn stored_at(world: &World, offset: u32) -> bool { world.stores.iter().any(|&(at, _, _)| at == FRAME.0 + offset) }

#[test]
fn the_area_grid_writes_its_nine_cells_before_its_areas() {
    let mut world = World::default();
    trace_area_grid(&mut world, &mut Block::default(), Some(FRAME), TraceSlot::Repeat);
    assert!(last_store(&world, 0x111e4, 0x111e4 + 9 * 20) < first_store(&world, 0x11298, 0x11298 + 3 * 0x120));
}

/// Scene 0's limits with a target of 30: the target offset is -30, under the first parameter
/// set's target ceiling of 50 and upper level of 200, and lens steps 2 to 9 capped at 200.
fn limit_world() -> World {
    let mut world = World::default();
    for limit in [0, u16::MAX] {
        world.near_steps.insert(limit, 2);
        world.far_steps.insert(limit, 9);
    }
    world.property(crate::exposure::ports::Property(0x61e1), 0xfd);
    world.property(crate::exposure::ports::Property(0x6125 + 11), 5);
    world.property(crate::exposure::ports::Property(0x6125 + 4), 20);
    world.property(crate::exposure::ports::Property(0x61c9), 20);
    world
}

fn plain_trace<'a>(levels: &'a Levels, count: u32, skip_grids: bool) -> PlainTrace<'a> {
    PlainTrace { frame: Some(FRAME), count, levels, record: fakes::handle(), lens_reference: 0x42, skip_grids }
}

#[test]
fn the_plain_trace_records_its_limit_target_levels_and_lens_step_levels() {
    let mut world = limit_world();
    world.steps = 3;
    for (step, level) in [(0, 11), (1, -5), (2, 30)] {
        world.compensated_levels.insert((step, 0x42), level);
    }
    world.put_u16(crate::stock::lists::CORRECTION_WORD, 0x1234);
    world.property(crate::stock::property::DUMP_SETTING, 9);
    let levels = Levels { start: 450, measured: 300, ..Levels::default() };
    trace_plain_selection(&mut world, &mut Block::default(), &mut Cells, &plain_trace(&levels, 1, false));
    assert_eq!([0xf8bc, 0xf8c0, 0xf8c4].map(|offset| frame_word(&world, offset)), [30, 300, 450]);
    assert_eq!([0xfcac, 0xfcb0, 0xfcb4].map(|offset| frame_word(&world, offset)), [11, -5i32 as u32, 30]);
    assert!(stored_at(&world, 0xfcac + 4 * 30) && !stored_at(&world, 0xfcac + 4 * 31));
    assert_eq!(frame_word(&world, 0xe7d0), 0x1234);
    assert_eq!(frame_word(&world, 0x116a4), 9);
    assert_eq!([0, 1, 2].map(|pass| frame_word(&world, 0xfb14 + 100 * pass + 0x4c)), [7; 3]);
}

#[test]
fn the_plain_trace_zeroes_the_rows_past_its_count() {
    let mut world = limit_world();
    let mut block = Block::default();
    block.put_u16(4 + 0x5a54 + 0xcc + 2 * 0x44 + 0xe, 0x55);
    let levels = Levels::default();
    trace_plain_selection(&mut world, &mut block, &mut Cells, &plain_trace(&levels, 1, true));
    assert_eq!(frame_word(&world, 0xfc7c), 0x55);
    assert!((0xfc7c + 0x10..0xfc7c + 0x30).step_by(4).all(|offset| stored_at(&world, offset) && frame_word(&world, offset) == 0));
}

#[test]
fn the_plain_trace_follows_the_plain_grid_with_each_grid_pass() {
    let mut world = limit_world();
    let mut block = Block::default();
    block.put(4 + 0x5cb8 + 0x264 + 0xcc * 2, &[6]);
    block.put_u16(4 + 0x1684 + 0x8dc * 2 + 0xfc + 0x54, 77);
    let levels = Levels::default();
    trace_plain_selection(&mut world, &mut block, &mut Cells, &plain_trace(&levels, 3, false));
    assert_eq!(world.u16_at(SharedAddress(FRAME.0 + 0xe7d4 + 0xb4 * 2 + 0x3c * 2 + 6)), 6);
    assert_eq!(world.u16_at(SharedAddress(FRAME.0 + 0xeaa4 + 0x120 * 3 + 0x20 + 0x480)), 77);
    let cells = |grid: u32| (0xe7d4 + 0xb4 * grid, 0xe7d4 + 0xb4 * (grid + 1));
    let areas = |grid: u32| (0xeaa4 + 0x120 * grid, 0xeaa4 + 0x120 * (grid + 1));
    let phases: std::vec::Vec<(usize, usize)> =
        (0..4).flat_map(|grid| [cells(grid), areas(grid)]).map(|(from, to)| (first_store(&world, from, to), last_store(&world, from, to))).collect();
    assert!(phases.windows(2).all(|pair| pair[0].1 < pair[1].0), "{phases:?}");
}

#[test]
fn a_skipped_grid_leaves_the_grids_and_still_records_the_pass_word() {
    let mut world = limit_world();
    let mut block = Block::default();
    block.put_u32(0x6418, 0xabcd);
    let levels = Levels::default();
    trace_plain_selection(&mut world, &mut block, &mut Cells, &plain_trace(&levels, 3, true));
    assert!(!world.stores.iter().any(|&(at, _, _)| (FRAME.0 + 0xe7d4..FRAME.0 + 0xf824).contains(&at)));
    assert_eq!(frame_word(&world, 0x116a0), 0xabcd);
}
