#![forbid(unsafe_code)]

//! The shot's area selection: the list a record gives (0xf162e4) and the class and cell the grid
//! update chooses for each slot (0xf1c4ac), or the one fixed class the configuration can ask for
//! instead (0xf1a92c).

use crate::calculation_work::CalculationProfile;
use crate::grid_boundary::{ClassLimits, DefaultQuery, GridLayers, LimitInputs};
use crate::state::{ShotState, StateMemory};
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, AreaIndex, AreaWord, GridPass, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{CellCode, SelectionClass, SelectionList, SelectionSlot};
use xm1_saclac_interface::RecordHandle;

/// Where a list's classes come from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListSource {
    /// The live sequence: one area of the record's row limit, or class 1.
    Live,
    /// The record's own defaults.
    Record,
    /// The half-press client's area count and the classes it names for the first areas.
    Client { count: u8, classes: [u32; AreaIndex::COUNT as usize] },
}

/// A layout that numbers its areas' classes instead of taking a default class.
const NUMBERED_LAYOUT: u32 = 13;
/// A layout whose areas carry no weight.
const UNWEIGHTED_LAYOUT: u32 = 12;
/// Layouts that give every area the first area's class.
const SHARED_LAYOUTS: [u32; 2] = [2, 12];

pub(crate) fn areas(count: u8) -> impl Iterator<Item = AreaIndex> { (0..count).map_while(AreaIndex::new) }

pub(crate) fn kinds(count: u8) -> impl Iterator<Item = TrackedKind> { (0..count).map_while(TrackedKind::new) }

pub(crate) fn passes() -> impl Iterator<Item = ResultPass> { GridPass::ALL.into_iter().map(ResultPass::Grid) }

/// The list a record gives.
pub fn record_list<M: StateMemory>(record: RecordHandle, source: ListSource, state: &mut ShotState<M>, layers: &mut impl GridLayers) -> SelectionList {
    let mut list = SelectionList::default();
    if source == ListSource::Live {
        let class = if layers.profile(record, CalculationProfile::SelectionKind) == 1 { layers.profile(record, CalculationProfile::RowLimit) } else { 1 };
        list.set_fixed(true);
        list.set_area_count(1);
        if let Some(first) = AreaIndex::new(0).map(SelectionSlot::plain) {
            list.set_class(first, SelectionClass(class));
            for pass in passes() { list.set_class(SelectionSlot { pass, ..first }, SelectionClass(class)); }
        }
        return list;
    }
    let defaults = source == ListSource::Record;
    let code = layers.selection_code(record);
    let layout = layers.profile(record, CalculationProfile::At0032);
    let query = DefaultQuery {
        code,
        selection_kind: layers.profile(record, CalculationProfile::SelectionKind),
        mode: if defaults { layers.record_mode(record) } else { layers.profile(record, CalculationProfile::HeaderAt31) },
        at0032: layout,
        at0033: layers.profile(record, CalculationProfile::At0033) as u8,
        at0026: layers.profile(record, CalculationProfile::At0026) as u16,
        adjustment_kind: layers.profile(record, CalculationProfile::AdjustmentKind),
        row_limit: layers.profile(record, CalculationProfile::RowLimit),
    };
    let weight = layers.profile(record, CalculationProfile::At003c);
    let (count, classes) = match source {
        ListSource::Client { count, classes } => (count, classes),
        _ => (layers.layout_areas(layout), [0; AreaIndex::COUNT as usize]),
    };
    list.set_area_count(count);
    let mut limits = record_limits(record, defaults, state, layers);
    if code != 0 { limits = limits.within(layers.total_class_limits(code)); }
    list.set_upper_code(limits.upper);
    if layout == NUMBERED_LAYOUT {
        list.set_fixed(true);
        for index in areas(count) {
            let class = if defaults { u32::from(index.get()) + 1 } else { classes[usize::from(index.get())] };
            list.set_class(SelectionSlot::plain(index), SelectionClass(class));
            list.set_weight(index, weight);
        }
        if code == 0 && layers.profile(record, CalculationProfile::At0045) == 4 {
            if let Some(third) = AreaIndex::new(2) { list.set_class(SelectionSlot::plain(third), SelectionClass(2)); }
        }
    } else {
        let class = layers.default_class(&query);
        list.set_fixed(class.0 != 0);
        let weight = if layout == UNWEIGHTED_LAYOUT { 0 } else { weight };
        for index in areas(count) {
            list.set_class(SelectionSlot::plain(index), class);
            list.set_weight(index, weight);
        }
    }
    for index in areas(count) {
        let class = list.class(SelectionSlot::plain(index));
        for pass in passes() { list.set_class(SelectionSlot { kind: None, pass, index }, class); }
    }
    list
}

/// The class limits a record allows (0xf1aac4), from its defaults or from the exposure's
/// refinement.
fn record_limits<M: StateMemory>(record: RecordHandle, defaults: bool, state: &mut ShotState<M>, layers: &mut impl GridLayers) -> ClassLimits {
    let (mode, flags) = if defaults {
        (layers.record_mode(record), layers.record_flags(record))
    } else {
        let refinement = state.refinement();
        (layers.selection_mode(record, refinement), layers.selection_flags(record, refinement))
    };
    let inputs = LimitInputs {
        mode,
        flags,
        code: layers.selection_code(record),
        at0045: layers.profile(record, CalculationProfile::At0045),
        adjustment_option: layers.profile(record, CalculationProfile::AdjustmentOption),
        adjustment_kind: layers.profile(record, CalculationProfile::AdjustmentKind),
        at003e: layers.profile(record, CalculationProfile::At003e),
    };
    let limits = layers.class_limits(&inputs);
    if layers.profile(record, CalculationProfile::At005d) == 0 { limits } else { limits.within(layers.fixed_class_limits()) }
}

/// Chooses the class and cell of every slot from the list's plain and pass classes.
pub fn choose_classes<M: StateMemory>(record: RecordHandle, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    let shared = SHARED_LAYOUTS.contains(&layers.profile(record, CalculationProfile::At0032));
    for index in areas(state.area_count()) {
        let mut fixed = state.selection_fixed();
        let source = match AreaIndex::new(0) {
            Some(first) if shared => {
                fixed |= index != first;
                first
            }
            _ => index,
        };
        let plain = SelectionSlot::plain(index);
        let from = SelectionSlot::plain(source);
        let value = state.selection_class(from);
        store_class(record, fixed, value, plain, state, layers);
        for kind in kinds(state.tracked_kinds()) {
            let value = state.selection_class(from);
            store_class(record, true, value, SelectionSlot { kind: Some(kind), ..plain }, state, layers);
        }
        if !state.grid_selected() { continue; }
        for pass in passes() {
            let slot = SelectionSlot { kind: None, pass, index };
            let from = SelectionSlot { kind: None, pass, index: source };
            let value = state.selection_class(from);
            store_class(record, fixed, value, slot, state, layers);
            for kind in kinds(state.tracked_kinds()) {
                let value = state.selection_class(from);
                store_class(record, true, value, SelectionSlot { kind: Some(kind), ..slot }, state, layers);
            }
        }
    }
}

/// Stores a slot's class: the given one when the classes are fixed, otherwise one the record's
/// limits bound or a search settles on (0xf1c324).
fn store_class<M: StateMemory>(record: RecordHandle, fixed: bool, value: SelectionClass, slot: SelectionSlot, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    let (class, cell) = if fixed {
        (value, layers.class_cell(value))
    } else if layers.selection_code(record) != 0 {
        bounded_class(record, value, state, layers)
    } else {
        searched_class(record, value, slot, state, layers)
    };
    state.set_selection_entry(slot, class, cell);
}

/// The class within the record's limits nearest to `value` (0xf1ac50).
fn bounded_class<M: StateMemory>(record: RecordHandle, value: SelectionClass, state: &mut ShotState<M>, layers: &mut impl GridLayers) -> (SelectionClass, CellCode) {
    let code = layers.selection_code(record);
    let limits = record_limits(record, false, state, layers).within(layers.total_class_limits(code));
    let code = layers.class_code(value).min(limits.upper).max(limits.lower);
    let class = layers.code_class(code);
    (class, layers.class_cell(class))
}

/// The highest class from `requested` down to the record's lower limit whose area's own limits
/// reach it and whose area is free, or the lowest class tried (0xf1c17c).
fn searched_class<M: StateMemory>(record: RecordHandle, requested: SelectionClass, slot: SelectionSlot, state: &mut ShotState<M>, layers: &mut impl GridLayers) -> (SelectionClass, CellCode) {
    let limits = record_limits(record, false, state, layers);
    let lower = limits.lower as i8;
    let start = (layers.class_code(requested) as i8).min(limits.upper as i8).max(lower);
    let mut chosen = (requested, CellCode::default());
    for code in (lower..=start).rev() {
        let class = layers.code_class(code as u8);
        let cell = layers.class_cell(class);
        chosen = (class, cell);
        let area = state.area(AreaAt::Cell { pass: slot.pass, cell: cell.cell(), index: slot.index });
        let mut weighed = area;
        if let Some(kind) = slot.kind {
            let raise = state.target(kind, slot.pass, cell.cell(), slot.index).raise;
            weighed.set_half(AreaHalf::At0c, area.half(AreaHalf::At0c).wrapping_add(raise));
        }
        let reach = limits.within(layers.total_class_limits(weighed.total()));
        if (reach.upper as i8) < code { continue; }
        if area.word(AreaWord::Occupied) == 0 { break; }
    }
    chosen
}

/// Gives every plain and pass slot the one class the image layer derives for the exposure.
pub fn fix_classes<M: StateMemory>(state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    let value = layers.exposure_value();
    let class = layers.level_class(0, value);
    let cell = CellCode { column: layers.cell_code(class), row: 0 };
    for index in areas(state.area_count()) {
        state.set_selection_entry(SelectionSlot::plain(index), class, cell);
        for pass in passes() { state.set_selection_entry(SelectionSlot { kind: None, pass, index }, class, cell); }
    }
}
