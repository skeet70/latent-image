use crate::boundary::*;
use crate::capture_boundary::{LensMoves, MoveProfile};
use crate::finish_work::*;
use crate::ports::{FinishEffect, FinishQuery, HalfPressMode, LensMove};
use crate::preparation::{FrameHeader, HalfPressMask, HalfPressParameters, Prepared};
use crate::summary::{AreaStatistics, FrameSummary};
use crate::tests::record;
use std::vec::Vec;
use xm1_saclac_interface::{Completion, RecordHandle};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Move { Relative { delta: i16, kind: u32, profile: MoveProfile }, Absolute { target: u32, profile: MoveProfile } }

pub(super) struct World {
    snapshot: LensSnapshot,
    designated: LensPoint,
    lens_position: u32,
    reference_limit: u32,
    override_: u32,
    selection_class: u32,
    configuration: [u32; 5],
    source: TargetSource,
    areas: AreaPositions,
    correction: CorrectionRecord,
    frame_correction: Option<u32>,
    capture_status: u32,
    stopped: bool,
    manual: u32,
    calculation_areas: u8,
    selection_mode: u32,
    area_writes: Vec<(&'static str, u8, u32)>,
    saved_mode: Option<u32>,
    lens_notice: Option<bool>,
    lens_target: Option<(i16, u32, u32)>,
    bounds: Option<[u32; 3]>,
    moves: Vec<Move>,
    traces: Vec<FinishTrace>,
}

impl World {
    pub(super) fn new() -> Self {
        Self {
            snapshot: LensSnapshot { current: LensPoint { position: 100, detail: 0, reference: LensReference(40), state: 3, kind: 1 }, ..LensSnapshot::default() },
            designated: LensPoint { position: 300, detail: 0, reference: LensReference(41), state: 4, kind: 0 },
            lens_position: 90,
            reference_limit: 0,
            override_: 0,
            selection_class: 0,
            configuration: [1, 10, 2, 0, 0],
            source: TargetSource::default(),
            areas: AreaPositions { last: 1, positions: [10, 20, 30, 40], bytes: [0; 36] },
            correction: CorrectionRecord { enabled: 0, pending: 0, bytes: [0; 64] },
            frame_correction: None,
            capture_status: 0,
            stopped: false,
            manual: 0,
            calculation_areas: 2,
            selection_mode: 0,
            area_writes: Vec::new(),
            saved_mode: None,
            lens_notice: None,
            lens_target: None,
            bounds: None,
            moves: Vec::new(),
            traces: Vec::new(),
        }
    }
}

struct Pipeline { point: LensPoint, relative_first: bool, record: Option<RecordHandle> }

impl FinishPipeline for Pipeline {
    fn frame_point(&self) -> LensPoint { self.point }
    fn relative_first(&self) -> bool { self.relative_first }
    fn pipeline_record(&self) -> Option<RecordHandle> { self.record }
}

fn pipeline() -> Pipeline { Pipeline { point: LensPoint { position: 500, detail: 0, reference: LensReference(42), state: 5, kind: 0 }, relative_first: false, record: Some(record()) } }

impl Records for World {
    fn setting(&mut self, _setting: Setting) -> u32 { 0 }
    fn lens_snapshot(&mut self) -> LensSnapshot { self.snapshot }
    fn store_lens_snapshot(&mut self, snapshot: &LensSnapshot) { self.snapshot = *snapshot; }
    fn focus_result(&mut self) -> FocusResult { FocusResult { reference: LensReference(0), valid: 0, kept: 0, refined: 0, extent: 0, bytes: [0; 132] } }
    fn store_focus_result(&mut self, _result: &FocusResult) {}
    fn focus_confirmation(&mut self) -> FocusConfirmation { FocusConfirmation { active: 0, reference: LensReference(0), bytes: [0; 32] } }
    fn store_focus_confirmation(&mut self, _confirmation: &FocusConfirmation) {}
    fn focus_history(&mut self) -> FocusHistory { FocusHistory { valid: 0, bytes: [0; 80] } }
    fn store_focus_history(&mut self, _history: &FocusHistory) {}
    fn clear_exposure_record(&mut self) {}
    fn area_statistics(&mut self) -> AreaStatistics { AreaStatistics::default() }
    fn publish(&mut self, _publication: Publication<'_>) {}
    fn select_first_mode(&mut self) {}
    fn load_lens_capabilities(&mut self) {}
}

impl Client for World {
    fn area_count(&mut self, _area_count: ClientRef) -> u32 { 0 }
    fn area(&mut self, _areas: ClientRef, _index: usize) -> u32 { 0 }
    fn write_lens_target(&mut self, _target: ClientRef, _point: LensPoint, _step: u16, _distance: u32) {}
    fn frame(&mut self, slot: ClientRef) -> Option<Frame> { Some(Frame(slot.0 + 1)) }
    fn clear_frame_progress(&mut self, _slot: ClientRef) {}
    fn announce_frame(&mut self, _frame: Frame, _header: FrameHeader) {}
    fn write_summary(&mut self, _frame: Frame, _summary: &FrameSummary) {}
}

impl FinishLayers for World {
    fn profile(&mut self, _record: RecordHandle, value: FinishProfile) -> u32 {
        match value {
            FinishProfile::Override => self.override_,
            FinishProfile::SelectionClass => self.selection_class,
            FinishProfile::LensControl => 77,
            FinishProfile::LiveTarget => 0x1001,
            FinishProfile::DefaultTarget => 0x1002,
            FinishProfile::LensMode | FinishProfile::LensKind | FinishProfile::AreaFlag => 0,
        }
    }
    fn lens(&mut self, value: FinishLensValue) -> u32 {
        match value {
            FinishLensValue::Position => self.lens_position,
            FinishLensValue::MotorState => if self.stopped { 2 } else { 0 },
            FinishLensValue::Manual => self.manual,
            FinishLensValue::ReferenceLimit => self.reference_limit,
            _ => 0,
        }
    }
    fn lens_pair(&mut self) -> (u32, u16) { (6, 7) }
    fn configuration(&mut self, value: FinishConfiguration) -> u32 {
        let index = match value {
            FinishConfiguration::KeepPoint => 0,
            FinishConfiguration::PositionUnit => 1,
            FinishConfiguration::PositionScale => 2,
            FinishConfiguration::DriveControl => 3,
            FinishConfiguration::DirectionClamp => 4,
        };
        self.configuration[index]
    }
    fn load_test(&mut self) -> u32 { 0 }
    fn trace(&mut self, trace: FinishTrace) { self.traces.push(trace); }
    fn designated_point(&mut self, _record: RecordHandle) -> LensPoint { self.designated }
    fn area_positions(&mut self, _record: RecordHandle) -> AreaPositions { self.areas }
    fn store_area_positions(&mut self, _record: RecordHandle, positions: &AreaPositions) { self.areas = *positions; }
    fn correction_record(&mut self, _record: RecordHandle) -> CorrectionRecord { self.correction }
    fn store_correction_record(&mut self, _record: RecordHandle, correction: &CorrectionRecord) { self.correction = *correction; }
    fn clamp_step(&mut self, position: i16, _kind: u32, direction: u32) -> i16 { position + if direction == 0 { 3 } else { -3 } }
    fn adjust(&mut self, position: i16, _point: &LensPoint, _area_flag: bool) -> i16 { position + 500 }
    fn near_bounds(&mut self, position: i16, distance: u32, target: u32, _drive: u32, _direction: u32, _step: u32) -> [u32; 3] { [position as u32, distance, target] }
    fn far_bounds(&mut self, position: i16, pair: (u32, u16), target: u32, _drive: u32, _direction: u32, _step: u32) -> [u32; 3] {
        [position as u32, pair.0 + u32::from(pair.1), target]
    }
    fn set_lens_target(&mut self, position: i16, reference: LensReference, _drive: u32, kind: u32) { self.lens_target = Some((position, reference.0, kind)); }
    fn store_area_selection(&mut self, _record: RecordHandle, index: u8, selection: u32) { self.area_writes.push(("selection", index, selection)); }
    fn store_area_mode(&mut self, _record: RecordHandle, index: u8, mode: u32) { self.area_writes.push(("mode", index, mode)); }
    fn notify_bounds(&mut self, bounds: [u32; 3]) { self.bounds = Some(bounds); }
}

impl FinishSources for World {
    fn target_source(&mut self) -> TargetSource { self.source }
    fn capture_slots(&mut self) -> (u8, u8) { (2, 3) }
    fn capture_target(&mut self, index: usize) -> u32 { 0x2000 + index as u32 }
    fn frame_drive(&mut self, _frame: Frame) -> u32 { 0 }
    fn store_frame_correction(&mut self, _frame: Frame, correction: u32) { self.frame_correction = Some(correction); }
    fn capture_status(&mut self) -> u32 { self.capture_status }
}

/// The calculation block and saved-input words of one effect, taken from the world and given back.
struct Block {
    areas: u8,
    selection_mode: u32,
    saved_mode: Option<u32>,
    lens_notice: Option<bool>,
}

impl<P> FinishCalculation<P> for Block {
    fn calculation_record(&mut self) -> RecordHandle { record() }
    fn calculation_areas(&mut self) -> u8 { self.areas }
    fn calculation_selection(&mut self) -> u32 { 1 }
    fn selection_mode(&mut self) -> u32 { self.selection_mode }
    fn set_selection_mode(&mut self, mode: u32) { self.selection_mode = mode; }
    fn exposure_held(&mut self) -> bool { false }
    fn set_saved_selection_mode(&mut self, mode: u32) { self.saved_mode = Some(mode); }
    fn lens_reference(&mut self) -> LensReference { LensReference(0) }
    fn set_lens_notice(&mut self, notice: bool) { self.lens_notice = Some(notice); }
    fn load_finish_record(&mut self, _parameters: &HalfPressParameters, _ports: &mut P) {}
}

fn effect(work: &mut FinishWork, effect: FinishEffect, prepared: &Prepared, pipeline: &Pipeline, world: &mut World) {
    let mut block = Block { areas: world.calculation_areas, selection_mode: world.selection_mode, saved_mode: world.saved_mode, lens_notice: world.lens_notice };
    work.effect(effect, prepared, pipeline, &mut block, world);
    world.selection_mode = block.selection_mode;
    world.saved_mode = block.saved_mode;
    world.lens_notice = block.lens_notice;
}

impl FinishLibrary for World {
    fn point_selection(&mut self, point: &LensPoint, _lens_reference: LensReference, _state: u32) -> u32 { 0x100 + point.position as u32 }
    fn target_selection(&mut self, _record: RecordHandle, _lens_reference: LensReference, _position: i16, _control: u32, _kind: u32, _lens_control: u32, _frame: u32) -> u32 { 2 }
    fn mode_selection(&mut self, _record: RecordHandle, _mode: u32) -> u32 { 4 }
    fn correction(&mut self, _point: &LensPoint, _drive: u32, _frame_drive: u32) -> u32 { 25 }
    fn adjust_position(&mut self, adjustment: PositionAdjustment) -> i16 { adjustment.position + 1000 }
}

impl LensMoves for World {
    fn absolute(&mut self, target: u32, profile: MoveProfile, completion: Completion) {
        assert_eq!(completion, Completion::Lens3a);
        self.moves.push(Move::Absolute { target, profile });
    }
    fn relative(&mut self, delta: i16, speed: u32, profile: MoveProfile, completion: Completion) {
        assert_eq!(completion, Completion::Lens35);
        self.moves.push(Move::Relative { delta, kind: speed, profile });
    }
}

fn prepared(mode: u32, mask: u32) -> Prepared {
    Prepared {
        parameters: Some(HalfPressParameters {
            mode: HalfPressMode(mode),
            option: 0,
            mask: HalfPressMask(mask),
            plan_option: 0,
            area_count: None,
            area_selection: 0,
            areas: None,
            lens_target: None,
            frame_slot: Some(ClientRef(0x7000)),
            lens_reference: LensReference(0),
            ..HalfPressParameters::default()
        }),
        ..Prepared::default()
    }
}

/// Runs the half-press finish's preparation effects in order, then both moves.
fn finish(work: &mut FinishWork, prepared: &Prepared, pipeline: &Pipeline, world: &mut World) {
    for effect_kind in [FinishEffect::Begin(record()), FinishEffect::LoadAdjustment, FinishEffect::PrepareRecords, FinishEffect::PrepareLens, FinishEffect::SelectTarget] {
        effect(work, effect_kind, prepared, pipeline, world);
    }
    work.lens_move(LensMove::FinishRelative, prepared, pipeline, world);
    work.lens_move(LensMove::FinishAbsolute, prepared, pipeline, world);
}

fn relative_delta(world: &World) -> i16 {
    world.moves.iter().find_map(|lens_move| match lens_move { Move::Relative { delta, .. } => Some(*delta), _ => None }).unwrap()
}

#[test]
fn a_mode_zero_finish_keeps_its_remembered_position_until_the_point_moves_past_the_scale() {
    let mut work = FinishWork::new();
    let mut world = World::new();
    finish(&mut work, &prepared(0, 0), &pipeline(), &mut world);
    assert_eq!(relative_delta(&world), 100 - 90);

    let mut world = World::new();
    world.snapshot.current.position = 115;
    finish(&mut work, &prepared(0, 0), &pipeline(), &mut world);
    assert_eq!(relative_delta(&world), 100 - 90, "within the scale of 20");

    let mut world = World::new();
    world.snapshot.current.position = 125;
    finish(&mut work, &prepared(0, 0), &pipeline(), &mut world);
    assert_eq!(relative_delta(&world), 125 - 90);
}

#[test]
fn the_point_comes_from_the_snapshot_the_record_or_the_frame_by_mask() {
    for (mask, position, kind) in [(0, 100, 1), (1 << 3, 300, 0), ((1 << 3) | (1 << 2), 500, 0)] {
        let mut work = FinishWork::new();
        let mut world = World::new();
        finish(&mut work, &prepared(1, mask), &pipeline(), &mut world);
        let relative = world.moves.iter().find(|lens_move| matches!(lens_move, Move::Relative { .. })).copied().unwrap();
        assert_eq!(relative, Move::Relative { delta: position - 90, kind, profile: MoveProfile::Finish { speed: 6 } }, "mask {mask:#x}");
    }
}

#[test]
fn the_absolute_target_follows_machine_three_and_the_capture_list() {
    let sources = [
        (TargetSource { capture: true, ..TargetSource::default() }, 0x2001),
        (TargetSource { lens_control: true, ..TargetSource::default() }, 77),
        (TargetSource { live: true, ..TargetSource::default() }, 0x1001),
        (TargetSource::default(), 0x1002),
    ];
    for (source, target) in sources {
        let mut work = FinishWork::new();
        let mut world = World { source, ..World::new() };
        finish(&mut work, &prepared(1, 0), &pipeline(), &mut world);
        assert!(world.moves.contains(&Move::Absolute { target, profile: MoveProfile::Finish { speed: 3 } }), "{source:?}");
    }
}

#[test]
fn move_speeds_follow_the_pipeline_record_and_the_preparation_gate() {
    let mut work = FinishWork::new();
    let mut world = World { override_: 1, ..World::new() };
    let mut prepared = prepared(1, 0);
    prepared.pipeline_gate = 2;
    finish(&mut work, &prepared, &pipeline(), &mut world);
    assert!(world.moves.iter().any(|lens_move| matches!(lens_move, Move::Relative { profile: MoveProfile::Finish { speed: 7 }, .. })));
    assert!(world.moves.iter().any(|lens_move| matches!(lens_move, Move::Absolute { profile: MoveProfile::Finish { speed: 2 }, .. })));
    assert_eq!(world.traces, [FinishTrace::RelativeMove, FinishTrace::AbsoluteMove]);
}

#[test]
fn a_pending_correction_moves_point_and_areas_and_the_snapshot_holds_the_uncorrected_point() {
    let mut work = FinishWork::new();
    let mut world = World { correction: CorrectionRecord { enabled: 1, pending: 1, bytes: [0; 64] }, ..World::new() };
    finish(&mut work, &prepared(1, 0), &pipeline(), &mut world);
    assert_eq!(relative_delta(&world), 125 - 90);
    assert_eq!(world.areas.positions, [35, 45, 30, 40]);
    assert_eq!(world.correction.pending, 0);
    assert_eq!(world.frame_correction, Some(25));
    assert_eq!(world.snapshot.held.position, 100);
    assert_eq!(world.snapshot.held.reference, LensReference(40));
    assert_eq!(world.snapshot.held.kind, 1);
}

#[test]
fn a_directed_point_is_clamped_one_step_towards_the_lens() {
    let mut work = FinishWork::new();
    let mut world = World::new();
    world.configuration[4] = 1;
    finish(&mut work, &prepared(1, 0), &pipeline(), &mut world);
    assert_eq!(relative_delta(&world), 103 - 90);
    let relative = world.moves.iter().find(|lens_move| matches!(lens_move, Move::Relative { .. })).copied().unwrap();
    assert!(matches!(relative, Move::Relative { kind: 0, .. }));
}

#[test]
fn the_capture_status_is_marked_in_the_lens_snapshot_at_the_start() {
    let mut work = FinishWork::new();
    let mut world = World { capture_status: 1, ..World::new() };
    effect(&mut work, FinishEffect::Begin(record()), &prepared(1, 0), &pipeline(), &mut world);
    assert_eq!(world.snapshot.tail[0..4], 1u32.to_le_bytes());
}

#[test]
fn a_new_calculation_records_area_selections_modes_and_the_saved_mode() {
    let mut work = FinishWork::new();
    let mut world = World::new();
    finish(&mut work, &prepared(0, 1 << 1), &pipeline(), &mut world);
    assert_eq!(world.area_writes, [("selection", 0, 0x100 + 100), ("selection", 1, 0x100 + 100), ("mode", 0, 2), ("mode", 1, 2)]);
    assert_eq!(world.saved_mode, Some(2));
}

#[test]
fn the_finish_has_the_reply_notify_the_lens_when_the_mask_asks_for_the_relative_notification() {
    for (mask, notice) in [(1 << 3, true), (1 << 2, false), (0, false)] {
        let mut work = FinishWork::new();
        let mut world = World::new();
        finish(&mut work, &prepared(0, mask), &pipeline(), &mut world);
        assert_eq!(world.lens_notice, Some(notice), "mask {mask:#x}");
    }
}

#[test]
fn without_a_new_calculation_only_mode_zero_records_area_selections() {
    for (mode, writes) in [(0, 2), (5, 0)] {
        let mut work = FinishWork::new();
        let mut world = World::new();
        finish(&mut work, &prepared(mode, 0), &pipeline(), &mut world);
        assert_eq!(world.area_writes.len(), writes, "mode {mode}");
        assert_eq!(world.saved_mode, None);
    }
}

#[test]
fn a_relative_completion_in_mode_one_sets_the_lens_target() {
    let mut work = FinishWork::new();
    let mut world = World::new();
    let prepared = prepared(1, 0);
    finish(&mut work, &prepared, &pipeline(), &mut world);
    effect(&mut work, FinishEffect::AbsorbRelative, &prepared, &pipeline(), &mut world);
    assert_eq!(world.lens_target, Some((1100, 40, 1)));
    assert_eq!(world.bounds, None);
}

#[test]
fn a_relative_completion_in_mode_zero_notifies_the_client_of_the_lens_bounds() {
    let cases = [
        (World::new(), Some([100, 0, 0x1002])),
        (World { manual: 1, ..World::new() }, Some([100, 13, 0x1002])),
        (World { stopped: true, ..World::new() }, Some([u32::MAX; 3])),
        (World { selection_class: 1, ..World::new() }, None),
    ];
    for (mut world, bounds) in cases {
        let mut work = FinishWork::new();
        let prepared = prepared(0, 0);
        finish(&mut work, &prepared, &pipeline(), &mut world);
        effect(&mut work, FinishEffect::AbsorbRelative, &prepared, &pipeline(), &mut world);
        assert_eq!(world.bounds, bounds);
    }
}

#[test]
fn the_live_finish_drives_to_the_live_target() {
    let mut work = FinishWork::new();
    let mut world = World::new();
    let prepared = Prepared::default();
    effect(&mut work, FinishEffect::Begin(record()), &prepared, &pipeline(), &mut world);
    effect(&mut work, FinishEffect::PrepareLiveTarget, &prepared, &pipeline(), &mut world);
    work.lens_move(LensMove::FinishAbsolute, &prepared, &pipeline(), &mut world);
    assert_eq!(world.moves, [Move::Absolute { target: 0x1001, profile: MoveProfile::Finish { speed: 3 } }]);
}

#[test]
fn the_relative_move_goes_first_when_the_pipeline_asks() {
    let work = FinishWork::new();
    let mut world = World::new();
    let mut pipeline = pipeline();
    assert!(!work.query(FinishQuery::RelativeFirst, &pipeline, &mut world));
    pipeline.relative_first = true;
    assert!(work.query(FinishQuery::RelativeFirst, &pipeline, &mut world));
}

#[test]
fn completion_traces_mark_each_move_done() {
    let mut work = FinishWork::new();
    let mut world = World::new();
    effect(&mut work, FinishEffect::MarkRelativeDone, &Prepared::default(), &pipeline(), &mut world);
    effect(&mut work, FinishEffect::MarkAbsoluteDone, &Prepared::default(), &pipeline(), &mut world);
    assert_eq!(world.traces, [FinishTrace::RelativeDone, FinishTrace::AbsoluteDone]);
}

#[test]
fn a_point_below_the_reference_limit_is_adjusted_with_its_areas_and_marked() {
    let mut work = FinishWork::new();
    let mut world = World { reference_limit: 1000, ..World::new() };
    finish(&mut work, &prepared(2, 1 << 3), &pipeline(), &mut world);
    assert_eq!(relative_delta(&world), 1300 - 90);
    assert_eq!(world.areas.positions, [1010, 1020, 30, 40]);
    assert_eq!(world.snapshot.marker, 1);
    assert_eq!(world.snapshot.held.position, 1800);
}
