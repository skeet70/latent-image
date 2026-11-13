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
    /// The subject kind a stage's bank keeps for an area; `UNTRACKED` names none.
    fn area_kind(&mut self, bank: ClassifyStage, index: AreaIndex) -> u8;
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

/// The selection grid the shot keeps beside the calculation block; `P` is what the grid's
/// operations call outside the shot.
pub trait SelectionGrid<P: ?Sized> {
    /// The cell the selection holds for an area of a pass.
    fn cell(&mut self, pass: ResultPass, index: AreaIndex) -> GridCell;
    fn update_grid(&mut self, record: RecordHandle, reset: bool, keep_gates: bool, lens_reference: LensReference, layers: &mut P);
    /// The record's area references classified into a stage's subject bank.
    fn classify_subjects(&mut self, stage: ClassifyStage, reference: &AreaReference, kind: u32, source: u32, area: u32, layers: &mut P);
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
    fn retain_target_word(&mut self, word: u32);
}

pub use xm1_saclac_interface::selection::ClassifyStage;

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

/// What the absorb calls outside the shot.
pub trait AbsorbPorts: AbsorbLibrary + AbsorbDiagnostics + AbsorbRecords + Client {}
impl<T: AbsorbLibrary + AbsorbDiagnostics + AbsorbRecords + Client> AbsorbPorts for T {}

/// The calculation block and the selection grid as the absorb uses them.
pub trait AbsorbShot<P: ?Sized>: CalculationResults + SelectionGrid<P> {}
impl<P: ?Sized, T: CalculationResults + SelectionGrid<P>> AbsorbShot<P> for T {}

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
    fn of(absorb: &Absorb, ports: &mut impl AbsorbPorts) -> Self {
        let plain = |mode, frame, count| Plan { whole: true, mode, frame, count, returning: false, return_pending: false };
        match absorb.origin {
            Origin::Return { pending } => {
                let frame = ports.value(absorb.record, RecordField::CaptureFrame);
                let count = ports.value(absorb.record, RecordField::CaptureSlotCount) as u8;
                Plan { returning: true, return_pending: pending, ..plain(0, (frame != 0).then_some(Frame(frame)), count) }
            }
            Origin::Live => plain(4, None, 1),
            Origin::HalfPress(parameters) => {
                let frame = parameters.frame_slot.and_then(|slot| ports.frame(slot));
                let count = match parameters.area_count.map(|count| ports.area_count(count)) {
                    None | Some(0) => 1,
                    Some(count) => count as u8,
                };
                Plan { whole: !parameters.mask.publishes_areas(), ..plain(parameters.mode.0, frame, count) }
            }
        }
    }

    /// Whether the grid update resets the grid and whether it keeps its target gates.
    fn grid_update(&self) -> (bool, bool) {
        let narrow = !self.whole && matches!(self.mode, 0..=2);
        let reset = self.mode == AUXILIARY_MODE || self.returning || narrow;
        let keep_gates = self.returning || (!self.whole && matches!(self.mode, 0 | 1));
        (reset, keep_gates)
    }

    fn target(&self, record: RecordHandle, eligibility: bool) -> AreaTarget {
        AreaTarget { record, mode: self.mode, skip_first: self.returning && self.return_pending, bank: ClassifyStage::Absorb, eligibility }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Form { Full, Short }

/// The record and selection the area stores write for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AreaTarget {
    pub record: RecordHandle,
    pub mode: u32,
    /// The lens return still has a lens point pending, so the first area keeps its value.
    pub skip_first: bool,
    /// The subject bank whose kinds pick each area's tracked kind.
    pub bank: ClassifyStage,
    /// A mode-7 first area keeps its own reference while its selection is unchanged.
    pub eligibility: bool,
}

pub(crate) const FIRST_AREA: AreaIndex = match AreaIndex::new(0) {
    Some(index) => index,
    None => panic!(),
};

/// Absorbs the calculation block's result into the bound record.
pub fn absorb<P: AbsorbPorts>(absorb: &Absorb, shot: &mut impl AbsorbShot<P>, ports: &mut P) {
    let plan = Plan::of(absorb, ports);
    let mode = ExposureMode(absorb.selection.0);
    let half_press = matches!(absorb.origin, Origin::HalfPress(_));
    let retained_export = ports.adjustment(AbsorbAdjustment::RetainedExport) & 4 != 0;
    if half_press {
        if !absorb.repeat {
            if mode.is_primary() { ports.clear_channel_diagnostics(plan.frame, true); }
        } else {
            if !mode.uses_adjustment() && retained_export { export_retained(absorb, &plan, shot, ports); }
            if mode.is_primary() && !retained_export { ports.clear_channel_diagnostics(plan.frame, false); }
        }
    }
    update_grid(absorb, &plan, shot, ports);
    let subjects = ports.adjustment(AbsorbAdjustment::Subjects) & 3 != 0;
    if shot.tracking() && subjects {
        let reference = ports.area_references(absorb.record);
        let source = ports.subject_source(absorb.record, absorb.refinement);
        let kind = ports.value(absorb.record, RecordField::SubjectKind);
        let area = ports.value(absorb.record, RecordField::SubjectArea);
        shot.classify_subjects(ClassifyStage::Absorb, &reference, kind, source, area, ports);
    }
    if subjects && ports.adjustment(AbsorbAdjustment::RateStatus) & 1 != 0 {
        if ports.value(absorb.exposure_record, RecordField::MapPresent) == 0 {
            ports.release_rate_status();
        } else {
            let reference = ports.area_references(absorb.record);
            ports.draw_rate_status(&reference);
        }
    }

    let (stored, positioned) = if plan.whole {
        let target = plan.target(absorb.record, shot.requires_eligibility());
        store_areas(Form::Full, ResultPass::Plain, target, shot, ports);
        let half = shot.retained_half();
        shot.retain_target_half(half);
        (true, true)
    } else if matches!(plan.mode, 0 | 1) {
        let target = plan.target(absorb.record, shot.requires_eligibility());
        store_areas(Form::Full, ResultPass::Plain, target, shot, ports);
        (true, true)
    } else if shot.tracking() {
        let target = plan.target(absorb.record, shot.requires_eligibility());
        store_areas(Form::Short, ResultPass::Plain, target, shot, ports);
        (true, false)
    } else {
        (false, false)
    };
    if positioned {
        let cell = shot.cell(ResultPass::Plain, FIRST_AREA);
        store_cell(absorb.record, cell, shot, ports);
    }
    let summarized = matches!(absorb.selection.0, 0 | 1 | 5);
    if summarized {
        let summary = shot.focus_summary();
        let level = ports.classify_level(summary.classified());
        ports.store_focus_summary(absorb.record, &summary, level);
    }
    store_metering(absorb, &plan, shot, ports);

    if half_press {
        let enabled = absorb.repeat && !mode.uses_adjustment() && !retained_export;
        if stored {
            ports.export_result(plan.frame, ResultExport::Policy, enabled);
            if enabled { ports.export_result(plan.frame, ResultExport::Compact, true); }
        }
        if positioned { ports.export_result(plan.frame, ResultExport::SelectedChannel, enabled); }
        if summarized { ports.export_result(plan.frame, ResultExport::SourceLimits, enabled); }
    }
}

/// With the retained result in the block, the grid is updated and the retained result exported.
fn export_retained<P: AbsorbPorts>(absorb: &Absorb, plan: &Plan, shot: &mut impl AbsorbShot<P>, ports: &mut P) {
    shot.set_working_aside();
    shot.restore_result();
    update_grid(absorb, plan, shot, ports);
    ports.export_result(plan.frame, ResultExport::Compact, true);
    ports.export_result(plan.frame, ResultExport::SelectedChannel, true);
    ports.export_result(plan.frame, ResultExport::SourceLimits, true);
    shot.restore_working();
}

fn update_grid<P: AbsorbPorts>(absorb: &Absorb, plan: &Plan, shot: &mut impl AbsorbShot<P>, ports: &mut P) {
    let (reset, keep_gates) = plan.grid_update();
    shot.update_grid(absorb.exposure_record, reset, keep_gates, absorb.lens_reference, ports);
}

fn area_indices(start: u8, count: u8) -> impl Iterator<Item = AreaIndex> { (start..count).map_while(AreaIndex::new) }

/// Stores each area of `pass` into the record, the half press's first area taking the auxiliary
/// reference in mode 7.
pub(crate) fn store_areas<P: AbsorbPorts>(form: Form, pass: ResultPass, target: AreaTarget, shot: &mut impl AbsorbShot<P>, ports: &mut P) {
    let record = target.record;
    let start = u8::from(target.skip_first);
    let tracking = shot.tracking();
    let count = shot.result_areas();
    for index in area_indices(start, count) {
        let cell = shot.cell(pass, index);
        let kind_word = if tracking { shot.area_kind(target.bank, index) } else { UNTRACKED };
        let kind = TrackedKind::new(kind_word);
        if let ResultPass::Grid(grid) = pass {
            shot.adopt_grid_area(grid, cell, index, kind);
            if form == Form::Full { shot.adopt_grid_tables(grid, cell, index); }
        }
        let at = match kind {
            None => AreaAt::Cell { pass: ResultPass::Plain, cell, index },
            Some(kind) => AreaAt::Tracked { pass: ResultPass::Plain, kind, index },
        };
        let area = shot.result_area(at);
        let auxiliary = auxiliary_reference(record, target.mode, target.eligibility, area.reference(), index, ports);
        if index.get() == 0 { shot.set_auxiliary(auxiliary.outcome); }
        if auxiliary.outcome == AuxiliaryOutcome::Replaced {
            ports.store_auxiliary(record, index, &auxiliary);
            let untracked = (kind_word == UNTRACKED).then_some(AreaAt::Cell { pass, cell, index });
            shot.retain_auxiliary(&auxiliary, untracked);
            continue;
        }
        let store = AreaStore { area, weight: shot.area_weight(index), marker: shot.marker(pass, kind, index) };
        ports.store_area(record, index, &store);
        if form == Form::Full {
            let detail = area_detail(pass, cell, index, record, shot, ports);
            ports.store_area_detail(record, index, &detail);
        }
    }
    if form == Form::Full && pass == ResultPass::Plain && shot.grid_selected() {
        for index in area_indices(0, count) {
            for grid in GridPass::ALL {
                let cell = shot.cell(ResultPass::Grid(grid), index);
                ports.evaluate_distances(ResultPass::Grid(grid), cell, index);
            }
        }
    }
    if target.mode == AUXILIARY_MODE && shot.auxiliary() != AuxiliaryOutcome::Replaced {
        shot.retain_result();
        shot.mark_auxiliary_retained();
    }
}

/// The plain pass evaluates the area's distances here; a grid pass carries its own rows.
fn area_detail(pass: ResultPass, cell: GridCell, index: AreaIndex, record: RecordHandle, shot: &mut impl CalculationResults, ports: &mut impl AbsorbPorts) -> AreaDetail {
    let plan_value = shot.plan_value(ResultPass::Plain, cell, index);
    let plan_half = shot.plan_half(ResultPass::Plain, cell, index);
    let plain = pass == ResultPass::Plain;
    if plain { ports.evaluate_distances(ResultPass::Plain, cell, index); }
    let row = shot.row(ResultPass::Plain, cell, index);
    AreaDetail {
        selection_class: SelectionFlags(plan_value as u8).category(),
        plan_half,
        plan_bit_two: RowFlags(plan_value).bit_two(),
        plan_bit_five: RowFlags(plan_value).bit_five(),
        distance: row.distance(),
        row_class: RowFlags(u32::from(row.flags())).class(),
        extended: plain.then(|| ports.extended_areas(record)),
        row_word: shot.row_word(pass, cell),
    }
}

/// Mode 7 lets the first area take an auxiliary reference when the selection has not changed
/// and no other reference is available for it.
fn auxiliary_reference(record: RecordHandle, mode: u32, require: bool, reference: u32, index: AreaIndex, ports: &mut impl AbsorbPorts) -> AuxiliaryReference {
    if mode != AUXILIARY_MODE || index.get() != 0 { return AuxiliaryReference::default(); }
    let source = ports.selection_source(record);
    let changed = require && ports.selection_changed(record, source, reference);
    let (available, target) = ports.compare_reference(record, source);
    let outcome = if changed {
        AuxiliaryOutcome::Unchecked
    } else if available || require {
        AuxiliaryOutcome::Kept
    } else {
        AuxiliaryOutcome::Replaced
    };
    AuxiliaryReference { outcome, source, target, normalized: ports.compensated(target) }
}

/// Stores the plain summary of `cell` with the first area's weight.
pub(crate) fn store_cell<P: AbsorbPorts>(record: RecordHandle, cell: GridCell, shot: &mut impl AbsorbShot<P>, ports: &mut P) {
    let summary = shot.cell_summary(ResultPass::Plain, cell);
    let weight = shot.area_weight(FIRST_AREA);
    ports.store_cell(record, &summary, weight);
}

fn store_metering<P: AbsorbPorts>(absorb: &Absorb, plan: &Plan, shot: &mut impl AbsorbShot<P>, ports: &mut P) {
    let mut grid_mode = ports.grid_mode(absorb.record, plan.mode);
    if plan.mode == AUXILIARY_MODE { grid_mode = shot.selection_mode(); }
    let mut block = ports.metering(absorb.record);
    let minimum = if !plan.whole || !shot.grid_selected() {
        MeteringMinimum { single_pass: true, minimum: record_minimum(absorb, plan.count, ports) }
    } else {
        MeteringMinimum { single_pass: grid_mode != ALL_PASSES, minimum: grid_minimum(absorb, grid_mode, shot, ports) }
    };
    crate::stock::absorb::set_metering_minimum(&mut block, minimum);
    ports.store_metering(absorb.record, &block);
}

/// The smallest lens reference of the record's first `count` areas; the configured split
/// reference, when nothing is below it, and the live sequence take the lower reference instead.
fn record_minimum(absorb: &Absorb, count: u8, ports: &mut impl AbsorbPorts) -> u32 {
    if absorb.live { return ports.lower_reference(absorb.lens_reference); }
    let split = ports.split_reference();
    let minimum = (0..count).map(|index| ports.area_lens_reference(absorb.record, index)).fold(split, u32::min);
    if minimum == ports.split_reference() { ports.lower_reference(absorb.lens_reference) } else { minimum }
}

/// As `record_minimum`, over the grid areas of one pass or, for `ALL_PASSES`, of all three.
fn grid_minimum<P: AbsorbPorts>(absorb: &Absorb, grid_mode: u32, shot: &mut impl AbsorbShot<P>, ports: &mut P) -> u32 {
    if absorb.live { return ports.lower_reference(absorb.lens_reference); }
    let passes: &[GridPass] = if grid_mode == ALL_PASSES { &GridPass::ALL } else { &[] };
    let single = GridPass::new(grid_mode);
    let split = ports.split_reference();
    let count = shot.result_areas();
    let mut minimum = split;
    for &pass in passes.iter().chain(single.iter()) {
        for index in area_indices(0, count) {
            let cell = shot.cell(ResultPass::Grid(pass), index);
            let reference = shot.result_area(AreaAt::Cell { pass: ResultPass::Grid(pass), cell, index }).reference();
            minimum = minimum.min(reference);
        }
    }
    if minimum == ports.split_reference() { ports.lower_reference(absorb.lens_reference) } else { minimum }
}
