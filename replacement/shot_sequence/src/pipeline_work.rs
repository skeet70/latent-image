#![forbid(unsafe_code)]

//! Leaf work of the half-press pipeline (machine 28, with machine 20's bind): the plan the motion
//! library makes for the half press, the frame, scan, tone and gradation requests and what their
//! results leave behind, the focus outcome published into the bound record, and the pipeline
//! state the finish reads.

use crate::boundary::{Client, Frame, Lens, LensPoint, LensReference};
use crate::finish_work::{AreaPositions, CorrectionRecord, FinishPipeline};
use crate::ports::{PipelineEffect as E, PipelinePhase, PipelineQuery as Q, PipelineRequest as R};
use crate::preparation::{HalfPressParameters, Prepared};
use crate::stock::pipeline::{self as layout, ToneWord};
use xm1_saclac_interface::pipeline::{
    CaptureArea, FrameRequest, GradationCalculation, GradationRequest, RawCalculation, RawCheck, RawFrameRequest, ScanRequest, SelectionRequest,
    StatisticsSetup, StatisticsWeights, SubjectGeometry, ToneCalculation, ToneProgress, ToneRequest, ToneStatus, STATISTICS_WEIGHTS, TONE_LEVELS,
};
use xm1_saclac_interface::stock::pipeline as results;
use xm1_saclac_interface::{Completed, Completion, RecordHandle};

const IMMEDIATE_MODE: u32 = 5;
const FINISH_ONLY_MODE: u32 = 6;
/// The load-test setting that checks the published position against the test's window.
const LOAD_TEST_WINDOW: u32 = 99;
/// Profile-record mode the frame synchronization begins and finishes.
const FRAME_RECORD_MODE: u32 = 4;
/// A focus status above this has no point.
const LAST_FOUND_STATUS: u32 = 1;
const NO_POSITION_CODE: u16 = 0xffff;
const NO_POSITION: u16 = 0x7fff;
/// Scan phases in which a gradation result does not restart the tone scan.
const KEEPING_PHASES: [u32; 2] = [0, 3];
/// The scan phase that prepares no frame.
const UNPLANNED_PHASE: u32 = 3;
const FINAL_PHASE: u32 = 11;
/// The capture phase in which only a driven tone result in a tone-final phase ends the tone scan.
const LONG_CAPTURE_PHASE: u32 = 50;
const FINAL_LENS_MODE: u32 = 12;
/// Lens-block kinds that take no weighted statistics.
const UNWEIGHTED_LENS_KINDS: [u32; 2] = [1, 3];
/// The scan selection that drives the lens; it changes only once the lens moved far enough.
const DRIVING_SELECTION: u32 = 2;
const LAST_SELECTION: u32 = 2;
const CHANNEL_USED: u8 = 3;
const CHANNEL_END: u8 = 0xfd;
const AREA_GRID: u32 = 7;
const OUTCOME_AREAS: usize = 5;
const OUTCOME_POSITIONS: usize = 3;
/// Tone decisions the scan block records.
const DECISION_PENDING: u32 = 1;
const DECISION_FINAL: u32 = 3;
const DECISION_CONTINUE: u32 = 2;
const EXPOSURE_DIVISOR: u64 = 100_000_000;

/// A block of the motion library's memory that its plan names; the calc task reads it too.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanBlock(pub u32);

/// The pipeline's work area in the client's frame, in the view the pipeline writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkArea(pub u32);

/// What the motion library plans for a half press.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    pub frame_flags: FrameFlags,
    pub subject: PlanBlock,
    pub capture: PlanBlock,
    pub lens: PlanBlock,
    pub scan: PlanBlock,
    pub image_buffer: u32,
    /// Where the capture sample copies its banks.
    pub sample_targets: [PlanBlock; 5],
    pub adjustment: [PlanBlock; 7],
    pub raw_selection: PlanBlock,
    pub raw_subject: PlanBlock,
    pub raw_first: PlanBlock,
    pub raw_second: PlanBlock,
    /// Where calc 0x3c03's result goes.
    pub raw_result: PlanBlock,
}

/// The plan's inputs besides the shot context the library reads in place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanInput {
    pub record: RecordHandle,
    pub lens_reference: u32,
    pub load_test_window: bool,
    pub area_count: u32,
    pub option: u32,
    pub frame_slot: u32,
}

/// Ready once the frame is prepared; the outcome bit stays until then.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameFlags(pub u8);

impl FrameFlags {
    const READY: u8 = 1;
    const OUTCOME: u8 = 1 << 1;
    pub const fn ready(self) -> bool { self.0 & Self::READY != 0 }
    pub const fn outcome(self) -> bool { self.0 & Self::OUTCOME != 0 }
    const fn prepared(self) -> Self { Self((self.0 | Self::READY) & !Self::OUTCOME) }
}

macro_rules! block {
    ($(#[$meta:meta])* $name:ident, $bytes:expr) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $name(pub [u8; $bytes]);

        impl Default for $name {
            fn default() -> Self { Self([0; $bytes]) }
        }
    };
}

block!(SubjectBlock, layout::SUBJECT_BYTES);
block!(CaptureBlock, layout::CAPTURE_BYTES);
block!(LensBlock, layout::LENS_BYTES);
block!(
    /// The motion library's scan block; a scan plan has its layout.
    ScanBlock,
    layout::SCAN_BYTES
);
block!(
    /// The capture's saved sample snapshot the gradation reads.
    SampleSnapshot,
    layout::SNAPSHOT_BYTES
);
block!(OutcomeHistory, layout::HISTORY_BYTES);

/// Writes into the scan block besides installing a scan plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanStore {
    ScanRequested,
    FirstSample(u16),
    Decision(u32),
    GradationReached,
    Selection(u32),
    FirstToneLimit(u32),
    SecondToneLimit(u32),
    Tone(ToneCalculation),
    Gradation(GradationCalculation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkStore {
    Phase(PipelinePhase),
    AdjustmentGate(bool),
    ToneMark(i16),
    Weights([u8; STATISTICS_WEIGHTS]),
    ChannelWindow { last: u8, count: u8 },
    ChannelMarks { channel: u8, marks: [u8; 4] },
}

/// What the motion library answers for the scan's focus result.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusResultOut {
    pub status: u32,
    pub point: LensPoint,
    pub pending: u32,
}

/// The focus result for a first configuration the client asked to be told of, or by the
/// library's policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultKind { Released, Policy }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjustmentPass { Primary, Secondary }

/// One of the three area blocks the record keeps for the outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AreaSlot { First, Second, Third }

/// The outcome's areas: a count, 20 bytes the scan block holds and the areas' grid cells.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaBlock {
    pub count: u8,
    pub source: [u8; 20],
    pub cells: [(u32, u32); OUTCOME_AREAS],
}

/// The focus outcome of a half press as the bound record keeps it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusOutcome {
    pub valid: bool,
    pub status: u32,
    pub point: LensPoint,
    pub source: u32,
    pub cell: (u32, u32),
    pub offset: i16,
    pub selected: u32,
    pub pending: u32,
    pub areas: AreaBlock,
    /// Entries `0..=last_position` are in use.
    pub positions: [(i16, u32); OUTCOME_POSITIONS + 1],
    pub last_position: u8,
}

/// Single outcome values the record stores, in stock's order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutcomeValue {
    Status(u32),
    PositionCode(u16),
    Point(LensPoint),
    AdjustedPoint(LensPoint),
    Cell(u32, u32),
    Offset(i16),
    Flag(u32),
    Source(u32),
}

/// Image-4 record values the statistics request carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsRecordValues {
    pub selection: u32,
    pub layout: u16,
    pub flags: [u8; 4],
    pub area: [u8; 16],
    pub extent: u32,
    pub sample_kind: u8,
}

/// The frame words the bind writes besides the record layer's stamp.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipelineFrame {
    pub resource: u32,
    pub bank: u32,
    pub lens_position: u32,
    pub lens_reference: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineLens { Position, Drive, Direction }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineSetting {
    ChannelStatistics,
    WeightDivisor,
    Weight(u8),
    ToneOffset,
    ExposureScale,
    ChannelThreshold,
    FirstToneFinal,
    SecondToneFinal,
    ChannelsForDecision,
    GatedToneFinal,
    ToneMask,
    SelectionOverride,
    SubjectKindLimit,
    Quadrant(u8),
    SelectionTable(u32),
    DrivingDistance,
    BoundedSelection,
    LoadTestTolerance,
    CorrectionTolerance,
    CorrectionBase,
    FallbackFirst,
    FallbackSecond,
    Adjustments,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineTrace { Prepare, RawRequested, FrameReleased, RawConcluded }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhaseAttribute { ToneFinal }

/// Calc-library entries (0xf4c000..0xf6b000) and the motion library behind them.
pub trait PipelineLibrary {
    fn plan(&mut self, input: &PlanInput) -> Option<Plan>;
    fn refresh_channels(&mut self);
    fn channel_item_ready(&mut self, record: RecordHandle) -> bool;
    fn apply_adjustment(&mut self, adjustment: &[PlanBlock; 7], pass: AdjustmentPass);
    fn adjustment_gated(&mut self) -> bool;
    /// The next scan plan, built from the scan block.
    fn build_scan_plan(&mut self, scan: PlanBlock) -> ScanBlock;
    fn focus_result(&mut self, scan: PlanBlock, kind: ResultKind) -> FocusResultOut;
    fn selected_value(&mut self) -> u32;
    fn needs_raw(&mut self) -> bool;
    fn conclude_raw(&mut self);
    fn store_converted_input(&mut self);
    fn begin_scan(&mut self);
    fn active_channels(&mut self) -> u8;
    fn release_second_channel(&mut self);
    fn position_correction(&mut self) -> u32;
    fn scaled_value(&mut self, scan: PlanBlock, index: u8) -> u16;
    fn scaled_levels(&mut self, scan: PlanBlock, value: u16, index: u8) -> [i16; TONE_LEVELS];
    /// Hands the capture's channel samples and the snapshot to the scan block's input.
    fn store_channel_samples(&mut self, scan: PlanBlock, snapshot: &SampleSnapshot);
    fn store_channel_sample(&mut self, scan: PlanBlock, sample: i16, index: u8);
    fn select_channel(&mut self, channel: u8);
    fn indexed_selection(&mut self, record: RecordHandle) -> u32;
    fn adjust_direction(&mut self, point: &LensPoint) -> i16;
    fn phase_attribute(&mut self, phase: u32, attribute: PhaseAttribute) -> bool;
    fn request_raw(&mut self, plan: &Plan, completion: Completion);
    fn request_tone(&mut self, request: &ToneRequest, completion: Completion);
    fn request_gradation(&mut self, request: &GradationRequest, completion: Completion);
    /// Calc 0x3c12.
    fn release_calculation(&mut self);
}

/// The motion library's blocks the plan names.
pub trait PlanMemory {
    fn subject(&mut self, block: PlanBlock) -> SubjectBlock;
    fn capture(&mut self, block: PlanBlock) -> CaptureBlock;
    fn lens(&mut self, block: PlanBlock) -> LensBlock;
    fn scan(&mut self, block: PlanBlock) -> ScanBlock;
    fn raw_selection(&mut self, block: PlanBlock) -> u32;
    fn raw_first_word(&mut self, block: PlanBlock) -> u32;
    fn store_scan(&mut self, block: PlanBlock, store: ScanStore);
    fn install_scan_plan(&mut self, block: PlanBlock, scan: &ScanBlock);
    fn mark_lens_driven(&mut self, block: PlanBlock);
    fn store_raw_calculation(&mut self, block: PlanBlock, result: &RawCalculation);
}

/// The image-4 records of the bound record and their lens geometry.
pub trait PipelineRecords {
    fn stamp_frame(&mut self, frame: Frame, lens_reference: u32);
    fn resource(&mut self, record: RecordHandle) -> u32;
    fn resource_bank(&mut self, record: RecordHandle) -> u32;
    fn outcome_history(&mut self, record: RecordHandle) -> OutcomeHistory;
    fn store_outcome_history(&mut self, record: RecordHandle, history: &OutcomeHistory);
    fn store_outcome(&mut self, record: RecordHandle, value: OutcomeValue);
    fn store_outcome_correction(&mut self, record: RecordHandle, correction: &CorrectionRecord);
    fn store_area_block(&mut self, record: RecordHandle, slot: AreaSlot, areas: &AreaBlock);
    fn store_outcome_positions(&mut self, record: RecordHandle, positions: &AreaPositions);
    fn statistics_values(&mut self, record: RecordHandle) -> StatisticsRecordValues;
    fn lens_value(&mut self, value: PipelineLens) -> u32;
    fn position_code(&mut self, point: &LensPoint, drive: u32) -> u16;
    fn tone_level(&mut self, level: i16, reference: u32, state: u32, geometry: u32, mode: u32) -> u32;
    fn level_bounds(&mut self, state: u32, reference: u32) -> (u32, u32);
    fn level_value(&mut self, level: u32) -> u32;
    fn channel_threshold(&mut self, reference: u32, state: u32, scaled: u32) -> u32;
    fn correction_base(&mut self, tolerance_kind: u32) -> u16;
}

/// Profile-record modes and configuration operations of the DCF profile geometry.
pub trait PipelineProfiles {
    fn requires_adjustment(&mut self, profile: u32) -> bool;
    fn record_mode_available(&mut self, mode: u32) -> bool;
    fn begin_record_mode(&mut self, mode: u32);
    fn finish_record_mode(&mut self, threshold: u8);
    fn configure_extended_operations(&mut self);
    fn configure_secondary_operations(&mut self);
}

/// Requests to the image and statistics tasks and the image task's capture sampling.
pub trait PipelinePeers {
    fn request_raw_frame(&mut self, request: &RawFrameRequest, completion: Completion);
    fn request_frame(&mut self, request: &FrameRequest, completion: Completion);
    fn request_scan(&mut self, request: &ScanRequest, progress: Completion, status: Completion);
    fn request_selection(&mut self, request: SelectionRequest);
    fn request_scan_selection(&mut self);
    fn request_output(&mut self, completion: Completion);
    fn request_commit(&mut self, completion: Completion);
    /// Statistics 0x03; the request's progress callback enters `PipelineWork::statistics_progress`.
    fn request_statistics(&mut self, setup: &StatisticsSetup, completion: Completion);
    fn request_statistics_commit(&mut self, completion: Completion);
    fn store_statistics_weights(&mut self, target: u32, weights: &StatisticsWeights);
    fn statistics_target(&mut self, lens: PlanBlock) -> u32;
    fn capture_idle(&mut self) -> bool;
    /// The level matched by the capture sample taken at `level`, which copies its banks to
    /// `targets`; `None` when none was taken there.
    fn take_sample(&mut self, level: u16, targets: &[PlanBlock; 5]) -> Option<i16>;
    fn channel_sample(&mut self, index: u8) -> i16;
    fn sample_snapshot(&mut self) -> SampleSnapshot;
}

/// Settings, the load test, the frame and the stock words other code reads.
pub trait PipelinePlatform {
    fn setting(&mut self, setting: PipelineSetting) -> u32;
    fn load_test_state(&mut self) -> u32;
    fn load_test_setting(&mut self) -> u32;
    fn load_test_register(&mut self, register: u8) -> u8;
    fn set_load_test_register(&mut self, register: u8, value: u8);
    fn trace(&mut self, trace: PipelineTrace);
    /// The shot context's lens reference.
    fn bound_lens_reference(&mut self) -> u32;
    fn stamp_pipeline_frame(&mut self, frame: Frame, words: &PipelineFrame);
    fn clear_frame_work(&mut self, frame: Frame);
    fn store_bound_frame(&mut self, frame: Option<Frame>);
    fn restart_marked(&mut self) -> bool;
    fn mark_restart(&mut self);
    fn store_work(&mut self, area: WorkArea, store: WorkStore);
    fn work_marked(&mut self, area: WorkArea) -> bool;
    fn flush_work(&mut self, area: WorkArea);
}

pub trait PipelineBoundary: PipelineLibrary + PlanMemory + PipelineRecords + PipelineProfiles + PipelinePeers + PipelinePlatform + Client + Lens {}
impl<T: PipelineLibrary + PlanMemory + PipelineRecords + PipelineProfiles + PipelinePeers + PipelinePlatform + Client + Lens> PipelineBoundary for T {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Flags {
    upload_done: bool,
    tone_statistics: bool,
    statistics_ready: bool,
    weighted: bool,
    statistics_pending: bool,
    finish_done: bool,
    tone_result: bool,
    tone_skipped: bool,
}

/// The capture sample the tone calculation reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Sample {
    level: u16,
    matched: i16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PipelineWork {
    record: Option<RecordHandle>,
    mode: u32,
    plan: Option<Plan>,
    work: Option<WorkArea>,
    flags: Flags,
    progress: ToneProgress,
    frame: FrameFlags,
    phase: u32,
    sample: Sample,
    scan_request: ScanRequest,
    adjustment_gated: bool,
    tone_status: ToneStatus,
    raw_check: RawCheck,
    snapshot: SampleSnapshot,
    scaled_index: u16,
    bound: Option<RecordHandle>,
    frame_point: LensPoint,
}

impl FinishPipeline for PipelineWork {
    fn frame_point(&self) -> LensPoint { self.frame_point }
    fn relative_first(&self) -> bool { self.statistics_pending() || self.upload_done() }
    fn pipeline_record(&self) -> Option<RecordHandle> { self.bound }
}

const fn grid_cell(cell: u32) -> (u32, u32) { (cell % AREA_GRID, cell / AREA_GRID) }

impl PipelineWork {
    pub fn new() -> Self { Self::default() }

    /// The statistics upload of the half press completed (0x16b5430).
    pub fn upload_done(&self) -> bool { self.flags.upload_done }

    /// A statistics upload is pending (0x16b5440).
    pub fn statistics_pending(&self) -> bool { self.flags.statistics_pending }

    /// The statistics 0x03 request's progress callback (0xf20604).
    pub fn statistics_progress(&self, library: &mut impl PipelineLibrary) {
        if self.flags.tone_statistics { library.release_calculation(); }
    }

    /// Takes the result a completion delivers to the pipeline.
    pub fn receive(&mut self, completed: Completed<'_>, boundary: &mut (impl PlanMemory + PipelinePlatform)) {
        let payload = completed.parameters.bytes();
        match completed.completion {
            Completion::Image4e => {
                if let Some(check) = results::raw_check(payload) { self.raw_check = check; }
            }
            Completion::Image50 => {
                if let Some(progress) = results::tone_progress(payload) { self.progress = progress; }
            }
            Completion::Image51 => {
                if let Some(status) = results::tone_status(payload) { self.tone_status = status; }
            }
            Completion::Calculation91 => {
                if let (Some(result), Some(plan)) = (results::raw_calculation(payload), self.plan) { boundary.store_raw_calculation(plan.raw_result, &result); }
            }
            Completion::Calculation92 => {
                if let Some(result) = results::tone_calculation(payload) { self.absorb_tone(&result, boundary); }
            }
            Completion::Calculation93 => {
                if let Some(result) = results::gradation_calculation(payload) { self.absorb_gradation(&result, boundary); }
            }
            _ => {}
        }
    }

    pub fn effect(&mut self, effect: E, prepared: &mut Prepared, boundary: &mut impl PipelineBoundary) {
        match effect {
            E::BindRecord(record) => self.bind(record, prepared, boundary),
            E::RecordLensStatus => prepared.record_lens_status(boundary),
            E::Prepare(record) => self.prepare(record, prepared.parameters.unwrap_or_default(), boundary),
            E::MarkRestart => boundary.mark_restart(),
            E::Configure { first, .. } => self.configure(first, prepared, boundary),
            E::ClearFrameReady => self.frame = FrameFlags::default(),
            E::MarkFrameReady => self.frame = self.frame.prepared(),
            E::ReleaseFrame => self.release_work(boundary),
            E::FrameReleased => boundary.trace(PipelineTrace::FrameReleased),
            E::StartFrame => self.start_frame(boundary),
            E::PrepareFrame => self.prepare_frame(boundary),
            E::ArmFrame => self.arm_frame(boundary),
            E::SynchronizeFrame => {
                if boundary.record_mode_available(FRAME_RECORD_MODE) { boundary.begin_record_mode(FRAME_RECORD_MODE); }
            }
            E::ResumeFrame => {
                let channels = boundary.active_channels();
                if boundary.record_mode_available(FRAME_RECORD_MODE) { boundary.finish_record_mode(channels); }
            }
            E::ReleaseFrameLocks => boundary.release_second_channel(),
            E::PrepareFallback => self.prepare_fallback(prepared, boundary),
            E::ConcludeRaw => {
                boundary.conclude_raw();
                boundary.trace(PipelineTrace::RawConcluded);
            }
            E::SetStatisticsPending(pending) => self.flags.statistics_pending = pending,
            E::SetUploadDone(done) => self.flags.upload_done = done,
            E::SetPhase(phase) => self.store_work(WorkStore::Phase(phase), boundary),
            E::SetToneResult(result) => self.set_tone_result(result),
            E::SetFinishDone => self.flags.finish_done = true,
            E::Reset => self.reset(boundary),
            E::Request(request) => self.request(request, boundary),
        }
    }

    pub fn query(&mut self, query: Q, boundary: &mut impl PipelineBoundary) -> bool {
        match query {
            Q::FinishOnly => self.mode == FINISH_ONLY_MODE,
            Q::CaptureIdle => boundary.capture_idle(),
            Q::SkipLensFinish => self.mode == IMMEDIATE_MODE,
            Q::FrameReady => self.frame.ready(),
            Q::Restarted => boundary.restart_marked(),
            Q::NeedsRaw => boundary.needs_raw(),
            Q::RawCalculationNeeded => self.raw_check.0 == 0,
            Q::StatisticsReady => self.flags.statistics_ready,
            Q::StatisticsPending => self.flags.statistics_pending,
            Q::UploadDone => self.flags.upload_done,
            // 0xf21c20 reads context word +0x24, which only the context's zeroing writes.
            Q::ToneRequested => false,
            Q::ToneSkipped => self.take_tone_sample(boundary),
            Q::ToneCalculationFinal => self.tone_calculation_final(boundary),
            Q::ToneResultSet => self.flags.tone_result,
            Q::ToneRestart => self.tone_restart(),
            Q::GradationNeeded => self.tone_status.0 == 0,
            Q::FinishDone => self.flags.finish_done,
        }
    }

    fn store_work(&self, store: WorkStore, boundary: &mut impl PipelinePlatform) {
        if let Some(area) = self.work { boundary.store_work(area, store); }
    }

    fn bind(&mut self, record: RecordHandle, prepared: &mut Prepared, boundary: &mut impl PipelineBoundary) {
        self.bound = Some(record);
        let parameters = prepared.parameters.unwrap_or_default();
        if parameters.mode.0 != FINISH_ONLY_MODE {
            let frame = parameters.frame_slot.and_then(|slot| boundary.frame(slot));
            boundary.store_bound_frame(frame);
            if let Some(frame) = frame {
                let lens_reference = boundary.bound_lens_reference();
                boundary.stamp_frame(frame, lens_reference);
                let words = PipelineFrame {
                    resource: boundary.resource(record),
                    bank: boundary.resource_bank(record),
                    lens_position: boundary.lens_value(PipelineLens::Position),
                    lens_reference: boundary.bound_lens_reference(),
                };
                boundary.stamp_pipeline_frame(frame, &words);
                if boundary.resource(record) != 0 || parameters.mode.0 != 0 { boundary.clear_frame_work(frame); }
            }
        }
        prepared.pipeline_gate = 0;
    }

    fn prepare(&mut self, record: RecordHandle, parameters: HalfPressParameters, boundary: &mut impl PipelineBoundary) {
        boundary.trace(PipelineTrace::Prepare);
        self.mode = parameters.mode.0;
        if self.mode != FINISH_ONLY_MODE {
            *self = Self { mode: self.mode, bound: self.bound, frame_point: self.frame_point, ..Self::default() };
        }
        self.record = Some(record);
        self.work = parameters.frame_slot.and_then(|slot| boundary.frame(slot)).map(layout::work_area);
        if !parameters.mask.notifications().frame_released {
            self.frame = FrameFlags::default();
            return;
        }
        let input = PlanInput {
            record,
            lens_reference: boundary.bound_lens_reference(),
            load_test_window: boundary.load_test_setting() == LOAD_TEST_WINDOW,
            area_count: parameters.area_count.map_or(0, |count| count.0),
            option: parameters.plan_option,
            frame_slot: parameters.frame_slot.map_or(0, |slot| slot.0),
        };
        if let Some(plan) = boundary.plan(&input) {
            self.frame = plan.frame_flags;
            self.plan = Some(plan);
        }
    }

    fn release_work(&mut self, boundary: &mut impl PipelinePlatform) {
        if let Some(area) = self.work {
            boundary.flush_work(area);
            self.work = Some(layout::released(area));
        }
    }

    fn reset(&mut self, boundary: &mut impl PipelineLibrary) {
        if self.frame != FrameFlags::default() { boundary.store_converted_input(); }
        self.flags = Flags { statistics_pending: self.flags.statistics_pending, ..Flags::default() };
        self.progress.level = 0;
        self.frame = FrameFlags::default();
        self.phase = 0;
    }

    fn set_tone_result(&mut self, result: bool) {
        self.flags.tone_result = result;
        self.flags.tone_statistics = result && (self.flags.tone_statistics || self.flags.weighted);
    }

    fn start_frame(&mut self, boundary: &mut impl PipelineBoundary) {
        boundary.refresh_channels();
        let Some(plan) = self.plan else {
            self.flags.statistics_ready = false;
            self.flags.weighted = false;
            return;
        };
        let profile = boundary.subject(plan.subject).profile();
        let ready = boundary.requires_adjustment(profile)
            && !boundary.record_mode_available(FRAME_RECORD_MODE)
            && self.record.is_some_and(|record| boundary.channel_item_ready(record));
        self.flags.statistics_ready = ready;
        self.flags.weighted = ready
            && boundary.setting(PipelineSetting::ChannelStatistics) != 0
            && !UNWEIGHTED_LENS_KINDS.contains(&boundary.lens(plan.lens).kind())
            && boundary.capture(plan.capture).weighting() != 0;
    }

    fn arm_frame(&mut self, boundary: &mut impl PipelineBoundary) {
        let adjustment = self.plan.unwrap_or_default().adjustment;
        boundary.apply_adjustment(&adjustment, AdjustmentPass::Primary);
        boundary.apply_adjustment(&adjustment, AdjustmentPass::Secondary);
        self.adjustment_gated = boundary.adjustment_gated();
        self.store_work(WorkStore::AdjustmentGate(self.adjustment_gated), boundary);
    }

    fn take_tone_sample(&mut self, boundary: &mut impl PipelineBoundary) -> bool {
        let mut taken = false;
        if self.progress.status == 0 {
            self.sample.level = self.progress.level;
            let targets = self.plan.unwrap_or_default().sample_targets;
            if let Some(matched) = boundary.take_sample(self.sample.level, &targets) {
                self.sample.matched = matched;
                taken = true;
            }
        }
        self.flags.tone_skipped = !taken;
        self.flags.tone_skipped
    }

    fn tone_restart(&mut self) -> bool {
        if KEEPING_PHASES.contains(&self.phase) { return false; }
        self.flags.tone_skipped = false;
        self.progress.level = 0;
        true
    }

    fn absorb_tone(&mut self, result: &ToneCalculation, boundary: &mut (impl PlanMemory + PipelinePlatform)) {
        let Some(plan) = self.plan else { return };
        let scan = boundary.scan(plan.scan);
        let (first, second) = scan.tone_limits();
        let (first_limit, second_limit) = layout::tone_limits(result);
        if first != 1 { boundary.store_scan(plan.scan, ScanStore::FirstToneLimit(first_limit)); }
        if second != 1 { boundary.store_scan(plan.scan, ScanStore::SecondToneLimit(second_limit)); }
        let merged = layout::merged_tone(&scan, result);
        boundary.store_scan(plan.scan, ScanStore::Tone(merged));
        if layout::tone_scanned(&merged) && self.work.is_some_and(|area| !boundary.work_marked(area)) {
            self.adjustment_gated = true;
            self.store_work(WorkStore::ToneMark(self.sample.matched), boundary);
        }
    }

    fn absorb_gradation(&mut self, result: &GradationCalculation, boundary: &mut impl PlanMemory) {
        let Some(plan) = self.plan else { return };
        boundary.store_scan(plan.scan, ScanStore::Gradation(*result));
        let scan = boundary.scan(plan.scan);
        if scan.sampling() == 1 && self.snapshot.selected_sample() >= scan.gradation_threshold() {
            boundary.store_scan(plan.scan, ScanStore::GradationReached);
        }
    }

    fn tone_calculation_final(&mut self, boundary: &mut impl PipelineBoundary) -> bool {
        let Some(plan) = self.plan else { return false };
        let scan = boundary.scan(plan.scan);
        let capture = boundary.capture(plan.capture);
        let lens_final = boundary.lens(plan.lens).mode() == FINAL_LENS_MODE;
        let tone = |word| scan.tone_word(word);
        let bounded = tone(ToneWord::LowBound) == 1 && tone(ToneWord::HighBound) == 1;
        if capture.phase() == LONG_CAPTURE_PHASE {
            return tone(ToneWord::Value) == 0
                && scan.scan_mode() == 1
                && tone(ToneWord::Driven) == 1
                && lens_final
                && boundary.phase_attribute(self.phase, PhaseAttribute::ToneFinal);
        }
        if tone(ToneWord::Value) == 0 { return true; }
        if scan.scan_mode() == 1 && bounded && tone(ToneWord::Scanned) == 0 && lens_final && boundary.phase_attribute(self.phase, PhaseAttribute::ToneFinal) {
            return true;
        }
        if boundary.setting(PipelineSetting::FirstToneFinal) != 0 && self.adjustment_gated && tone(ToneWord::FirstReached) == 1 { return true; }
        if boundary.setting(PipelineSetting::SecondToneFinal) != 0 && self.adjustment_gated && tone(ToneWord::SecondReached) == 1 { return true; }
        if self.phase == FINAL_PHASE && tone(ToneWord::Scanned) != 1 && tone(ToneWord::Moved) != 1 && bounded { return true; }
        if scan.decision() != DECISION_PENDING || scan.tone_level() > self.sample.matched { return false; }
        if u32::from(boundary.active_channels()) < boundary.setting(PipelineSetting::ChannelsForDecision) { return false; }
        boundary.store_scan(plan.scan, ScanStore::Decision(DECISION_FINAL));
        if self.adjustment_gated {
            if boundary.setting(PipelineSetting::GatedToneFinal) != 0 && tone(ToneWord::FirstReached) == 1 { return true; }
            if boundary.setting(PipelineSetting::ToneMask) & 1 == 0 { return true; }
        }
        if capture.restrained() == 1 || tone(ToneWord::Held) == 1 || tone(ToneWord::LowBound) == 1 {
            boundary.store_scan(plan.scan, ScanStore::Decision(DECISION_CONTINUE));
            return false;
        }
        true
    }

    fn prepare_frame(&mut self, boundary: &mut impl PipelineBoundary) {
        let Some(plan) = self.plan else { return };
        let mut scan = boundary.build_scan_plan(plan.scan);
        self.phase = scan.phase();
        if self.phase == UNPLANNED_PHASE { return; }
        layout::start_selection(&mut scan);
        boundary.install_scan_plan(plan.scan, &scan);
        let capture = boundary.capture(plan.capture);
        let lens = boundary.lens(plan.lens);
        let mut request = layout::scan_request(&scan, &capture, &lens);
        request.selection_setting = match boundary.setting(PipelineSetting::BoundedSelection) {
            setting @ 0..=7 => setting,
            _ => 0,
        };
        request.correction = self.scan_correction(request.positions, capture.tolerance_kind(), boundary);
        self.scan_request = request;
    }

    /// The lens correction the scan starts with, from the scan's first position and the lens.
    fn scan_correction(&self, positions: [u16; 3], tolerance_kind: u32, boundary: &mut impl PipelineBoundary) -> u16 {
        let distance = positions[0].wrapping_sub(boundary.lens_value(PipelineLens::Position) as u16);
        let tolerance = boundary.setting(PipelineSetting::CorrectionTolerance) as u16;
        let direction = boundary.lens_value(PipelineLens::Direction);
        let forward = positions[0] as i16 >= positions[1] as i16;
        let behind = distance as i16 <= 0;
        let base = boundary.setting(PipelineSetting::CorrectionBase) as u16;
        let reach = boundary.correction_base(tolerance_kind).wrapping_add(base);
        let turned = u32::from(forward) != direction;
        let magnitude = i32::from(distance as i16).abs();
        let correction = if forward == behind && turned && magnitude < i32::from(tolerance as i16) {
            let unsigned = if distance & 0x8000 != 0 { distance.wrapping_neg() } else { distance };
            reach.wrapping_add(tolerance).wrapping_sub(unsigned)
        } else if forward != behind && turned && magnitude <= i32::from(reach as i16) {
            reach.wrapping_add(tolerance)
        } else {
            return 0;
        };
        if correction != 0 && direction == 1 { correction.wrapping_neg() } else { correction }
    }

    fn configure(&mut self, first: bool, prepared: &Prepared, boundary: &mut impl PipelineBoundary) {
        let released = first && prepared.parameters.is_some_and(|parameters| parameters.mask.notifications().frame_released);
        let kind = if released { ResultKind::Released } else { ResultKind::Policy };
        let (result, cell) = match self.plan {
            Some(plan) => (boundary.focus_result(plan.scan, kind), boundary.scan(plan.scan).area_cell()),
            None => (FocusResultOut::default(), 0),
        };
        let entry = (result.point.position, result.point.kind);
        let outcome = FocusOutcome {
            valid: self.frame.outcome(),
            status: result.status,
            point: result.point,
            source: 0,
            cell: grid_cell(cell),
            offset: 0,
            selected: 0,
            pending: result.pending,
            areas: AreaBlock { count: 1, ..AreaBlock::default() },
            positions: [entry, entry, (0, 0), (0, 0)],
            last_position: 1,
        };
        self.publish(&outcome, prepared, boundary);
    }

    fn prepare_fallback(&mut self, prepared: &Prepared, boundary: &mut impl PipelineBoundary) {
        let Some(plan) = self.plan else { return };
        let scan = boundary.scan(plan.scan);
        let subject = boundary.subject(plan.subject);
        let capture = boundary.capture(plan.capture);
        let status = if scan.focus_missing() != 0 { 2 } else if scan.focus_found() { 0 } else { 1 };
        let source = if boundary.position_correction() == layout::CORRECTED_POSITION {
            1
        } else if boundary.raw_selection(plan.raw_selection) == 1 {
            2
        } else {
            0
        };
        let selected = boundary.selected_value();
        let pending = u32::from(self.fallback_pending(&scan, boundary));
        let count = scan.area_count();
        let mut areas = AreaBlock { count, source: scan.area_source(), ..AreaBlock::default() };
        for (index, cell) in areas.cells.iter_mut().enumerate().take(usize::from(count)) { *cell = grid_cell(u32::from(scan.area_index(index))); }
        let kind = scan.area_kind();
        let last = count.min(OUTCOME_POSITIONS as u8);
        let mut positions = [(0, 0); OUTCOME_POSITIONS + 1];
        for (index, entry) in positions.iter_mut().enumerate().take(usize::from(last)) { *entry = (scan.area_position(index), kind); }
        positions[usize::from(last)] = (scan.area_position(0), kind);
        let outcome = FocusOutcome {
            valid: true,
            status,
            point: LensPoint { position: scan.focus_position(), detail: 0, reference: LensReference(capture.reference()), state: subject.state(), kind },
            source,
            cell: grid_cell(scan.area_cell()),
            offset: subject.offset(),
            selected,
            pending,
            areas,
            positions,
            last_position: last,
        };
        self.publish(&outcome, prepared, boundary);
    }

    fn fallback_pending(&self, scan: &ScanBlock, boundary: &mut impl PipelineBoundary) -> bool {
        boundary.setting(PipelineSetting::FallbackFirst) == 1
            && boundary.setting(PipelineSetting::FallbackSecond) == 1
            && !(boundary.load_test_state() == 1 && boundary.load_test_setting() == LOAD_TEST_WINDOW)
            && boundary.setting(PipelineSetting::Adjustments) & 2 == 0
            && layout::pending_tone(scan.tone_class())
    }

    /// Publishes the outcome into the bound record and keeps its point for the finish.
    fn publish(&mut self, outcome: &FocusOutcome, prepared: &Prepared, boundary: &mut impl PipelineBoundary) {
        self.frame_point = outcome.point;
        let mut adjusted = outcome.point;
        let code = if outcome.status <= LAST_FOUND_STATUS {
            adjusted.position = boundary.adjust_direction(&outcome.point);
            let drive = boundary.lens_value(PipelineLens::Drive);
            boundary.position_code(&outcome.point, drive)
        } else {
            NO_POSITION_CODE
        };
        let Some(record) = self.bound else { return };
        let mut history = boundary.outcome_history(record);
        layout::record_outcome(&mut history, outcome);
        boundary.store_outcome_history(record, &history);
        if boundary.load_test_state() != 0 && boundary.load_test_setting() == LOAD_TEST_WINDOW { check_load_test_window(outcome, boundary); }
        if !prepared.parameters.is_some_and(|parameters| parameters.mask.notifications().frame_released) { return; }
        boundary.store_outcome_correction(record, &layout::outcome_correction(outcome));
        for value in [
            OutcomeValue::Status(outcome.status),
            OutcomeValue::PositionCode(code),
            OutcomeValue::Point(outcome.point),
            OutcomeValue::AdjustedPoint(adjusted),
            OutcomeValue::Cell(outcome.cell.0, outcome.cell.1),
            OutcomeValue::Offset(outcome.offset),
            OutcomeValue::Flag(0),
            OutcomeValue::Source(outcome.source),
        ] {
            boundary.store_outcome(record, value);
        }
        boundary.store_area_block(record, AreaSlot::First, &outcome.areas);
        if outcome.areas.count > 1 {
            boundary.store_area_block(record, AreaSlot::Second, &outcome.areas);
            boundary.store_area_block(record, AreaSlot::Third, &outcome.areas);
        }
        boundary.store_outcome_positions(record, &layout::outcome_positions(outcome));
    }

    fn request(&mut self, request: R, boundary: &mut impl PipelineBoundary) {
        match request {
            R::Image1d => self.request_raw_frame(boundary),
            R::Image1e => self.request_frame(boundary),
            R::Image1f => {
                self.progress.level = 0;
                self.set_tone_result(true);
                boundary.request_scan(&self.scan_request, Completion::Image50, Completion::Image51);
            }
            R::Image20 => self.request_selection(boundary),
            R::Image21 => {
                if let Some(plan) = self.plan { boundary.store_scan(plan.scan, ScanStore::ScanRequested); }
                boundary.begin_scan();
                boundary.request_scan_selection();
            }
            R::Image22 => {
                boundary.configure_secondary_operations();
                boundary.request_output(Completion::Image52);
            }
            R::Image23 => boundary.request_commit(Completion::Image53),
            R::Calc03 => {
                if let Some(plan) = self.plan { boundary.request_raw(&plan, Completion::Calculation91); }
            }
            R::Calc04 => self.request_tone(boundary),
            R::Calc05 => self.request_gradation(boundary),
            R::Statistics03 => self.request_statistics(boundary),
            R::Statistics07 => boundary.request_statistics_commit(Completion::Statistics6b),
        }
    }

    fn subject_and_area(plan: &Plan, subject: PlanBlock, boundary: &mut impl PipelineBoundary) -> (SubjectBlock, SubjectGeometry, CaptureArea) {
        let subject = boundary.subject(subject);
        let capture = boundary.capture(plan.capture);
        let geometry = SubjectGeometry { profile: subject.profile(), state: subject.state(), horizontal: subject.horizontal(), vertical: subject.vertical(), extent: subject.extent() };
        (subject, geometry, CaptureArea { reference: capture.reference(), areas: capture.areas(), area: capture.area() })
    }

    fn request_raw_frame(&mut self, boundary: &mut impl PipelineBoundary) {
        boundary.trace(PipelineTrace::RawRequested);
        let Some(plan) = self.plan else { return };
        let (_, subject, area) = Self::subject_and_area(&plan, plan.raw_subject, boundary);
        let raw_word = boundary.raw_first_word(plan.raw_first);
        boundary.request_raw_frame(&RawFrameRequest { subject, area, image_buffer: plan.image_buffer, raw_word }, Completion::Image4e);
    }

    fn request_frame(&mut self, boundary: &mut impl PipelineBoundary) {
        let Some(plan) = self.plan else { return };
        let (block, subject, area) = Self::subject_and_area(&plan, plan.subject, boundary);
        let sampling = boundary.lens(plan.lens).sampling();
        boundary.configure_extended_operations();
        let request = FrameRequest { subject, kind: block.kind(), area, image_buffer: plan.image_buffer, sampling, mode: block.mode() };
        boundary.request_frame(&request, Completion::Image4f);
    }

    fn request_tone(&mut self, boundary: &mut impl PipelineBoundary) {
        let Some(plan) = self.plan else { return };
        let scan = boundary.scan(plan.scan);
        let levels = if self.scan_request.start == 1 {
            let level = self.sample.level;
            let index = if level == 0 { 0 } else { scan.index_base().wrapping_add(level) as u8 };
            self.scaled_index = boundary.scaled_value(plan.scan, index);
            boundary.scaled_levels(plan.scan, self.scaled_index, level as u8)
        } else {
            [self.sample.matched; TONE_LEVELS]
        };
        let offset = if boundary.setting(PipelineSetting::ToneOffset) == 0 {
            0
        } else {
            let capture = boundary.capture(plan.capture);
            let state = boundary.subject(plan.subject).state();
            let level = boundary.tone_level(self.sample.matched, capture.reference(), state, capture.geometry(), scan.scan_mode());
            let (low, high) = boundary.level_bounds(state, capture.reference());
            (boundary.level_value(level) as u16).wrapping_sub(low.wrapping_add(high) as u16)
        };
        let mut window = scan.tone_window();
        if self.adjustment_gated { window[0] = 0; }
        boundary.request_tone(&ToneRequest { levels, offset, window }, Completion::Calculation92);
    }

    fn request_gradation(&mut self, boundary: &mut impl PipelineBoundary) {
        self.snapshot = boundary.sample_snapshot();
        let Some(plan) = self.plan else { return };
        let scan = boundary.scan(plan.scan);
        let sampled = scan.sampling() != 0;
        let exposure = if sampled {
            let scale = boundary.subject(plan.subject).scale();
            let factor = boundary.setting(PipelineSetting::ExposureScale) as u8;
            let product = u64::from(self.snapshot.selected_sample()).wrapping_mul(u64::from(scale)).wrapping_mul(u64::from(factor));
            (product / EXPOSURE_DIVISOR) as u16
        } else {
            0
        };
        let starts = self.scan_request.start == 1;
        if starts && scan.scan_requested() == 0 && !self.adjustment_gated { self.narrow_channels(&plan, boundary); }
        if starts {
            let first = boundary.channel_sample(0);
            let channels = boundary.active_channels();
            boundary.store_scan(plan.scan, ScanStore::FirstSample(first as u16));
            boundary.store_channel_samples(plan.scan, &self.snapshot);
            for index in 0..channels {
                let sample = boundary.channel_sample(index);
                boundary.store_channel_sample(plan.scan, sample, index);
            }
        }
        boundary.request_gradation(&GradationRequest { tone: scan.tone(), gated: self.adjustment_gated, sampled, exposure }, Completion::Calculation93);
    }

    /// Drops the trailing channels whose samples moved less than the lens threshold.
    fn narrow_channels(&self, plan: &Plan, boundary: &mut impl PipelineBoundary) {
        let channels = boundary.active_channels();
        if channels <= 1 { return; }
        let reference = boundary.capture(plan.capture).reference();
        let state = boundary.subject(plan.subject).state();
        let scaled = boundary.setting(PipelineSetting::ChannelThreshold).wrapping_mul(10) & 0xfffe;
        let threshold = boundary.channel_threshold(reference, state, scaled);
        let mut last = channels;
        while last != 1 {
            let earlier = boundary.channel_sample(last - 2);
            let later = boundary.channel_sample(last - 1);
            if threshold <= (i32::from(earlier) - i32::from(later)).unsigned_abs() & 0xffff { break; }
            last -= 1;
        }
        if last != channels {
            boundary.select_channel(last);
            self.store_work(WorkStore::ChannelWindow { last, count: channels }, boundary);
        }
    }

    fn request_statistics(&mut self, boundary: &mut impl PipelineBoundary) {
        let Some(plan) = self.plan else { return };
        let subject = boundary.subject(plan.subject);
        let lens = boundary.lens(plan.lens);
        if self.flags.weighted {
            let scale = u32::from(self.scan_request.weighting >> 8 & 0xff);
            let mut weights = [0; STATISTICS_WEIGHTS];
            for (index, weight) in (0u8..).zip(weights.iter_mut()) {
                let value = boundary.setting(PipelineSetting::Weight(index)).wrapping_mul(scale);
                let divisor = boundary.setting(PipelineSetting::WeightDivisor).wrapping_add(1);
                *weight = (value / divisor) as u8;
            }
            self.store_work(WorkStore::Weights(weights), boundary);
            let target = boundary.statistics_target(plan.lens);
            boundary.store_statistics_weights(target, &layout::statistics_weights(weights, &subject, &lens));
        }
        let Some(record) = self.record else { return };
        let capture = boundary.capture(plan.capture);
        let selection = boundary.indexed_selection(record);
        let values = boundary.statistics_values(record);
        let setup = layout::statistics_setup(&capture, &lens, &subject, selection, &values, self.flags.weighted);
        boundary.request_statistics(&setup, Completion::Statistics7a);
    }

    /// Imgdc 0x3f20: picks the scan selection from the channel classes and asks for it when it
    /// changed.
    fn request_selection(&mut self, boundary: &mut impl PipelineBoundary) {
        let Some(plan) = self.plan else { return };
        let scan = boundary.scan(plan.scan);
        let channels = boundary.active_channels();
        if channels != 0 {
            self.store_work(WorkStore::ChannelMarks { channel: channels - 1, marks: [CHANNEL_USED; 4] }, boundary);
            self.store_work(WorkStore::ChannelMarks { channel: channels, marks: [CHANNEL_END; 4] }, boundary);
        }
        let current = scan.selection();
        let selection = if scan.sampling() == 0 {
            current
        } else {
            let overridden = if scan.tone_word(ToneWord::Scanned) != 0 { boundary.setting(PipelineSetting::SelectionOverride) } else { 0 };
            match overridden {
                1 => 0,
                2 => 1,
                _ => self.classified_selection(&plan, &scan, channels, boundary),
            }
        };
        if selection == current { return; }
        if selection == DRIVING_SELECTION {
            let distance = (i32::from(scan.position()) - i32::from(self.scaled_index as i16)).unsigned_abs();
            let moved = distance.wrapping_mul(10).checked_div(u32::from(scan.position_divisor())).unwrap_or(0);
            if moved <= boundary.setting(PipelineSetting::DrivingDistance) { return; }
        }
        boundary.store_scan(plan.scan, ScanStore::Selection(selection));
        boundary.request_selection(SelectionRequest(scan.selection_word(selection)));
        if selection == DRIVING_SELECTION { boundary.mark_lens_driven(plan.lens); }
    }

    fn classified_selection(&self, plan: &Plan, scan: &ScanBlock, channels: u8, boundary: &mut impl PipelineBoundary) -> u32 {
        let kind = boundary.subject(plan.subject).kind();
        let class = if u32::from(kind) > boundary.setting(PipelineSetting::SubjectKindLimit) { 3 } else { 1 };
        let mut classes = [0u32; 4];
        for (quadrant, value) in (0u8..).zip(classes.iter_mut()) {
            let setting = boundary.setting(PipelineSetting::Quadrant(quadrant));
            let own = (setting & 0xf).min(2);
            *value = if own == class { 0 } else { scan.class_word(own * 4 + (setting >> 4 & 0xff).min(3)) };
        }
        if channels != 0 { self.store_work(WorkStore::ChannelMarks { channel: channels - 1, marks: classes.map(|class| class as u8) }, boundary); }
        let current = scan.selection();
        if current > LAST_SELECTION || classes.iter().any(|&class| class > LAST_SELECTION) { return current; }
        let index = classes[0] * 27 + classes[1] * 9 + classes[2] * 3 + classes[3];
        match boundary.setting(PipelineSetting::SelectionTable(index)) >> (current * 2) & 3 {
            selection @ 0..=LAST_SELECTION => selection,
            _ => current,
        }
    }
}

/// Writes the published position into the load-test registers and whether the lens stands inside
/// the test's window around its target.
fn check_load_test_window(outcome: &FocusOutcome, boundary: &mut impl PipelineBoundary) {
    let position = if outcome.status == 0 { outcome.point.position as u16 } else { NO_POSITION };
    boundary.set_load_test_register(layout::POSITION_HIGH, (position >> 8) as u8);
    boundary.set_load_test_register(layout::POSITION_LOW, position as u8);
    let target = (u16::from(boundary.load_test_register(layout::TARGET_HIGH)) << 8 | u16::from(boundary.load_test_register(layout::TARGET_HIGH + 1))) as i16;
    let spread = (u16::from(boundary.load_test_register(layout::SPREAD_HIGH)) << 8 | u16::from(boundary.load_test_register(layout::SPREAD_HIGH + 1))) as i16;
    let spread = i32::from(if spread < 0 { spread.wrapping_neg() } else { spread });
    let lens = boundary.lens_value(PipelineLens::Position) as i32;
    let low_factor = boundary.setting(PipelineSetting::LoadTestTolerance) as i32;
    let high_factor = boundary.setting(PipelineSetting::LoadTestTolerance) as i32;
    let low = (i32::from(target) - low_factor * spread / 10) as i16;
    let high = (i32::from(target) + high_factor * spread / 10) as i16;
    let inside = i32::from(high) > lens && lens > i32::from(low);
    boundary.set_load_test_register(layout::WINDOW_RESULT, if inside { 0 } else { 2 });
}
