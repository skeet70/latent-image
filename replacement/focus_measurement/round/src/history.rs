#![forbid(unsafe_code)]

//! The measurement history: one entry per measurement tick, in a ring of the
//! last five. The round writes it; the search and shot modules read it through
//! [`MeasurementHistory`].

use crate::pipeline::{HistoryStatistics, PipelineSnapshot, PipelineWord};

/// One measurement tick as the history keeps it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HistoryEntry {
    written: bool,
    statistics: HistoryStatistics,
    exposure: i16,
    reference_level: u16,
    retained: [PipelineWord; 6],
}

impl HistoryEntry {
    const EMPTY: Self = Self { written: false, statistics: HistoryStatistics::EMPTY, exposure: 0, reference_level: 0, retained: [PipelineWord(0); 6] };

    /// `exposure` is the tick's exposure difference the round derived from `snapshot`.
    pub fn new(snapshot: &PipelineSnapshot, statistics: HistoryStatistics, exposure: i16) -> Self {
        Self { written: true, statistics, exposure, reference_level: snapshot.levels[1], retained: snapshot.retained }
    }

    /// False for an entry cleared when the history started and not written since.
    pub const fn written(&self) -> bool { self.written }
    pub const fn statistics(&self) -> &HistoryStatistics { &self.statistics }
    /// The two statistics levels at the profile's focus cell.
    pub const fn cell_levels(&self) -> [u16; 2] { self.statistics.cell_levels }
    pub const fn exposure(&self) -> i16 { self.exposure }
    /// The snapshot's second level.
    pub const fn reference_level(&self) -> u16 { self.reference_level }
    /// Snapshot words the history carries for its readers.
    pub const fn retained(&self) -> [PipelineWord; 6] { self.retained }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HistorySlot(u8);

impl HistorySlot {
    pub const fn index(self) -> usize { self.0 as usize }
    const fn next(self) -> Self { if self.0 as usize + 1 < MeasurementHistory::CAPACITY { Self(self.0 + 1) } else { Self(0) } }
    const fn previous(self) -> Self { if self.0 == 0 { Self(MeasurementHistory::CAPACITY as u8 - 1) } else { Self(self.0 - 1) } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasurementHistory {
    started: bool,
    newest: HistorySlot,
    entries: [HistoryEntry; MeasurementHistory::CAPACITY],
}

impl Default for MeasurementHistory { fn default() -> Self { Self::new() } }

impl MeasurementHistory {
    pub const CAPACITY: usize = 5;

    pub const fn new() -> Self { Self { started: false, newest: HistorySlot(0), entries: [HistoryEntry::EMPTY; Self::CAPACITY] } }

    /// False after a restart, until the next entry.
    pub const fn started(&self) -> bool { self.started }
    pub const fn newest_slot(&self) -> HistorySlot { self.newest }

    /// The entry at the newest slot, whether or not the history has started.
    pub const fn newest(&self) -> &HistoryEntry { &self.entries[self.newest.index()] }

    /// Every entry from the newest back to the oldest slot.
    pub fn recent(&self) -> impl Iterator<Item = &HistoryEntry> {
        let mut slot = self.newest;
        (0..Self::CAPACITY).map(move |_| {
            let entry = &self.entries[slot.index()];
            slot = slot.previous();
            entry
        })
    }

    /// Entries in slot order.
    pub const fn entries(&self) -> &[HistoryEntry; MeasurementHistory::CAPACITY] { &self.entries }

    /// The next entry starts the history again; `kept` is false when the
    /// measurement record does not grant the history.
    pub fn restart(&mut self, kept: bool) {
        if kept { self.started = false; }
    }

    /// Writes `entry` after the newest, or into the first slot of a history it
    /// starts, clearing the others. Without `kept` the position does not advance.
    pub fn record(&mut self, entry: HistoryEntry, kept: bool) {
        let slot = if self.started {
            self.newest.next()
        } else {
            self.entries.iter_mut().for_each(|entry| entry.written = false);
            HistorySlot(0)
        };
        self.entries[slot.index()] = entry;
        if kept {
            self.started = true;
            self.newest = slot;
        }
    }
}
