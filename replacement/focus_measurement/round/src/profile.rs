#![forbid(unsafe_code)]

//! What the measurement round reads and writes in the shared measurement record
//! and the profile records. The profile-record layer implements these traits.

use crate::lanes::Selection;
use xm1_saclac_interface::round::{GeometryPair, RetainedGeometryValue};
use xm1_saclac_interface::RecordHandle;

/// Pipeline statistics the measurement lanes keep in use; each is captured at
/// the measurement tick.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsInUse {
    /// Kept by the acquisition and window lanes.
    pub primary: bool,
    /// Kept by the search and image lanes.
    pub search: bool,
    pub auxiliary: bool,
}

/// A measurement result the shared record publishes that the round does not type; invalidating
/// one clears its first word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishedResult {
    Search,
    /// The source publication's flag; its summary, means and strength stay.
    Source,
}

/// How the profile a record selects chooses its measurement. `Other` carries a
/// value the round only passes on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProfileSelection { #[default] Unselected, Record, Other(u8) }

/// The profile's measurement mode, a value the round tests for six and for zero
/// and otherwise passes on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileMode(pub u8);

impl ProfileMode {
    pub const fn six(self) -> bool { self.0 == 6 }
    pub const fn set(self) -> bool { self.0 != 0 }
}

/// What the selected profile does with the frame-interval history when a
/// measurement tick or plan asks for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntervalTracking { Clear, Record, Keep }

/// A frame interval in the pipeline's interval unit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameInterval(pub u32);

impl FrameInterval {
    /// One million over the interval, keeping the low sixteen bits; zero for a zero interval.
    pub const fn rate(self) -> FrameRate {
        match 1_000_000u32.checked_div(self.0) { Some(rate) => FrameRate(rate as u16), None => FrameRate(0) }
    }
}

/// Frames per second as the planner compares them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameRate(pub u16);

/// The last ten frame intervals the round recorded, oldest first.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IntervalHistory {
    intervals: [FrameInterval; IntervalHistory::CAPACITY],
    len: usize,
}

impl IntervalHistory {
    pub const CAPACITY: usize = 10;

    /// `len` beyond the capacity is read as a full history.
    pub fn new(intervals: [FrameInterval; Self::CAPACITY], len: usize) -> Self {
        Self { intervals, len: len.min(Self::CAPACITY) }
    }

    pub fn intervals(&self) -> &[FrameInterval] { &self.intervals[..self.len] }

    /// Appends `interval`, dropping the oldest when the history is full.
    pub fn record(&mut self, interval: FrameInterval) {
        if self.len == Self::CAPACITY {
            self.intervals.copy_within(1.., 0);
            self.intervals[Self::CAPACITY - 1] = interval;
        } else {
            self.intervals[self.len] = interval;
            self.len += 1;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Profile {
    pub selection: ProfileSelection,
    pub mode: ProfileMode,
    pub interval_tracking: IntervalTracking,
    /// An autofocus command refreshes the measurement geometry.
    pub geometry_on_autofocus: bool,
    /// The planner takes the profile's own cadence.
    pub profile_cadence: bool,
    /// The planner takes its cadence by the acquisition's frame rate.
    pub frame_rate_cadence: bool,
    /// The measurement-grid cell of the profile's focus area.
    pub focus_cell: GridCell,
    /// The window request carries the search's adjustment word.
    pub takes_search_adjustment: bool,
}

/// A cell of the eight-by-eight measurement grid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridCell { pub column: u32, pub row: u32 }

/// The profile's part of the measurement geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileGeometry {
    pub resource: GeometryPair,
    pub converted_profile: RetainedGeometryValue,
    pub first_parameter: RetainedGeometryValue,
    pub second_parameter: RetainedGeometryValue,
}

/// A value the shared record holds for a retained consumer, which alone gives it meaning.
pub use xm1_saclac_interface::round_acquisition::RetainedWord;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetainedHalfword(pub u16);

/// The status of the latest window calculation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowStatus(pub u32);

impl WindowStatus {
    /// Statuses 1 and 2 keep the measurement power requested.
    pub const fn holds_power(self) -> bool { matches!(self.0, 1 | 2) }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowPosition(pub [u16; 2]);

/// The window calculation's part of the shared record; the window request
/// carries `first`, `halfword` and `second` to calc unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowRecord {
    pub status: WindowStatus,
    pub first: RetainedWord,
    pub halfword: RetainedHalfword,
    pub second: RetainedWord,
}

impl WindowRecord {
    pub const EMPTY: Self = Self { status: WindowStatus(0), first: RetainedWord(0), halfword: RetainedHalfword(0), second: RetainedWord(0) };
}

/// The round's view of the shared measurement record and the profile records.
/// A write names the record whose grant covers it; a record without that grant
/// leaves the shared record unchanged.
pub trait ProfileRecords {
    /// The measurement kinds the shared record offers; `None` when it offers none.
    fn offered(&mut self) -> Option<Selection>;
    fn statistics_in_use(&mut self) -> StatisticsInUse;
    fn set_statistics_in_use(&mut self, record: RecordHandle, in_use: StatisticsInUse);
    /// Detaches the buffer that receives measurement captures.
    fn detach_capture_buffer(&mut self, record: RecordHandle);
    /// Clears the first word of `result`'s record.
    fn invalidate(&mut self, record: RecordHandle, result: PublishedResult);
    /// Clears the first word of the published focus result, under the record the focus
    /// controller holds.
    fn invalidate_focus_result(&mut self);
    /// Whether `record`'s grant covers the measurement history's position.
    fn grants_measurement_history(&mut self, record: RecordHandle) -> bool;
    /// The level the acquisition last published.
    fn acquisition_level(&mut self) -> i16;
    fn interval_history(&mut self) -> IntervalHistory;
    fn set_interval_history(&mut self, record: RecordHandle, history: IntervalHistory);
    fn profile(&mut self, record: RecordHandle) -> Profile;
    fn geometry(&mut self, record: RecordHandle) -> ProfileGeometry;
    fn window(&mut self) -> WindowRecord;
    fn store_window_status(&mut self, record: RecordHandle, status: WindowStatus);
    fn store_window_position(&mut self, record: RecordHandle, position: WindowPosition);
}
