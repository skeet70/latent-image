#![forbid(unsafe_code)]

//! Traits for what the shot's leaf work calls outside the module: the image-4 record layer shared
//! with the other saclac modules, lens control, the area planner shared with the measurement
//! round, client memory named by the command parameters, diagnostic dumps and platform services.
//! Byte layouts of the values that cross are in `stock`.

use crate::subjects::Subject;
use crate::summary::{AreaStatistics, FrameSummary};

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

/// The selection plan the shared area planner writes for a half press or the live sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionPlan(pub [u8; 536]);

impl Default for SelectionPlan {
    fn default() -> Self { Self([0; 536]) }
}

pub trait Planner {
    fn plan_selection(&mut self, area_count: Option<ClientRef>, live: bool) -> SelectionPlan;
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
pub enum Dump { Statistics, Frame, Lens, Numbered }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trace { Prepare, HalfPressReplied }

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
