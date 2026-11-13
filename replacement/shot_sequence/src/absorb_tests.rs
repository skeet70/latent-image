use crate::absorb::*;
use crate::boundary::*;
use crate::calculation_fakes::*;
use crate::ports::HalfPressMode;
use crate::preparation::{HalfPressMask, HalfPressParameters};
use std::vec::Vec;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, FocusSummary, GridCell, GridPass, ResultPass};

/// A half press in `mode` that publishes (`keep_areas` false) or keeps its areas.
fn half_press(mode: u32, keep_areas: bool) -> Absorb {
    let parameters = HalfPressParameters {
        mode: HalfPressMode(mode),
        mask: HalfPressMask(if keep_areas { 0b11 } else { 0b01 }),
        area_count: Some(ClientRef(0x900)),
        frame_slot: Some(ClientRef(0x800)),
        ..HalfPressParameters::default()
    };
    absorb_from(Origin::HalfPress(parameters))
}

fn absorb_from(origin: Origin) -> Absorb {
    Absorb {
        origin,
        record: record(),
        exposure_record: record(),
        selection: SelectionKind(2),
        lens_reference: LensReference(0x55),
        refinement: 0,
        repeat: false,
        live: matches!(origin, Origin::Live),
    }
}

fn run(input: &Absorb, world: &mut World) { absorb(input, world); }

#[test]
fn a_published_half_press_stores_every_area_with_its_detail_and_the_cell() {
    let mut world = World::new();
    run(&half_press(0, false), &mut world);
    assert_eq!(world.area_events(), [(0, 100), (1, 101), (2, 102)]);
    let Some(Event::Area(_, first)) = world.events.iter().find(|event| matches!(event, Event::Area(0, _))) else { panic!() };
    assert_eq!((first.weight, first.marker), (30, 90));
    let details: Vec<_> = world.events.iter().filter_map(|event| match event { Event::Detail(index, detail) => Some((*index, *detail)), _ => None }).collect();
    assert_eq!(details.len(), 3);
    let (_, detail) = details[1];
    assert_eq!(detail.selection_class, 2, "plan word 0x26 has bit 1 and neither bit 4 nor bit 0");
    assert_eq!((detail.plan_half, detail.plan_bit_two, detail.plan_bit_five), (-1, true, true));
    assert_eq!((detail.distance, detail.row_class, detail.extended, detail.row_word), (51, 3, Some(true), 0x700));
    assert!(world.position(&Event::Evaluate(ResultPass::Plain, 1)) < world.position(&Event::Detail(1, detail)));
    assert!(world.has(&Event::Cell(0x0000, 30)));
    assert!(!world.events.iter().any(|event| matches!(event, Event::RetainHalf(_))));
}

#[test]
fn modes_other_than_0_and_1_store_the_short_form_only_while_tracking() {
    for (tracking, stored) in [(true, true), (false, false)] {
        let mut world = World { tracking, ..World::new() };
        run(&half_press(2, false), &mut world);
        assert_eq!(!world.area_events().is_empty(), stored);
        assert!(!world.events.iter().any(|event| matches!(event, Event::Detail(..) | Event::Cell(..))));
        let policy = world.has(&Event::Export(ResultExport::Policy, false));
        assert_eq!(policy, stored);
        assert!(!world.has(&Event::Export(ResultExport::SelectedChannel, false)));
    }
}

#[test]
fn a_tracked_area_takes_its_kind_area_and_marker() {
    let mut world = World { tracking: true, kinds: [UNTRACKED, 3, 6], ..World::new() };
    run(&half_press(0, false), &mut world);
    assert_eq!(world.area_events(), [(0, 100), (1, tracked_reference(3, 1)), (2, 102)]);
    let markers: Vec<_> = world.events.iter().filter_map(|event| match event { Event::Area(_, store) => Some(store.marker), _ => None }).collect();
    assert_eq!(markers, [90, 31, 92]);
}

#[test]
fn keeping_the_areas_retains_the_result_half_before_the_cell_is_stored() {
    let mut world = World::new();
    run(&half_press(3, true), &mut world);
    assert_eq!(world.area_events().len(), 3);
    let half = world.position(&Event::RetainHalf(0x1234));
    assert!(world.position(&Event::Cell(0x0000, 30)) > half);
    assert!(world.position(&Event::Area(2, AreaStore { area: area_named(102), weight: 32, marker: 92 })) < half);
}

#[test]
fn a_returning_absorb_with_a_pending_point_leaves_the_first_area() {
    for (pending, first) in [(true, 1), (false, 0)] {
        let mut world = World::new();
        run(&absorb_from(Origin::Return { pending }), &mut world);
        assert_eq!(world.area_events().first().map(|(index, _)| *index), Some(first));
        assert!(!world.events.iter().any(|event| matches!(event, Event::Export(..) | Event::ClearChannels(_))));
    }
}

#[test]
fn mode_7_replaces_the_first_area_by_the_auxiliary_reference_when_none_is_available() {
    let mut world = World { available: false, ..World::new() };
    run(&half_press(7, true), &mut world);
    let auxiliary = AuxiliaryReference { outcome: AuxiliaryOutcome::Replaced, source: 0x5050, target: 0x4444, normalized: 0x2222 };
    assert!(world.has(&Event::Auxiliary(0, auxiliary)));
    let untracked = AreaAt::Cell { pass: ResultPass::Plain, cell: GridCell::new(0, 0).unwrap(), index: AreaIndex::new(0).unwrap() };
    assert!(world.has(&Event::RetainAuxiliary(auxiliary, Some(untracked))));
    assert_eq!(world.area_events(), [(1, 101), (2, 102)]);
    assert!(!world.has(&Event::RetainResult));
}

#[test]
fn mode_7_retains_the_result_unless_the_first_area_was_replaced() {
    for (available, require, changed) in [(true, false, false), (false, true, false), (false, true, true)] {
        let mut world = World { available, require, changed, ..World::new() };
        run(&half_press(7, true), &mut world);
        assert!(!world.events.iter().any(|event| matches!(event, Event::Auxiliary(..))));
        assert_eq!(world.area_events().len(), 3);
        let retained = world.position(&Event::RetainResult);
        assert_eq!(world.events.get(retained + 1), Some(&Event::MarkAuxiliaryRetained));
    }
    let mut world = World::new();
    run(&half_press(0, false), &mut world);
    assert!(!world.has(&Event::RetainResult));
}

#[test]
fn the_metering_minimum_is_the_smallest_record_reference_below_the_split() {
    let mut world = World::new();
    run(&half_press(0, false), &mut world);
    assert_eq!(world.metering(), Some((true, 400)));

    let mut world = World { client_count: 1, ..World::new() };
    run(&half_press(0, false), &mut world);
    assert_eq!(world.metering(), Some((true, 500)));

    let mut world = World { split: 300, ..World::new() };
    run(&half_press(0, false), &mut world);
    assert_eq!(world.metering(), Some((true, 77)), "nothing below the split takes the lower reference");

    let mut world = World::new();
    run(&absorb_from(Origin::Live), &mut world);
    assert_eq!(world.metering(), Some((true, 77)));
}

#[test]
fn a_selected_grid_takes_the_minimum_over_the_grid_passes() {
    let mut world = World { grid_selected: true, ..World::new() };
    run(&half_press(0, true), &mut world);
    assert_eq!(world.metering(), Some((false, 300 + 1)), "mode 0 spans the three passes");

    let mut world = World { grid_selected: true, ..World::new() };
    run(&half_press(1, true), &mut world);
    assert_eq!(world.metering(), Some((true, 301)), "mode 1 takes the first pass");

    let mut world = World { grid_selected: true, selection_mode: 2, ..World::new() };
    run(&half_press(7, true), &mut world);
    assert_eq!(world.metering(), Some((true, 303)), "mode 7 takes the block's selection mode");

    let mut world = World { grid_selected: true, ..World::new() };
    run(&half_press(0, false), &mut world);
    assert_eq!(world.metering(), Some((true, 400)), "published areas take the record's minimum");
}

#[test]
fn a_selected_grid_has_each_pass_evaluated_after_the_plain_areas() {
    let mut world = World { grid_selected: true, areas: 1, ..World::new() };
    run(&half_press(0, false), &mut world);
    let evaluations: Vec<_> = world.events.iter().filter(|event| matches!(event, Event::Evaluate(..))).cloned().collect();
    let grid = |pass| ResultPass::Grid(GridPass::new(pass).unwrap());
    assert_eq!(evaluations, [Event::Evaluate(ResultPass::Plain, 0), Event::Evaluate(grid(0), 0), Event::Evaluate(grid(1), 0), Event::Evaluate(grid(2), 0)]);
}

#[test]
fn the_grid_update_follows_the_origin() {
    let update = |input: Absorb| {
        let mut world = World::new();
        run(&input, &mut world);
        world.events.iter().find_map(|event| match event { Event::GridUpdate { reset, keep_gates } => Some((*reset, *keep_gates)), _ => None }).unwrap()
    };
    assert_eq!(update(half_press(0, false)), (true, true));
    assert_eq!(update(half_press(1, false)), (true, true));
    assert_eq!(update(half_press(2, false)), (true, false));
    assert_eq!(update(half_press(3, false)), (false, false));
    assert_eq!(update(half_press(0, true)), (false, false));
    assert_eq!(update(half_press(7, true)), (true, false));
    assert_eq!(update(absorb_from(Origin::Return { pending: false })), (true, true));
    assert_eq!(update(absorb_from(Origin::Live)), (false, false));
}

#[test]
fn a_repeated_half_press_exports_the_retained_result_between_set_aside_and_restore() {
    let mut world = World { retained_export: 4, ..World::new() };
    run(&Absorb { repeat: true, selection: SelectionKind(0), ..half_press(0, false) }, &mut world);
    let aside = world.position(&Event::SetAside);
    let expected = [
        Event::RestoreResult,
        Event::GridUpdate { reset: true, keep_gates: true },
        Event::Export(ResultExport::Compact, true),
        Event::Export(ResultExport::SelectedChannel, true),
        Event::Export(ResultExport::SourceLimits, true),
        Event::RestoreWorking,
    ];
    assert_eq!(&world.events[aside + 1..aside + 7], &expected);
    assert!(!world.events.iter().any(|event| matches!(event, Event::ClearChannels(_))));
    assert!(world.has(&Event::Export(ResultExport::Policy, false)));
}

#[test]
fn the_working_result_exports_enabled_when_the_retained_one_is_not_exported() {
    let mut world = World::new();
    run(&Absorb { repeat: true, selection: SelectionKind(1), ..half_press(0, false) }, &mut world);
    assert_eq!(world.events.first(), Some(&Event::ClearChannels(false)));
    for export in [ResultExport::Policy, ResultExport::Compact, ResultExport::SelectedChannel, ResultExport::SourceLimits] {
        assert!(world.has(&Event::Export(export, true)), "{export:?}");
    }

    let mut world = World::new();
    run(&Absorb { selection: SelectionKind(4), ..half_press(0, false) }, &mut world);
    assert_eq!(world.events.first(), Some(&Event::ClearChannels(true)));
    assert!(world.has(&Event::Export(ResultExport::Policy, false)));
    assert!(!world.has(&Event::Export(ResultExport::Compact, false)));
}

#[test]
fn the_focus_summary_is_stored_for_selections_0_1_and_5_with_its_level() {
    for selection in 0..8 {
        let mut world = World::new();
        run(&Absorb { selection: SelectionKind(selection), ..half_press(3, false) }, &mut world);
        let stored = world.has(&Event::Summary(FocusSummary([1, 2, 3, 4, 5, 6, 7, 8]), 0x106));
        assert_eq!(stored, matches!(selection, 0 | 1 | 5), "selection {selection}");
        assert_eq!(world.has(&Event::Export(ResultExport::SourceLimits, false)), stored);
    }
}

#[test]
fn subjects_are_classified_while_tracking_and_the_rate_status_follows_the_map() {
    let mut world = World { tracking: true, subjects: 1, rate_status: 1, ..World::new() };
    run(&half_press(3, false), &mut world);
    assert!(world.has(&Event::Classify(ClassifyStage::Absorb)));
    assert!(world.has(&Event::ReleaseRateStatus));

    let mut world = World { subjects: 2, rate_status: 1, map_present: 1, ..World::new() };
    run(&half_press(3, false), &mut world);
    assert!(!world.has(&Event::Classify(ClassifyStage::Absorb)));
    assert!(world.has(&Event::DrawRateStatus));

    let mut world = World { tracking: true, rate_status: 1, ..World::new() };
    run(&half_press(3, false), &mut world);
    assert!(!world.events.iter().any(|event| matches!(event, Event::Classify(_) | Event::DrawRateStatus | Event::ReleaseRateStatus)));
}

#[test]
fn the_metering_block_keeps_its_first_56_bytes() {
    let mut world = World::new();
    run(&half_press(0, false), &mut world);
    let Some(Event::Metering(block)) = world.events.iter().find(|event| matches!(event, Event::Metering(_))) else { panic!() };
    assert!(block.0[..56].iter().all(|byte| *byte == 0xee));
}
