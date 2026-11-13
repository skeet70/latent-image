use crate::image::{ChannelCount, LensReport, ReplyPorts};
use crate::lanes::Selection;
use crate::planner::{Cadence, Mode, Work};
use crate::pipeline::{Exposure, GainStep, Pipeline, PipelineSnapshot, PipelineWord, Statistics};
use crate::profile::{FrameInterval, IntervalHistory, IntervalTracking, Profile, ProfileGeometry, ProfileMode, ProfileRecords, ProfileSelection, PublishedResult, StatisticsInUse, WindowPosition, WindowRecord, WindowStatus};
use crate::record::{self, AlternateSearch, Power, Request, Settings, Trace, TraceMark};
use crate::window::{self, WindowResult};
use xm1_saclac_controllers::replies;
use xm1_saclac_interface::round::{GeometryCalibration, GeometryPair, RetainedGeometryValue};
use xm1_saclac_interface::{ClientCallback, Completed, CompletionStatus, RecordHandle, ReplyTarget};
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
    pub window: WindowRecord,
    pub window_position: WindowPosition,
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
                mode: ProfileMode(0),
                interval_tracking: IntervalTracking::Keep,
                geometry_on_autofocus: false,
            },
            geometry: ProfileGeometry {
                resource: GeometryPair([0; 2]),
                converted_profile: RetainedGeometryValue(0),
                first_parameter: RetainedGeometryValue(0),
                second_parameter: RetainedGeometryValue(0),
            },
            window: WindowRecord::default(),
            window_position: WindowPosition::default(),
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
    Reply { target: ReplyTarget, callback: ClientCallback, status: CompletionStatus, values: [u32; 3] },
    MeasurementHistory,
    Power(bool),
    Trace(TraceMark),
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
    pub snapshot: PipelineSnapshot,
    pub window_refresh: bool,
    pub gain_offsets: [u32; 11],
    pub exposure_value: i16,
    pub traces: bool,
    pub window_result: Option<WindowResult>,
    pub channel_count: ChannelCount,
    pub lens: LensReport,
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
            snapshot: PipelineSnapshot::default(),
            window_refresh: false,
            gain_offsets: [0; 11],
            exposure_value: 0,
            traces: false,
            window_result: None,
            channel_count: ChannelCount(1),
            lens: LensReport::default(),
            outbound: Vec::new(),
            extra,
        }
    }

    pub fn take(&mut self) -> Vec<Outbound> { core::mem::take(&mut self.outbound) }
}

impl<X> ProfileRecords for Ports<X> {
    fn offered(&mut self) -> Option<Selection> { self.records.offered }
    fn statistics_in_use(&mut self) -> StatisticsInUse { self.records.in_use }
    fn set_statistics_in_use(&mut self, _: RecordHandle, in_use: StatisticsInUse) {
        if self.records.granted { self.records.in_use = in_use; }
    }
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
    fn window(&mut self) -> WindowRecord { self.records.window }
    fn store_window_status(&mut self, _: RecordHandle, status: WindowStatus) {
        if self.records.granted { self.records.window.status = status; }
    }
    fn store_window_position(&mut self, _: RecordHandle, position: WindowPosition) {
        if self.records.granted { self.records.window_position = position; }
    }
}

impl<X> Settings for Ports<X> {
    fn search_permitted(&mut self) -> bool { self.search_permitted }
    fn alternate_search(&mut self) -> AlternateSearch { self.alternate }
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: true, third: true, fourth: true } }
    fn keeps_interval_history(&mut self) -> bool { self.interval_history }
    fn geometry_calibration(&mut self) -> GeometryCalibration { self.calibration }
    fn traces_measurement(&mut self) -> bool { self.traces }
}

impl<X> Pipeline for Ports<X> {
    fn capture(&mut self, statistics: Statistics) { self.outbound.push(Outbound::Capture(statistics)); }
    fn frame_interval(&mut self) -> Option<FrameInterval> { self.frame_interval }
    fn request_mode_six_statistics(&mut self) { self.outbound.push(Outbound::ModeSixStatistics); }
    fn snapshot(&mut self, _: Statistics) -> PipelineSnapshot { self.snapshot }
    fn take_window_refresh(&mut self) -> bool { core::mem::take(&mut self.window_refresh) }
}

impl<X> Exposure for Ports<X> {
    fn gain_offset(&mut self, step: GainStep) -> u32 { self.gain_offsets[step.0 as usize] }
    fn exposure_value(&mut self, _: [PipelineWord; 2]) -> i16 { self.exposure_value }
}

impl<X> Power for Ports<X> {
    fn request_measurement_power(&mut self, held: bool) { self.outbound.push(Outbound::Power(held)); }
}

impl<X> Trace for Ports<X> {
    fn trace(&mut self, mark: TraceMark) { self.outbound.push(Outbound::Trace(mark)); }
}

impl<X> window::Ports for Ports<X> {
    fn window_result(&mut self, _: Completed<'_>) -> Option<WindowResult> { self.window_result }
}

impl<X> replies::Effects for Ports<X> {
    fn emit(&mut self, effect: replies::Effect<'_>) {
        self.outbound.push(match effect {
            replies::Effect::ClearPending(target) => Outbound::ClearPending(target),
            replies::Effect::Deliver { target, callback, status, values } => {
                let mut words = [0; 3];
                for (word, bytes) in words.iter_mut().zip(values.bytes().chunks_exact(4)) {
                    *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                }
                Outbound::Reply { target, callback, status, values: words }
            }
        });
    }
}

impl<X> record::Ports for Ports<X> {
    fn request(&mut self, request: Request) { self.outbound.push(Outbound::Request(request)); }
    fn record_measurement_history(&mut self, _: RecordHandle) { self.outbound.push(Outbound::MeasurementHistory); }
}

impl<X> ReplyPorts for Ports<X> {
    fn channel_count(&mut self) -> ChannelCount { self.channel_count }
    fn lens_report(&mut self) -> LensReport { self.lens }
}
