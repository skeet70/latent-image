#![forbid(unsafe_code)]

//! Traits for what the shot's leaf work calls outside the module: the image-4 record layer shared
//! with the other saclac modules, lens control, the area planner shared with the measurement
//! round, client memory named by the command parameters, diagnostic dumps and platform services.
//! Byte layouts of the values that cross are in `stock`.

use crate::exposure_work::ExposureSetup;
use crate::ports::{EvaluationIndex, HalfPressMode, Reply};
use crate::subjects::SubjectList;
use crate::subjects::Subject;
use crate::summary::{AreaStatistics, FrameSummary};
use xm1_saclac_interface::exposure::{Correction, Evaluation, MeasuredEntry, StatisticsRequest, STATISTICS_AREAS_BYTES};
use xm1_saclac_interface::{Completion, RecordHandle};

/// A client address carried in the command parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientRef(pub u32);

/// The capture frame a half-press client names through its parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensReference(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensPoint {
    pub position: i16,
    pub detail: u16,
    pub reference: LensReference,
    pub state: u32,
    pub kind: u32,
}

/// The bound record's lens snapshot: the current point and the point held for the shot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensSnapshot {
    pub current: LensPoint,
    pub held: LensPoint,
    pub marker: u32,
    pub tail: [u8; 12],
}

/// The bound record's focus result, taken at `reference`. Unnamed words stay in `bytes`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusResult {
    pub reference: LensReference,
    pub valid: u32,
    /// Cleared with `valid`.
    pub kept: u32,
    /// Cleared when the half press asks for an unrefined result.
    pub refined: u32,
    /// 128 keeps `refined` when the client keeps its areas.
    pub extent: u32,
    pub bytes: [u8; 132],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusConfirmation {
    pub active: u32,
    pub reference: LensReference,
    pub bytes: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusHistory {
    pub valid: u32,
    pub bytes: [u8; 80],
}

/// Capture-profile fields of the bound record, named by what the shot decides with them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    /// 1 permits the numbered dump and skips the subject selection wait; 0 permits a two-pass
    /// exposure.
    ContextMode,
    /// 0 asks for the half-press lens synchronization in mode 0; 4..=6 asks for the lens dump.
    LensPolicy,
    PrimarySubjects,
    SecondarySubjects,
    SelectionStyle,
    SelectionConfirmed,
    SelectionKind,
    /// 0x39 withholds the frame from the record.
    SelectionOption,
    /// 1 drops the refined focus result in half-press mode 1.
    CaptureVariant,
    /// Upper and lower halfwords; (4, 3) and (3, 2) keep the focus result in mode 2.
    AreaLayout,
    SubjectQueryA,
    SubjectQueryB,
    DumpArgument,
    /// Bound into the exposure before the calculation library prepares it.
    ExposureSource,
    /// Sent with the statistics measurement.
    StatisticsOption,
    /// 1 and 2 select the export the exposure conclusion writes; other values export kind 0.
    CorrectionExport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishedList { Selected, Tracked }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SummaryWord { First, Second, Third, Count }

/// A value the shot publishes into the bound record for other readers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Publication<'a> {
    Subject { list: PublishedList, index: usize, subject: &'a Subject },
    SubjectCount { list: PublishedList, count: u32 },
    SubjectFlag { index: usize, value: u16 },
    SubjectSummary { word: SummaryWord, value: u32 },
    SelectedSubject(&'a Subject),
    SubjectTime(u32),
    TrackedMarker(u32),
    AreaSelection(u32),
    Area { index: usize, word: u32 },
    /// The layer copies the target from client memory.
    LensTargetArea(ClientRef),
    CaptureFrame(Option<Frame>),
    CaptureLink(u32),
}

/// The image-4 record layer for the bound record. The layer refuses writes the record does not
/// grant; the shot does not observe a refusal.
pub trait Records {
    fn setting(&mut self, setting: Setting) -> u32;
    fn lens_snapshot(&mut self) -> LensSnapshot;
    fn store_lens_snapshot(&mut self, snapshot: &LensSnapshot);
    fn focus_result(&mut self) -> FocusResult;
    fn store_focus_result(&mut self, result: &FocusResult);
    fn focus_confirmation(&mut self) -> FocusConfirmation;
    fn store_focus_confirmation(&mut self, confirmation: &FocusConfirmation);
    fn focus_history(&mut self) -> FocusHistory;
    fn store_focus_history(&mut self, history: &FocusHistory);
    fn clear_exposure_record(&mut self);
    fn area_statistics(&mut self) -> AreaStatistics;
    fn publish(&mut self, publication: Publication<'_>);
    fn select_first_mode(&mut self);
    fn load_lens_capabilities(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensReading {
    Position,
    State,
    Kind,
    Control,
    /// Compared with the parameters' lens reference.
    Reference,
}

/// Lens control state, owned by the lens service.
pub trait Lens {
    fn read(&mut self, reading: LensReading) -> u32;
    /// Takes the lens change latch, clearing it.
    fn take_change(&mut self) -> u32;
    fn designated_point(&mut self) -> Option<LensPoint>;
    fn target_distance(&mut self, point: LensPoint, control: u32) -> u32;
    fn target_step(&mut self, point: LensPoint) -> u16;
}

/// Client memory the half-press parameters name.
pub trait Client {
    fn area_count(&mut self, area_count: ClientRef) -> u32;
    fn area(&mut self, areas: ClientRef, index: usize) -> u32;
    fn write_lens_target(&mut self, target: ClientRef, point: LensPoint, step: u16, distance: u32);
    fn frame(&mut self, slot: ClientRef) -> Option<Frame>;
    fn clear_frame_progress(&mut self, slot: ClientRef);
    /// Writes the frame header and names it to the calculation task.
    fn announce_frame(&mut self, frame: Frame, header: crate::preparation::FrameHeader);
    fn write_summary(&mut self, frame: Frame, summary: &FrameSummary);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dump { Frame, Lens, Numbered }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trace { Prepare, HalfPressReplied, ExposureBegun, EvaluationRequested, CorrectionRequested, ExposureConcluded }

pub trait Diagnostics {
    fn trace(&mut self, trace: Trace);
    fn dump(&mut self, dump: Dump, frame: Frame, argument: u32);
    /// Context mode of the calculation library's own record.
    fn calculation_context_mode(&mut self) -> u32;
    fn dump_calculation(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hold { Preparation }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Adjustment { TrackedSubjectsWide, StatisticsDump }

pub trait Platform {
    fn hold(&mut self, hold: Hold, held: bool);
    fn adjustment(&mut self, adjustment: Adjustment) -> u8;
    fn level(&mut self) -> u8;
    fn tick(&mut self) -> u32;
    fn reset_image_progress(&mut self);
}

/// The selection kind the calculation library derives for a record and half-press mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionKind(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureLevels {
    pub base: i16,
    pub reference: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrectionRequest { Sent, Skipped }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportKind { Plain, First, Second }

/// The calculation library's exposure tables, which it keeps between calls.
pub trait ExposureCalculation {
    fn selection(&mut self, record: RecordHandle, mode: HalfPressMode, refinement_pass: bool) -> SelectionKind;
    fn selects_primary(&mut self, selection: SelectionKind) -> bool;
    fn selects_secondary(&mut self, selection: SelectionKind) -> bool;
    /// Clears the tables and fills them for `setup`, tagging its frame for the calculation task.
    fn prepare(&mut self, setup: &ExposureSetup);
    fn calculation_mode(&mut self, record: RecordHandle) -> u32;
    fn correction_mode(&mut self, record: RecordHandle, refinement_pass: bool) -> u32;
    fn scaled_statistics(&mut self) -> bool;
    fn evaluation_areas(&mut self) -> [u8; STATISTICS_AREAS_BYTES];
    fn evaluation_area_count(&mut self) -> u8;
    fn evaluated_count(&mut self) -> u8;
    fn select_measured(&mut self, entry: MeasuredEntry);
    /// The image task's statistics check of the selected entry, recorded in the entry.
    fn check_measured(&mut self, entry: MeasuredEntry) -> bool;
    fn accept_measured(&mut self, entry: MeasuredEntry);
    fn next_evaluation(&mut self) -> EvaluationIndex;
    /// Calc 0x3c00, completion 0x8e.
    fn request_evaluation(&mut self, index: EvaluationIndex);
    fn absorb_evaluation(&mut self, evaluation: &Evaluation);
    fn correction_needed(&mut self) -> bool;
    /// Calc 0x3c01, completion 0x8f, unless the tables leave nothing to correct.
    fn request_correction(&mut self, setup: &ExposureSetup) -> CorrectionRequest;
    fn levels(&mut self) -> ExposureLevels;
    fn alternate(&mut self) -> bool;
    fn variant(&mut self) -> u32;
    fn exporting(&mut self) -> bool;
    fn export(&mut self, frame: Option<Frame>, correction: &Correction, kind: ExportKind);
    fn clear_selection(&mut self, frame: Option<Frame>, primary: bool);
    fn finish_primary(&mut self);
}

/// An entry of the record's exposure table, 0..=24 in the record's order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureSlot(pub u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExposureOffset { First, Second, Third }

pub const METERING_BYTES: usize = 64;
pub const METERING_SOURCE_BYTES: usize = 56;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeteringBlock(pub [u8; METERING_BYTES]);

/// The image-4 record layer's exposure fields of the bound record.
pub trait ExposureRecord {
    fn store_exposure(&mut self, slot: ExposureSlot, value: i32);
    fn store_offset(&mut self, offset: ExposureOffset, value: i16);
    fn store_area_exposure(&mut self, index: usize, value: i16);
    fn metering(&mut self) -> MeteringBlock;
    /// The record's metering values that replace the block's first bytes at the conclusion.
    fn metering_source(&mut self) -> [u8; METERING_SOURCE_BYTES];
    fn store_metering(&mut self, block: &MeteringBlock);
}

pub trait ExposureServices {
    fn lens_offset(&mut self, lens_reference: LensReference) -> u16;
    fn calibration_offset(&mut self) -> u16;
    fn exports_last_correction(&mut self) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecorderMode { Exposure }

/// The saclac diagnostic recorder shared with the other saclac modules.
pub trait Recorder {
    fn recording(&mut self, mode: RecorderMode) -> bool;
    fn start_recording(&mut self, mode: RecorderMode);
    fn record_evaluations(&mut self, count: u8);
}

pub trait StatisticsPort {
    /// Imgdc 0x3f1a, completions 0x4d then 0x4c; the port stages `subjects` where the image task
    /// reads them.
    fn request_statistics(&mut self, request: &StatisticsRequest, subjects: &SubjectList);
    /// Imgdc 0x3f1b.
    fn release_statistics(&mut self);
}

pub trait Replies {
    fn reply(&mut self, reply: Reply);
}

pub trait Completions {
    /// Enters the completion's callback in task 43 without a result, which posts it to queue 43.
    fn self_complete(&mut self, completion: Completion);
}
