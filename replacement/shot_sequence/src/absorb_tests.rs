use crate::absorb::*;
use crate::boundary::*;
use crate::capture_boundary::{AreaReference, RecordField};
use crate::ports::HalfPressMode;
use crate::preparation::{FrameHeader, HalfPressMask, HalfPressParameters};
use crate::summary::FrameSummary;
use std::vec::Vec;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, CellSummary, FocusSummary, GridCell, GridPass, ResultPass, TrackedKind};
use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;

#[derive(Clone, Debug, PartialEq)]
enum Event {
    Area(u8, AreaStore),
    Detail(u8, AreaDetail),
    Auxiliary(u8, AuxiliaryReference),
    Cell(u32, u32),
    Summary(FocusSummary, u32),
    Metering(MeteringBlock),
    Export(ResultExport, bool),
    ClearChannels(bool),
    GridUpdate { reset: bool, focus: bool },
    Classify(ClassifyStage),
    RetainResult,
    RestoreResult,
    RetainAuxiliary(AuxiliaryReference, Option<AreaAt>),
    MarkAuxiliaryRetained,
    RetainHalf(u16),
    SetAside,
    RestoreWorking,
    Evaluate(ResultPass, u8),
    AdoptArea(GridPass, u8, Option<TrackedKind>),
    AdoptTables(GridPass, u8),
    DrawRateStatus,
    ReleaseRateStatus,
}

struct World {
    events: Vec<Event>,
    areas: u8,
    tracking: bool,
    kinds: [u8; 3],
    grid_selected: bool,
    selection_mode: u32,
    auxiliary: AuxiliaryOutcome,
    retained_export: u8,
    subjects: u8,
    rate_status: u8,
    map_present: u32,
    record_references: [u32; 3],
    grid_references: [u32; 3],
    split: u32,
    lower: u32,
    require: bool,
    changed: bool,
    available: bool,
    client_count: u32,
    capture_frame: u32,
    capture_slots: u32,
}

impl World {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            areas: 3,
            tracking: false,
            kinds: [UNTRACKED; 3],
            grid_selected: false,
            selection_mode: 0,
            auxiliary: AuxiliaryOutcome::Unchecked,
            retained_export: 0,
            subjects: 0,
            rate_status: 0,
            map_present: 0,
            record_references: [500, 400, 600],
            grid_references: [700, 300, 900],
            split: 1000,
            lower: 77,
            require: false,
            changed: false,
            available: true,
            client_count: 3,
            capture_frame: 0x40,
            capture_slots: 2,
        }
    }

    fn area_events(&self) -> Vec<(u8, u32)> {
        self.events.iter().filter_map(|event| match event { Event::Area(index, store) => Some((*index, store.area.reference())), _ => None }).collect()
    }

    fn metering(&self) -> Option<(bool, u32)> {
        self.events.iter().rev().find_map(|event| match event {
            Event::Metering(block) => {
                let word = |at: usize| u32::from_le_bytes(block.0[at..at + 4].try_into().unwrap());
                Some((word(0x38) != 0, word(0x3c)))
            }
            _ => None,
        })
    }

    fn has(&self, wanted: &Event) -> bool { self.events.iter().any(|event| event == wanted) }

    fn position(&self, wanted: &Event) -> usize { self.events.iter().position(|event| event == wanted).unwrap_or_else(|| panic!("{wanted:?} missing")) }
}

fn pass_code(pass: ResultPass) -> u32 {
    match pass {
        ResultPass::Plain => 0,
        ResultPass::Grid(pass) => 1 + u32::from(pass.index()),
    }
}

/// An area whose reference names where it was read.
fn area_named(reference: u32) -> AreaResult {
    let mut bytes = [0; AreaResult::BYTES];
    bytes[4..8].copy_from_slice(&reference.to_le_bytes());
    AreaResult::from_bytes(bytes)
}

fn plain_reference(index: u8) -> u32 { 100 + u32::from(index) }

fn tracked_reference(kind: u8, index: u8) -> u32 { 200 + 10 * u32::from(kind) + u32::from(index) }

impl CalculationResults for World {
    fn result_areas(&mut self) -> u8 { self.areas }
    fn tracking(&mut self) -> bool { self.tracking }
    fn area_kind(&mut self, index: AreaIndex) -> u8 { self.kinds[usize::from(index.get())] }
    fn grid_selected(&mut self) -> bool { self.grid_selected }
    fn selection_mode(&mut self) -> u32 { self.selection_mode }
    fn result_area(&mut self, at: AreaAt) -> AreaResult {
        match at {
            AreaAt::Cell { pass: ResultPass::Plain, index, .. } => area_named(plain_reference(index.get())),
            AreaAt::Cell { pass, index, .. } => area_named(self.grid_references[usize::from(index.get())] + pass_code(pass)),
            AreaAt::Tracked { kind, index, .. } => area_named(tracked_reference(kind.get(), index.get())),
        }
    }
    fn marker(&mut self, pass: ResultPass, kind: Option<TrackedKind>, index: AreaIndex) -> u32 {
        1000 * pass_code(pass) + kind.map_or(90, |kind| u32::from(kind.get()) * 10) + u32::from(index.get())
    }
    fn plan_value(&mut self, _pass: ResultPass, _cell: GridCell, _index: AreaIndex) -> u32 { 0x26 }
    fn plan_half(&mut self, _pass: ResultPass, _cell: GridCell, index: AreaIndex) -> i16 { -(i16::from(index.get())) }
    fn row(&mut self, _pass: ResultPass, _cell: GridCell, index: AreaIndex) -> AreaRow {
        let mut row = AreaRow::default();
        row.0[0] = 4;
        row.0[4] = 50 + index.get();
        row
    }
    fn row_word(&mut self, pass: ResultPass, _cell: GridCell) -> u32 { 0x700 + pass_code(pass) }
    fn cell_summary(&mut self, _pass: ResultPass, cell: GridCell) -> CellSummary {
        let mut bytes = [0; CellSummary::BYTES];
        bytes[0] = cell.column;
        bytes[1] = cell.row;
        CellSummary::from_bytes(bytes)
    }
    fn focus_summary(&mut self) -> FocusSummary { FocusSummary([1, 2, 3, 4, 5, 6, 7, 8]) }
    fn retained_half(&mut self) -> u16 { 0x1234 }
    fn auxiliary(&mut self) -> AuxiliaryOutcome { self.auxiliary }
    fn set_auxiliary(&mut self, outcome: AuxiliaryOutcome) { self.auxiliary = outcome; }
    fn set_working_aside(&mut self) { self.events.push(Event::SetAside); }
    fn restore_working(&mut self) { self.events.push(Event::RestoreWorking); }
    fn adopt_grid_area(&mut self, pass: GridPass, _cell: GridCell, index: AreaIndex, kind: Option<TrackedKind>) { self.events.push(Event::AdoptArea(pass, index.get(), kind)); }
    fn adopt_grid_tables(&mut self, pass: GridPass, _cell: GridCell, index: AreaIndex) { self.events.push(Event::AdoptTables(pass, index.get())); }
    fn adopt_grid_cell(&mut self, _pass: GridPass, _cell: GridCell) {}
}

impl SelectionGrid for World {
    fn cell(&mut self, pass: ResultPass, index: AreaIndex) -> GridCell { GridCell::new(index.get(), pass_code(pass).min(2) as u8).unwrap() }
    fn update_grid(&mut self, _record: RecordHandle, reset: bool, focus: bool, _lens_reference: LensReference) { self.events.push(Event::GridUpdate { reset, focus }); }
    fn classify_subjects(&mut self, stage: ClassifyStage, _reference: &AreaReference, _kind: u32, _source: u32, _area: u32) { self.events.push(Event::Classify(stage)); }
    fn retain_result(&mut self) { self.events.push(Event::RetainResult); }
    fn restore_result(&mut self) { self.events.push(Event::RestoreResult); }
    fn retain_auxiliary(&mut self, auxiliary: &AuxiliaryReference, untracked: Option<AreaAt>) { self.events.push(Event::RetainAuxiliary(*auxiliary, untracked)); }
    fn mark_auxiliary_retained(&mut self) { self.events.push(Event::MarkAuxiliaryRetained); }
    fn requires_eligibility(&mut self) -> bool { self.require }
    fn area_weight(&mut self, index: AreaIndex) -> u32 { 30 + u32::from(index.get()) }
    fn retain_target_half(&mut self, half: u16) { self.events.push(Event::RetainHalf(half)); }
}

impl AbsorbLibrary for World {
    fn subject_source(&mut self, _record: RecordHandle, _refinement: u32) -> u32 { 0 }
    fn grid_mode(&mut self, _record: RecordHandle, mode: u32) -> u32 { if mode == 1 { 0 } else { 4 } }
    fn evaluate_distances(&mut self, pass: ResultPass, _cell: GridCell, index: AreaIndex) { self.events.push(Event::Evaluate(pass, index.get())); }
    fn selection_changed(&mut self, _record: RecordHandle, _source: u32, _reference: u32) -> bool { self.changed }
    fn compare_reference(&mut self, _record: RecordHandle, _source: u32) -> (bool, u32) { (self.available, 0x4444) }
    fn compensated(&mut self, target: u32) -> i16 { target as i16 / 2 }
    fn classify_level(&mut self, value: i16) -> u32 { 0x100 + value as u32 }
    fn split_reference(&mut self) -> u32 { self.split }
    fn lower_reference(&mut self, _lens_reference: LensReference) -> u32 { self.lower }
}

impl AbsorbDiagnostics for World {
    fn adjustment(&mut self, adjustment: AbsorbAdjustment) -> u8 {
        match adjustment {
            AbsorbAdjustment::RetainedExport => self.retained_export,
            AbsorbAdjustment::Subjects => self.subjects,
            AbsorbAdjustment::RateStatus => self.rate_status,
        }
    }
    fn export_result(&mut self, _frame: Option<Frame>, export: ResultExport, enabled: bool) { self.events.push(Event::Export(export, enabled)); }
    fn clear_channel_diagnostics(&mut self, _frame: Option<Frame>, cleared: bool) { self.events.push(Event::ClearChannels(cleared)); }
    fn draw_rate_status(&mut self, _reference: &AreaReference) { self.events.push(Event::DrawRateStatus); }
    fn release_rate_status(&mut self) { self.events.push(Event::ReleaseRateStatus); }
}

impl AbsorbRecords for World {
    fn store_area(&mut self, _record: RecordHandle, index: AreaIndex, store: &AreaStore) { self.events.push(Event::Area(index.get(), *store)); }
    fn store_area_detail(&mut self, _record: RecordHandle, index: AreaIndex, detail: &AreaDetail) { self.events.push(Event::Detail(index.get(), *detail)); }
    fn store_auxiliary(&mut self, _record: RecordHandle, index: AreaIndex, auxiliary: &AuxiliaryReference) { self.events.push(Event::Auxiliary(index.get(), *auxiliary)); }
    fn store_cell(&mut self, _record: RecordHandle, cell: &CellSummary, weight: u32) {
        self.events.push(Event::Cell(u32::from(cell.bytes()[0]) << 8 | u32::from(cell.bytes()[1]), weight));
    }
    fn store_focus_summary(&mut self, _record: RecordHandle, summary: &FocusSummary, level: u32) { self.events.push(Event::Summary(*summary, level)); }
    fn area_lens_reference(&mut self, _record: RecordHandle, index: u8) -> u32 { self.record_references.get(usize::from(index)).copied().unwrap_or(u32::MAX) }
    fn extended_areas(&mut self, _record: RecordHandle) -> bool { true }
    fn selection_source(&mut self, _record: RecordHandle) -> u32 { 0x5050 }
    fn metering(&mut self, _record: RecordHandle) -> MeteringBlock { MeteringBlock([0xee; METERING_BYTES]) }
    fn store_metering(&mut self, _record: RecordHandle, block: &MeteringBlock) { self.events.push(Event::Metering(*block)); }
    fn value(&mut self, _record: RecordHandle, field: RecordField) -> u32 {
        match field {
            RecordField::CaptureFrame => self.capture_frame,
            RecordField::CaptureSlotCount => self.capture_slots,
            RecordField::MapPresent => self.map_present,
            _ => 0,
        }
    }
    fn area_references(&mut self, _record: RecordHandle) -> AreaReference { AreaReference::default() }
}

impl Client for World {
    fn area_count(&mut self, _area_count: ClientRef) -> u32 { self.client_count }
    fn area(&mut self, _areas: ClientRef, _index: usize) -> u32 { 0 }
    fn write_lens_target(&mut self, _target: ClientRef, _point: LensPoint, _step: u16, _distance: u32) {}
    fn frame(&mut self, slot: ClientRef) -> Option<Frame> { Some(Frame(slot.0)) }
    fn clear_frame_progress(&mut self, _slot: ClientRef) {}
    fn announce_frame(&mut self, _frame: Frame, _header: FrameHeader) {}
    fn write_summary(&mut self, _frame: Frame, _summary: &FrameSummary) {}
}

fn record() -> RecordHandle { record_handle_from_a2(0x2004) }

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
        world.events.iter().find_map(|event| match event { Event::GridUpdate { reset, focus } => Some((*reset, *focus)), _ => None }).unwrap()
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
        Event::GridUpdate { reset: true, focus: true },
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
