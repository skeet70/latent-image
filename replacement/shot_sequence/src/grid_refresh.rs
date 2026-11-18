#![forbid(unsafe_code)]

//! The grid update's walks over the result's areas: reclassifying each area (0xf1be50), settling
//! its level at the ceiling (0xf1b2cc) and raising its level to the focus margin (0xf1cc5c).

use crate::calculation_work::CalculationProfile;
use crate::grid::{area_at, selected_cell, Extent};
use crate::grid_boundary::{BlendQuery, ClassQuery, GridLayers, GridSetting};
use crate::state::{ShotState, StateMemory};
use xm1_saclac_interface::calculation::{AreaHalf, AreaIndex, AreaResult, AreaWord, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::CellCode;
use xm1_saclac_interface::RecordHandle;

/// Selection modes whose areas the focus refresh raises.
const FOCUS_MODES: [u32; 6] = [12, 43, 44, 46, 54, 57];
/// A row flag that lets a grid pass take the blended focus allowance.
const ROW_BLEND: u8 = 4;
/// The class status the focus margin table keeps its own column for.
const MARGIN_STATUS: u8 = 3;

/// An area a walk visits: a tracked kind's (`kind`) or a cell's own, at `cell`.
#[derive(Clone, Copy, Debug)]
struct Visit {
    kind: Option<TrackedKind>,
    pass: ResultPass,
    cell: CellCode,
    index: AreaIndex,
}

impl Visit {
    fn area<M: StateMemory>(self, state: &mut ShotState<M>) -> AreaResult { state.area(area_at(self.kind, self.pass, self.cell.cell(), self.index)) }

    fn store<M: StateMemory>(self, area: &AreaResult, state: &mut ShotState<M>) { state.set_area(area_at(self.kind, self.pass, self.cell.cell(), self.index), area); }

    fn row_flags<M: StateMemory>(self, state: &mut ShotState<M>) -> u8 { state.row_flags(self.pass, self.cell.cell(), self.index) }
}

/// Every cell area, then every tracked kind's area at the cell the selection holds.
fn visit_all<M: StateMemory>(state: &mut ShotState<M>, mut visit: impl FnMut(Visit, &mut ShotState<M>)) {
    let extent = Extent::of(state);
    for (cell, index, pass) in extent.cell_areas() {
        visit(Visit { kind: None, pass, cell: CellCode { column: cell.column, row: cell.row }, index }, state);
    }
    for (kind, index, pass) in extent.kind_areas() {
        let cell = selected_cell(pass, index, state);
        visit(Visit { kind: Some(kind), pass, cell, index }, state);
    }
}

/// Each area the image layer reports a class change for, and whose +0x18 is clear, takes a new
/// class: the record's class when the area's lens band is 1 or 2, otherwise the first class,
/// whose level leaves +0xc above the floor (0xf1bde8, 0xf1bcdc).
pub(crate) fn reclassify_areas<M: StateMemory>(record: RecordHandle, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    let refinement = state.refinement();
    let base = ClassQuery {
        total: 0,
        mode: layers.selection_mode(record, refinement),
        flags: layers.selection_flags(record, refinement),
        at0032: layers.profile(record, CalculationProfile::At0032),
        at0033: layers.profile(record, CalculationProfile::At0033) as u8,
        at0026: layers.profile(record, CalculationProfile::At0026) as u16,
        adjustment_kind: layers.profile(record, CalculationProfile::AdjustmentKind),
        at0034: layers.profile(record, CalculationProfile::At0034),
        resume_lead: layers.profile(record, CalculationProfile::ResumeLead) as i16,
        cell_class: 0,
        row_flagged: false,
    };
    let first_class = layers.first_class_index();
    let first_level = layers.class_level(first_class) as i16;
    visit_all(state, |visit, state| {
        let mut area = visit.area(state);
        let query = ClassQuery { total: area.total(), cell_class: layers.cell_class(visit.cell), row_flagged: visit.row_flags(state) != 0, ..base };
        if !layers.class_changed(&query) || area.half(AreaHalf::At18) != 0 { return; }
        if matches!(layers.lens_band(area.half(AreaHalf::At0c)), 1 | 2) {
            area.set_word(AreaWord::At20, 1);
            let class = layers.record_class(&query);
            area.set_word(AreaWord::ClassIndex, class);
            area.set_word(AreaWord::At34, class);
        } else {
            let level = layers.floor_level(area.half(AreaHalf::At0c).wrapping_sub(first_level));
            area.set_half(AreaHalf::At0c, level);
            area.set_word(AreaWord::ClassIndex, first_class);
            area.set_half(AreaHalf::At10, first_level);
            area.set_word(AreaWord::At34, first_class);
        }
        visit.store(&area, state);
    });
}

/// Each area's +0xe joins its level at +0xc, which keeps what fits under the ceiling; +0xe keeps
/// the excess (0xf1aefc).
pub(crate) fn settle_areas<M: StateMemory>(state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    visit_all(state, |visit, state| {
        let mut area = visit.area(state);
        let (kept, excess) = layers.settle_level(area.half(AreaHalf::At0c).wrapping_add(area.half(AreaHalf::At0e)));
        area.set_half(AreaHalf::At0c, kept);
        area.set_half(AreaHalf::At0e, excess);
        visit.store(&area, state);
    });
}

/// In the focus selection modes, each area's level rises to its focus margin, and +0x4e rises by
/// what the margin passes the area's reach; an area with no row flags marks its plan word
/// (0xf1ca60).
pub(crate) fn refresh_focus<M: StateMemory>(record: RecordHandle, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    let refinement = state.refinement();
    let mode = layers.selection_mode(record, refinement);
    if !FOCUS_MODES.contains(&mode) { return; }
    let at0045 = layers.profile(record, CalculationProfile::At0045);
    let adjustment_option = layers.profile(record, CalculationProfile::AdjustmentOption);
    let at003e = layers.profile(record, CalculationProfile::At003e);
    let blend = |pass| BlendQuery { mode, pass, at0045, adjustment_option, at003e };
    let focused = state.exposure_focus_mode() == 1;
    let lens_reference = state.exposure_lens_reference();
    visit_all(state, |visit, state| {
        let mut area = visit.area(state);
        let flags = visit.row_flags(state);
        let mut level = area.total();
        if focused {
            if flags & ROW_BLEND != 0 && visit.pass != ResultPass::Plain && layers.focus_blend(&blend(visit.pass)) > 10 {
                level = level.wrapping_add(100);
            }
            let cap = i32::from(layers.lens_threshold(3)) + i32::from(area.half(AreaHalf::At14));
            if i32::from(level) > cap { level = cap as i16; }
        }
        let status = layers.class_status(area.word(AreaWord::ClassIndex));
        let margin = focus_margin(mode, status, level, flags, layers);
        if area.half(AreaHalf::Level) >= margin { return; }
        area.set_half(AreaHalf::Level, margin);
        let distance = layers.reference_level(area.reference(), lens_reference);
        let reach = area.half(AreaHalf::At4e).wrapping_add(area.half(AreaHalf::At44)).wrapping_sub(distance);
        if margin > reach { area.set_half(AreaHalf::At4e, area.half(AreaHalf::At4e).wrapping_add(margin.wrapping_sub(reach))); }
        visit.store(&area, state);
        if flags == 0 { state.mark_plan(visit.pass, visit.cell.cell(), visit.index, 1); }
    });
}

/// The configured margin in tenths for a level, by the mode's margin family, the class status and
/// the row's blend flag, at the first lens threshold step the level does not pass (0xf1a2a8).
fn focus_margin(mode: u32, status: u8, level: i16, flags: u8, layers: &mut impl GridLayers) -> i16 {
    let family = match mode {
        46 | 54 => 1,
        12 => 2,
        _ => 0,
    };
    let entry = 4 * family + 2 * u8::from(status == MARGIN_STATUS) + u8::from(flags & ROW_BLEND != 0);
    let step = (2..=9).position(|step| level <= layers.lens_threshold(step)).unwrap_or(8) as u8;
    (10 * layers.setting(GridSetting::FocusMargin { entry, step })) as i16
}
