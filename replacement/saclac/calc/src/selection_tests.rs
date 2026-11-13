use crate::selection::*;

struct Exact(bool);

impl SelectionSettings for Exact {
    fn exact_range(&mut self) -> bool { self.0 }
}

fn row(enabled: bool, phase: RowPhase) -> Row { Row { enabled, phase } }

fn bank(levels: &[u8], phases: &[RowPhase]) -> SelectionBank {
    let mut bank = SelectionBank { count: levels.len() as u8, ..SelectionBank::default() };
    bank.levels[..levels.len()].copy_from_slice(levels);
    for (index, phase) in phases.iter().enumerate() { bank.rows[index] = row(true, *phase); }
    bank
}

#[test]
fn the_open_row_is_the_first_idle_row_if_it_is_enabled() {
    use RowPhase::*;
    let mut selection = bank(&[1, 2, 3], &[Requested, Idle, Idle]);
    assert_eq!(selection.open_row(), Some(1));
    selection.rows[1].enabled = false;
    assert_eq!(selection.open_row(), None);
    assert_eq!(bank(&[1, 2], &[Applied, Requested]).open_row(), None);
    assert_eq!(bank(&[], &[]).open_row(), None);
}

#[test]
fn requesting_and_applying_move_a_row_through_its_phases() {
    let mut selection = bank(&[4, 6], &[RowPhase::Idle, RowPhase::Idle]);
    selection.mark_requested(0);
    assert_eq!(selection.open_row(), Some(1));
    let limits = RangeLimits { lowest: true, highest: false };
    selection.apply(RowUpdate { row: 0, first: limits, second: RangeLimits::default() });
    assert_eq!((selection.rows[0].phase, selection.first_limits), (RowPhase::Applied, limits));
}

#[test]
fn range_spans_the_applied_leading_candidates() {
    use RowPhase::*;
    let mut selection = bank(&[5, 3, 1], &[Applied, Applied, Idle]);
    selection.expected = LevelRange { lowest: 3, highest: 5 };
    assert!(selection.range_is_valid(&mut Exact(true)));
    selection.expected.highest = 6;
    assert!(!selection.range_is_valid(&mut Exact(true)));
    assert!(selection.range_is_valid(&mut Exact(false)));

    selection.first_limits.highest = true;
    assert!(!selection.range_is_valid(&mut Exact(false)));
    selection.first_limits.highest = false;
    selection.expected.lowest = 2;
    selection.second_limits.lowest = true;
    assert!(!selection.range_is_valid(&mut Exact(false)));
}

#[test]
fn nothing_applied_or_all_unset_levels_give_no_range() {
    use RowPhase::*;
    assert!(!bank(&[5, 3], &[Requested, Applied]).range_is_valid(&mut Exact(false)));
    assert!(!bank(&[255, 255], &[Applied, Applied]).range_is_valid(&mut Exact(false)));
    assert!(!bank(&[], &[]).range_is_valid(&mut Exact(false)));
}

#[test]
fn a_single_level_at_the_reference_is_not_a_range_unless_any_range_is_allowed() {
    use RowPhase::*;
    let mut selection = bank(&[4, 4], &[Applied, Applied]);
    selection.reference_level = 4;
    assert!(!selection.range_is_valid(&mut Exact(false)));
    selection.reference_level = 3;
    assert!(selection.range_is_valid(&mut Exact(false)));
    selection.reference_level = 4;
    selection.any_range = true;
    assert!(selection.range_is_valid(&mut Exact(false)));
    assert!(bank(&[4], &[Applied]).range_is_valid(&mut Exact(false)));
}
