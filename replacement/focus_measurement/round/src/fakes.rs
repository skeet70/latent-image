use crate::auxiliary::{self, AuxiliaryProfile, AuxiliaryPublication, AuxiliaryReading, AuxiliarySettings, Curve, CurveInputs, RequestProfile, ResourceIndex};
use crate::image::{self, ChannelCount, ImageLog, ImageLogging, LensReport, ReplyPorts};
use crate::lanes::Selection;
use crate::planner::{Cadence, Mode, Work};
use crate::pipeline::{Exposure, GainStep, HistoryStatistics, Pipeline, PipelineSnapshot, PipelineWord, SnapshotStatus, Statistics, TimingWord};
use crate::profile::{FrameInterval, FrameRate, GridCell, IntervalHistory, IntervalTracking, Profile, ProfileGeometry, ProfileMode, ProfileRecords, ProfileSelection, PublishedResult, RetainedWord, StatisticsInUse, WindowPosition, WindowRecord, WindowStatus};
use crate::record::{self, Adjustment, AdjustmentWords, AlternateSearch, Power, Request, Settings, Trace, TraceMark};
use crate::source::{self, SampleProfile, SourceProfile, SourcePublication, SourceSettings, SourceStrength, Summary, ProcessingRequest};
use crate::window::{self, WindowResult};
use crate::replies;
use xm1_saclac_interface::round::{GeometryCalibration, GeometryPair, RetainedGeometryValue};
use xm1_saclac_interface::round_image::{ImageArea, ImageChannel, ImageLaneRequest};
use xm1_saclac_interface::stock::StockCallbackWord;
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
                takes_search_adjustment: false,
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
    pub rotations: usize,
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
            rotations: 0,
        }
    }
}

/// What the source measurement last published, as its readers see it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Published {
    Withdrawn,
    Fresh { summary: Summary, strength: SourceStrength },
    Averaged { summary: Summary, means: Summary, strength: SourceStrength },
}

/// The source measurement's helpers and its part of the shared record.
pub struct SourceRecords {
    pub profile: SourceProfile,
    pub sample: SampleProfile,
    pub settings: SourceSettings,
    /// What each processing request summarises to, by call; the last repeats.
    pub summaries: Vec<Summary>,
    pub requests: Vec<ProcessingRequest>,
    pub callbacks_enabled: Option<bool>,
    pub published: Option<Published>,
    /// The strength the shared record shows; a write that the grant refuses leaves it.
    pub strength: SourceStrength,
}

impl Default for SourceRecords {
    fn default() -> Self {
        Self {
            profile: SourceProfile::default(),
            sample: SampleProfile::default(),
            settings: SourceSettings::default(),
            summaries: Vec::new(),
            requests: Vec::new(),
            callbacks_enabled: None,
            published: None,
            strength: SourceStrength(0),
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
    NotifyImageClient(StockCallbackWord),
    PrepareImageArea(ImageChannel, ImageArea),
    ImageLog(ImageLog),
    Image(ImageLaneRequest),
    Power(bool),
    Trace(TraceMark),
    Acquisition(AcquisitionTraffic),
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
    pub image_client: Option<StockCallbackWord>,
    pub image_logging: ImageLogging,
    /// What the area configuration leaves, from the channel and the previous halfwords.
    pub image_area: fn(ImageChannel, ImageArea) -> ImageArea,
    pub auxiliary: AuxiliaryRecords,
    pub source: SourceRecords,
    /// The relative exposure of every exposure word.
    pub relative_exposure: i16,
    pub adjustment: AdjustmentWords,
    pub acquisition: AcquisitionRecords,
    pub faults: usize,
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
            image_client: None,
            image_logging: ImageLogging::default(),
            image_area: |_, area| area,
            auxiliary: AuxiliaryRecords::default(),
            source: SourceRecords::default(),
            relative_exposure: 0,
            adjustment: AdjustmentWords::default(),
            acquisition: AcquisitionRecords::default(),
            faults: 0,
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
    fn relative_exposure(&mut self, _: PipelineWord) -> i16 { self.relative_exposure }
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

impl<X> image::Ports for Ports<X> {
    fn image_client_callback(&mut self) -> Option<StockCallbackWord> { self.image_client }
    fn notify_image_client(&mut self, callback: StockCallbackWord) { self.outbound.push(Outbound::NotifyImageClient(callback)); }
    fn prepare_image_area(&mut self, channel: ImageChannel, area: ImageArea) -> ImageArea {
        self.outbound.push(Outbound::PrepareImageArea(channel, area));
        (self.image_area)(channel, area)
    }
    fn image_logging(&mut self) -> ImageLogging { self.image_logging }
    fn log_image(&mut self, entry: ImageLog) { self.outbound.push(Outbound::ImageLog(entry)); }
    fn request_image(&mut self, request: ImageLaneRequest) { self.outbound.push(Outbound::Image(request)); }
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
    fn rotate_adjustment_snapshot(&mut self, _: RecordHandle) { self.auxiliary.rotations += 1; }
}

impl<X> Adjustment for Ports<X> {
    fn adjustment_words(&mut self) -> AdjustmentWords { self.adjustment }
}

impl<X> source::Ports for Ports<X> {
    fn source_profile(&mut self, _: RecordHandle) -> SourceProfile { self.source.profile }
    fn sample_profile(&mut self, _: RecordHandle) -> SampleProfile { self.source.sample }
    fn source_settings(&mut self) -> SourceSettings { self.source.settings }
    fn processing_summary(&mut self, request: &ProcessingRequest) -> Summary {
        let index = self.source.requests.len().min(self.source.summaries.len().saturating_sub(1));
        self.source.requests.push(*request);
        self.source.summaries.get(index).copied().unwrap_or(Summary::ZERO)
    }
    fn enable_processing_callbacks(&mut self, enabled: bool) { self.source.callbacks_enabled = Some(enabled); }
    fn published_strength(&mut self) -> SourceStrength { self.source.strength }
    fn publish_source(&mut self, _: RecordHandle, publication: SourcePublication<'_>) {
        if !self.records.granted { return; }
        let (published, strength) = match publication {
            SourcePublication::Withdrawn => (Published::Withdrawn, SourceStrength(0)),
            SourcePublication::Fresh { summary, strength } => (Published::Fresh { summary: *summary, strength }, strength),
            SourcePublication::Averaged { summary, means, strength } => (Published::Averaged { summary: *summary, means: *means, strength }, strength),
        };
        self.source.published = Some(published);
        self.source.strength = strength;
    }
}

use crate::acquisition::ports::{
    AcquisitionBlock, AcquisitionSettings, Branch as AcquisitionBranch, ExposurePublication, ExposureRecordUpdate, GridInputs,
    GridReference, ModeInputs, PositionInputs, PositionSlot, Program, ProgramChoice, ProgramInputs, ProgramTarget,
    PublishedExposure, Range, RangeInputs, RateNotice, RateRecord, RecordView, Release, ReleasedResult, SelectionNotice,
    SelectionPosition, SelectionRecord, SelectionSlot, SettingEcho, ShotContext, StatisticsBuffer, TargetInputs,
};
use crate::acquisition::selection::SelectionMemory;
use xm1_saclac_interface::calc::{AcquisitionGrid, SharedAddress};
use xm1_saclac_interface::round_acquisition::{
    AcquisitionRequest, AcquisitionResult, ChangeNotice, FocusTable, PrimaryLevels, RestoredSetting, SavedExposure,
    SavedSelection, SelectionLevels, SelectionLimits, SelectionRequest, SelectionResult, SelectionSetting, SelectionTable,
    SelectionWindow, ShotRecordLevels,
};

/// What the acquisition sends out of the round, as the fake records it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcquisitionTraffic {
    Acquisition(AcquisitionBranch, AcquisitionRequest),
    Selection(AcquisitionBranch, SelectionRequest),
    Exposure(ExposurePublication),
    ChangeNotice(ChangeNotice),
    SelectionTable(SelectionTable),
    FocusTable(FocusTable),
    Selected(SelectionNotice),
    Released(ReleasedResult),
    /// The release's statistics; their content is in `AcquisitionRecords::releases`.
    Release,
}

/// One release's statistics as the port received them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseRecord {
    pub classes: [u8; 256],
    pub offsets: [[u16; 256]; 4],
    pub snapshot: Option<(Vec<u16>, StatisticsBuffer)>,
}

/// The profile record and layer values the acquisition reads, and what it last wrote back.
pub struct AcquisitionRecords {
    pub view: RecordView,
    pub published: PublishedExposure,
    pub block: AcquisitionBlock,
    pub selection: SelectionRecord,
    pub slot: SelectionSlot,
    pub memory: SelectionMemory,
    pub settings: AcquisitionSettings,
    pub program_kind: u32,
    pub program: Program,
    pub range: Range,
    pub target: ProgramTarget,
    pub lens: ResourceIndex,
    pub statistics: [u16; 256],
    /// The extra statistics the release reads, from the first.
    pub extra: [u16; 600],
    pub releases: Vec<ReleaseRecord>,
    pub acquisition_result: Option<AcquisitionResult>,
    pub selection_result: Option<SelectionResult>,
    pub word: u32,
}

impl Default for AcquisitionRecords {
    fn default() -> Self {
        Self {
            view: RecordView::default(),
            published: PublishedExposure::default(),
            block: AcquisitionBlock::default(),
            selection: SelectionRecord::default(),
            slot: SelectionSlot::default(),
            memory: SelectionMemory::new(),
            settings: AcquisitionSettings::default(),
            program_kind: 0,
            program: Program(0),
            range: Range::default(),
            target: ProgramTarget::default(),
            lens: ResourceIndex(0),
            statistics: [0; 256],
            extra: [0; 600],
            releases: Vec::new(),
            acquisition_result: None,
            selection_result: None,
            word: 0,
        }
    }
}

impl<X> Ports<X> {
    fn send(&mut self, traffic: AcquisitionTraffic) { self.outbound.push(Outbound::Acquisition(traffic)); }
}

impl<X> crate::acquisition::ports::Ports for Ports<X> {
    fn selection_memory(&mut self) -> &mut SelectionMemory { &mut self.acquisition.memory }
    fn acquisition_settings(&mut self) -> AcquisitionSettings { AcquisitionSettings { traces: self.traces, ..self.acquisition.settings } }
    fn record_view(&mut self, _: RecordHandle) -> RecordView { self.acquisition.view }
    fn published_exposure(&mut self, _: RecordHandle) -> PublishedExposure { self.acquisition.published }
    fn publish_exposure(&mut self, _: RecordHandle, publication: &ExposurePublication) {
        let published = &mut self.acquisition.published;
        published.exposure = publication.exposure;
        published.level = publication.level;
        published.second = publication.second;
        published.correction = publication.correction;
        published.aspect = publication.aspect;
        published.reference = publication.reference;
        published.offset = publication.offset;
        published.gain_step = publication.gain_step;
        published.timing = publication.timing;
        published.code = publication.code;
        published.selected = publication.selected;
        published.adjustment = publication.adjustment;
        published.combined = publication.combined;
        published.reference_level = publication.reference_level;
        self.send(AcquisitionTraffic::Exposure(*publication));
    }
    fn publish_primary_levels(&mut self, _: RecordHandle, levels: &PrimaryLevels) {
        self.acquisition.published.primary_levels = [levels.levels[0], levels.levels[1], levels.levels[2], levels.levels[3]];
    }
    fn publish_grid(&mut self, _: RecordHandle, _: &AcquisitionGrid) {}
    fn publish_selection_levels(&mut self, _: RecordHandle, levels: &SelectionLevels) { self.acquisition.published.selection_reference = levels.reference; }
    fn update_exposure_record(&mut self, _: RecordHandle, _: ExposureRecordUpdate) {}
    fn store_lens(&mut self, _: RecordHandle, lens: ResourceIndex) { self.acquisition.published.lens = lens; }
    fn store_reference(&mut self, _: RecordHandle, reference: u16) { self.acquisition.published.reference = reference; }
    fn store_selection_reference(&mut self, _: RecordHandle, reference: u16) { self.acquisition.published.selection_reference = reference; }
    fn program_kind(&mut self, _: RecordHandle) -> u32 { self.acquisition.program_kind }
    fn store_program_kind(&mut self, _: RecordHandle, kind: u32) { self.acquisition.program_kind = kind; }
    fn stored_program(&mut self, _: RecordHandle) -> Program { self.acquisition.program }
    fn store_program(&mut self, _: RecordHandle, program: Program) { self.acquisition.program = program; }
    fn store_program_word(&mut self, _: RecordHandle, word: [u32; 2]) { self.acquisition.view.program_word = word; }
    fn store_program_choice(&mut self, _: RecordHandle, _: &ProgramChoice) {}
    fn acquisition_block(&mut self, _: RecordHandle) -> AcquisitionBlock { self.acquisition.block }
    fn store_acquisition_block(&mut self, _: RecordHandle, block: &AcquisitionBlock) { self.acquisition.block = *block; }
    fn selection_record(&mut self, _: RecordHandle) -> SelectionRecord { self.acquisition.selection }
    fn store_selection_record(&mut self, _: RecordHandle, selection: &SelectionRecord) { self.acquisition.selection = *selection; }
    fn selection_slot(&mut self, _: RecordHandle) -> SelectionSlot { self.acquisition.slot }
    fn publish_selection_slot(&mut self, _: RecordHandle, slot: &SelectionSlot) { self.acquisition.slot = *slot; }
    fn store_slot_word(&mut self, _: RecordHandle, word: u32) { self.acquisition.slot.word = word; }
    fn position_pair(&mut self, _: RecordHandle, _: PositionSlot) -> [u32; 2] { [0; 2] }
    fn store_position_pair(&mut self, _: RecordHandle, _: PositionSlot, _: [u32; 2]) {}
    fn store_restore_levels(&mut self, _: RecordHandle, _: u16, _: [u16; 2]) {}
    fn store_s1_level(&mut self, _: RecordHandle, _: i16) {}
    fn clear_grid_selection(&mut self, _: RecordHandle) {}
    fn acquisition_word(&mut self, _: RecordHandle) -> u32 { self.acquisition.word }
    fn store_acquisition_word(&mut self, _: RecordHandle, word: u32) { self.acquisition.word = word; }
    fn rate_record(&mut self, _: RecordHandle) -> RateRecord { RateRecord::default() }
    fn store_rate_selection(&mut self, _: RecordHandle, _: u32) {}
    fn clear_selection_reuse(&mut self, _: RecordHandle) {}
    fn clear_controller_reuse(&mut self) {}
    fn notify_selection(&mut self, _: RecordHandle, notice: &SelectionNotice) { self.send(AcquisitionTraffic::Selected(*notice)); }
    fn notify_rate(&mut self, _: RecordHandle, _: &RateNotice) {}
    fn current_lens(&mut self) -> ResourceIndex { self.acquisition.lens }
    fn lens_word(&mut self, _: RecordHandle, lens: Option<ResourceIndex>) -> u32 { lens.map_or(0xffff, |lens| lens.0) }
    fn collapse_index(&mut self, exposure: PipelineWord) -> u32 { exposure.0 }
    fn mapped_value(&mut self, first: u32, second: u32) -> u32 { first ^ second }
    fn lens_position(&mut self, _: [u32; 2], position: u32) -> u32 { position }
    fn gain_step_for(&mut self, offset: i32) -> GainStep { GainStep(offset as u32) }
    fn acquisition_gain_step(&mut self, _: u32) -> GainStep { GainStep(3) }
    fn kind_decade(&mut self, kind: u32) -> u32 { kind }
    fn change_positions(&mut self, _: RecordHandle) -> [u32; 2] { [0; 2] }
    fn select_program(&mut self, inputs: &ProgramInputs) -> Option<Program> { Some(Program(inputs.kind)) }
    fn program_adjustment(&mut self, _: Program) -> u32 { 0 }
    fn program_range_limit(&mut self, _: Program) -> u32 { 0 }
    fn program_policy(&mut self, _: Program) -> u32 { 0 }
    fn program_secondary_gate(&mut self, _: Program) -> u32 { 0 }
    fn program_scale(&mut self, _: Program) -> u32 { 0 }
    fn program_percent(&mut self, _: Program) -> u16 { 0 }
    fn program_target(&mut self, _: &TargetInputs) -> ProgramTarget { self.acquisition.target }
    fn program_range(&mut self, _: &RangeInputs) -> Range { self.acquisition.range }
    fn program_lower_bound(&mut self, _: Program, _: u32) -> u32 { 0 }
    fn profile_percent(&mut self, _: Program, _: u16, _: ResourceIndex, _: u32) -> u16 { 0 }
    fn grid_count(&mut self, _: u32, _: u32, _: u32, _: u32) -> u32 { 0 }
    fn indexed_configuration(&mut self, _: u32, _: u32, _: u32) -> u8 { 0 }
    fn level_category(&mut self, _: i16, _: u32, _: u32) -> u32 { 0 }
    fn selection_limits(&mut self, _: u32, _: u32, _: u32, _: u32) -> SelectionLimits { SelectionLimits::default() }
    fn fill_grid(&mut self, _: &GridInputs, _: &mut AcquisitionGrid) {}
    fn snapshot_grid(&mut self, _: Statistics, _: &mut AcquisitionGrid) {}
    fn adjustment_class(&mut self, _: u32, _: i16) -> u8 { 0 }
    fn adjustment_option(&mut self) -> u32 { 0 }
    fn refreshed_position(&mut self) -> u32 { 0 }
    fn mode_one(&mut self, _: &ModeInputs) -> u32 { 0 }
    fn both_modes(&mut self, _: &ModeInputs) -> u32 { 0 }
    fn five_parameter_adjustment(&mut self, _: u32, _: u32, _: u32, _: u32, _: u32) -> u32 { 0 }
    fn rate_divisor(&mut self, _: u32, _: u32, _: ResourceIndex, _: u16) -> u32 { 0 }
    fn rate_limit(&mut self, _: u32, _: u32, _: u16) -> u32 { 0 }
    fn rate_mode(&mut self, _: u32, _: u16, _: u32, _: u32) -> u16 { 0 }
    fn selection_mode_category(&mut self, _: u32) -> u32 { 0 }
    fn offset_position(&mut self, level: i16, _: u16, _: u32) -> u16 { level as u16 }
    fn clamp_selection_difference(&mut self, _: u32, _: ResourceIndex, level: i16, _: u16, _: u32, _: i16) -> i16 { level }
    fn has_valid_offset(&mut self, _: u32, _: u32, _: i16) -> u32 { 0 }
    fn allows_unflagged_selection(&mut self, _: u32, _: u16) -> bool { true }
    fn capture_selection_position(&mut self, _: AcquisitionBranch, _: &PositionInputs) -> SelectionPosition { SelectionPosition::default() }
    fn copy_rate_grid_source(&mut self, _: StatisticsBuffer) {}
    fn rate_pair(&mut self, _: RecordHandle, _: SharedAddress, _: u32, _: &mut [u32; 2]) {}
    fn clear_rate_configuration(&mut self, _: RecordHandle) {}
    fn rate_flag(&mut self, _: RecordHandle, _: u32) {}
    fn draw_rate(&mut self, _: SharedAddress, _: u32) {}
    fn draw_rate_status(&mut self, _: u16, _: u32, _: u32) {}
    fn rate_position(&mut self, _: RecordHandle) -> (u32, u16) { (0, 0) }
    fn statistics_buffer(&mut self, _: StatisticsBuffer) -> [u16; 256] { self.acquisition.statistics }
    fn extra_statistics(&mut self, values: &mut [u16]) { values.copy_from_slice(&self.acquisition.extra[..values.len()]); }
    fn store_release(&mut self, release: &Release<'_>) {
        self.acquisition.releases.push(ReleaseRecord {
            classes: *release.classes,
            offsets: *release.offsets,
            snapshot: release.snapshot.map(|snapshot| (snapshot.extra.to_vec(), snapshot.copy)),
        });
        self.send(AcquisitionTraffic::Release);
    }
    fn set_pipeline_reference(&mut self, _: i16) {}
    fn prepare_s1_sample(&mut self) {}
    fn s1_sample(&mut self) -> PipelineSnapshot { self.snapshot }
    fn post_change_notice(&mut self, notice: &ChangeNotice) { self.send(AcquisitionTraffic::ChangeNotice(*notice)); }
    fn post_selection_table(&mut self, table: &SelectionTable) { self.send(AcquisitionTraffic::SelectionTable(*table)); }
    fn post_focus_table(&mut self, table: &FocusTable) { self.send(AcquisitionTraffic::FocusTable(*table)); }
    fn shot_reference_level(&mut self) -> u16 { 0 }
    fn shot_selection_word(&mut self) -> u32 { 0 }
    fn prepare_selection_defaults(&mut self, _: RecordHandle) -> ShotContext { ShotContext::default() }
    fn restore_selection_setting(&mut self, _: AcquisitionBranch, _: ResourceIndex) {}
    fn copy_reference_backup(&mut self, _: AcquisitionBranch) {}
    fn write_saved_settings(&mut self, _: SharedAddress) {}
    fn copy_selection_to_saved(&mut self, _: SharedAddress) {}
    fn restore_selection_if_enabled(&mut self, _: RecordHandle, _: u32) {}
    fn apply_selection_value_change(&mut self) {}
    fn save_selection(&mut self, _: u32, _: &SavedSelection, _: u32) {}
    fn selected_grid_reference(&mut self, _: RecordHandle, reference: GridReference) -> GridReference { reference }
    fn refresh_hardware_state(&mut self) {}
    fn hardware_ready(&mut self) -> bool { false }
    fn clock_word(&mut self) -> u32 { 0 }
    fn store_selection_window(&mut self, _: AcquisitionBranch, _: &SelectionWindow, _: &AcquisitionGrid) {}
    fn store_selection_setting(&mut self, _: AcquisitionBranch, _: &SelectionSetting) {}
    fn store_restored_setting(&mut self, _: AcquisitionBranch, _: &RestoredSetting) {}
    fn setting_echo(&mut self, _: AcquisitionBranch) -> SettingEcho { SettingEcho::default() }
    fn request_acquisition(&mut self, branch: AcquisitionBranch, request: &AcquisitionRequest, _: &AcquisitionGrid) {
        self.send(AcquisitionTraffic::Acquisition(branch, *request));
    }
    fn request_selection(&mut self, branch: AcquisitionBranch, request: &SelectionRequest) {
        self.send(AcquisitionTraffic::Selection(branch, *request));
    }
    fn acquisition_result(&mut self, _: Completed<'_>) -> Option<AcquisitionResult> { self.acquisition.acquisition_result }
    fn selection_result(&mut self, _: Completed<'_>) -> Option<SelectionResult> { self.acquisition.selection_result }
    fn shot_record_levels(&mut self, _: SharedAddress) -> ShotRecordLevels { ShotRecordLevels::default() }
    fn saved_exposure(&mut self, _: SharedAddress) -> SavedExposure { SavedExposure::default() }
    fn publish_saved_exposure(&mut self, _: RecordHandle, _: &SavedExposure) {}
    fn release_result(&mut self, released: ReleasedResult) { self.send(AcquisitionTraffic::Released(released)); }
}

impl<X> crate::children::Ports for Ports<X> {
    fn fault(&mut self) { self.faults += 1; }
}
