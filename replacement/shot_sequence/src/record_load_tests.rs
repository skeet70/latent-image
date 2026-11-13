use crate::absorb::AuxiliaryOutcome;
use crate::boundary::{ClientRef, Frame};
use crate::calculation_fakes::*;
use crate::capture_boundary::ReferencePair;
use crate::ports::{HalfPressMode, Reply};
use crate::preparation::{HalfPressMask, HalfPressParameters};
use crate::record_load::*;
use crate::stock::record_load as layout;
use std::vec::Vec;
use xm1_saclac_interface::calculation::GridPass;
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::CommandKind;

const AREAS: ClientRef = ClientRef(0x900);
const FRAME_SLOT: ClientRef = ClientRef(0x800);

fn half_press(mode: u32, keep_areas: bool) -> RecordLoad {
    let parameters = HalfPressParameters {
        mode: HalfPressMode(mode),
        mask: HalfPressMask(if keep_areas { 0b11 } else { 0b01 }),
        area_count: Some(ClientRef(0x700)),
        areas: Some(AREAS),
        frame_slot: Some(FRAME_SLOT),
        client_reference: ReferencePair { reference: 0xc1, detail: 0xc2 },
        ..HalfPressParameters::default()
    };
    RecordLoad { origin: LoadOrigin::HalfPress(parameters), record: record(), refinement: 0, repeat: false }
}

fn lens_return(pending: bool) -> RecordLoad { RecordLoad { origin: LoadOrigin::Return { pending }, record: record(), refinement: 0, repeat: false } }

fn run(input: &RecordLoad, world: &mut World) { load(input, world); }

fn first_pair(world: &World) -> ReferencePair { ReferencePair { reference: world.lens_reference, detail: world.stored_area.reference } }

fn kept_reply() -> Event { Event::Reply(Reply { command: CommandKind::HalfPress, record: Some(record()), status: AREAS_KEPT_STATUS }) }

fn client_slots(world: &World) -> Vec<(u32, u8)> {
    world.events.iter().filter_map(|event| match event { Event::ClientSlot { target, slot } => Some((*target, *slot)), _ => None }).collect()
}

fn loaded(world: &World) -> Vec<LoadExport> {
    world.events.iter().filter_map(|event| match event { Event::Loaded(export, _) => Some(*export), _ => None }).collect()
}

#[test]
fn a_half_press_that_keeps_its_areas_points_every_pair_at_the_lens_and_notifies_the_client() {
    let mut world = World::new();
    run(&half_press(2, true), &mut world);
    let pair = first_pair(&world);
    assert_eq!(world.pairs(), [(0, pair), (1, pair), (2, pair), (3, pair), (4, pair)]);
    let snapshot = world.events.iter().find_map(|event| match event { Event::Snapshot(snapshot) => Some(*snapshot), _ => None }).expect("snapshot stored");
    assert_eq!(snapshot, layout::with_resume_offset(world.snapshot, world.profile_offset));
    assert!(world.has(&Event::ResumeOffset(world.profile_offset)));
    assert!(world.has(&Event::Starts(AreaStarts { stored: world.stored_area.start, selected: plain_reference(0) as i16 })));
    assert!(world.has(&Event::RetainWord(world.stored_area.reference)));
    assert_eq!(world.events.last(), Some(&kept_reply()));
    assert!(client_slots(&world).is_empty(), "kept areas reach the client through the notification");
    assert!(loaded(&world).is_empty(), "nothing is loaded without a selected grid");
}

#[test]
fn a_repeated_half_press_resumes_from_the_stored_offset() {
    let mut world = World::new();
    run(&RecordLoad { repeat: true, ..half_press(2, true) }, &mut world);
    assert!(world.has(&Event::ResumeOffset(world.stored_offset)));
    assert!(!world.has(&Event::ResumeOffset(world.profile_offset)));
}

#[test]
fn the_client_reference_replaces_the_third_pair_for_its_selection_sources() {
    for (source, replaced) in [(46, true), (47, true), (54, true), (56, true), (48, false), (55, false)] {
        let mut world = World { subject_source: source, ..World::new() };
        run(&half_press(2, true), &mut world);
        let pairs = world.pairs();
        assert_eq!(pairs.len(), 5 + usize::from(replaced), "source {source}");
        if replaced { assert_eq!(pairs[5], (2, ReferencePair { reference: 0xc1, detail: 0xc2 }), "source {source}"); }
    }
}

#[test]
fn modes_zero_and_six_rewrite_the_first_pair() {
    for (mode, keep, expected) in [(0, false, 1), (6, false, 1), (0, true, 2), (1, false, 0), (3, true, 1)] {
        let mut world = World::new();
        run(&half_press(mode, keep), &mut world);
        let pair = first_pair(&world);
        assert_eq!(world.pairs().iter().filter(|(slot, written)| *slot == 0 && *written == pair).count(), expected, "mode {mode}, keep {keep}");
    }
}

#[test]
fn published_areas_of_the_first_three_modes_reach_the_clients_slots() {
    let mut world = World { client_count: 2, ..World::new() };
    run(&half_press(1, false), &mut world);
    assert_eq!(client_slots(&world), [(AREAS.0, 0), (AREAS.0 + 4, 1)]);
    assert!(!world.events.iter().any(|event| matches!(event, Event::Reply(_))));

    let mut world = World { client_count: 2, ..World::new() };
    run(&half_press(3, false), &mut world);
    assert!(client_slots(&world).is_empty(), "mode 3 leaves the client's slots");
}

#[test]
fn a_selected_grid_loads_its_pass_and_first_cell_and_exports_both() {
    let mut world = World { grid_selected: true, selection_mode: 1, ..World::new() };
    run(&half_press(0, false), &mut world);
    let pass = GridPass::new(1).unwrap();
    assert_eq!(world.area_events(), [(0, 100), (1, 101), (2, 102)], "the plain areas hold the adopted grid areas");
    for index in 0..3 {
        assert!(world.has(&Event::AdoptArea(pass, index, None)));
        assert!(world.has(&Event::AdoptTables(pass, index)));
    }
    let adopt = world.position(&Event::AdoptCell(pass));
    assert!(adopt < world.position(&Event::Cell(2, 30)), "the cell is stored after it takes the pass's summary");
    assert_eq!(loaded(&world), [LoadExport::Areas, LoadExport::Cell]);
    assert!(world.has(&Event::Loaded(LoadExport::Areas, Some(Frame(FRAME_SLOT.0)))));
}

#[test]
fn a_narrow_half_press_loads_short_areas_only_while_tracking() {
    let mut world = World { grid_selected: true, selection_mode: 0, tracking: true, ..World::new() };
    run(&half_press(3, false), &mut world);
    assert_eq!(world.area_events().len(), 3);
    assert!(!world.events.iter().any(|event| matches!(event, Event::AdoptTables(..) | Event::Detail(..) | Event::AdoptCell(_))));
    assert_eq!(loaded(&world), [LoadExport::Areas]);

    let mut world = World { grid_selected: true, selection_mode: 0, ..World::new() };
    run(&half_press(3, false), &mut world);
    assert!(world.area_events().is_empty());
    assert!(loaded(&world).is_empty());
}

#[test]
fn a_tracked_grid_classifies_the_subjects_for_the_record_load() {
    let mut world = World { grid_selected: true, tracking: true, subjects: 1, ..World::new() };
    run(&half_press(0, false), &mut world);
    assert!(world.has(&Event::Classify(crate::absorb::ClassifyStage::RecordLoad)));

    let mut world = World { grid_selected: false, tracking: true, subjects: 1, ..World::new() };
    run(&half_press(0, false), &mut world);
    assert!(!world.events.iter().any(|event| matches!(event, Event::Classify(_))));
}

#[test]
fn a_selection_mode_outside_the_passes_loads_no_grid_area() {
    let mut world = World { grid_selected: true, selection_mode: 4, ..World::new() };
    run(&half_press(0, true), &mut world);
    assert!(world.area_events().is_empty());
    assert!(loaded(&world).is_empty());
    assert!(world.has(&Event::Starts(AreaStarts { stored: world.stored_area.start, selected: plain_reference(0) as i16 })));
}

#[test]
fn the_selected_start_comes_from_the_selected_pass() {
    let mut world = World { grid_selected: true, selection_mode: 2, ..World::new() };
    run(&half_press(2, true), &mut world);
    assert!(world.has(&Event::Starts(AreaStarts { stored: world.stored_area.start, selected: (world.grid_references[0] + 3) as i16 })));
}

#[test]
fn the_lens_return_loads_the_grid_and_leaves_the_pairs_and_the_client() {
    let mut world = World { grid_selected: true, selection_mode: 0, ..World::new() };
    run(&lens_return(true), &mut world);
    assert_eq!(world.area_events(), [(1, 101), (2, 102)], "a pending lens point keeps the first area");
    assert!(world.pairs().is_empty());
    assert!(!world.events.iter().any(|event| matches!(event, Event::Reply(_) | Event::Snapshot(_) | Event::ClientSlot { .. })));
    assert!(world.has(&Event::Loaded(LoadExport::Cell, Some(Frame(world.capture_frame)))));
    assert!(world.has(&Event::RetainWord(world.stored_area.reference)));

    let mut world = World { grid_selected: true, selection_mode: 0, ..World::new() };
    run(&lens_return(false), &mut world);
    assert_eq!(world.area_events().len(), 3);
}

#[test]
fn mode_five_writes_the_profile_and_the_selected_row_word_into_its_block() {
    for (grid_selected, row_word) in [(false, 0x700), (true, 0x702)] {
        let mut world = World { grid_selected, selection_mode: 1, ..World::new() };
        run(&half_press(5, true), &mut world);
        let mut expected = ModeFiveBlock([0xaa; MODE_FIVE_BLOCK_BYTES]);
        layout::set_mode_five(&mut expected, ModeFiveValues { profile: [-1, 2, -3], row_word });
        assert!(world.has(&Event::ModeFive(expected)), "grid selected: {grid_selected}");
    }
    let mut world = World::new();
    run(&lens_return(false), &mut world);
    assert!(!world.events.iter().any(|event| matches!(event, Event::ModeFive(_))));
}

#[test]
fn mode_seven_hands_the_first_area_to_the_hook_and_publishes_the_call() {
    let hook = StockCallbackWord(0x00ab_cdef);
    let mut world = World { hook: Some(hook), ..World::new() };
    run(&half_press(7, true), &mut world);
    let pair = ReferencePair { reference: 0x6060, detail: world.stored_area.reference };
    assert_eq!(world.pairs(), [(4, pair), (2, pair), (1, pair)]);
    let call = AuxiliaryCall { hook, area: world.stored_area, reference: 0x6060 };
    let called = world.position(&Event::HookCall(call));
    assert!(called < world.position(&Event::Published(call)));
    assert!(!world.events.iter().any(|event| matches!(event, Event::Reply(_) | Event::Snapshot(_))));

    for (outcome, slots) in [(AuxiliaryOutcome::Kept, &[4u8, 1][..]), (AuxiliaryOutcome::Replaced, &[1][..])] {
        let mut world = World { auxiliary: outcome, ..World::new() };
        run(&half_press(7, true), &mut world);
        assert_eq!(world.pairs().iter().map(|(slot, _)| *slot).collect::<Vec<_>>(), slots, "{outcome:?}");
        assert!(!world.events.iter().any(|event| matches!(event, Event::HookCall(_) | Event::Published(_))), "no hook");
    }
}

#[test]
fn the_hook_takes_thirteen_words_and_the_publication_carries_the_same_values() {
    let area = StoredArea { start: 9, reference: 0x4100, selection_class: 2, plan_half: -7, plan_bit_two: 1, plan_bit_five: 0, halves: [-1, 2, -3, 4, -5] };
    let call = AuxiliaryCall { hook: StockCallbackWord(0xf00d), area, reference: 0x6060 };
    let words = layout::auxiliary_hook_arguments(&call);
    assert_eq!(words, [2, 0xffff_ffff, 0x4100, 0xffff_fff9, 1, 0, 0, 1, 2, 0xffff_fffd, 4, 0xffff_fffb, 0x6060]);
    let bytes = layout::auxiliary_publication(&call);
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let half = |at: usize| i16::from_le_bytes([bytes[at], bytes[at + 1]]);
    assert_eq!([word(0), word(4), word(0xc), word(0x14), word(0x18), word(0x1c), word(0x20), word(0x2c)], [0xf00d, 2, 0x4100, 1, 0, 0, 1, 0x6060]);
    assert_eq!([half(8), half(0x10), half(0x24), half(0x26), half(0x28), half(0x2a)], [-1, -7, 2, -3, 4, -5]);
    assert_eq!([half(0xa), half(0x12)], [0, 0]);
}
