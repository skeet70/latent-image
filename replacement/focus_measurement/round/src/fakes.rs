use crate::lanes::Selection;
use crate::planner::{Cadence, Mode, Work};
use crate::profile::{FrameInterval, IntervalHistory, IntervalTracking, Profile, ProfileGeometry, ProfileRecords, ProfileSelection, PublishedResult, StatisticsInUse};
use crate::record::{self, AlternateSearch, Pipeline, Request, Settings, Statistics};
use xm1_saclac_controllers::replies;
use xm1_saclac_interface::round::{GeometryCalibration, GeometryPair, RetainedGeometryValue};
use xm1_saclac_interface::{ClientCallback, CompletionStatus, RecordHandle, ReplyTarget};
use std::vec::Vec;

/// The shared measurement record and the selected profile as their readers see them.
pub struct Records {
    pub offered: Option<Selection>,
    pub in_use: StatisticsInUse,
    pub capture_attached: bool,
    pub invalid: Vec<PublishedResult>,
    pub focus_invalid: bool,
    pub history_restarted: bool,
    pub intervals: IntervalHistory,
    pub profile: Profile,
    pub geometry: ProfileGeometry,
    /// Whether the writing record's grant covers its writes.
    pub granted: bool,
}

impl Records {
    pub fn offering(offered: Option<Selection>) -> Self {
        Self {
            offered,
            in_use: StatisticsInUse::default(),
            capture_attached: true,
            invalid: Vec::new(),
            focus_invalid: false,
            history_restarted: false,
            intervals: IntervalHistory::default(),
            profile: Profile {
                selection: ProfileSelection::Unselected,
                mode_six: false,
                interval_tracking: IntervalTracking::Keep,
                geometry_on_autofocus: false,
            },
            geometry: ProfileGeometry {
                resource: GeometryPair([0; 2]),
                converted_profile: RetainedGeometryValue(0),
                first_parameter: RetainedGeometryValue(0),
                second_parameter: RetainedGeometryValue(0),
            },
            granted: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outbound {
    Capture(Statistics),
    ModeSixStatistics,
    Request(Request),
    ClearPending(ReplyTarget),
    Reply { target: ReplyTarget, callback: ClientCallback, status: CompletionStatus },
    MeasurementHistory,
}

/// Record ports over [`Records`], logging what leaves the round; `extra` carries
/// a test's own state.
pub struct Ports<X = ()> {
    pub records: Records,
    pub search_permitted: bool,
    pub alternate: AlternateSearch,
    pub interval_history: bool,
    pub calibration: GeometryCalibration,
    pub frame_interval: Option<FrameInterval>,
    pub outbound: Vec<Outbound>,
    pub extra: X,
}

impl<X> Ports<X> {
    pub fn new(offered: Option<Selection>, extra: X) -> Self {
        Self {
            records: Records::offering(offered),
            search_permitted: true,
            alternate: AlternateSearch::Never,
            interval_history: false,
            calibration: GeometryCalibration([0; 4]),
            frame_interval: None,
            outbound: Vec::new(),
            extra,
        }
    }

    pub fn take(&mut self) -> Vec<Outbound> { core::mem::take(&mut self.outbound) }
}

impl<X> ProfileRecords for Ports<X> {
    fn offered(&mut self) -> Option<Selection> { self.records.offered }
    fn statistics_in_use(&mut self) -> StatisticsInUse { self.records.in_use }
    fn detach_capture_buffer(&mut self, _: RecordHandle) {
        if self.records.granted { self.records.capture_attached = false; }
    }
    fn invalidate(&mut self, _: RecordHandle, result: PublishedResult) {
        if self.records.granted && !self.records.invalid.contains(&result) { self.records.invalid.push(result); }
    }
    fn invalidate_focus_result(&mut self) { self.records.focus_invalid = true; }
    fn restart_measurement_history(&mut self, _: RecordHandle) {
        if self.records.granted { self.records.history_restarted = true; }
    }
    fn interval_history(&mut self) -> IntervalHistory { self.records.intervals }
    fn set_interval_history(&mut self, _: RecordHandle, history: IntervalHistory) {
        if self.records.granted { self.records.intervals = history; }
    }
    fn profile(&mut self, _: RecordHandle) -> Profile { self.records.profile }
    fn geometry(&mut self, _: RecordHandle) -> ProfileGeometry { self.records.geometry }
}

impl<X> Settings for Ports<X> {
    fn search_permitted(&mut self) -> bool { self.search_permitted }
    fn alternate_search(&mut self) -> AlternateSearch { self.alternate }
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: true, third: true, fourth: true } }
    fn keeps_interval_history(&mut self) -> bool { self.interval_history }
    fn geometry_calibration(&mut self) -> GeometryCalibration { self.calibration }
}

impl<X> Pipeline for Ports<X> {
    fn capture(&mut self, statistics: Statistics) { self.outbound.push(Outbound::Capture(statistics)); }
    fn frame_interval(&mut self) -> Option<FrameInterval> { self.frame_interval }
    fn request_mode_six_statistics(&mut self) { self.outbound.push(Outbound::ModeSixStatistics); }
}

impl<X> replies::Effects for Ports<X> {
    fn emit(&mut self, effect: replies::Effect<'_>) {
        self.outbound.push(match effect {
            replies::Effect::ClearPending(target) => Outbound::ClearPending(target),
            replies::Effect::Deliver { target, callback, status, .. } => Outbound::Reply { target, callback, status },
        });
    }
}

impl<X> record::Ports for Ports<X> {
    fn request(&mut self, request: Request) { self.outbound.push(Outbound::Request(request)); }
    fn record_measurement_history(&mut self, _: RecordHandle) { self.outbound.push(Outbound::MeasurementHistory); }
}
