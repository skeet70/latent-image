#![forbid(unsafe_code)]

//! The calculation absorb: what a calc 0x3c02 result leaves in the bound record, in the retained
//! result and in the frame's diagnostics, for a half press, the live sequence and the capture's
//! lens return.

use crate::boundary::{Client, Frame, LensReference, MeteringBlock, SelectionKind};
use crate::capture_boundary::{AreaReference, RecordField};
use crate::preparation::HalfPressParameters;
use xm1_saclac_calc::policy::{ExposureMode, RowFlags, SelectionFlags};
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, CellSummary, FocusSummary, GridCell, GridPass, ResultPass, TrackedKind};
use xm1_saclac_interface::RecordHandle;

/// The half-press mode whose first area may take an auxiliary reference.
const AUXILIARY_MODE: u32 = 7;
/// A plan area kind that names no tracked subject.
pub const UNTRACKED: u8 = 7;
/// The selection mode that spans the three grid passes.
const ALL_PASSES: u32 = 4;

/// Which step asked for the absorb.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    HalfPress(HalfPressParameters),
    Live,
    /// The capture's lens return; the record names the frame and the slot count.
    Return { pending: bool },
}

/// The exposure context the absorb reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Absorb {
    pub origin: Origin,
    /// The record the calculation block is bound to.
    pub record: RecordHandle,
    /// The record the exposure is bound to.
    pub exposure_record: RecordHandle,
    pub selection: SelectionKind,
    pub lens_reference: LensReference,
    pub refinement: u32,
    /// The exposure asked for the half press to be repeated.
    pub repeat: bool,
    /// The exposure belongs to the live sequence.
    pub live: bool,
}

/// What the auxiliary reference query decided for an area, as the calculation block keeps it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AuxiliaryOutcome {
    /// Not queried, or the selection changed.
    #[default]
    Unchecked,
    Kept,
    /// The area takes the auxiliary reference.
    Replaced,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuxiliaryReference {
    pub outcome: AuxiliaryOutcome,
    pub source: u32,
    pub target: u32,
    pub normalized: i16,
}

/// The fields of an area both store forms write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaStore {
    pub area: AreaResult,
    pub weight: u32,
    pub marker: u32,
}

/// The fields only the full store form writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaDetail {
    pub selection_class: u8,
    pub plan_half: i16,
    pub plan_bit_two: bool,
    pub plan_bit_five: bool,
    pub distance: u32,
    pub row_class: u8,
    /// `None` where the store leaves the field alone.
    pub extended: Option<bool>,
    pub row_word: u32,
}

/// The two words the absorb leaves at the end of the record's metering block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeteringMinimum {
    pub single_pass: bool,
    pub minimum: u32,
}

/// Frame diagnostics the calculation library writes for the absorbed result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultExport { Policy, Compact, SelectedChannel, SourceLimits }

/// The calculation block: the calc 0x3c02 result and the plan words beside it.
pub trait CalculationResults {
    fn result_areas(&mut self) -> u8;
    /// The plan's row-data word is set.
    fn tracking(&mut self) -> bool;
    /// The plan's subject kind for an area; `UNTRACKED` names none.
    fn area_kind(&mut self, index: AreaIndex) -> u8;
    fn grid_selected(&mut self) -> bool;
    fn selection_mode(&mut self) -> u32;
    fn result_area(&mut self, at: AreaAt) -> AreaResult;
    /// The marker of an area: per tracked kind, or the pass's default.
    fn marker(&mut self, pass: ResultPass, kind: Option<TrackedKind>, index: AreaIndex) -> u32;
    fn plan_value(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) -> u32;
    fn plan_half(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) -> i16;
    fn row(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) -> AreaRow;
    fn row_word(&mut self, pass: ResultPass, cell: GridCell) -> u32;
    fn cell_summary(&mut self, pass: ResultPass, cell: GridCell) -> CellSummary;
    fn focus_summary(&mut self) -> FocusSummary;
    fn retained_half(&mut self) -> u16;
    fn auxiliary(&mut self) -> AuxiliaryOutcome;
    fn set_auxiliary(&mut self, outcome: AuxiliaryOutcome);
    /// Keeps the working result aside while the retained one is exported.
    fn set_working_aside(&mut self);
    fn restore_working(&mut self);
    /// Copies a grid pass's area over the plain area of the cell; a tracked kind's plain area also
    /// takes the pass's area of that kind.
    fn adopt_grid_area(&mut self, pass: GridPass, cell: GridCell, index: AreaIndex, kind: Option<TrackedKind>);
    /// Copies a grid pass's plan word, plan halfword and row over the plain ones.
    fn adopt_grid_tables(&mut self, pass: GridPass, cell: GridCell, index: AreaIndex);
    fn adopt_grid_cell(&mut self, pass: GridPass, cell: GridCell);
}

/// The selection grid the shot keeps beside the calculation block.
pub trait SelectionGrid {
    /// The cell the selection holds for an area of a pass.
    fn cell(&mut self, pass: ResultPass, index: AreaIndex) -> GridCell;
    fn update_grid(&mut self, record: RecordHandle, reset: bool, focus: bool, lens_reference: LensReference);
    /// The record's area references classified into the subject banks.
    fn classify_subjects(&mut self, stage: ClassifyStage, reference: &AreaReference, kind: u32, source: u32, area: u32);
    /// Copies the working result into the retained result.
    fn retain_result(&mut self);
    /// Copies the retained result into the working result.
    fn restore_result(&mut self);
    /// The retained result takes the auxiliary source and, for an untracked area, its reference.
    fn retain_auxiliary(&mut self, auxiliary: &AuxiliaryReference, untracked: Option<AreaAt>);
    fn mark_auxiliary_retained(&mut self);
    fn requires_eligibility(&mut self) -> bool;
    fn area_weight(&mut self, index: AreaIndex) -> u32;
    /// The retained targets and their copies in the saved calculation input.
    fn retain_target_half(&mut self, half: u16);
}

/// Which caller classifies the subjects (the classification's first argument, 0 to 3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassifyStage { Absorb, RecordLoad, Round, CaptureJob }

/// Calculation-library and configuration entries the absorb calls.
pub trait AbsorbLibrary {
    fn subject_source(&mut self, record: RecordHandle, refinement: u32) -> u32;
    fn grid_mode(&mut self, record: RecordHandle, mode: u32) -> u32;
    /// Fills an area's row from the area.
    fn evaluate_distances(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex);
    fn selection_changed(&mut self, record: RecordHandle, source: u32, reference: u32) -> bool;
    /// Whether a reference is available for `source`, and the target the comparison leaves.
    fn compare_reference(&mut self, record: RecordHandle, source: u32) -> (bool, u32);
    fn compensated(&mut self, target: u32) -> i16;
    fn classify_level(&mut self, value: i16) -> u32;
    fn split_reference(&mut self) -> u32;
    fn lower_reference(&mut self, lens_reference: LensReference) -> u32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbsorbAdjustment {
    /// Bit 2 exports the retained result in place of the working one.
    RetainedExport,
    /// Bits 0..1 classify the subjects and enable the rate status.
    Subjects,
    /// Bit 0 draws the rate status.
    RateStatus,
}

pub trait AbsorbDiagnostics {
    fn adjustment(&mut self, adjustment: AbsorbAdjustment) -> u8;
    fn export_result(&mut self, frame: Option<Frame>, export: ResultExport, enabled: bool);
    fn clear_channel_diagnostics(&mut self, frame: Option<Frame>, cleared: bool);
    fn draw_rate_status(&mut self, reference: &AreaReference);
    fn release_rate_status(&mut self);
}

pub trait AbsorbRecords {
    fn store_area(&mut self, record: RecordHandle, index: AreaIndex, store: &AreaStore);
    fn store_area_detail(&mut self, record: RecordHandle, index: AreaIndex, detail: &AreaDetail);
    fn store_auxiliary(&mut self, record: RecordHandle, index: AreaIndex, auxiliary: &AuxiliaryReference);
    fn store_cell(&mut self, record: RecordHandle, cell: &CellSummary, weight: u32);
    fn store_focus_summary(&mut self, record: RecordHandle, summary: &FocusSummary, level: u32);
    fn area_lens_reference(&mut self, record: RecordHandle, index: u8) -> u32;
    fn extended_areas(&mut self, record: RecordHandle) -> bool;
    fn selection_source(&mut self, record: RecordHandle) -> u32;
    fn metering(&mut self, record: RecordHandle) -> MeteringBlock;
    fn store_metering(&mut self, record: RecordHandle, block: &MeteringBlock);
    fn value(&mut self, record: RecordHandle, field: RecordField) -> u32;
    fn area_references(&mut self, record: RecordHandle) -> AreaReference;
}

pub trait AbsorbBoundary: CalculationResults + SelectionGrid + AbsorbLibrary + AbsorbDiagnostics + AbsorbRecords + Client {}
impl<T: CalculationResults + SelectionGrid + AbsorbLibrary + AbsorbDiagnostics + AbsorbRecords + Client> AbsorbBoundary for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Plan {
    /// Every area is kept: the half press keeps its areas, or the origin has no client areas.
    whole: bool,
    mode: u32,
    frame: Option<Frame>,
    count: u8,
    returning: bool,
    return_pending: bool,
}

impl Plan {
    fn of(absorb: &Absorb, boundary: &mut impl AbsorbBoundary) -> Self {
        let plain = |mode, frame, count| Plan { whole: true, mode, frame, count, returning: false, return_pending: false };
        match absorb.origin {
            Origin::Return { pending } => {
                let frame = boundary.value(absorb.record, RecordField::CaptureFrame);
                let count = boundary.value(absorb.record, RecordField::CaptureSlotCount) as u8;
                Plan { returning: true, return_pending: pending, ..plain(0, (frame != 0).then_some(Frame(frame)), count) }
            }
            Origin::Live => plain(4, None, 1),
            Origin::HalfPress(parameters) => {
                let frame = parameters.frame_slot.and_then(|slot| boundary.frame(slot));
                let count = match parameters.area_count.map(|count| boundary.area_count(count)) {
                    None | Some(0) => 1,
                    Some(count) => count as u8,
                };
                Plan { whole: !parameters.mask.publishes_areas(), ..plain(parameters.mode.0, frame, count) }
            }
        }
    }

    /// Whether the grid update resets the grid and whether it refreshes its focus references.
    fn grid_update(&self) -> (bool, bool) {
        let narrow = !self.whole && matches!(self.mode, 0..=2);
        let reset = self.mode == AUXILIARY_MODE || self.returning || narrow;
        let focus = self.returning || (!self.whole && matches!(self.mode, 0 | 1));
        (reset, focus)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Form { Full, Short }

/// Absorbs the calculation block's result into the bound record.
pub fn absorb(absorb: &Absorb, boundary: &mut impl AbsorbBoundary) {
    let plan = Plan::of(absorb, boundary);
    let mode = ExposureMode(absorb.selection.0);
    let half_press = matches!(absorb.origin, Origin::HalfPress(_));
    let retained_export = boundary.adjustment(AbsorbAdjustment::RetainedExport) & 4 != 0;
    if half_press {
        if !absorb.repeat {
            if mode.is_primary() { boundary.clear_channel_diagnostics(plan.frame, true); }
        } else {
            if !mode.uses_adjustment() && retained_export { export_retained(absorb, &plan, boundary); }
            if mode.is_primary() && !retained_export { boundary.clear_channel_diagnostics(plan.frame, false); }
        }
    }
    update_grid(absorb, &plan, boundary);
    let subjects = boundary.adjustment(AbsorbAdjustment::Subjects) & 3 != 0;
    if boundary.tracking() && subjects {
        let reference = boundary.area_references(absorb.record);
        let source = boundary.subject_source(absorb.record, absorb.refinement);
        let kind = boundary.value(absorb.record, RecordField::SubjectKind);
        let area = boundary.value(absorb.record, RecordField::SubjectArea);
        boundary.classify_subjects(ClassifyStage::Absorb, &reference, kind, source, area);
    }
    if subjects && boundary.adjustment(AbsorbAdjustment::RateStatus) & 1 != 0 {
        if boundary.value(absorb.exposure_record, RecordField::MapPresent) == 0 {
            boundary.release_rate_status();
        } else {
            let reference = boundary.area_references(absorb.record);
            boundary.draw_rate_status(&reference);
        }
    }

    let (stored, positioned) = if plan.whole {
        store_areas(Form::Full, ResultPass::Plain, absorb, &plan, boundary);
        let half = boundary.retained_half();
        boundary.retain_target_half(half);
        (true, true)
    } else if matches!(plan.mode, 0 | 1) {
        store_areas(Form::Full, ResultPass::Plain, absorb, &plan, boundary);
        (true, true)
    } else if boundary.tracking() {
        store_areas(Form::Short, ResultPass::Plain, absorb, &plan, boundary);
        (true, false)
    } else {
        (false, false)
    };
    if positioned { store_cell(ResultPass::Plain, absorb.record, boundary); }
    let summarized = matches!(absorb.selection.0, 0 | 1 | 5);
    if summarized {
        let summary = boundary.focus_summary();
        let level = boundary.classify_level(summary.classified());
        boundary.store_focus_summary(absorb.record, &summary, level);
    }
    store_metering(absorb, &plan, boundary);

    if half_press {
        let enabled = absorb.repeat && !mode.uses_adjustment() && !retained_export;
        if stored {
            boundary.export_result(plan.frame, ResultExport::Policy, enabled);
            if enabled { boundary.export_result(plan.frame, ResultExport::Compact, true); }
        }
        if positioned { boundary.export_result(plan.frame, ResultExport::SelectedChannel, enabled); }
        if summarized { boundary.export_result(plan.frame, ResultExport::SourceLimits, enabled); }
    }
}

/// With the retained result in the block, the grid is updated and the retained result exported.
fn export_retained(absorb: &Absorb, plan: &Plan, boundary: &mut impl AbsorbBoundary) {
    boundary.set_working_aside();
    boundary.restore_result();
    update_grid(absorb, plan, boundary);
    boundary.export_result(plan.frame, ResultExport::Compact, true);
    boundary.export_result(plan.frame, ResultExport::SelectedChannel, true);
    boundary.export_result(plan.frame, ResultExport::SourceLimits, true);
    boundary.restore_working();
}

fn update_grid(absorb: &Absorb, plan: &Plan, boundary: &mut impl AbsorbBoundary) {
    let (reset, focus) = plan.grid_update();
    boundary.update_grid(absorb.exposure_record, reset, focus, absorb.lens_reference);
}

fn area_indices(start: u8, count: u8) -> impl Iterator<Item = AreaIndex> { (start..count).map_while(AreaIndex::new) }

/// Stores each area of `pass` into the record, the half press's first area taking the auxiliary
/// reference in mode 7; a returning absorb with a pending lens point leaves the first area.
fn store_areas(form: Form, pass: ResultPass, absorb: &Absorb, plan: &Plan, boundary: &mut impl AbsorbBoundary) {
    let record = absorb.record;
    let start = u8::from(plan.returning && plan.return_pending);
    let tracking = boundary.tracking();
    let count = boundary.result_areas();
    let require = boundary.requires_eligibility();
    for index in area_indices(start, count) {
        let cell = boundary.cell(pass, index);
        let kind_word = if tracking { boundary.area_kind(index) } else { UNTRACKED };
        let kind = TrackedKind::new(kind_word);
        if let ResultPass::Grid(grid) = pass {
            boundary.adopt_grid_area(grid, cell, index, kind);
            if form == Form::Full { boundary.adopt_grid_tables(grid, cell, index); }
        }
        let at = match kind {
            None => AreaAt::Cell { pass: ResultPass::Plain, cell, index },
            Some(kind) => AreaAt::Tracked { pass: ResultPass::Plain, kind, index },
        };
        let area = boundary.result_area(at);
        let auxiliary = auxiliary_reference(record, plan.mode, require, area.reference(), index, boundary);
        if index.get() == 0 { boundary.set_auxiliary(auxiliary.outcome); }
        if auxiliary.outcome == AuxiliaryOutcome::Replaced {
            boundary.store_auxiliary(record, index, &auxiliary);
            let untracked = (kind_word == UNTRACKED).then_some(AreaAt::Cell { pass, cell, index });
            boundary.retain_auxiliary(&auxiliary, untracked);
            continue;
        }
        let store = AreaStore { area, weight: boundary.area_weight(index), marker: boundary.marker(pass, kind, index) };
        boundary.store_area(record, index, &store);
        if form == Form::Full {
            let detail = area_detail(pass, cell, index, record, boundary);
            boundary.store_area_detail(record, index, &detail);
        }
    }
    if form == Form::Full && pass == ResultPass::Plain && boundary.grid_selected() {
        for index in area_indices(0, count) {
            for grid in GridPass::ALL {
                let cell = boundary.cell(ResultPass::Grid(grid), index);
                boundary.evaluate_distances(ResultPass::Grid(grid), cell, index);
            }
        }
    }
    if plan.mode == AUXILIARY_MODE && boundary.auxiliary() != AuxiliaryOutcome::Replaced {
        boundary.retain_result();
        boundary.mark_auxiliary_retained();
    }
}

/// The plain pass evaluates the area's distances here; a grid pass carries its own rows.
fn area_detail(pass: ResultPass, cell: GridCell, index: AreaIndex, record: RecordHandle, boundary: &mut impl AbsorbBoundary) -> AreaDetail {
    let plan_value = boundary.plan_value(ResultPass::Plain, cell, index);
    let plan_half = boundary.plan_half(ResultPass::Plain, cell, index);
    let plain = pass == ResultPass::Plain;
    if plain { boundary.evaluate_distances(ResultPass::Plain, cell, index); }
    let row = boundary.row(ResultPass::Plain, cell, index);
    AreaDetail {
        selection_class: SelectionFlags(plan_value as u8).category(),
        plan_half,
        plan_bit_two: RowFlags(plan_value).bit_two(),
        plan_bit_five: RowFlags(plan_value).bit_five(),
        distance: row.distance(),
        row_class: RowFlags(u32::from(row.flags())).class(),
        extended: plain.then(|| boundary.extended_areas(record)),
        row_word: boundary.row_word(pass, cell),
    }
}

/// Mode 7 lets the first area take an auxiliary reference when the selection has not changed
/// and no other reference is available for it.
fn auxiliary_reference(record: RecordHandle, mode: u32, require: bool, reference: u32, index: AreaIndex, boundary: &mut impl AbsorbBoundary) -> AuxiliaryReference {
    if mode != AUXILIARY_MODE || index.get() != 0 { return AuxiliaryReference::default(); }
    let source = boundary.selection_source(record);
    let changed = require && boundary.selection_changed(record, source, reference);
    let (available, target) = boundary.compare_reference(record, source);
    let outcome = if changed {
        AuxiliaryOutcome::Unchecked
    } else if available || require {
        AuxiliaryOutcome::Kept
    } else {
        AuxiliaryOutcome::Replaced
    };
    AuxiliaryReference { outcome, source, target, normalized: boundary.compensated(target) }
}

fn store_cell(pass: ResultPass, record: RecordHandle, boundary: &mut impl AbsorbBoundary) {
    let Some(first) = AreaIndex::new(0) else { return };
    let cell = boundary.cell(pass, first);
    let summary = boundary.cell_summary(ResultPass::Plain, cell);
    let weight = boundary.area_weight(first);
    boundary.store_cell(record, &summary, weight);
}

fn store_metering(absorb: &Absorb, plan: &Plan, boundary: &mut impl AbsorbBoundary) {
    let mut grid_mode = boundary.grid_mode(absorb.record, plan.mode);
    if plan.mode == AUXILIARY_MODE { grid_mode = boundary.selection_mode(); }
    let mut block = boundary.metering(absorb.record);
    let minimum = if !plan.whole || !boundary.grid_selected() {
        MeteringMinimum { single_pass: true, minimum: record_minimum(absorb, plan.count, boundary) }
    } else {
        MeteringMinimum { single_pass: grid_mode != ALL_PASSES, minimum: grid_minimum(absorb, grid_mode, boundary) }
    };
    crate::stock::absorb::set_metering_minimum(&mut block, minimum);
    boundary.store_metering(absorb.record, &block);
}

/// The smallest lens reference of the record's first `count` areas; the configured split
/// reference, when nothing is below it, and the live sequence take the lower reference instead.
fn record_minimum(absorb: &Absorb, count: u8, boundary: &mut impl AbsorbBoundary) -> u32 {
    if absorb.live { return boundary.lower_reference(absorb.lens_reference); }
    let split = boundary.split_reference();
    let minimum = (0..count).map(|index| boundary.area_lens_reference(absorb.record, index)).fold(split, u32::min);
    if minimum == boundary.split_reference() { boundary.lower_reference(absorb.lens_reference) } else { minimum }
}

/// As `record_minimum`, over the grid areas of one pass or, for `ALL_PASSES`, of all three.
fn grid_minimum(absorb: &Absorb, grid_mode: u32, boundary: &mut impl AbsorbBoundary) -> u32 {
    if absorb.live { return boundary.lower_reference(absorb.lens_reference); }
    let passes: &[GridPass] = if grid_mode == ALL_PASSES { &GridPass::ALL } else { &[] };
    let single = GridPass::new(grid_mode);
    let split = boundary.split_reference();
    let count = boundary.result_areas();
    let mut minimum = split;
    for &pass in passes.iter().chain(single.iter()) {
        for index in area_indices(0, count) {
            let cell = boundary.cell(ResultPass::Grid(pass), index);
            let reference = boundary.result_area(AreaAt::Cell { pass: ResultPass::Grid(pass), cell, index }).reference();
            minimum = minimum.min(reference);
        }
    }
    if minimum == boundary.split_reference() { boundary.lower_reference(absorb.lens_reference) } else { minimum }
}
