//! The selection bank of an exposure evaluation: up to eight candidate levels, the row each
//! is evaluated in, and the level range the evaluation expects.

use crate::exposure::context::SUBJECTS;

pub const ROWS: usize = 8;

/// A row of the bank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowIndex(u8);

impl RowIndex {
    pub const fn new(index: u8) -> Option<Self> { if (index as usize) < ROWS { Some(Self(index)) } else { None } }

    pub const fn get(self) -> usize { self.0 as usize }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RowPhase {
    #[default]
    Idle,
    /// Calc 0x3c00 has been posted for the row.
    Requested,
    /// The row's evaluation result has been applied.
    Applied,
}

/// What the image task's statistics check writes into a row through the addresses it is given.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RowMeasurement {
    pub pairs: [[u32; 2]; 4],
    /// The check's reply word.
    pub reply: u32,
    pub statistics: [u32; 4],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Row {
    pub enabled: bool,
    pub phase: RowPhase,
    pub measurement: RowMeasurement,
}

/// The subjects an evaluation row covers, read in place by the image task and by calc.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Column {
    pub count: u32,
    pub regions: [[u8; 8]; SUBJECTS],
}

/// One candidate level of the span.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Candidate {
    pub value: i16,
    pub level: u8,
    /// 3 and 4 mark a candidate the request gives a fixed offset.
    pub state: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelRange {
    pub lowest: u8,
    pub highest: u8,
}

/// Which ends of the expected range bind the evaluated levels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangeLimits {
    pub lowest: bool,
    pub highest: bool,
}

/// What applying a row's result stores in the bank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowUpdate {
    pub row: usize,
    pub first: RangeLimits,
    pub second: RangeLimits,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionBank {
    /// `count` of the candidates are in use.
    pub candidates: [Candidate; ROWS],
    pub count: u8,
    pub filled: u8,
    /// The candidate number of the first candidate.
    pub first: u8,
    pub reference_level: u8,
    /// The highest level from candidate 1 to the span's last.
    pub highest_below: u8,
    pub expected: LevelRange,
    pub any_range: bool,
    pub rows: [Row; ROWS],
    pub columns: [Column; ROWS],
    pub first_limits: RangeLimits,
    pub second_limits: RangeLimits,
}

pub trait SelectionSettings {
    /// Whether the evaluated range must equal the expected range.
    fn exact_range(&mut self) -> bool;
}

impl SelectionBank {
    fn rows_in_use(&self) -> &[Row] { &self.rows[..usize::from(self.count).min(ROWS)] }

    /// The first idle row, when it is enabled.
    pub fn open_row(&self) -> Option<usize> {
        let (index, row) = self.rows_in_use().iter().enumerate().find(|(_, row)| row.phase == RowPhase::Idle)?;
        row.enabled.then_some(index)
    }

    pub fn enable_row(&mut self, row: usize) { self.rows[row].enabled = true; }

    pub fn mark_requested(&mut self, row: usize) { self.rows[row].phase = RowPhase::Requested; }

    pub fn apply(&mut self, update: RowUpdate) {
        self.rows[update.row].phase = RowPhase::Applied;
        self.first_limits = update.first;
        self.second_limits = update.second;
    }

    /// The lowest and highest level over the leading candidates whose rows are applied; the first
    /// candidate counts once the bank's first row is applied.
    fn applied_range(&self) -> Option<LevelRange> {
        if self.count == 0 || self.rows[0].phase != RowPhase::Applied {
            return None;
        }
        let mut range = LevelRange { lowest: u8::MAX, highest: 0 };
        for (index, candidate) in self.candidates.iter().enumerate().take(usize::from(self.count).min(ROWS)) {
            if index > 0 && self.rows[index].phase != RowPhase::Applied {
                break;
            }
            range.lowest = range.lowest.min(candidate.level);
            range.highest = range.highest.max(candidate.level);
        }
        (range.lowest != u8::MAX).then_some(range)
    }

    /// Whether the applied candidates span a range the evaluation can conclude from.
    pub fn range_is_valid(&self, settings: &mut impl SelectionSettings) -> bool {
        let Some(range) = self.applied_range() else { return false };
        if settings.exact_range() {
            return range == self.expected;
        }
        for limits in [self.first_limits, self.second_limits] {
            if (limits.lowest && self.expected.lowest < range.lowest) || (limits.highest && self.expected.highest > range.highest) {
                return false;
            }
        }
        if self.any_range || self.count < 2 {
            return true;
        }
        !(self.reference_level == range.lowest && range.lowest == range.highest)
    }
}
