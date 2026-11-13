#![forbid(unsafe_code)]

//! Leaf work of the calculation step (calc 0x3c02): the calculation input for a half press or the
//! live sequence, the half-press choice between a new calculation and the retained result, and the
//! word a result carries into the bound record's lens snapshot.

use crate::boundary::{ClientRef, LensSnapshot, Records};
use crate::ports::CalculationPlan;
use crate::preparation::HalfPressParameters;
use crate::stock::calculation::with_result_word;
use xm1_saclac_interface::RecordHandle;

/// Half-press mode that resumes the previous calculation input.
const RESUME_MODE: u32 = 7;
/// Mask bit of the half-press parameters that asks for a new calculation.
const NEW_CALCULATION: u32 = 1 << 1;
/// Property bit that keeps a repeated half press from reusing the retained result.
const REUSE_BLOCKED: u32 = 1 << 2;

/// Words of the exposure context the calculation reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExposureWord {
    /// The half-press mode the exposure step took.
    Mode,
    /// The exposure class the calculation library chose at the exposure's start.
    Class,
    /// The exposure step's profile word for the record.
    Profile,
    /// The word the exposure evaluation left for the selection.
    Evaluation,
    /// The refinement-pass flag the exposure step took.
    Refinement,
    /// Nonzero when the exposure asks for the half press to be repeated.
    Repeat,
}

/// Profile values of the bound record the calculation reads (image-4 profile records).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationProfile {
    /// Lens reference the calculation input is taken at.
    LensReference,
    /// First argument of the row-data check and the selection policy.
    SelectionKind,
    RowLimit,
    RowOption,
    AdjustmentKind,
    AdjustmentOption,
    /// Resume offsets copied into a resumed input.
    ResumeLead,
    ResumeOffset,
}

/// The exposure context the calculation binds and the calculation input inside it, which the
/// calc task reads.
pub trait CalculationContext {
    fn exposure(&mut self, word: ExposureWord) -> u32;
    /// Names the record in the exposure context and sets the lens reference and option the
    /// input is built from.
    fn bind_exposure(&mut self, record: RecordHandle, lens_reference: u32, option: u32);
    fn clear_repeat(&mut self);
    /// The target words the last finish and the last result left (also kept in the saved input).
    fn retained_targets(&mut self) -> (u32, u16);
    fn set_input_targets(&mut self, word: u32, half: u16);
    /// Keeps the input for a later resumed half press.
    fn save_input(&mut self);
    /// Takes the saved input back, moved to `lens_reference`.
    fn resume_input(&mut self, lens_reference: u32);
    fn set_resume_offsets(&mut self, lead: u16, lens_word: u32, offset: u16);
}

/// The plan words beside the result in the calculation block.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanWords {
    pub row_data: u32,
    pub adjustment: u8,
}

/// The calculation block: the bound record, the result calc 0x3c02 writes and the plan words.
pub trait CalculationBlock {
    fn bind_calculation(&mut self, record: RecordHandle);
    /// Clears the plan words, sets every grid cell to its start value and writes `plan`.
    fn reset_plan(&mut self, plan: PlanWords);
    fn set_selection(&mut self, selection: u32);
    fn mark_requested(&mut self);
    /// Takes the retained result back into the block.
    fn restore_result(&mut self);
    /// Keeps the block's result as the retained result.
    fn retain_result(&mut self);
    /// The result word the bound record's lens snapshot takes.
    fn result_word(&mut self) -> u32;
}

/// The calculation library's planning functions.
pub trait CalculationLibrary {
    fn selection_mode(&mut self, record: RecordHandle, refinement: u32) -> u32;
    fn row_data(&mut self, kind: u32, mode: u32, evaluation: u32, limit: u32, option: u32) -> u32;
    fn adjustment_status(&mut self, kind: u32, mode: u32, option: u32) -> u8;
    fn class_skips_selection(&mut self, class: u32) -> bool;
    fn display_class(&mut self, record: RecordHandle, refinement: u32) -> u32;
    fn selection_policy(&mut self, kind: u32, display: u32, evaluation: u32, option: u32) -> u32;
    /// Builds the calc 0x3c02 input from the exposure context.
    fn build_input(&mut self, area: [u32; 2], selection: u32);
    fn class_reuses_result(&mut self, class: u32) -> bool;
}

/// Image-4 profile reads and the one profile store the calculation makes.
pub trait CalculationProfiles {
    fn profile(&mut self, record: RecordHandle, value: CalculationProfile) -> u32;
    fn store_resume_offset(&mut self, record: RecordHandle, offset: u16);
    /// Configuration property that blocks reusing the retained result.
    fn reuse_control(&mut self) -> u32;
    /// Two words of client memory named by the half-press parameters.
    fn client_pair(&mut self, at: ClientRef) -> [u32; 2];
}

pub trait CalculationBoundary: CalculationContext + CalculationBlock + CalculationLibrary + CalculationProfiles + Records {}
impl<T: CalculationContext + CalculationBlock + CalculationLibrary + CalculationProfiles + Records> CalculationBoundary for T {}

/// State of the calculation step between half presses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalculationWork {
    /// The selection of the last half press that asked for a new calculation; others reuse it.
    retained_selection: u32,
}

impl CalculationWork {
    pub const fn new() -> Self { Self { retained_selection: 0 } }

    /// Prepares the live sequence's calculation; the caller then requests calc 0x3c02.
    pub fn prepare_live(&mut self, record: RecordHandle, boundary: &mut impl CalculationBoundary) {
        bind(record, 1, boundary);
        boundary.reset_plan(PlanWords::default());
        boundary.set_selection(0);
        boundary.mark_requested();
        boundary.build_input([0; 2], 0);
    }

    /// Prepares the half-press calculation and reports whether calc 0x3c02 is requested or the
    /// retained result stands.
    pub fn plan(&mut self, record: RecordHandle, parameters: &HalfPressParameters, boundary: &mut impl CalculationBoundary) -> CalculationPlan {
        let area = parameters.area_count.map_or([0; 2], |at| boundary.client_pair(at));
        let lens_reference = bind(record, parameters.option, boundary);
        let refinement = boundary.exposure(ExposureWord::Refinement);
        let evaluation = boundary.exposure(ExposureWord::Evaluation);
        let kind = boundary.profile(record, CalculationProfile::SelectionKind);
        let mode = boundary.selection_mode(record, refinement);
        let limit = boundary.profile(record, CalculationProfile::RowLimit);
        let row_option = boundary.profile(record, CalculationProfile::RowOption);
        let row_data = boundary.row_data(kind, mode, evaluation, limit, row_option);
        let adjustment = if row_data == 0 {
            0
        } else {
            let adjustment_kind = boundary.profile(record, CalculationProfile::AdjustmentKind);
            let mode = boundary.selection_mode(record, refinement);
            let adjustment_option = boundary.profile(record, CalculationProfile::AdjustmentOption);
            boundary.adjustment_status(adjustment_kind, mode, adjustment_option)
        };
        boundary.reset_plan(PlanWords { row_data, adjustment });

        let new_calculation = parameters.mask.0 & NEW_CALCULATION != 0;
        let resumed = parameters.mode.0 == RESUME_MODE;
        let selection = if new_calculation && !resumed {
            let selection = self.selection(record, refinement, evaluation, parameters.option, boundary);
            boundary.mark_requested();
            self.retained_selection = selection;
            selection
        } else {
            self.retained_selection
        };
        boundary.set_selection(selection);
        boundary.build_input(area, selection);

        if !new_calculation {
            let (word, half) = boundary.retained_targets();
            boundary.set_input_targets(word, half);
        } else if !resumed {
            boundary.save_input();
        }

        if resumed {
            resume(record, lens_reference, boundary);
        } else if boundary.exposure(ExposureWord::Repeat) != 0 {
            let class = boundary.exposure(ExposureWord::Class);
            if !boundary.class_reuses_result(class) && boundary.reuse_control() & REUSE_BLOCKED == 0 { return CalculationPlan::Skip; }
        }
        if new_calculation { CalculationPlan::Request } else { CalculationPlan::Skip }
    }

    /// The retained result stands for a skipped calculation; the bound record takes its word.
    pub fn record_skipped(&mut self, boundary: &mut impl CalculationBoundary) {
        boundary.restore_result();
        record_result(boundary);
    }

    /// A requested calculation completed: its result becomes the retained one unless the half
    /// press resumed, and the bound record takes its word.
    pub fn record_result(&mut self, boundary: &mut impl CalculationBoundary) {
        if boundary.exposure(ExposureWord::Mode) != RESUME_MODE { boundary.retain_result(); }
        record_result(boundary);
    }

    fn selection(&self, record: RecordHandle, refinement: u32, evaluation: u32, option: u32, boundary: &mut impl CalculationBoundary) -> u32 {
        let class = boundary.exposure(ExposureWord::Class);
        if boundary.class_skips_selection(class) { return 0; }
        let kind = boundary.profile(record, CalculationProfile::SelectionKind);
        let display = boundary.display_class(record, refinement);
        boundary.selection_policy(kind, display, evaluation, option)
    }
}

fn bind(record: RecordHandle, option: u32, boundary: &mut impl CalculationBoundary) -> u32 {
    boundary.bind_calculation(record);
    let lens_reference = boundary.profile(record, CalculationProfile::LensReference);
    boundary.bind_exposure(record, lens_reference, option);
    lens_reference
}

/// A resumed half press takes the saved input back at the current lens reference.
fn resume(record: RecordHandle, lens_reference: u32, boundary: &mut impl CalculationBoundary) {
    let lens_word = boundary.exposure(ExposureWord::Profile);
    let lead = boundary.profile(record, CalculationProfile::ResumeLead) as u16;
    let offset = boundary.profile(record, CalculationProfile::ResumeOffset) as u16;
    boundary.resume_input(lens_reference);
    boundary.set_resume_offsets(lead, lens_word, offset);
    boundary.store_resume_offset(record, offset);
    boundary.clear_repeat();
}

fn record_result(boundary: &mut impl CalculationBoundary) {
    let word = boundary.result_word();
    let snapshot: LensSnapshot = boundary.lens_snapshot();
    boundary.store_lens_snapshot(&with_result_word(snapshot, word));
}
