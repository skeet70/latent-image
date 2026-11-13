//! The exposure context: what the shot binds for one exposure, the record snapshot the engine
//! evaluates, the selection bank, the requests built for calc, and the results other saclac
//! modules read back.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::exposure::{CorrectionRequest, EvaluationRequest};
use xm1_saclac_interface::RecordHandle;

use crate::policy::ExposureMode;
use crate::selection::SelectionBank;
use crate::stock::record::UNBOUND;

pub const SUBJECTS: usize = 10;
pub const SUBJECT_DETAIL_BYTES: usize = 100;
pub const SUMMARY_WORDS: usize = 10;
pub const EVALUATION_AREAS: usize = 8;

/// Calc reads subject lists in place in this layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Subject {
    pub id: u32,
    pub detail: [u8; SUBJECT_DETAIL_BYTES],
}

impl Default for Subject {
    fn default() -> Self { Self { id: 0, detail: [0; SUBJECT_DETAIL_BYTES] } }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectList {
    pub count: u32,
    pub entries: [Subject; SUBJECTS],
}

impl SubjectList {
    pub fn listed(&self) -> &[Subject] { &self.entries[..(self.count as usize).min(SUBJECTS)] }
}

/// What the shot binds before the engine prepares its tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Binding {
    /// The half-press mask; the live exposure binds 1.
    pub mask: u32,
    /// The half-press mode; the live exposure binds 4.
    pub mode: u32,
    pub exposure_mode: ExposureMode,
    /// The client's dump frame the engine fills when one is named.
    pub frame: Option<SharedAddress>,
    pub lens_reference: u32,
    pub source: u32,
    pub option: u32,
    /// The record whose focus result and adjustment tables the snapshot takes.
    pub snapshot_record: Option<RecordHandle>,
    /// The record every other profile read uses.
    pub record: Option<RecordHandle>,
    pub subjects: SubjectList,
    pub summary: [u32; SUMMARY_WORDS],
    /// The half press's refinement pass: profile policies that a suppression flag disables read 0.
    pub refinement_pass: bool,
}

impl Binding {
    /// The handle profile reads use; an unbound context reads the selected record.
    pub fn record_handle(&self) -> RecordHandle { self.record.unwrap_or(UNBOUND) }

    pub fn snapshot_handle(&self) -> RecordHandle { self.snapshot_record.unwrap_or(UNBOUND) }
}

/// The levels the evaluation starts from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Levels {
    pub target: i16,
    pub start: i16,
    pub measured: u16,
    pub curve: u32,
    /// The measured level was zero and the target took its place.
    pub unmeasured: bool,
    pub policy: u32,
}

/// The record's focus result fields the engine reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusResult {
    pub reference: u32,
    pub valid: u32,
    pub lens: u32,
    pub terms: [u16; 6],
    pub limit: i16,
    /// Nonzero: the program starts no lower than the snapshot's offset (0xf64a1c).
    pub pinned: u32,
    pub level_tables: Option<SharedAddress>,
    pub subject_levels: Option<SharedAddress>,
    pub level_bias: i16,
    pub level_classes: Option<SharedAddress>,
    /// The statistics block the focus result names (+0x48).
    pub statistics: Option<SharedAddress>,
    pub weights: [u16; 5],
    pub lens_detail: u32,
    pub levels: [i16; 5],
    pub extent: u32,
    pub adjustment: [u32; 3],
    pub adjustment_kind: u16,
    pub subject_count: u32,
    pub adjustment_level: i16,
}

/// The record taken at preparation: its focus result, judged against the bound lens reference,
/// and its adjustment tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    /// The focus result's valid word when it was taken at the bound lens reference, else 0.
    pub valid: u32,
    pub extent: u32,
    pub offset: i16,
    pub focus: FocusResult,
    pub bounds: [i16; 4],
    /// The record's subjects; `focus.subject_levels` names one level per listed subject.
    pub subjects: SubjectList,
    pub table_words: [u32; 10],
    pub table_block: [u32; 20],
    pub limits: [i16; 6],
    pub limit_word: u32,
}

/// What set the program's start level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StartBound {
    /// The configured margin above the low end, or the ceiling.
    #[default]
    Margin,
    Floor,
    /// The snapshot's offset.
    Pinned,
}

impl StartBound {
    /// The word the dump frame records.
    pub const fn code(self) -> u32 {
        match self {
            Self::Margin => 0,
            Self::Floor => 1,
            Self::Pinned => 2,
        }
    }
}

/// The program's level range and its division into steps; `first..=last` are the 1-based steps
/// the evaluation spans.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CandidateSpan {
    pub first: u8,
    pub last: u8,
    pub low: i16,
    pub high: i16,
    pub start: i16,
    pub end: i16,
    pub first_step: i16,
    pub step: i16,
    pub bound: StartBound,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureStep {
    /// 0-based.
    pub index: u8,
    pub position: i16,
    /// What is left of the configured offset after the steps so far.
    pub offset: i16,
    /// The level before `converted`.
    pub level: i16,
    pub profile: u32,
    /// The next step's profile.
    pub next_profile: u32,
    pub lens_step: u32,
    /// `level` converted for `profile`.
    pub converted: u16,
    pub extended: bool,
}

pub const PROGRAM_STEPS: usize = 8;

/// One step of the span as the image task measures it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvaluationArea {
    pub profile: u32,
    pub next_profile: u32,
    pub lens_step: u32,
    pub converted: u16,
    /// The step's offset above the target level.
    pub level: u16,
    pub extended: bool,
}

/// The adjustment the second evaluation path settled on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Adjustment {
    pub level: u16,
    pub state: u32,
    /// The adjustment path's code for its state and step count: 0 to 7 (0xf6b7f4).
    pub code: u8,
}

/// What the shot's exposure step and its calculation step read back from the engine.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    /// The scene admits a subject treatment (0xfa672c), and admits the first (0xfa66d4).
    pub treated: bool,
    pub first_treatment: bool,
    /// The adjustment path replaces the evaluation's conclusion.
    pub adjusting: bool,
    /// The snapshot's focus result stands for the current record (0xf6b890).
    pub focus_reused: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureContext {
    pub binding: Binding,
    pub levels: Levels,
    pub outcome: Outcome,
    pub snapshot: Snapshot,
    pub span: CandidateSpan,
    pub program: [ExposureStep; PROGRAM_STEPS],
    pub areas: [EvaluationArea; EVALUATION_AREAS],
    pub selection: SelectionBank,
    /// The last evaluation request built for calc (0x3c00).
    pub evaluation: EvaluationRequest,
    pub correction: CorrectionRequest,
    pub adjustment: Adjustment,
    pub live: bool,
    pub return_mode: u32,
}
