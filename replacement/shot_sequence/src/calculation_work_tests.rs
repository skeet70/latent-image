use crate::boundary::*;
use crate::calculation::Calculation;
use crate::calculation_work::*;
use crate::ports::{Branch, CalculationEffect, CalculationPlan, Effect};
use crate::preparation::{HalfPressMask, HalfPressParameters};
use crate::ports::HalfPressMode;
use crate::summary::AreaStatistics;
use crate::tests::{record, Call, Recorder};
use crate::Outcome;
use std::cell::Cell;
use std::rc::Rc;
use xm1_saclac_interface::{Completion, RecordHandle};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Input {
    built: Option<([u32; 2], u32)>,
    targets: Option<(u32, u16)>,
    resumed_at: Option<u32>,
    offsets: Option<(u16, u32, u16)>,
}

struct World {
    mode: u32,
    evaluation: u32,
    class: u32,
    repeat: u32,
    reuse_control: u32,
    skips_selection: bool,
    reuses_result: bool,
    bound: Option<(RecordHandle, u32, u32)>,
    block_record: Option<RecordHandle>,
    plan: Option<PlanWords>,
    selection: Option<u32>,
    requested: bool,
    input: Rc<Cell<Input>>,
    saved_input: Option<Input>,
    result: u32,
    retained: Option<u32>,
    stored_offset: Option<u16>,
    snapshot: LensSnapshot,
}

impl World {
    fn new() -> Self {
        Self {
            mode: 0,
            evaluation: 0x30,
            class: 0,
            repeat: 0,
            reuse_control: 0,
            skips_selection: false,
            reuses_result: false,
            bound: None,
            block_record: None,
            plan: None,
            selection: None,
            requested: false,
            input: Rc::default(),
            saved_input: None,
            result: 0x1234_5678,
            retained: None,
            stored_offset: None,
            snapshot: LensSnapshot::default(),
        }
    }

    fn snapshot_result(&self) -> u32 { u32::from_le_bytes(self.snapshot.tail[8..12].try_into().unwrap()) }

    fn input(&self) -> Input { self.input.get() }
}

/// The exposure context and calculation block of one call, taken from the world and given back.
struct Shot {
    mode: u32,
    evaluation: u32,
    class: u32,
    repeat: u32,
    bound: Option<(RecordHandle, u32, u32)>,
    block_record: Option<RecordHandle>,
    plan: Option<PlanWords>,
    selection: Option<u32>,
    requested: bool,
    input: Rc<Cell<Input>>,
    saved_input: Option<Input>,
    result: u32,
    retained: Option<u32>,
}

fn call<R>(world: &mut World, step: impl FnOnce(&mut Shot, &mut World) -> R) -> R {
    let mut shot = Shot {
        mode: world.mode,
        evaluation: world.evaluation,
        class: world.class,
        repeat: world.repeat,
        bound: world.bound,
        block_record: world.block_record,
        plan: world.plan,
        selection: world.selection,
        requested: world.requested,
        input: world.input.clone(),
        saved_input: world.saved_input,
        result: world.result,
        retained: world.retained,
    };
    let answer = step(&mut shot, world);
    world.repeat = shot.repeat;
    world.bound = shot.bound;
    world.block_record = shot.block_record;
    world.plan = shot.plan;
    world.selection = shot.selection;
    world.requested = shot.requested;
    world.saved_input = shot.saved_input;
    world.result = shot.result;
    world.retained = shot.retained;
    answer
}

impl<P> CalculationContext<P> for Shot {
    fn exposure(&mut self, word: ExposureWord) -> u32 {
        match word {
            ExposureWord::Mode => self.mode,
            ExposureWord::Class => self.class,
            ExposureWord::Profile => 0x50,
            ExposureWord::Evaluation => self.evaluation,
            ExposureWord::Refinement => 1,
            ExposureWord::Repeat => self.repeat,
        }
    }
    fn bind_exposure(&mut self, record: RecordHandle, lens_reference: u32, option: u32) { self.bound = Some((record, lens_reference, option)); }
    fn clear_repeat(&mut self) { self.repeat = 0; }
    fn retained_targets(&mut self) -> (u32, u16) { (0xaaaa, 0xbb) }
    fn set_input_targets(&mut self, word: u32, half: u16) { self.input.set(Input { targets: Some((word, half)), ..self.input.get() }); }
    fn save_input(&mut self) { self.saved_input = Some(self.input.get()); }
    fn resume_input(&mut self, lens_reference: u32, _layers: &mut P) {
        self.input.set(Input { resumed_at: Some(lens_reference), ..self.saved_input.unwrap_or_default() });
    }
    fn set_resume_offsets(&mut self, lead: u16, lens_word: u32, offset: u16) { self.input.set(Input { offsets: Some((lead, lens_word, offset)), ..self.input.get() }); }
}

impl CalculationBlock for Shot {
    fn bind_calculation(&mut self, record: RecordHandle) { self.block_record = Some(record); }
    fn reset_plan(&mut self, plan: PlanWords) { self.plan = Some(plan); }
    fn set_selection(&mut self, selection: u32) { self.selection = Some(selection); }
    fn mark_requested(&mut self) { self.requested = true; }
    fn restore_result(&mut self) { self.result = self.retained.unwrap_or(0); }
    fn retain_result(&mut self) { self.retained = Some(self.result); }
    fn result_word(&mut self) -> u32 { self.result }
}

impl CalculationLibrary for World {
    fn selection_mode(&mut self, _record: RecordHandle, refinement: u32) -> u32 { 0x100 + refinement }
    fn row_data(&mut self, kind: u32, mode: u32, evaluation: u32, limit: u32, option: u32) -> u32 {
        if [kind, mode, evaluation, limit, option] == [0x11, 0x101, 0x30, 0x12, 0x13] { 0x77 } else { 0 }
    }
    fn adjustment_status(&mut self, kind: u32, mode: u32, option: u32) -> u8 { if [kind, mode, option] == [0x14, 0x101, 0x15] { 6 } else { 3 } }
    fn class_skips_selection(&mut self, _class: u32) -> bool { self.skips_selection }
    fn display_class(&mut self, _record: RecordHandle, refinement: u32) -> u32 { 0x200 + refinement }
    fn selection_policy(&mut self, kind: u32, display: u32, evaluation: u32, option: u32) -> u32 {
        if [kind, display, evaluation] == [0x11, 0x201, 0x30] { 0x40 + option } else { 0xdead }
    }
    fn build_input(&mut self, area: [u32; 2], selection: u32) { self.input.set(Input { built: Some((area, selection)), ..Input::default() }); }
    fn class_reuses_result(&mut self, _class: u32) -> bool { self.reuses_result }
}

impl CalculationProfiles for World {
    fn profile(&mut self, _record: RecordHandle, value: CalculationProfile) -> u32 {
        match value {
            CalculationProfile::LensReference => 0x10,
            CalculationProfile::SelectionKind => 0x11,
            CalculationProfile::RowLimit => 0x12,
            CalculationProfile::RowOption => 0x13,
            CalculationProfile::AdjustmentKind => 0x14,
            CalculationProfile::AdjustmentOption => 0x15,
            CalculationProfile::ResumeLead => 0x16,
            CalculationProfile::ResumeOffset => 0x17,
            _ => 0,
        }
    }
    fn store_resume_offset(&mut self, _record: RecordHandle, offset: u16) { self.stored_offset = Some(offset); }
    fn reuse_control(&mut self) -> u32 { self.reuse_control }
    fn client_pair(&mut self, at: ClientRef) -> [u32; 2] { [at.0, at.0 + 1] }
}

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

fn parameters(mode: u32, mask: u32) -> HalfPressParameters {
    HalfPressParameters {
        mode: HalfPressMode(mode),
        option: 2,
        mask: HalfPressMask(mask),
        plan_option: 0,
        area_count: Some(ClientRef(0x900)),
        area_selection: 0,
        areas: None,
        lens_target: None,
        frame_slot: None,
        lens_reference: LensReference(0),
        ..HalfPressParameters::default()
    }
}

const NEW: u32 = 1 << 1;

#[test]
fn a_half_press_asking_for_a_new_calculation_requests_it_with_a_fresh_selection() {
    let mut work = CalculationWork::new();
    let mut world = World::new();
    assert_eq!(call(&mut world, |shot, world| work.plan(record(), &parameters(0, NEW), shot, world)), CalculationPlan::Request);
    assert_eq!(world.block_record, Some(record()));
    assert_eq!(world.bound, Some((record(), 0x10, 2)));
    assert_eq!(world.plan, Some(PlanWords { row_data: 0x77, adjustment: 6 }));
    assert_eq!(world.selection, Some(0x42));
    assert!(world.requested);
    assert_eq!(world.input().built, Some(([0x900, 0x901], 0x42)));
    assert_eq!(world.saved_input.and_then(|input| input.built), Some(([0x900, 0x901], 0x42)));
    assert_eq!(world.input().targets, None);
}

#[test]
fn without_a_new_calculation_the_retained_result_stands_with_the_last_selection() {
    let mut work = CalculationWork::new();
    let mut world = World::new();
    call(&mut world, |shot, world| work.plan(record(), &parameters(0, NEW), shot, world));
    let mut world = World::new();
    assert_eq!(call(&mut world, |shot, world| work.plan(record(), &parameters(0, 0), shot, world)), CalculationPlan::Skip);
    assert_eq!(world.selection, Some(0x42));
    assert!(!world.requested);
    assert_eq!(world.input().targets, Some((0xaaaa, 0xbb)));
    assert_eq!(world.saved_input, None);
}

#[test]
fn a_row_data_check_that_fails_leaves_no_adjustment() {
    let mut work = CalculationWork::new();
    let mut world = World { evaluation: 0x31, ..World::new() };
    call(&mut world, |shot, world| work.plan(record(), &parameters(0, NEW), shot, world));
    assert_eq!(world.plan, Some(PlanWords { row_data: 0, adjustment: 0 }));
}

#[test]
fn a_class_without_selection_selects_nothing() {
    let mut work = CalculationWork::new();
    let mut world = World::new();
    world.skips_selection = true;
    call(&mut world, |shot, world| work.plan(record(), &parameters(0, NEW), shot, world));
    assert_eq!(world.selection, Some(0));
}

#[test]
fn a_resumed_half_press_takes_the_saved_input_back_at_the_current_reference() {
    let mut work = CalculationWork::new();
    let mut world = World::new();
    call(&mut world, |shot, world| work.plan(record(), &parameters(0, NEW), shot, world));
    let saved = world.saved_input;
    let mut world = World { saved_input: saved, repeat: 1, ..World::new() };
    assert_eq!(call(&mut world, |shot, world| work.plan(record(), &parameters(7, NEW), shot, world)), CalculationPlan::Request);
    assert_eq!(world.selection, Some(0x42));
    assert!(!world.requested);
    assert_eq!(world.input().built, Some(([0x900, 0x901], 0x42)));
    assert_eq!(world.input().resumed_at, Some(0x10));
    assert_eq!(world.input().offsets, Some((0x16, 0x50, 0x17)));
    assert_eq!(world.stored_offset, Some(0x17));
    assert_eq!(world.repeat, 0);
}

#[test]
fn a_repeated_half_press_reuses_the_retained_result_unless_class_or_property_forbid_it() {
    for (reuses, control, plan) in [(false, 0, CalculationPlan::Skip), (true, 0, CalculationPlan::Request), (false, 4, CalculationPlan::Request)] {
        let mut work = CalculationWork::new();
        let mut world = World { repeat: 1, reuses_result: reuses, reuse_control: control, ..World::new() };
        assert_eq!(call(&mut world, |shot, world| work.plan(record(), &parameters(0, NEW), shot, world)), plan, "reuses {reuses} control {control}");
    }
}

#[test]
fn the_live_calculation_is_requested_with_an_empty_selection() {
    let mut work = CalculationWork::new();
    let mut world = World::new();
    call(&mut world, |shot, world| work.prepare_live(record(), shot, world));
    assert_eq!(world.bound, Some((record(), 0x10, 1)));
    assert_eq!(world.plan, Some(PlanWords::default()));
    assert_eq!(world.selection, Some(0));
    assert!(world.requested);
    assert_eq!(world.input().built, Some(([0, 0], 0)));
}

#[test]
fn a_result_reaches_the_lens_snapshot_and_is_retained_unless_resumed() {
    let mut work = CalculationWork::new();
    let mut world = World::new();
    call(&mut world, |shot, world| work.record_result(shot, world));
    assert_eq!(world.snapshot_result(), 0x1234_5678);
    assert_eq!(world.retained, Some(0x1234_5678));

    let mut world = World { mode: 7, ..World::new() };
    call(&mut world, |shot, world| work.record_result(shot, world));
    assert_eq!(world.snapshot_result(), 0x1234_5678);
    assert_eq!(world.retained, None);
}

#[test]
fn a_skipped_calculation_gives_the_record_the_retained_result() {
    let mut work = CalculationWork::new();
    let mut world = World { retained: Some(0x55), ..World::new() };
    call(&mut world, |shot, world| work.record_skipped(shot, world));
    assert_eq!(world.snapshot_result(), 0x55);
}

#[test]
fn only_a_requested_calculation_records_its_result_before_the_absorb() {
    let effects = |plan: CalculationPlan, branch: Branch| {
        let mut calculation = Calculation::new();
        let mut ports = Recorder::new();
        ports.plan = plan;
        calculation.start(branch, record(), &mut ports);
        ports.take();
        assert_eq!(calculation.completed(Completion::Calculation90, &mut ports), Outcome::Complete);
        ports.take()
    };
    let record_then_absorb = [Call::Effect(Effect::Calculation(CalculationEffect::RecordResult)), Call::Effect(Effect::Calculation(CalculationEffect::Absorb))];
    assert_eq!(effects(CalculationPlan::Request, Branch::HalfPress), record_then_absorb);
    assert_eq!(effects(CalculationPlan::Request, Branch::Live), record_then_absorb);
    assert_eq!(effects(CalculationPlan::Skip, Branch::HalfPress), [Call::Effect(Effect::Calculation(CalculationEffect::Absorb))]);
}
