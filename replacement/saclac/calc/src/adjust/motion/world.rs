//! A test world for the preparation and the exposure program: the engine's world plus the lens
//! calibration, image 6's exposure services, the capture frames and the level scale, by table.

use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::Bytes;

use crate::adjust::lens::{BiasTable, LensConfiguration};
use crate::adjust::metering::MeteringTables;
use crate::adjust::motion::drive::{DriveKind, DriveMode, DriveSpeed};
use crate::adjust::motion::exposure::{FrameExposure, ProgramStep, PROGRAM_STEPS};
use crate::adjust::motion::plan::Plan;
use crate::adjust::motion::layout::SampleLayout;
use crate::adjust::motion::ports::{
    AreaCurves, AreaGrid, BoundsQuery, CaptureFraming, ChannelRates, Clock, CropGeometry, ExposureTimes, FrameGeometry, GridArea,
    LensBounds, LensCalibration, LensLimits, LensTables, LimitQuery, Logarithms10, MeasurementReadings, ProfileGains, SampleFrameRequest, SearchBounds,
    SearchFrameRequest, StatisticsFrameRequest, Ticks,
};
use crate::adjust::motion::measured::LevelCheck;
use crate::adjust::motion::preparation::{CaptureFrames, FrameSize, LensStatus, PreviousFocus};
use crate::adjust::motion::readings::{AreaList, FocusRequest, RecordReadings, SelectionLists, StepLimits, AREA_POSITIONS};
use crate::adjust::motion::search::SearchSetup;
use crate::adjust::motion::state::{Drive, FrameExposures, MotionState, SampleSchedule};
use crate::adjust::motion::statistics::Statistics;
use crate::adjust::peak::Logarithms;
use crate::adjust::timing::{CaptureTiming, LevelScale};
use crate::exposure::fakes::{handle, Headers, World};
use crate::exposure::ports::{CaptureProfiles, Lens, ProfileAccess, Properties, Property, StockMemory};
use crate::gains::{BlendSelector, CaptureGains, GainRequest, SelectedGains};
use crate::profile::Profile;

pub struct MotionWorld {
    pub world: World,
    pub steps: u32,
    pub lowest_steps: BTreeMap<u32, u32>,
    pub highest_steps: BTreeMap<u32, u32>,
    /// Positions per step; a depth spans this many times the depth in tenths.
    pub depth_positions: BTreeMap<u32, i16>,
    pub lens_state: u32,
    pub drive_kind: DriveKind,
    pub top_speed: u32,
    /// Exposure times by profile, level and gain; others take `default_time`.
    pub times: BTreeMap<(u32, i16, u32), u32>,
    pub default_time: u32,
    pub shortest_times: BTreeMap<u32, u32>,
    pub gain_levels: BTreeMap<(u32, u32), i16>,
    pub selected_area: GridArea,
    pub selection_value: u32,
    pub control_state: u32,
    pub frames: BTreeMap<u32, FrameSize>,
    /// Bounds by channel for every other query; each query is kept.
    pub bounds: BTreeMap<u8, SearchBounds>,
    pub bounds_queries: Vec<BoundsQuery>,
    pub search_frames: [AreaGrid; 2],
    pub search_frame_requests: Vec<SearchFrameRequest>,
    pub statistics_frame: [u32; 2],
    pub feature_classes: BTreeMap<u32, u8>,
    pub channel_rates: BTreeMap<(u32, u8), u32>,
    pub line_periods: BTreeMap<u32, u32>,
    pub channel_weights: [u16; 3],
    pub feature_bytes: [u8; 2],
    pub now: u32,
    /// Channel 0's table positions by lens step.
    pub table_positions: BTreeMap<u32, [i16; 2]>,
    pub channel_starts: BTreeMap<u8, i16>,
    pub period_positions: BTreeMap<u16, i32>,
    pub lens_limits: LensLimits,
    pub limit_queries: Vec<LimitQuery>,
    pub sample_frames: [AreaGrid; 2],
    pub sample_frame_requests: Vec<SampleFrameRequest>,
    pub crop: CropGeometry,
    pub cell_steps: [u32; 2],
    pub readout_start: u32,
    pub reference_line: u16,
    pub selected_gains: SelectedGains,
    pub gain_requests: Vec<GainRequest>,
}

impl Default for MotionWorld {
    fn default() -> Self {
        Self {
            world: World::default(),
            steps: 64,
            lowest_steps: BTreeMap::new(),
            highest_steps: BTreeMap::new(),
            depth_positions: BTreeMap::new(),
            lens_state: 0,
            drive_kind: DriveKind(0),
            top_speed: 0,
            times: BTreeMap::new(),
            default_time: 0,
            shortest_times: BTreeMap::new(),
            gain_levels: BTreeMap::new(),
            selected_area: GridArea::default(),
            selection_value: 0,
            control_state: 0,
            frames: BTreeMap::new(),
            bounds: BTreeMap::new(),
            bounds_queries: Vec::new(),
            search_frames: [AreaGrid::default(); 2],
            search_frame_requests: Vec::new(),
            statistics_frame: [0; 2],
            feature_classes: BTreeMap::new(),
            channel_rates: BTreeMap::new(),
            line_periods: BTreeMap::new(),
            channel_weights: [0; 3],
            feature_bytes: [0; 2],
            table_positions: BTreeMap::new(),
            channel_starts: BTreeMap::new(),
            period_positions: BTreeMap::new(),
            lens_limits: LensLimits::default(),
            limit_queries: Vec::new(),
            sample_frames: [AreaGrid::default(); 2],
            sample_frame_requests: Vec::new(),
            crop: CropGeometry::default(),
            cell_steps: [1, 1],
            readout_start: 0,
            reference_line: 0,
            selected_gains: SelectedGains::default(),
            gain_requests: Vec::new(),
            now: 0,
        }
    }
}

impl MotionWorld {
    pub fn property(&mut self, id: Property, value: u32) { self.world.property(id, value) }

    pub fn property_word(&mut self, id: Property, value: u16) { self.world.property_word(id, value) }
}

/// Readings of a record whose every value is zero, for `lens`.
pub fn blank_readings(lens: u32) -> RecordReadings {
    RecordReadings {
        record: handle(),
        measurement_selection: Default::default(),
        scene: 0,
        channel: 0,
        at_0043: 0,
        class: 0,
        policy: 0,
        lens,
        limit_word: 0,
        level_step: 0,
        at_0035: 0,
        at_0038: 0,
        at_20cc: 0,
        at_20c8: 0,
        at_20d0: 0,
        locked: 0,
        row_scale: [0; 2],
        selection_value: 0,
        at_0164: 0,
        at_0168: 0,
        request: FocusRequest::default(),
        confirmation: [0; 8],
        area: Default::default(),
        at_0028: 0,
        levels: [0; 5],
        at_00a4: 0,
        at_00b8: 0,
        measured_level: 0,
        load_test: false,
        at_005f: 0,
        at_005b: 0,
        compares_subjects: 0,
        area_positions: [0; 10],
        armed: 0,
        auxiliary: 0,
        area_words: [0; 2],
        drive_kind: Default::default(),
        reference: 0,
        at_004e: 0,
        at_003c: 0,
        priority: 0,
        at_0039: 0,
        at_003a: 0,
        at_003b: 0,
        designation: [0; 7],
        at_0194: (0, 0),
        record_3e: 0,
        lens_state: 0,
    }
}

/// A prepared state of a blank record whose frame exposures last `frame_period`.
pub fn blank_state(section: SharedAddress, results: SharedAddress, frame_period: u32) -> MotionState {
    let second = FrameExposure { time: frame_period, ..FrameExposure::default() };
    MotionState {
        section,
        results,
        prepared_at: Ticks(0),
        readings: blank_readings(0),
        limits: StepLimits::default(),
        search: SearchSetup::default(),
        areas: AreaList::default(),
        exposures: FrameExposures { first: FrameExposure::default(), second, prepared_second: second },
        program: [ProgramStep::default(); PROGRAM_STEPS],
        previous: PreviousFocus::default(),
        bounds: SearchBounds::default(),
        statistics: Statistics {
            cell_size: [0; 2],
            trace: section,
            rates: [0; 4],
            channel_scales: [0; 3],
            channel_weights: [0; 3],
            exposure_offset: 0,
            level_bounds: [0; 2],
            curve_table: Property(0),
            class: 0,
            area: GridArea::default(),
            buffers: [section; 5],
        },
        treated_settings: [0; 4],
        plan: Plan::default(),
        sampling: SampleSchedule::default(),
        layout: SampleLayout::default(),
        drive: Drive::default(),
        level_check: LevelCheck::default(),
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Lists {
    pub subjects: u8,
    pub single_subject: u8,
    pub areas: AreaList,
    pub area_positions: [i16; AREA_POSITIONS],
    pub reference: u32,
    pub area_words: [u32; 2],
}

impl SelectionLists for Lists {
    fn subjects(&mut self) -> u8 { self.subjects }
    fn single_subject(&mut self) -> u8 { self.single_subject }
    fn areas(&mut self) -> AreaList { self.areas }
    fn area_positions(&mut self) -> [i16; AREA_POSITIONS] { self.area_positions }
    fn reference(&mut self) -> u32 { self.reference }
    fn area_words(&mut self) -> [u32; 2] { self.area_words }
}

/// The linear factor of a level is the level plus 1000; a difference is `value - reference`.
impl LevelScale for MotionWorld {
    fn linear(&mut self, level: i16) -> u32 { (i32::from(level) + 1000) as u32 }
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16 { (value as i32 - reference as i32) as i16 }
}

impl Properties for MotionWorld {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl Lens for MotionWorld {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.world.level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.world.base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.world.reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.world.calibration_index(lens) }
}

impl CaptureProfiles for MotionWorld {
    fn level_offset(&mut self, profile: u32) -> i16 { self.world.level_offset(profile) }
    fn class_offset(&mut self, class: u8) -> i16 { self.world.class_offset(class) }
    fn offset_class(&mut self, profile: u32) -> u8 { self.world.offset_class(profile) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { self.world.sensitivity_class(profile) }
    fn sensitivity_class_for(&mut self, value: u32) -> u8 { self.world.sensitivity_class_for(value) }
    fn configured_level(&mut self) -> u16 { self.world.configured_level() }
    fn configured_profile(&mut self, level: i16) -> u32 { self.world.configured_profile(level) }
    fn record_profile(&mut self) -> u32 { self.world.record_profile() }
    fn default_dimensions(&mut self) -> [u16; 3] { self.world.default_dimensions() }
    fn grid_steps(&mut self, layout: u8) -> [u8; 2] { self.world.grid_steps(layout) }
}

impl StockMemory for MotionWorld {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.world.read_u8(at) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.world.read_u16(at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.world.read_u32(at) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.world.write_u8(at, value) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.world.write_u16(at, value) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.world.write_u32(at, value) }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { self.world.fill(at, bytes, value) }
}

impl ProfileAccess for MotionWorld {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

/// Positions are ten per step of difference; a period's position is in `period_positions`.
impl LensTables for MotionWorld {
    fn channel_positions(&mut self, _lens: u32, step: u32, _reference: u32, _mode: u32) -> [i16; 2] { self.table_positions.get(&step).copied().unwrap_or_default() }
    fn side_adjusted(&mut self, value: i16, _from: u32, _to: u32) -> i16 { value }
    fn table_offset(&mut self, _lens: u32, _step: u32, delta: i16) -> i32 { i32::from(delta) * 10 }
    fn channel_start(&mut self, _lens: u32, channel: u8, _reference: u32) -> i16 { self.channel_starts.get(&channel).copied().unwrap_or(0) }
    fn period_position(&mut self, period: u16, _lens: u32, _step: u32, _reference: u32) -> i32 { self.period_positions.get(&period).copied().unwrap_or(0) }
}

impl LensCalibration for MotionWorld {
    fn steps(&mut self) -> u32 { self.steps }
    fn lowest_step(&mut self, lens: u32) -> u32 { self.lowest_steps.get(&lens).copied().unwrap_or(0) }
    fn highest_step(&mut self, lens: u32) -> u32 { self.highest_steps.get(&lens).copied().unwrap_or(self.steps - 1) }
    fn depth_positions(&mut self, _lens: u32, step: u32, depth: u16) -> i16 { self.depth_positions.get(&step).copied().unwrap_or(1).wrapping_mul(depth as i16) }
    fn table_entry(&mut self, _lens: u32, _table: Property, _entries: u32) -> u32 { 0 }
    fn lens_state(&mut self) -> u32 { self.lens_state }
    fn drive_kind(&mut self) -> DriveKind { self.drive_kind }
    fn lens_limits(&mut self, query: &LimitQuery) -> LensLimits {
        self.limit_queries.push(*query);
        self.lens_limits
    }
}

impl DriveSpeed for MotionWorld {
    fn top_speed(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 { self.top_speed }
}

impl ExposureTimes for MotionWorld {
    fn exposure_time(&mut self, profile: u32, level: i16, gain: u32) -> u32 { self.times.get(&(profile, level, gain)).copied().unwrap_or(self.default_time) }
}

impl ProfileGains for MotionWorld {
    fn gain_level(&mut self, profile: u32, gain: u32) -> i16 { self.gain_levels.get(&(profile, gain)).copied().unwrap_or(0) }
    fn shortest_time(&mut self, profile: u32) -> u32 { self.shortest_times.get(&profile).copied().unwrap_or(0) }
}

impl LensStatus for MotionWorld {
    fn control_state(&mut self) -> u32 { self.control_state }
}

impl CaptureFrames for MotionWorld {
    fn frame_size(&mut self, mode: u32) -> FrameSize { self.frames.get(&mode).copied().unwrap_or_default() }
}

impl MeasurementReadings for MotionWorld {
    fn selected_area(&mut self, _record: RecordHandle) -> GridArea { self.selected_area }
    fn selection_value(&mut self, _record: RecordHandle, _lens: u32) -> u32 { self.selection_value }
}

impl LensConfiguration for MotionWorld {
    fn channel_value(&mut self, channel: i32) -> u32 { self.world.channel_value(channel) }
    fn channel_curve(&mut self, channel: i32) -> u32 { self.world.channel_curve(channel) }
    fn bias(&mut self, resource: i32, table: BiasTable) -> i16 { self.world.bias(resource, table) }
    fn biased_position(&mut self, resource: i32, table: BiasTable) -> i16 { self.world.biased_position(resource, table) }
    fn resource_count(&mut self) -> u8 { self.world.resource_count() }
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32 { self.world.selection_word(channel, flags, offset) }
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32 { self.world.remap(word, channel, mode) }
    fn position_limits(&mut self, key: u32) -> [i16; 2] { self.world.position_limits(key) }
    fn mapped_value(&mut self, key: u32) -> u32 { self.world.mapped_value(key) }
}

impl LensBounds for MotionWorld {
    fn search_bounds(&mut self, query: &BoundsQuery) -> SearchBounds {
        self.bounds_queries.push(*query);
        self.bounds.get(&query.channel).copied().unwrap_or_default()
    }
}

impl Logarithms10 for MotionWorld {
    fn log10(&mut self, value: f32) -> f32 { value.log10() }
}

impl FrameGeometry for MotionWorld {
    fn search_frames(&mut self, request: &SearchFrameRequest) -> [AreaGrid; 2] {
        self.search_frame_requests.push(*request);
        self.search_frames
    }
    fn statistics_frame(&mut self, _request: &StatisticsFrameRequest) -> [u32; 2] { self.statistics_frame }
    fn sample_frames(&mut self, request: &SampleFrameRequest) -> [AreaGrid; 2] {
        self.sample_frame_requests.push(*request);
        self.sample_frames
    }
    fn crop_geometry(&mut self, _framing: &CaptureFraming) -> CropGeometry { self.crop }
    fn cell_steps(&mut self, _profile: u32) -> [u32; 2] { self.cell_steps }
    /// Doubles the value.
    fn expanded_ratio(&mut self, value: u32) -> u32 { value * 2 }
    fn feature_class(&mut self, profile: u32) -> u8 { self.feature_classes.get(&profile).copied().unwrap_or(0) }
}

/// The selector adds to the first point and each delta to the next point.
impl AreaCurves for MotionWorld {
    fn with_profile_factors(&mut self, [first, second, third]: [u16; 3], [first_delta, second_delta]: [u8; 2], selector: u8) -> [u16; 3] {
        [first + u16::from(selector), second + u16::from(first_delta), third + u16::from(second_delta)]
    }
}

impl CaptureGains for MotionWorld {
    fn gains(&mut self, request: &GainRequest) -> SelectedGains {
        self.gain_requests.push(*request);
        self.selected_gains
    }
    fn blend_weight(&mut self, _selector: &BlendSelector) -> u8 { 0 }
}

impl ChannelRates for MotionWorld {
    fn channel_rate(&mut self, lens: u32, channel: u8) -> u32 { self.channel_rates.get(&(lens, channel)).copied().unwrap_or(0) }
}

/// The floor of a hundred times the binary logarithm.
impl Logarithms for MotionWorld {
    fn centi_log2(&mut self, value: u64) -> Option<u32> { (value != 0).then(|| (100.0 * (value as f64).log2()).floor() as u32) }
}

impl CaptureTiming for MotionWorld {
    fn readout_start(&mut self, _source: u32) -> u32 { self.readout_start }
    fn reference_line(&mut self, _source: u32) -> u16 { self.reference_line }
    fn line_period(&mut self, source: u32) -> Option<u32> { self.line_periods.get(&source).copied() }
}

impl MeteringTables for MotionWorld {
    fn channel_weights(&mut self) -> [u16; 3] { self.channel_weights }
    fn feature_bytes(&mut self, _profile: u32) -> [u8; 2] { self.feature_bytes }
}

impl Clock for MotionWorld {
    fn now(&mut self) -> Ticks { Ticks(self.now) }
    fn elapsed(&mut self, since: Ticks) -> u32 { self.now.wrapping_sub(since.0) * 10 }
}
