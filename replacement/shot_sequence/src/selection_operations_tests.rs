use crate::grid_fakes::{area, cell, grid, index, kind, Layers};
use crate::selection_operations::{retain_reference, SelectionOperations};
use crate::state::{ShotState, StateAddress, StateMemory};
use crate::state_tests::Memory;
use crate::stock::state as layout;
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, ReferenceRequest, ResultPass};
use xm1_saclac_interface::selection::{CellCode, ResultBuffer, RetainedReference, ShotSelection};
use xm1_saclac_interface::stock::calculation_result::area_offset;

const BUFFER: u32 = 0x2000_0000;

fn at_buffer(at: AreaAt) -> StateAddress { layout::result_buffer(ResultBuffer(BUFFER + area_offset(at) as u32)) }

fn operations(memory: Memory, layers: Layers) -> SelectionOperations<Memory, Layers> { SelectionOperations::new(memory, layers) }

#[test]
fn a_reference_request_is_the_saved_input_moved_to_the_new_reference() {
    let mut saved = ReferenceRequest::from_bytes([0; ReferenceRequest::BYTES]);
    saved.set_reference(0x111);
    saved.shift_positions(10);
    let mut memory = Memory::default();
    memory.write(layout::SAVED_INPUT, saved.bytes());
    let mut layers = Layers::default();
    layers.offsets.insert(0x111, 5);
    layers.offsets.insert(0x222, 2);
    let request = operations(memory, layers).reference_request(0x222);
    assert_eq!((request.reference(), request.reference_copy()), (0x222, 0x222));
    assert_eq!(request.position(), 13);
    assert_eq!(request.area_positions(), [13; 9]);
}

#[test]
fn a_retained_buffer_comes_back_into_the_working_result() {
    let at = AreaAt::Cell { pass: grid(1), cell: cell(2, 1), index: index(2) };
    let mut memory = Memory::default();
    memory.write(at_buffer(at), area(0x77, &[(AreaHalf::Level, 42)]).bytes());
    let mut operations = operations(memory, Layers::default());
    operations.retain_result(ResultBuffer(BUFFER));
    operations.restore_result();
    let restored = operations.area(None, grid(1), CellCode { column: 2, row: 1 }, index(2));
    assert_eq!((restored.reference(), restored.half(AreaHalf::Level)), (0x77, 42));
}

#[test]
fn a_retained_reference_replaces_only_an_untracked_area_and_always_keeps_the_source() {
    let untracked = AreaAt::Cell { pass: ResultPass::Plain, cell: cell(1, 1), index: index(0) };
    let other = AreaAt::Cell { pass: ResultPass::Plain, cell: cell(1, 1), index: index(1) };
    let mut state = ShotState::new(Memory::default());
    for place in [untracked, other] { state.set_area(place, &area(0x10, &[(AreaHalf::Level, 5)])); }
    state.retain_result();
    retain_reference(RetainedReference { source: 3, reference: 0x99, position: -4, untracked: Some(untracked) }, &mut state);
    retain_reference(RetainedReference { source: 6, reference: 0x55, position: 8, untracked: None }, &mut state);
    assert_eq!(state.retained_source(), 6);
    let replaced = state.retained_area(untracked);
    assert_eq!((replaced.reference(), replaced.position(), replaced.half(AreaHalf::Level)), (0x99, -4, 5));
    assert_eq!(state.retained_area(other).reference(), 0x10);
}

#[test]
fn a_tracked_kind_area_is_read_whatever_the_cell() {
    let at = AreaAt::Tracked { pass: grid(0), kind: kind(4), index: index(1) };
    let mut memory = Memory::default();
    memory.write(layout::area(at), area(0x31, &[]).bytes());
    let mut operations = operations(memory, Layers::default());
    assert_eq!(operations.area(Some(kind(4)), grid(0), CellCode { column: 9, row: 9 }, index(1)).reference(), 0x31);
}

#[test]
fn the_block_reads_report_the_selection_words_and_the_retained_targets() {
    let mut memory = Memory::default();
    memory.write(layout::GRID_SELECTED, &1u32.to_le_bytes());
    memory.write(layout::SELECTION_MODE, &4u32.to_le_bytes());
    memory.write(layout::TRACKING, &1u32.to_le_bytes());
    memory.write(layout::RETAINED_TARGET_WORD[0], &0xdead_beefu32.to_le_bytes());
    memory.write(layout::RETAINED_TARGET_HALF[0], &0x4242u16.to_le_bytes());
    let mut operations = operations(memory, Layers::default());
    assert!(operations.grid_selected());
    assert_eq!(operations.selection_mode(), 4);
    assert!(operations.tracking());
    assert_eq!(operations.retained_targets(), (0xdead_beef, 0x4242));
}
