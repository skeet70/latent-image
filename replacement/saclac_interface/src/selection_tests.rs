use crate::calculation::{AreaIndex, GridCell, GridPass, ResultPass, TrackedKind};
use crate::selection::{CellCode, SelectionClass, SelectionList, SelectionSlot};

/// The list's address in the shot context (0xf17830's pool word).
const LIST: usize = 0x16a_026c;

fn index(index: u8) -> AreaIndex { AreaIndex::new(index).unwrap() }

fn slots() -> impl Iterator<Item = SelectionSlot> {
    let kinds = core::iter::once(None).chain((0..6).map(|kind| TrackedKind::new(kind)));
    kinds.flat_map(|kind| {
        let passes = core::iter::once(ResultPass::Plain).chain(GridPass::ALL.into_iter().map(ResultPass::Grid));
        passes.flat_map(move |pass| (0..3).map(move |area| SelectionSlot { kind, pass, index: index(area) }))
    })
}

#[test]
fn every_slot_has_its_own_class_and_cell() {
    let mut seen = [false; SelectionSlot::COUNT];
    for slot in slots() {
        let position = slot.position();
        assert!(!seen[position], "{slot:?}");
        seen[position] = true;
    }
    assert!(seen.iter().all(|seen| *seen));
    let mut list = SelectionList::default();
    for (seed, slot) in slots().enumerate() {
        list.set_class(slot, SelectionClass(seed as u32 + 100));
        list.set_cell(slot, CellCode { column: seed as u8, row: !(seed as u8) });
    }
    for (seed, slot) in slots().enumerate() {
        assert_eq!(list.class(slot), SelectionClass(seed as u32 + 100));
        assert_eq!(list.cell(slot), CellCode { column: seed as u8, row: !(seed as u8) });
    }
}

#[test]
fn cells_and_classes_sit_where_the_grid_helpers_address_them() {
    let mut list = SelectionList::default();
    let plain = SelectionSlot::plain(index(1));
    let tracked = SelectionSlot { kind: TrackedKind::new(0), pass: ResultPass::Plain, index: index(0) };
    let pass = SelectionSlot { kind: None, pass: ResultPass::Grid(GridPass::new(0).unwrap()), index: index(0) };
    let tracked_pass = SelectionSlot { kind: TrackedKind::new(0), pass: ResultPass::Grid(GridPass::new(0).unwrap()), index: index(0) };
    list.set_cell(plain, CellCode { column: 1, row: 2 });
    list.set_cell(tracked, CellCode { column: 3, row: 4 });
    list.set_cell(pass, CellCode { column: 5, row: 6 });
    list.set_cell(tracked_pass, CellCode { column: 7, row: 8 });
    list.set_class(pass, SelectionClass(0x0102_0304));
    let byte = |address: usize| list.bytes()[address - LIST];
    assert_eq!([byte(0x16a_03c6), byte(0x16a_03c7)], [1, 2]);
    assert_eq!([byte(0x16a_03ca), byte(0x16a_03cb)], [3, 4]);
    assert_eq!([byte(0x16a_03ee), byte(0x16a_03ef)], [5, 6]);
    assert_eq!([byte(0x16a_0400), byte(0x16a_0401)], [7, 8]);
    assert_eq!(byte(0x16a_02c8), 4);
}

#[test]
fn the_head_weights_and_tail_keep_their_places() {
    let mut list = SelectionList::default();
    list.set_fixed(true);
    list.set_area_count(3);
    list.set_weight(index(2), 0x55);
    list.set_exposure_half(4, 0xbeef);
    list.set_upper_code(9);
    assert!(list.fixed());
    assert_eq!(list.bytes()[..8], [1, 0, 0, 0, 3, 0, 0, 0]);
    assert_eq!(list.bytes()[0x16a_0474 - LIST], 0x55);
    assert_eq!(list.weight(index(2)), 0x55);
    assert_eq!(list.exposure_half(4), 0xbeef);
    assert_eq!(list.bytes()[0x16a_0480 - LIST..0x16a_0482 - LIST], [0xef, 0xbe]);
    assert_eq!(list.upper_code(), 9);
    assert_eq!(list.bytes()[SelectionList::BYTES - 2], 9);
    list.set_fixed(false);
    assert!(!list.fixed());
}

#[test]
fn a_cell_code_past_the_grid_addresses_its_last_column_or_row() {
    assert_eq!(CellCode { column: 0, row: 4 }.cell(), GridCell::new(0, 2).unwrap());
    assert_eq!(CellCode { column: 3, row: 1 }.cell(), GridCell::new(2, 1).unwrap());
    assert_eq!(CellCode { column: 2, row: 2 }.cell(), GridCell::new(2, 2).unwrap());
}
