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

/// A measurement result the round publishes in the shared measurement record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishedResult { Acquisition, Search, Auxiliary, Source }

/// How the profile a record selects chooses its measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileSelection { Unselected, Record, Other }

/// What the selected profile does with the frame-interval history when a
/// measurement tick or plan asks for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntervalTracking { Clear, Record, Keep }

/// A frame interval in the pipeline's interval unit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameInterval(pub u32);

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
    /// The profile's measurement mode is six.
    pub mode_six: bool,
    pub interval_tracking: IntervalTracking,
    /// An autofocus command refreshes the measurement geometry.
    pub geometry_on_autofocus: bool,
}

/// The profile's part of the measurement geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileGeometry {
    pub resource: GeometryPair,
    pub converted_profile: RetainedGeometryValue,
    pub first_parameter: RetainedGeometryValue,
    pub second_parameter: RetainedGeometryValue,
}

/// The round's view of the shared measurement record and the profile records.
/// A write names the record whose grant covers it; a record without that grant
/// leaves the shared record unchanged.
pub trait ProfileRecords {
    /// The measurement kinds the shared record offers; `None` when it offers none.
    fn offered(&mut self) -> Option<Selection>;
    fn statistics_in_use(&mut self) -> StatisticsInUse;
    /// Detaches the buffer that receives measurement captures.
    fn detach_capture_buffer(&mut self, record: RecordHandle);
    /// Marks `result` invalid for its readers.
    fn invalidate(&mut self, record: RecordHandle, result: PublishedResult);
    /// Marks the published focus result invalid, under the record the focus
    /// controller holds.
    fn invalidate_focus_result(&mut self);
    /// Marks the measurement history empty; the next entry starts it again.
    fn restart_measurement_history(&mut self, record: RecordHandle);
    fn interval_history(&mut self) -> IntervalHistory;
    fn set_interval_history(&mut self, record: RecordHandle, history: IntervalHistory);
    fn profile(&mut self, record: RecordHandle) -> Profile;
    fn geometry(&mut self, record: RecordHandle) -> ProfileGeometry;
}
