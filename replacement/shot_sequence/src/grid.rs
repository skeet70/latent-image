#![forbid(unsafe_code)]

//! The selection grid update (X-M1 1.50 id5 0xf1cf68): the tracked kinds' level targets, the
//! list's classes, the tracked kinds' adjusted areas, the area refreshes of `grid_refresh` and the
//! area markers.

use crate::calculation_work::CalculationProfile;
use crate::grid_boundary::{GridLayers, GridSetting, LevelQuery, ReferenceQuery};
use crate::grid_refresh::{reclassify_areas, refresh_focus, settle_areas};
use crate::selection::{areas, choose_classes, fix_classes, kinds, passes};
use crate::state::{ShotState, StateMemory};
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, AreaIndex, AreaResult, AreaTarget, AreaWord, GridCell, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{CellCode, SelectionSlot};
use xm1_saclac_interface::RecordHandle;

/// The grid has three columns and three rows.
const GRID_SIDE: u8 = 3;
/// Selection modes whose targets move without the area keeping the grid's reference.
const FREE_MODES: [u32; 2] = [46, 54];
/// The layout whose first area takes its targets from the plain pass.
const PLAIN_TARGET_LAYOUT: u32 = 5;

/// A reset takes the saved list back instead of choosing classes; `keep_gates` leaves the target
/// gates unwritten and a closed gate closed.
pub fn update_grid<M: StateMemory>(record: RecordHandle, reset: bool, keep_gates: bool, lens_reference: u32, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    update_targets(record, keep_gates, lens_reference, state, layers);
    let fixed = layers.setting(GridSetting::Control) & 1 != 0;
    if reset {
        state.load_saved_selection();
    } else {
        if fixed { fix_classes(state, layers) } else { choose_classes(record, state, layers) }
        state.save_selection();
    }
    adjust_tracked_areas(state, layers);
    if !fixed && layers.profile(record, CalculationProfile::SelectionKind) == 0 {
        reclassify_areas(record, state, layers);
        settle_areas(state, layers);
    }
    refresh_focus(record, state, layers);
    store_markers(record, state, layers);
}

/// How far the grid's walks reach: the exposure's columns and rows, the block's areas, the plan's
/// tracked kinds and whether the grid passes are selected.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Extent {
    columns: u8,
    rows: u8,
    pub(crate) areas: u8,
    pub(crate) kinds: u8,
    grid: bool,
}

impl Extent {
    pub(crate) fn of<M: StateMemory>(state: &mut ShotState<M>) -> Self {
        Self {
            columns: state.exposure_columns(),
            rows: state.exposure_rows(),
            areas: state.area_count(),
            kinds: state.tracked_kinds(),
            grid: state.grid_selected(),
        }
    }

    /// The plain pass, then the grid passes when the grid is selected.
    pub(crate) fn passes(self) -> impl Iterator<Item = ResultPass> {
        core::iter::once(ResultPass::Plain).chain(passes().filter(move |_| self.grid))
    }

    /// Every area of every cell the exposure covers, by column, row, area and pass.
    pub(crate) fn cell_areas(self) -> impl Iterator<Item = (GridCell, AreaIndex, ResultPass)> {
        let rows = self.rows.min(GRID_SIDE);
        (0..self.columns.min(GRID_SIDE))
            .flat_map(move |column| (0..rows).map(move |row| GridCell { column, row }))
            .flat_map(move |cell| areas(self.areas).flat_map(move |index| self.passes().map(move |pass| (cell, index, pass))))
    }

    /// Every area of every tracked kind, by kind, area and pass.
    pub(crate) fn kind_areas(self) -> impl Iterator<Item = (TrackedKind, AreaIndex, ResultPass)> {
        kinds(self.kinds).flat_map(move |kind| areas(self.areas).flat_map(move |index| self.passes().map(move |pass| (kind, index, pass))))
    }

    /// Every area the selection keeps without a tracked kind, by area and pass.
    fn plain_areas(self) -> impl Iterator<Item = (AreaIndex, ResultPass)> {
        areas(self.areas).flat_map(move |index| self.passes().map(move |pass| (index, pass)))
    }
}

pub(crate) fn area_at(kind: Option<TrackedKind>, pass: ResultPass, cell: GridCell, index: AreaIndex) -> AreaAt {
    match kind {
        None => AreaAt::Cell { pass, cell, index },
        Some(kind) => AreaAt::Tracked { pass, kind, index },
    }
}

/// The cell the selection holds for an area of a pass without a tracked kind.
pub(crate) fn selected_cell<M: StateMemory>(pass: ResultPass, index: AreaIndex, state: &mut ShotState<M>) -> CellCode {
    state.selection_cell(SelectionSlot { kind: None, pass, index })
}

/// Each tracked kind's level target in every cell area (0xf1b7c8).
fn update_targets<M: StateMemory>(record: RecordHandle, keep_gates: bool, lens_reference: u32, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    let extent = Extent::of(state);
    if extent.kinds == 0 { return; }
    let query = ReferenceQuery {
        record,
        target_word: state.exposure_target_word(),
        focus_mode: state.exposure_focus_mode(),
        cell: selected_cell(ResultPass::Plain, AreaIndex::FIRST, state),
        lens_reference,
        refinement: state.refinement(),
    };
    let reference = layers.target_reference(&query);
    let targets = TargetInputs::of(record, state, layers);
    for (cell, index, pass) in extent.cell_areas() {
        let source = state.area(AreaAt::Cell { pass, cell, index });
        for kind in kinds(extent.kinds) {
            let gate = state.target_gate(kind, pass, cell, index);
            let open = targets.gate(&source, reference, keep_gates, gate, layers);
            if !keep_gates { state.set_target_gate(kind, pass, cell, index, open); }
            let target = if open { targets.target(&source, kind, pass, index, layers) } else { AreaTarget { level: source.half(AreaHalf::Level), raise: 0, bound: source.half(AreaHalf::At14) } };
            state.set_target(kind, pass, cell, index, target);
        }
    }
}

/// The record values every target of one update is computed from.
struct TargetInputs {
    mode: u32,
    flags: u16,
    at004b: u32,
    at0066: u32,
    at0045: u32,
    adjustment_option: u32,
    at003e: u32,
    at0024: i16,
    plain_first_area: bool,
}

impl TargetInputs {
    fn of<M: StateMemory>(record: RecordHandle, state: &mut ShotState<M>, layers: &mut impl GridLayers) -> Self {
        let refinement = state.refinement();
        Self {
            mode: layers.selection_mode(record, refinement),
            flags: layers.selection_flags(record, refinement),
            at004b: layers.profile(record, CalculationProfile::At004b),
            at0066: layers.profile(record, CalculationProfile::At0066),
            at0045: layers.profile(record, CalculationProfile::At0045),
            adjustment_option: layers.profile(record, CalculationProfile::AdjustmentOption),
            at003e: layers.profile(record, CalculationProfile::At003e),
            at0024: layers.profile(record, CalculationProfile::At0024) as i16,
            plain_first_area: layers.profile(record, CalculationProfile::At0032) == PLAIN_TARGET_LAYOUT,
        }
    }

    fn query(&self, pass: ResultPass, kind: TrackedKind) -> LevelQuery {
        LevelQuery {
            mode: self.mode,
            flags: self.flags,
            at004b: self.at004b,
            at0066: self.at0066,
            pass,
            at0045: self.at0045,
            adjustment_option: self.adjustment_option,
            kind,
            at003e: self.at003e,
            at0024: self.at0024,
        }
    }

    /// Whether a target may move (0xf1a674): the area's level is within the configured tenths
    /// and, outside the free selection modes, the area follows: it keeps the grid's reference, or
    /// with `keep_gates` its gate is open.
    fn gate(&self, source: &AreaResult, reference: u32, keep_gates: bool, gate: bool, layers: &mut impl GridLayers) -> bool {
        let free = FREE_MODES.contains(&self.mode);
        let follows = if keep_gates { gate } else { source.reference() == reference };
        if !free && !follows { return false; }
        i32::from(source.half(AreaHalf::Level)) <= 10 * layers.setting(GridSetting::TargetGate)
    }

    /// The area's level raised toward the kind's target level, by at most the kind's share and
    /// the area's headroom (0xf1a75c).
    fn target(&self, source: &AreaResult, kind: TrackedKind, pass: ResultPass, index: AreaIndex, layers: &mut impl GridLayers) -> AreaTarget {
        let pass = if self.plain_first_area && index == AreaIndex::FIRST { ResultPass::Plain } else { pass };
        let level = layers.target_level(&self.query(pass, kind));
        let bound = layers.target_level(&self.query(ResultPass::Plain, kind));
        let base = source.half(AreaHalf::Level);
        let room = source.half(AreaHalf::At4a).wrapping_sub(base).min(kind_share(kind));
        let raise = (i32::from(level) - i32::from(source.half(AreaHalf::At12))).max(0).min(i32::from(room)) as i16;
        AreaTarget { level: base.wrapping_add(raise), raise, bound }
    }
}

/// The most a kind's target raises an area: 100, 200 or 300 by the kind's place in its triple
/// (0xf1a718).
fn kind_share(kind: TrackedKind) -> i16 { 100 * (i16::from(kind.get() % 3) + 1) }

/// Each tracked kind's area of each pass becomes the area at the cell the selection holds,
/// raised by the kind's target (0xf1b6e4, 0xf1b4ec).
fn adjust_tracked_areas<M: StateMemory>(state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    for (kind, index, pass) in Extent::of(state).kind_areas() {
        let shared = selected_cell(pass, index, state);
        let own = state.selection_cell(SelectionSlot { kind: Some(kind), pass, index });
        let source = state.area(AreaAt::Cell { pass, cell: shared.cell(), index });
        let target = state.target(kind, pass, shared.cell(), index);
        let mut area = source;
        if target.raise == 0 {
            area.set_half(AreaHalf::At46, 0);
        } else {
            area.set_half(AreaHalf::Level, target.level);
            raise(&mut area, AreaHalf::At0c, target.raise);
            raise(&mut area, AreaHalf::At48, target.raise);
            if layers.cell_kind(shared) != layers.cell_kind(own) { settle_at_cell(&mut area, own, layers); }
            let reach = area.half(AreaHalf::At10).wrapping_add(area.half(AreaHalf::At48)).wrapping_add(area.half(AreaHalf::At1a));
            area.set_half(AreaHalf::At12, reach);
            area.set_half(AreaHalf::At46, target.raise);
            let lead = source.half(AreaHalf::At4e).wrapping_sub(source.half(AreaHalf::Level)).wrapping_add(target.level);
            area.set_half(AreaHalf::At4e, lead);
        }
        area.set_half(AreaHalf::At14, target.bound);
        state.set_area(AreaAt::Tracked { pass, kind, index }, &area);
    }
}

fn raise(area: &mut AreaResult, half: AreaHalf, by: i16) { area.set_half(half, area.half(half).wrapping_add(by)); }

/// Once the raise reaches the level of the kind's own cell, the area takes that level and the
/// cell's class, and the raise leaves +0xc and +0x48 (0xf1b5c8..0xf1b680).
fn settle_at_cell(area: &mut AreaResult, own: CellCode, layers: &mut impl GridLayers) {
    let level = layers.cell_level(own);
    let raised = i32::from(area.half(AreaHalf::At48)) + i32::from(layers.lens_threshold(3)) % 100;
    if raised < i32::from(level) { return; }
    area.set_half(AreaHalf::At18, 0);
    area.set_half(AreaHalf::At1a, level);
    area.set_word(AreaWord::At1c, layers.cell_class(own));
    area.set_word(AreaWord::At20, 0);
    area.set_word(AreaWord::At24, 0);
    let moved = area.half(AreaHalf::At48).min(level);
    raise(area, AreaHalf::At0c, moved.wrapping_neg());
    raise(area, AreaHalf::At48, moved.wrapping_neg());
}

/// Each area's marker: the record's status when it has one, 2 for an area whose cell is set,
/// otherwise 1, 2 or 3 by the status of the area's class (0xf1b164, 0xf1b0dc, 0xf1ade0).
fn store_markers<M: StateMemory>(record: RecordHandle, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    let status = layers.profile(record, CalculationProfile::At0034) as u8;
    let extent = Extent::of(state);
    let untracked = extent.plain_areas().map(|(index, pass)| (None, index, pass));
    let tracked = extent.kind_areas().map(|(kind, index, pass)| (Some(kind), index, pass));
    for (kind, index, pass) in untracked.chain(tracked) {
        let cell = state.selection_cell(SelectionSlot { kind, pass, index });
        let area = state.area(area_at(kind, pass, cell.cell(), index));
        let marker = if status != 0 {
            u32::from(status)
        } else if !layers.cell_unset(cell) {
            2
        } else {
            match layers.class_status(area.word(AreaWord::ClassIndex)) {
                0 => 1,
                1 => 2,
                _ => 3,
            }
        };
        state.set_marker(pass, kind, index, marker);
    }
}
