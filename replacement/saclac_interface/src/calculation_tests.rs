use crate::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, GridCell, GridPass, ReferenceLimits, ReferenceRequest, ResultPass, TrackedKind};
use crate::stock::calculation_result as layout;

/// The result's address in the shot's calculation block (0xf1e464).
const RESULT: usize = 0x16a_d744;

fn request() -> ReferenceRequest {
    let mut bytes = [0u8; ReferenceRequest::BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = index as u8;
    }
    ReferenceRequest::from_bytes(bytes)
}

#[test]
fn a_shift_moves_the_ten_positions_and_nothing_else() {
    let before = request();
    let mut after = before;
    after.shift_positions(-3);
    assert_eq!(after.position(), before.position().wrapping_sub(3));
    for (moved, original) in after.area_positions().iter().zip(before.area_positions()) {
        assert_eq!(*moved, original.wrapping_sub(3));
    }
    for index in (0..ReferenceRequest::BYTES).filter(|index| !(0x38..0x3a).contains(index) && !(0x104..0x116).contains(index)) {
        assert_eq!(after.bytes()[index], before.bytes()[index], "byte {index:#x}");
    }
}

#[test]
fn limits_round_trip_and_keep_the_references() {
    let mut block = request();
    let limits = ReferenceLimits { first: -2, second: 0x1234, option: 7 };
    let reference = block.reference();
    block.set_limits(limits);
    assert_eq!(block.limits(), limits);
    assert_eq!(block.reference(), reference);
    assert_eq!(&block.bytes()[0x80..0x82], &(-2i16).to_le_bytes());
    assert_eq!(&block.bytes()[0x8c..0x90], &7u32.to_le_bytes());
}

fn cell(column: u8, row: u8) -> GridCell { GridCell::new(column, row).unwrap() }

fn index(index: u8) -> AreaIndex { AreaIndex::new(index).unwrap() }

fn grid(pass: u32) -> ResultPass { ResultPass::Grid(GridPass::new(pass).unwrap()) }

#[test]
fn areas_sit_where_the_absorb_reads_them() {
    let plain = layout::area_offset(AreaAt::Cell { pass: ResultPass::Plain, cell: cell(0, 0), index: index(0) });
    assert_eq!(RESULT + plain, 0x16a_d7a0);
    let tracked = layout::area_offset(AreaAt::Tracked { pass: ResultPass::Plain, kind: TrackedKind::new(0).unwrap(), index: index(0) });
    assert_eq!(RESULT + tracked, 0x16a_e448);
    let tracked_grid = layout::area_offset(AreaAt::Tracked { pass: grid(1), kind: TrackedKind::new(2).unwrap(), index: index(0) });
    assert_eq!(RESULT + tracked_grid, 0x16b_13c0 + 2 * 0x2f4 + 0xfc);
    let last_tracked = layout::area_offset(AreaAt::Tracked { pass: grid(2), kind: TrackedKind::new(5).unwrap(), index: index(2) });
    assert_eq!(RESULT + last_tracked + AreaResult::BYTES, RESULT + layout::plan_value_offset(grid(0), cell(0, 0), index(0)));
    let first_grid = layout::area_offset(AreaAt::Cell { pass: grid(0), cell: cell(0, 0), index: index(0) });
    assert_eq!(RESULT + first_grid, 0x16a_edc8);
    let last = layout::area_offset(AreaAt::Cell { pass: grid(2), cell: cell(2, 2), index: index(2) });
    assert!(last + AreaResult::BYTES <= layout::BYTES);
    let next_cell = layout::area_offset(AreaAt::Cell { pass: ResultPass::Plain, cell: cell(0, 1), index: index(0) });
    assert_eq!(next_cell - plain, 3 * AreaResult::BYTES);
}

#[test]
fn cell_tables_sit_where_the_absorb_reads_them() {
    assert_eq!(RESULT + layout::plan_value_offset(ResultPass::Plain, cell(0, 0), index(0)), 0x16a_ea30);
    assert_eq!(RESULT + layout::cell_summary_offset(ResultPass::Plain, cell(0, 0)), 0x16a_ead4);
    assert_eq!(RESULT + layout::cell_summary_offset(grid(0), cell(0, 0)), 0x16b_2760);
    assert_eq!(RESULT + layout::row_offset(ResultPass::Plain, cell(0, 0), index(0)), 0x16b_3198);
    assert_eq!(RESULT + layout::row_word_offset(ResultPass::Plain, cell(0, 0)), 0x16b_31d8);
    assert_eq!(RESULT + layout::row_offset(grid(0), cell(0, 0), index(0)), 0x16b_33fc);
    assert_eq!(RESULT + layout::row_word_offset(grid(0), cell(0, 0)), 0x16b_343c);
    let rows_end = layout::row_word_offset(grid(2), cell(2, 2)) + 4;
    assert!(rows_end <= layout::BYTES);
    assert_eq!(layout::row_offset(ResultPass::Plain, cell(0, 0), index(1)) - layout::row_offset(ResultPass::Plain, cell(0, 0), index(0)), AreaRow::BYTES);
}

#[test]
fn the_focus_summary_sign_extends_its_halfwords() {
    let summary = layout::focus_summary(|offset| 0xffff_0000 | offset as u32 | 0x8000);
    assert_eq!(summary.0[0], 0xffff_8004);
    assert_eq!(summary.0[1], 0xffff_8008);
    assert_eq!(summary.classified(), (0x8000 | 0x3c) as u16 as i16);
}

#[test]
fn an_auxiliary_reference_replaces_the_reference_and_position_only() {
    let mut area = AreaResult::from_bytes(core::array::from_fn(|byte| byte as u8));
    let before = area;
    area.set_reference(0x1234_5678, -2);
    assert_eq!(area.reference(), 0x1234_5678);
    assert_eq!(area.position(), -2);
    for byte in (0..AreaResult::BYTES).filter(|byte| !(4..10).contains(byte)) {
        assert_eq!(area.bytes()[byte], before.bytes()[byte], "byte {byte}");
    }
}
