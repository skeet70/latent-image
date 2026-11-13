use crate::absorb::CalculationResults;
use crate::calculation_work::CalculationProfile;
use crate::grid::update_grid;
use crate::grid_boundary::GridSetting;
use crate::grid_fakes::{area, cell, grid, index, kind, put_area, record, save_list, Layers, Setup};
use crate::state::{ShotState, StateMemory};
use crate::state_tests::Memory;
use crate::stock::state as layout;
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, AreaResult, AreaTarget, AreaWord, ResultPass};
use xm1_saclac_interface::selection::{CellCode, SelectionClass, SelectionList, SelectionSlot};

const REFERENCE: u32 = 0x55;

fn plain(column: u8, row: u8, area: u8) -> AreaAt { AreaAt::Cell { pass: ResultPass::Plain, cell: cell(column, row), index: index(area) } }

/// One area of one cell, the given tracked kinds, targets reaching 250 and a gate at 200.
fn targets_world(kinds: u8) -> (ShotState<Memory>, Layers) {
    let state = Setup { areas: 1, kinds, grid: false, columns: 1, rows: 1 }.state();
    let mut layers = Layers { reference: REFERENCE, plain_level: 250, ..Layers::default() };
    layers.set(GridSetting::TargetGate, 20);
    layers.set_profile(CalculationProfile::SelectionKind, 1);
    (state, layers)
}

fn source(reference: u32, level: i16) -> AreaResult {
    area(reference, &[(AreaHalf::Level, level), (AreaHalf::At4a, 1000), (AreaHalf::At14, 7), (AreaHalf::At0c, 30), (AreaHalf::At10, 10), (AreaHalf::At1a, 5), (AreaHalf::At4e, 400)])
}

fn update(state: &mut ShotState<Memory>, layers: &mut Layers, reset: bool, keep_gates: bool) { update_grid(record(), reset, keep_gates, 0x40, state, layers); }

fn plain_target(state: &mut ShotState<Memory>, tracked: u8) -> (AreaTarget, bool) {
    let at = (kind(tracked), ResultPass::Plain, cell(0, 0), index(0));
    (state.target(at.0, at.1, at.2, at.3), state.target_gate(at.0, at.1, at.2, at.3))
}

#[test]
fn a_target_raises_the_area_level_by_at_most_the_kind_share() {
    let (mut state, mut layers) = targets_world(2);
    put_area(&mut state, plain(0, 0, 0), &source(REFERENCE, 100));
    update(&mut state, &mut layers, true, false);
    assert_eq!(plain_target(&mut state, 0), (AreaTarget { level: 200, raise: 100, bound: 250 }, true));
    assert_eq!(plain_target(&mut state, 1), (AreaTarget { level: 300, raise: 200, bound: 250 }, true));
}

#[test]
fn an_area_that_left_the_grid_reference_closes_its_gates() {
    let (mut state, mut layers) = targets_world(1);
    put_area(&mut state, plain(0, 0, 0), &source(0x66, 100));
    state.set_target_gate(kind(0), ResultPass::Plain, cell(0, 0), index(0), true);
    update(&mut state, &mut layers, true, false);
    assert_eq!(plain_target(&mut state, 0), (AreaTarget { level: 100, raise: 0, bound: 7 }, false));
}

#[test]
fn an_area_above_the_configured_level_closes_its_gates() {
    let (mut state, mut layers) = targets_world(1);
    put_area(&mut state, plain(0, 0, 0), &source(REFERENCE, 201));
    update(&mut state, &mut layers, true, false);
    assert_eq!(plain_target(&mut state, 0), (AreaTarget { level: 201, raise: 0, bound: 7 }, false));
}

#[test]
fn kept_gates_decide_instead_of_the_reference_and_stay_as_they_were() {
    let (mut state, mut layers) = targets_world(2);
    put_area(&mut state, plain(0, 0, 0), &source(0x66, 100));
    state.set_target_gate(kind(0), ResultPass::Plain, cell(0, 0), index(0), true);
    update(&mut state, &mut layers, true, true);
    assert_eq!(plain_target(&mut state, 0), (AreaTarget { level: 200, raise: 100, bound: 250 }, true));
    assert_eq!(plain_target(&mut state, 1), (AreaTarget { level: 100, raise: 0, bound: 7 }, false));
}

#[test]
fn the_free_selection_modes_follow_an_area_off_the_grid_reference() {
    let (mut state, mut layers) = targets_world(1);
    layers.mode = 46;
    put_area(&mut state, plain(0, 0, 0), &source(0x66, 100));
    update(&mut state, &mut layers, true, false);
    assert_eq!(plain_target(&mut state, 0), (AreaTarget { level: 200, raise: 100, bound: 250 }, true));
}

#[test]
fn layout_5_takes_the_first_area_target_from_the_plain_pass() {
    let raises = |layout: u32| {
        let mut state = Setup { areas: 2, kinds: 1, grid: true, columns: 1, rows: 1 }.state();
        let mut layers = Layers { reference: REFERENCE, plain_level: 250, grid_level: 500, ..Layers::default() };
        layers.set(GridSetting::TargetGate, 20);
        layers.set_profile(CalculationProfile::SelectionKind, 1);
        layers.set_profile(CalculationProfile::At0032, layout);
        let mut raised = source(REFERENCE, 100);
        raised.set_half(AreaHalf::At12, 200);
        for area in 0..2 { put_area(&mut state, AreaAt::Cell { pass: grid(0), cell: cell(0, 0), index: index(area) }, &raised); }
        update(&mut state, &mut layers, true, false);
        [0, 1].map(|area| state.target(kind(0), grid(0), cell(0, 0), index(area)))
    };
    let bound = 250;
    assert_eq!(raises(5), [AreaTarget { level: 150, raise: 50, bound }, AreaTarget { level: 200, raise: 100, bound }]);
    assert_eq!(raises(0), [AreaTarget { level: 200, raise: 100, bound }, AreaTarget { level: 200, raise: 100, bound }]);
}

#[test]
fn a_tracked_area_takes_the_selected_cell_area_raised_by_its_target() {
    let (mut state, mut layers) = targets_world(1);
    put_area(&mut state, plain(0, 0, 0), &source(REFERENCE, 100));
    update(&mut state, &mut layers, true, false);
    let tracked = state.area(AreaAt::Tracked { pass: ResultPass::Plain, kind: kind(0), index: index(0) });
    assert_eq!(tracked.reference(), REFERENCE);
    let halves = [AreaHalf::Level, AreaHalf::At0c, AreaHalf::At48, AreaHalf::At12, AreaHalf::At46, AreaHalf::At4e, AreaHalf::At14];
    assert_eq!(halves.map(|half| tracked.half(half)), [200, 130, 100, 115, 100, 500, 250]);
}

#[test]
fn a_tracked_area_without_a_raise_keeps_the_selected_cell_area() {
    let (mut state, mut layers) = targets_world(1);
    let mut closed = source(0x66, 100);
    closed.set_half(AreaHalf::At46, 33);
    put_area(&mut state, plain(0, 0, 0), &closed);
    update(&mut state, &mut layers, true, false);
    let tracked = state.area(AreaAt::Tracked { pass: ResultPass::Plain, kind: kind(0), index: index(0) });
    let halves = [AreaHalf::Level, AreaHalf::At0c, AreaHalf::At46, AreaHalf::At14];
    assert_eq!(halves.map(|half| tracked.half(half)), [100, 30, 0, 7]);
}

#[test]
fn a_kind_whose_own_cell_differs_settles_at_that_cell_level() {
    let (mut state, mut layers) = targets_world(1);
    layers.cell_level = 50;
    let mut list = SelectionList::default();
    list.set_cell(SelectionSlot { kind: Some(kind(0)), ..SelectionSlot::plain(index(0)) }, CellCode { column: 2, row: 0 });
    save_list(&mut state, &list);
    let mut shared = source(REFERENCE, 100);
    shared.set_half(AreaHalf::At18, 9);
    for word in [AreaWord::At1c, AreaWord::At20, AreaWord::At24] { shared.set_word(word, 0xaa); }
    put_area(&mut state, plain(0, 0, 0), &shared);
    update(&mut state, &mut layers, true, false);
    let tracked = state.area(AreaAt::Tracked { pass: ResultPass::Plain, kind: kind(0), index: index(0) });
    let halves = [AreaHalf::Level, AreaHalf::At18, AreaHalf::At1a, AreaHalf::At0c, AreaHalf::At48, AreaHalf::At12];
    assert_eq!(halves.map(|half| tracked.half(half)), [200, 0, 50, 80, 50, 110]);
    assert_eq!([AreaWord::At1c, AreaWord::At20, AreaWord::At24].map(|word| tracked.word(word)), [12, 0, 0]);
}

/// One cell, no tracked kinds, the refreshes enabled and every class changing.
fn refresh_world(areas: u32) -> (ShotState<Memory>, Layers) {
    let state = Setup { areas, kinds: 0, grid: false, columns: 1, rows: 1 }.state();
    let layers = Layers { class_changed: true, ..Layers::default() };
    (state, layers)
}

#[test]
fn a_changed_area_in_lens_bands_1_and_2_takes_the_record_class() {
    let (mut state, mut layers) = refresh_world(1);
    layers.lens_band = 2;
    layers.record_class = 9;
    put_area(&mut state, plain(0, 0, 0), &area(0, &[(AreaHalf::At0c, 300)]));
    update(&mut state, &mut layers, true, false);
    let refreshed = state.area(plain(0, 0, 0));
    assert_eq!([AreaWord::At20, AreaWord::ClassIndex, AreaWord::At34].map(|word| refreshed.word(word)), [1, 9, 9]);
    assert_eq!(refreshed.half(AreaHalf::At0c), 300);
}

#[test]
fn a_changed_area_in_other_bands_takes_the_first_class_and_keeps_its_floor() {
    let (mut state, mut layers) = refresh_world(1);
    layers.first_class = 3;
    layers.first_level = 40;
    layers.floor = 280;
    put_area(&mut state, plain(0, 0, 0), &area(0, &[(AreaHalf::At0c, 300)]));
    update(&mut state, &mut layers, true, false);
    let refreshed = state.area(plain(0, 0, 0));
    assert_eq!([AreaWord::ClassIndex, AreaWord::At34].map(|word| refreshed.word(word)), [3, 3]);
    assert_eq!([AreaHalf::At0c, AreaHalf::At10].map(|half| refreshed.half(half)), [280, 40]);
}

#[test]
fn a_held_or_unchanged_area_keeps_its_class() {
    let (mut state, mut layers) = refresh_world(1);
    layers.lens_band = 1;
    layers.record_class = 9;
    let mut held = area(0, &[(AreaHalf::At18, 1)]);
    held.set_word(AreaWord::ClassIndex, 4);
    put_area(&mut state, plain(0, 0, 0), &held);
    update(&mut state, &mut layers, true, false);
    assert_eq!(state.area(plain(0, 0, 0)).word(AreaWord::ClassIndex), 4);
    layers.class_changed = false;
    put_area(&mut state, plain(0, 0, 0), &area(0, &[]));
    update(&mut state, &mut layers, true, false);
    assert_eq!(state.area(plain(0, 0, 0)).word(AreaWord::ClassIndex), 0);
}

#[test]
fn settling_keeps_a_level_under_the_ceiling_and_the_excess_in_0e() {
    let (mut state, mut layers) = refresh_world(2);
    layers.class_changed = false;
    layers.ceiling = 1000;
    put_area(&mut state, plain(0, 0, 0), &area(0, &[(AreaHalf::At0c, 900), (AreaHalf::At0e, 300)]));
    put_area(&mut state, plain(0, 0, 1), &area(0, &[(AreaHalf::At0c, 100), (AreaHalf::At0e, 50)]));
    update(&mut state, &mut layers, true, false);
    let settled = |state: &mut ShotState<Memory>, at| {
        let area = state.area(at);
        (area.half(AreaHalf::At0c), area.half(AreaHalf::At0e))
    };
    assert_eq!(settled(&mut state, plain(0, 0, 0)), (1000, 200));
    assert_eq!(settled(&mut state, plain(0, 0, 1)), (150, 0));
}

#[test]
fn configured_fixed_classes_replace_the_choice_and_skip_the_refreshes() {
    let (mut state, mut layers) = refresh_world(1);
    layers.set(GridSetting::Control, 1);
    layers.lens_band = 1;
    layers.record_class = 9;
    let mut kept = area(0, &[]);
    kept.set_word(AreaWord::ClassIndex, 4);
    put_area(&mut state, plain(0, 0, 0), &kept);
    update(&mut state, &mut layers, false, false);
    assert_eq!(state.area(plain(0, 0, 0)).word(AreaWord::ClassIndex), 4);
    let slot = SelectionSlot::plain(index(0));
    assert_eq!((state.selection_class(slot), state.selection_cell(slot)), (SelectionClass(2), CellCode { column: 2, row: 0 }));
    assert_eq!(state.saved_selection(), state.selection());
}

#[test]
fn a_reset_takes_the_saved_list_back_with_the_current_weights() {
    let (mut state, mut layers) = refresh_world(1);
    let mut live = SelectionList::default();
    live.set_weight(index(0), 0x40);
    state.set_selection(&live);
    let mut saved = SelectionList::default();
    saved.set_class(SelectionSlot::plain(index(0)), SelectionClass(7));
    saved.set_weight(index(0), 0x99);
    save_list(&mut state, &saved);
    update(&mut state, &mut layers, true, false);
    let list = state.selection();
    assert_eq!((list.class(SelectionSlot::plain(index(0))), list.weight(index(0))), (SelectionClass(7), 0x40));
}

/// Focus selection mode 12 with a focused exposure.
fn focus_world() -> (ShotState<Memory>, Layers) {
    let (mut state, mut layers) = refresh_world(1);
    layers.class_changed = false;
    layers.mode = 12;
    layers.distance = 10;
    layers.set(GridSetting::FocusMargin { entry: 8, step: 0 }, 25);
    layers.set(GridSetting::FocusMargin { entry: 8, step: 1 }, 30);
    layers.set_profile(CalculationProfile::SelectionKind, 1);
    state.memory().write(layout::EXPOSURE_FOCUS_MODE, &1u32.to_le_bytes());
    (state, layers)
}

fn focus_area(level: i16, bound: i16) -> AreaResult {
    area(0, &[(AreaHalf::At0c, 150), (AreaHalf::Level, level), (AreaHalf::At4e, 50), (AreaHalf::At44, 20), (AreaHalf::At14, bound)])
}

#[test]
fn a_focus_refresh_raises_a_low_area_to_its_margin_and_marks_the_plan() {
    let (mut state, mut layers) = focus_world();
    put_area(&mut state, plain(0, 0, 0), &focus_area(120, 0));
    update(&mut state, &mut layers, true, false);
    let refreshed = state.area(plain(0, 0, 0));
    assert_eq!((refreshed.half(AreaHalf::Level), refreshed.half(AreaHalf::At4e)), (300, 290));
    assert_eq!(state.plan_value(ResultPass::Plain, cell(0, 0), index(0)) & 1, 1);
}

#[test]
fn a_focused_exposure_caps_the_area_total_at_its_bound() {
    let (mut state, mut layers) = focus_world();
    put_area(&mut state, plain(0, 0, 0), &focus_area(0, -100));
    update(&mut state, &mut layers, true, false);
    assert_eq!(state.area(plain(0, 0, 0)).half(AreaHalf::Level), 250);
    state.memory().write(layout::EXPOSURE_FOCUS_MODE, &0u32.to_le_bytes());
    put_area(&mut state, plain(0, 0, 0), &focus_area(0, -100));
    update(&mut state, &mut layers, true, false);
    assert_eq!(state.area(plain(0, 0, 0)).half(AreaHalf::Level), 300);
}

#[test]
fn a_flagged_grid_row_takes_the_blend_allowance() {
    let mut state = Setup { areas: 1, kinds: 0, grid: true, columns: 1, rows: 1 }.state();
    let (_, mut layers) = focus_world();
    layers.blend = 11;
    layers.set(GridSetting::FocusMargin { entry: 9, step: 2 }, 40);
    state.memory().write(layout::EXPOSURE_FOCUS_MODE, &1u32.to_le_bytes());
    state.memory().write(layout::row(grid(0), cell(0, 0), index(0)), &[4]);
    let at = AreaAt::Cell { pass: grid(0), cell: cell(0, 0), index: index(0) };
    put_area(&mut state, at, &focus_area(0, 100));
    update(&mut state, &mut layers, true, false);
    assert_eq!(state.area(at).half(AreaHalf::Level), 400);
    assert_eq!(state.plan_value(grid(0), cell(0, 0), index(0)), 0);
}

#[test]
fn a_focus_refresh_leaves_areas_at_their_margin_and_other_modes_alone() {
    let (mut state, mut layers) = focus_world();
    put_area(&mut state, plain(0, 0, 0), &focus_area(300, 0));
    update(&mut state, &mut layers, true, false);
    assert_eq!(state.area(plain(0, 0, 0)), focus_area(300, 0));
    layers.mode = 13;
    put_area(&mut state, plain(0, 0, 0), &focus_area(120, 0));
    update(&mut state, &mut layers, true, false);
    assert_eq!(state.area(plain(0, 0, 0)), focus_area(120, 0));
    assert_eq!(state.plan_value(ResultPass::Plain, cell(0, 0), index(0)), 0);
}

#[test]
fn markers_take_the_record_status_then_a_set_cell_then_the_class_status() {
    let markers = |status: u32| {
        let (mut state, mut layers) = refresh_world(3);
        layers.set_profile(CalculationProfile::SelectionKind, 1);
        layers.set_profile(CalculationProfile::At0034, status);
        let mut list = SelectionList::default();
        list.set_cell(SelectionSlot::plain(index(0)), CellCode { column: 1, row: 0 });
        save_list(&mut state, &list);
        for (area, class) in [(1, 0), (2, 5)] {
            let mut classed = AreaResult::default();
            classed.set_word(AreaWord::ClassIndex, class);
            put_area(&mut state, plain(0, 0, area), &classed);
        }
        update(&mut state, &mut layers, true, false);
        [0, 1, 2].map(|area| state.marker(ResultPass::Plain, None, index(area)))
    };
    assert_eq!(markers(0), [2, 1, 3]);
    assert_eq!(markers(6), [6, 6, 6]);
}

#[test]
fn the_walks_cover_every_column_and_row_of_the_exposure() {
    let mut state = Setup { areas: 1, kinds: 0, grid: false, columns: 2, rows: 3 }.state();
    let mut layers = Layers { ceiling: 1000, ..Layers::default() };
    for column in 0..3 {
        for row in 0..3 { put_area(&mut state, plain(column, row, 0), &area(0, &[(AreaHalf::At0c, 1200)])); }
    }
    update(&mut state, &mut layers, true, false);
    let settled = |state: &mut ShotState<Memory>, column, row| state.area(plain(column, row, 0)).half(AreaHalf::At0c);
    assert_eq!([settled(&mut state, 0, 2), settled(&mut state, 1, 2), settled(&mut state, 2, 0)], [1000, 1000, 1200]);
}
