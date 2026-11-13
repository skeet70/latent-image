//! The selection bank of an exposure evaluation: up to eight candidate levels, the row each
//! is evaluated in, and the level range the evaluation expects.

pub const ROWS: usize = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RowPhase {
    #[default]
    Idle,
    /// Calc 0x3c00 has been posted for the row.
    Requested,
    /// The row's evaluation result has been applied.
    Applied,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Row {
    pub enabled: bool,
    pub phase: RowPhase,
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
    /// The level of each candidate; `count` of them are in use.
    pub levels: [u8; ROWS],
    pub count: u8,
    pub reference_level: u8,
    pub expected: LevelRange,
    pub any_range: bool,
    pub rows: [Row; ROWS],
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
        for (index, level) in self.levels.iter().enumerate().take(usize::from(self.count).min(ROWS)) {
            if index > 0 && self.rows[index].phase != RowPhase::Applied {
                break;
            }
            range.lowest = range.lowest.min(*level);
            range.highest = range.highest.max(*level);
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
