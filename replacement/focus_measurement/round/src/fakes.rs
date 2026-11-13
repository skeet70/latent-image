use crate::auxiliary::{self, AuxiliaryProfile, AuxiliaryPublication, AuxiliaryReading, AuxiliarySettings, Curve, CurveInputs, RequestProfile, ResourceIndex};
use crate::image::{ChannelCount, LensReport, ReplyPorts};
use crate::lanes::Selection;
use crate::planner::{Cadence, Mode, Work};
use crate::pipeline::{Exposure, GainStep, HistoryStatistics, Pipeline, PipelineSnapshot, PipelineWord, SnapshotStatus, Statistics, TimingWord};
use crate::profile::{FrameInterval, FrameRate, GridCell, IntervalHistory, IntervalTracking, Profile, ProfileGeometry, ProfileMode, ProfileRecords, ProfileSelection, PublishedResult, RetainedWord, StatisticsInUse, WindowPosition, WindowRecord, WindowStatus};
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
    pub acquisition_level: i16,
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
            acquisition_level: 0,
            intervals: IntervalHistory::default(),
            profile: Profile {
                selection: ProfileSelection::Unselected,
                mode: ProfileMode(0),
                interval_tracking: IntervalTracking::Keep,
                geometry_on_autofocus: false,
                profile_cadence: false,
                frame_rate_cadence: false,
                focus_cell: GridCell::default(),
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

/// The auxiliary measurement's helpers and its part of the shared record.
pub struct AuxiliaryRecords {
    pub reading: Option<AuxiliaryReading>,
    pub settings: AuxiliarySettings,
    pub default_levels: [u16; 3],
    pub continues: bool,
    pub lens: ResourceIndex,
    /// A resource's level is its index plus this.
    pub resource_base: i16,
    pub follow_limit: u32,
    /// The rate per pixel of the statistics image.
    pub rate: u32,
    pub profile: AuxiliaryProfile,
    pub request_profile: RequestProfile,
    pub follow_profile: RetainedWord,
    pub curve_offset: i16,
    /// Inputs of every curve asked for, in order.
    pub curves: Vec<CurveInputs>,
    pub publication: AuxiliaryPublication,
    pub value: i16,
    pub grid_selection: bool,
    pub grid_refreshes: usize,
    pub capture_levels: Option<[u16; 3]>,
}

impl Default for AuxiliaryRecords {
    fn default() -> Self {
        Self {
            reading: None,
            settings: AuxiliarySettings::default(),
            default_levels: [0; 3],
            continues: false,
            lens: ResourceIndex(0),
            resource_base: 0,
            follow_limit: 0,
            rate: 0,
            profile: AuxiliaryProfile::default(),
            request_profile: RequestProfile::default(),
            follow_profile: RetainedWord(0),
            curve_offset: 0,
            curves: Vec::new(),
            publication: AuxiliaryPublication::default(),
            value: 0,
            grid_selection: false,
            grid_refreshes: 0,
            capture_levels: None,
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
    /// The pipeline's current timing.
    pub timing: TimingWord,
    /// Frame intervals by timing; a timing not listed has none.
    pub intervals: Vec<(TimingWord, FrameInterval)>,
    pub frame_rate_threshold: FrameRate,
    pub cadence: fn(Work, Mode) -> Cadence,
    pub snapshot: PipelineSnapshot,
    pub history: HistoryStatistics,
    /// Levels the history capture reports per cell; other cells report zero.
    pub cell_levels: Vec<(GridCell, [u16; 2])>,
    pub timing_offset: u16,
    pub window_refresh: bool,
    pub gain_offsets: [u32; 11],
    pub exposure_value: i16,
    pub traces: bool,
    pub window_result: Option<WindowResult>,
    pub channel_count: ChannelCount,
    pub lens: LensReport,
    pub auxiliary: AuxiliaryRecords,
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
            timing: TimingWord(0x7000),
            intervals: Vec::new(),
            frame_rate_threshold: FrameRate(0),
            cadence: |_, _| Cadence { first: true, second: true, third: true, fourth: true },
            snapshot: PipelineSnapshot { status: SnapshotStatus(5), ..PipelineSnapshot::default() },
            history: HistoryStatistics::default(),
            cell_levels: Vec::new(),
            timing_offset: 0,
            window_refresh: false,
            gain_offsets: [0; 11],
            exposure_value: 0,
            traces: false,
            window_result: None,
            channel_count: ChannelCount(1),
            lens: LensReport::default(),
            auxiliary: AuxiliaryRecords::default(),
            outbound: Vec::new(),
            extra,
        }
    }

    pub fn take(&mut self) -> Vec<Outbound> { core::mem::take(&mut self.outbound) }

    /// Gives the pipeline's current timing `interval`, or none.
    pub fn set_current_interval(&mut self, interval: Option<FrameInterval>) {
        let timing = self.timing;
        self.intervals.retain(|&(listed, _)| listed != timing);
        if let Some(interval) = interval { self.intervals.push((timing, interval)); }
    }
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
    fn grants_measurement_history(&mut self, _: RecordHandle) -> bool { self.records.granted }
    fn acquisition_level(&mut self) -> i16 { self.records.acquisition_level }
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
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence { (self.cadence)(work, mode) }
    fn frame_rate_threshold(&mut self) -> FrameRate { self.frame_rate_threshold }
    fn keeps_interval_history(&mut self) -> bool { self.interval_history }
    fn geometry_calibration(&mut self) -> GeometryCalibration { self.calibration }
    fn traces_measurement(&mut self) -> bool { self.traces }
}

impl<X> Pipeline for Ports<X> {
    fn capture(&mut self, statistics: Statistics) { self.outbound.push(Outbound::Capture(statistics)); }
    fn timing(&mut self) -> TimingWord { self.timing }
    fn frame_interval(&mut self, timing: TimingWord) -> Option<FrameInterval> {
        self.intervals.iter().find(|&&(listed, _)| listed == timing).map(|&(_, interval)| interval)
    }
    fn request_mode_six_statistics(&mut self) { self.outbound.push(Outbound::ModeSixStatistics); }
    fn snapshot(&mut self, _: Statistics) -> PipelineSnapshot { self.snapshot }
    fn take_window_refresh(&mut self) -> bool { core::mem::take(&mut self.window_refresh) }
    fn history_statistics(&mut self, cell: GridCell) -> HistoryStatistics {
        let cell_levels = self.cell_levels.iter().find(|&&(listed, _)| listed == cell).map_or([0; 2], |&(_, levels)| levels);
        HistoryStatistics { cell_levels, ..self.history }
    }
}

impl<X> Exposure for Ports<X> {
    fn gain_offset(&mut self, step: GainStep) -> u32 { self.gain_offsets[step.0 as usize] }
    fn exposure_value(&mut self, _: [PipelineWord; 2]) -> i16 { self.exposure_value }
    fn timing_offset(&mut self, _: TimingWord) -> u16 { self.timing_offset }
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
}

impl<X> ReplyPorts for Ports<X> {
    fn channel_count(&mut self) -> ChannelCount { self.channel_count }
    fn lens_report(&mut self) -> LensReport { self.lens }
}

/// The curve the fake publishes: each average raised by the offset; the word is the lens resource.
fn fake_curve(inputs: &CurveInputs) -> Curve {
    Curve { levels: inputs.averages.map(|average| average.wrapping_add(inputs.offset as u16)), word: RetainedWord(inputs.lens.0) }
}

impl<X> auxiliary::Ports for Ports<X> {
    fn reading(&mut self, _: Completed<'_>) -> Option<AuxiliaryReading> { self.auxiliary.reading }
    fn auxiliary_settings(&mut self) -> AuxiliarySettings { self.auxiliary.settings }
    fn default_levels(&mut self) -> [u16; 3] { self.auxiliary.default_levels }
    fn curve(&mut self, inputs: &CurveInputs) -> Curve {
        self.auxiliary.curves.push(*inputs);
        fake_curve(inputs)
    }
    fn lens_resource(&mut self) -> ResourceIndex { self.auxiliary.lens }
    fn resource_level(&mut self, resource: ResourceIndex) -> i16 { (resource.0 as i16).wrapping_add(self.auxiliary.resource_base) }
    fn follow_limit(&mut self, _: ProfileSelection, _: RetainedWord, _: TimingWord) -> u32 { self.auxiliary.follow_limit }
    fn rate(&mut self, size: [PipelineWord; 2]) -> u32 { self.auxiliary.rate.wrapping_mul(size[0].0).wrapping_mul(size[1].0) }
    fn level_class(&mut self, level: i16, _: ProfileSelection, window: RetainedWord) -> RetainedWord {
        RetainedWord((level as u32).wrapping_add(window.0))
    }
    fn refresh_grid_selection(&mut self, _: RecordHandle) { self.auxiliary.grid_refreshes += 1; }
    fn auxiliary_continues(&mut self, _: RecordHandle) -> bool { self.auxiliary.continues }
    fn auxiliary_profile(&mut self, _: RecordHandle) -> AuxiliaryProfile { self.auxiliary.profile }
    fn request_profile(&mut self, _: RecordHandle) -> RequestProfile { self.auxiliary.request_profile }
    fn follow_profile(&mut self, _: RecordHandle) -> RetainedWord { self.auxiliary.follow_profile }
    fn curve_offset(&mut self, _: RecordHandle) -> i16 { self.auxiliary.curve_offset }
    fn auxiliary_publication(&mut self, _: RecordHandle) -> AuxiliaryPublication { self.auxiliary.publication }
    fn publish_auxiliary(&mut self, _: RecordHandle, publication: AuxiliaryPublication) {
        if self.records.granted { self.auxiliary.publication = publication; }
    }
    fn store_auxiliary_value(&mut self, _: RecordHandle, value: i16) {
        if self.records.granted { self.auxiliary.value = value; }
    }
    fn grid_selection(&mut self, _: RecordHandle) -> bool { self.auxiliary.grid_selection }
    fn record_capture_levels(&mut self, _: RecordHandle, levels: [u16; 3]) {
        if self.records.capture_attached { self.auxiliary.capture_levels = Some(levels); }
    }
}
