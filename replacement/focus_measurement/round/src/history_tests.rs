use crate::history::{HistoryEntry, MeasurementHistory};
use crate::pipeline::{HistoryStatistics, PipelineSnapshot};
use std::vec::Vec;

fn entry(exposure: i16) -> HistoryEntry { HistoryEntry::new(&PipelineSnapshot::default(), HistoryStatistics::default(), exposure) }

fn exposures<'a>(entries: impl Iterator<Item = &'a HistoryEntry>) -> Vec<(bool, i16)> {
    entries.map(|entry| (entry.written(), entry.exposure())).collect()
}

#[test]
fn entries_fill_the_ring_in_order_and_wrap_after_five() {
    let mut history = MeasurementHistory::new();
    for exposure in 1..=7 { history.record(entry(exposure), true); }
    assert!(history.started());
    assert_eq!(history.newest().exposure(), 7);
    assert_eq!(exposures(history.entries().iter()), [(true, 6), (true, 7), (true, 3), (true, 4), (true, 5)]);
    assert_eq!(exposures(history.recent()), [(true, 7), (true, 6), (true, 5), (true, 4), (true, 3)]);
}

#[test]
fn a_restarted_history_clears_every_entry_and_starts_again_at_the_first_slot() {
    let mut history = MeasurementHistory::new();
    for exposure in 1..=3 { history.record(entry(exposure), true); }
    history.restart(true);
    assert!(!history.started());
    assert_eq!(history.newest().exposure(), 3);
    history.record(entry(9), true);
    assert_eq!(exposures(history.entries().iter()), [(true, 9), (false, 2), (false, 3), (false, 0), (false, 0)]);
    assert_eq!(history.newest().exposure(), 9);
}

#[test]
fn a_record_without_the_grant_rewrites_the_same_slot() {
    let mut history = MeasurementHistory::new();
    history.record(entry(1), true);
    history.record(entry(2), false);
    history.record(entry(3), false);
    assert_eq!(history.newest().exposure(), 1);
    assert_eq!(exposures(history.entries().iter().take(2)), [(true, 1), (true, 3)]);
    history.record(entry(4), true);
    assert_eq!(exposures(history.recent().take(2)), [(true, 4), (true, 1)]);
}

#[test]
fn an_unstarted_history_without_the_grant_stays_unstarted() {
    let mut history = MeasurementHistory::new();
    history.record(entry(5), false);
    assert!(!history.started());
    assert_eq!(exposures(history.entries().iter().take(1)), [(true, 5)]);
    history.restart(false);
    history.record(entry(6), true);
    history.restart(false);
    assert!(history.started());
}
