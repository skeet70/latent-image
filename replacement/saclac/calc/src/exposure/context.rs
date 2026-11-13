//! The exposure context: what the shot binds for one exposure, the record snapshot the engine
//! evaluates, the selection bank, the requests built for calc, and the results other saclac
//! modules read back.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;

use crate::policy::ExposureMode;
use crate::selection::SelectionBank;

pub const SUBJECTS: usize = 10;
pub const SUBJECT_DETAIL_BYTES: usize = 100;
pub const SUMMARY_WORDS: usize = 10;
pub const EVALUATION_AREAS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Subject {
    pub id: u32,
    pub detail: [u8; SUBJECT_DETAIL_BYTES],
}

impl Default for Subject {
    fn default() -> Self { Self { id: 0, detail: [0; SUBJECT_DETAIL_BYTES] } }
}

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
    pub level_tables: Option<SharedAddress>,
    pub subject_levels: Option<SharedAddress>,
    pub level_bias: i16,
    pub level_classes: Option<SharedAddress>,
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

/// The candidate numbers the evaluation spans, 1-based.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CandidateSpan {
    pub first: u8,
    pub last: u8,
    pub chosen: u32,
}

/// One statistics area as the image task measures it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvaluationArea {
    pub first: u32,
    pub second: u32,
    pub third: u32,
    pub size: u16,
    pub level: u16,
    pub weight: u32,
}

/// The adjustment the second evaluation path settled on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Adjustment {
    pub level: u16,
    pub state: u32,
}

/// What the shot's exposure step and its calculation step read back from the engine.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    /// The level the evaluation concluded at and the level it is measured against.
    pub base_level: i32,
    pub reference_level: i32,
    /// The adjustment path replaces the evaluation's conclusion.
    pub adjusting: bool,
    /// Nonzero when the conclusion is exported to the frame.
    pub export: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureContext {
    pub binding: Binding,
    pub levels: Levels,
    pub outcome: Outcome,
    pub snapshot: Snapshot,
    pub span: CandidateSpan,
    pub areas: [EvaluationArea; EVALUATION_AREAS],
    pub selection: SelectionBank,
    pub adjustment: Adjustment,
    pub live: bool,
    pub return_mode: u32,
}
