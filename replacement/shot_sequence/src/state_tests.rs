use crate::absorb::{AuxiliaryOutcome, CalculationResults};
use crate::state::{ShotState, StateAddress, StateMemory};
use crate::stock::state as layout;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, CellSummary, GridCell, GridPass, ResultPass, TrackedKind};

/// Sparse memory; unwritten bytes read as zero.
#[derive(Default)]
pub(crate) struct Memory(pub BTreeMap<u32, u8>);

impl StateMemory for Memory {
    fn read(&mut self, at: StateAddress, into: &mut [u8]) {
        for (offset, byte) in into.iter_mut().enumerate() { *byte = self.0.get(&(at.get() + offset as u32)).copied().unwrap_or(0); }
    }
    fn write(&mut self, at: StateAddress, bytes: &[u8]) {
        for (offset, byte) in bytes.iter().enumerate() { self.0.insert(at.get() + offset as u32, *byte); }
    }
    fn copy(&mut self, to: StateAddress, from: StateAddress, bytes: usize) {
        for offset in 0..bytes as u32 {
            let byte = self.0.get(&(from.get() + offset)).copied().unwrap_or(0);
            self.0.insert(to.get() + offset, byte);
        }
    }
}

fn state() -> ShotState<Memory> { ShotState::new(Memory::default()) }

fn filled<const N: usize>(seed: u8) -> [u8; N] { core::array::from_fn(|index| seed.wrapping_add(index as u8)) }

fn index(value: u8) -> AreaIndex { AreaIndex::new(value).unwrap() }

fn cell(column: u8, row: u8) -> GridCell { GridCell::new(column, row).unwrap() }

#[test]
fn adopting_a_grid_area_makes_the_plain_area_read_the_pass() {
    let mut state = state();
    let pass = GridPass::new(1).unwrap();
    let kind = TrackedKind::new(3).unwrap();
    let at = |pass, index| AreaAt::Cell { pass, cell: cell(2, 1), index };
    let tracked = |pass| AreaAt::Tracked { pass, kind, index: index(2) };
    state.memory().write(layout::area(at(ResultPass::Grid(pass), index(2))), &filled::<84>(10));
    state.memory().write(layout::area(tracked(ResultPass::Grid(pass))), &filled::<84>(90));

    state.adopt_grid_area(pass, cell(2, 1), index(2), None);
    assert_eq!(state.result_area(at(ResultPass::Plain, index(2))), AreaResult::from_bytes(filled(10)));
    assert_eq!(state.result_area(tracked(ResultPass::Plain)), AreaResult::default(), "an untracked area leaves the kinds alone");
    assert_eq!(state.result_area(at(ResultPass::Plain, index(1))), AreaResult::default());

    state.adopt_grid_area(pass, cell(2, 1), index(2), Some(kind));
    assert_eq!(state.result_area(tracked(ResultPass::Plain)), AreaResult::from_bytes(filled(90)));
}

#[test]
fn adopting_grid_tables_and_cells_copies_the_pass_values() {
    let mut state = state();
    let pass = GridPass::new(2).unwrap();
    let grid = ResultPass::Grid(pass);
    state.memory().write(layout::plan_value(grid, cell(1, 1), index(0)), &0x1234_5678u32.to_le_bytes());
    state.memory().write(layout::plan_half(grid, cell(1, 1), index(0)), &(-9i16).to_le_bytes());
    state.memory().write(layout::row(grid, cell(1, 1), index(0)), &filled::<16>(40));
    state.memory().write(layout::cell_summary(grid, cell(1, 1)), &filled::<84>(7));

    state.adopt_grid_tables(pass, cell(1, 1), index(0));
    state.adopt_grid_cell(pass, cell(1, 1));
    assert_eq!(state.plan_value(ResultPass::Plain, cell(1, 1), index(0)), 0x1234_5678);
    assert_eq!(state.plan_half(ResultPass::Plain, cell(1, 1), index(0)), -9);
    assert_eq!(state.row(ResultPass::Plain, cell(1, 1), index(0)), AreaRow(filled(40)));
    assert_eq!(state.cell_summary(ResultPass::Plain, cell(1, 1)), CellSummary::from_bytes(filled(7)));
    assert_eq!(state.plan_value(ResultPass::Plain, cell(1, 1), index(1)), 0);
}

#[test]
fn the_retained_and_set_aside_results_round_trip_the_whole_result() {
    let mut state = state();
    let first = AreaAt::Cell { pass: ResultPass::Plain, cell: cell(0, 0), index: index(0) };
    let last = AreaAt::Tracked { pass: ResultPass::Grid(GridPass::new(2).unwrap()), kind: TrackedKind::new(5).unwrap(), index: index(2) };
    state.memory().write(layout::area(first), &filled::<84>(1));
    state.memory().write(layout::area(last), &filled::<84>(2));
    state.retain_result();
    state.memory().write(layout::area(first), &[0; 84]);
    state.set_working_aside();
    state.memory().write(layout::area(last), &[0; 84]);
    state.restore_result();
    assert_eq!(state.result_area(first), AreaResult::from_bytes(filled(1)));
    assert_eq!(state.result_area(last), AreaResult::from_bytes(filled(2)));
    state.restore_working();
    assert_eq!(state.result_area(first), AreaResult::default(), "the working result set aside had the first area cleared");
    assert_eq!(state.result_area(last), AreaResult::from_bytes(filled(2)));
}

#[test]
fn every_kind_and_pass_keeps_its_own_cells_and_markers() {
    let mut state = state();
    let kinds = [None].into_iter().chain((0..6).map(TrackedKind::new));
    let passes = [ResultPass::Plain].into_iter().chain(GridPass::ALL.map(ResultPass::Grid));
    let mut places = Vec::new();
    for kind in kinds {
        for pass in passes.clone() {
            for slot in 0..3 { places.push((kind, pass, index(slot))); }
        }
    }
    for (number, (kind, pass, index)) in places.iter().enumerate() {
        let number = number as u8;
        state.set_cell_of(*kind, *pass, *index, cell(number % 3, number / 3 % 3));
        state.memory().write(layout::marker(*pass, *kind, *index), &u32::from(number).to_le_bytes());
    }
    for (number, (kind, pass, index)) in places.iter().enumerate() {
        let number = number as u8;
        assert_eq!(state.cell_of(*kind, *pass, *index), cell(number % 3, number / 3 % 3), "{kind:?} {pass:?} {index:?}");
        assert_eq!(state.marker(*pass, *kind, *index), u32::from(number), "{kind:?} {pass:?} {index:?}");
    }
}

#[test]
fn retained_targets_reach_the_block_and_the_saved_input() {
    let mut state = state();
    state.retain_target_word(0xdead_beef);
    state.retain_target_half(0x4242);
    for at in layout::RETAINED_TARGET_WORD {
        let mut word = [0; 4];
        state.memory().read(at, &mut word);
        assert_eq!(u32::from_le_bytes(word), 0xdead_beef);
    }
    for at in layout::RETAINED_TARGET_HALF {
        let mut half = [0; 2];
        state.memory().read(at, &mut half);
        assert_eq!(u16::from_le_bytes(half), 0x4242);
    }
}

#[test]
fn the_auxiliary_outcome_round_trips() {
    let mut state = state();
    for outcome in [AuxiliaryOutcome::Kept, AuxiliaryOutcome::Replaced, AuxiliaryOutcome::Unchecked] {
        state.set_auxiliary(outcome);
        assert_eq!(state.auxiliary(), outcome);
    }
}

fn word_at(state: &mut ShotState<Memory>, at: StateAddress) -> u32 {
    let mut word = [0; 4];
    state.memory().read(at, &mut word);
    u32::from_le_bytes(word)
}

#[test]
fn mode_7_carries_the_previous_value_and_others_clear_it() {
    let mut state = state();
    state.memory().write(layout::CARRIED, &5u32.to_le_bytes());
    state.memory().write(layout::AUXILIARY_RETAINED, &3u32.to_le_bytes());
    state.carry_words(Some(7));
    let words = |state: &mut ShotState<Memory>| [layout::CARRIED, layout::ELIGIBILITY, layout::AUXILIARY_RETAINED].map(|at| word_at(state, at));
    assert_eq!(words(&mut state), [0, 5, 3]);
    state.memory().write(layout::CARRIED, &4u32.to_le_bytes());
    state.carry_words(Some(5));
    assert_eq!(words(&mut state), [0, 0, 0]);
}

#[test]
fn the_lens_track_starts_unread_and_marks_a_reading_off_the_bound_reference() {
    use crate::dump_boundary::ShotWord;
    use crate::preparation::{LensReadings, LensStage};
    for (bound, moved) in [(40u32, 1), (41, 0)] {
        let mut state = state();
        state.memory().write(layout::REQUEST_WORD, &bound.to_le_bytes());
        state.record_lens(LensStage::Conclusion, 9, true);
        state.begin_lens_track(41);
        let words = [ShotWord::StartReference, ShotWord::SecondReference, ShotWord::ThirdReference, ShotWord::Moved, ShotWord::MovedLate].map(|word| state.shot_word(word));
        assert_eq!(words, [41, 0xff, 0xff, moved, 0], "bound {bound}");
    }
}

/// The word the half-press reply path reads through 0xf16184 (shot context +0x1ce0).
pub(crate) fn reply_lens_notice(state: &mut ShotState<Memory>) -> u32 { word_at(state, StateAddress(0x16a_0260)) }

#[test]
fn the_lens_notice_the_half_press_writes_is_what_the_reply_path_reads() {
    use crate::preparation::LensReadings;
    let mut state = state();
    LensReadings::set_lens_notice(&mut state, true);
    assert_eq!(reply_lens_notice(&mut state), 1);
    LensReadings::set_lens_notice(&mut state, false);
    assert_eq!(reply_lens_notice(&mut state), 0);
}
