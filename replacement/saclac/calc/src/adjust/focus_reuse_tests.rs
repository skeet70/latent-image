use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;

use crate::adjust::focus_reuse::*;
use crate::exposure::context::ExposureContext;
use crate::policy::SelectionKind;

#[derive(Clone, Copy)]
struct Current {
    kind: u32,
    selection: u8,
    compares_subjects: bool,
    armed: bool,
    scene: u32,
    follow: u16,
}

const CURRENT: Current = Current { kind: 3, selection: 0, compares_subjects: true, armed: true, scene: 12, follow: 0x30 };

impl ReuseRecord for Current {
    fn selection_kind(&self, _: Option<RecordHandle>) -> SelectionKind { SelectionKind(self.kind) }
    fn measurement_selection(&self, _: Option<RecordHandle>) -> u8 { self.selection }
    fn compares_subjects(&self, _: Option<RecordHandle>) -> bool { self.compares_subjects }
    fn callback_armed(&self, _: Option<RecordHandle>) -> bool { self.armed }
    fn scene_kind(&self, _: Option<RecordHandle>) -> u32 { self.scene }
    fn follow_word(&self, _: Option<RecordHandle>) -> u16 { self.follow }
}

struct Tolerance(u8);

impl ReuseSettings for Tolerance {
    fn level_tolerance(&self) -> u8 { self.0 }
}

#[derive(Debug, Eq, PartialEq)]
enum Dumped {
    Trace(SharedAddress, ReuseTrace),
    OutOfTolerance(SharedAddress),
}

#[derive(Default)]
struct Dump(Vec<Dumped>);

impl ReuseDump for Dump {
    fn trace(&mut self, frame: SharedAddress, trace: &ReuseTrace) { self.0.push(Dumped::Trace(frame, *trace)); }
    fn mark_out_of_tolerance(&mut self, frame: SharedAddress) { self.0.push(Dumped::OutOfTolerance(frame)); }
}

/// A context whose snapshot matches `CURRENT` in every compared value.
fn matching() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.binding.mask = 2;
    context.binding.option = 5;
    context.binding.subjects.count = 2;
    context.outcome.adjusting = true;
    let focus = &mut context.snapshot.focus;
    focus.adjustment = [1, 5, 12];
    focus.adjustment_kind = 0x30;
    focus.subject_count = 2;
    focus.limit = 100;
    focus.adjustment_level = 80;
    context
}

fn reuse(context: &ExposureContext, current: Current) -> bool {
    reuse_focus_result(context, &current, &Tolerance(3), &mut Dump::default())
}

#[test]
fn a_result_matching_the_current_record_is_reused() {
    assert!(reuse(&matching(), CURRENT));
    assert!(reuse(&matching(), Current { armed: false, scene: 0, follow: 0, ..CURRENT }));
}

#[test]
fn a_bound_mode_one_without_the_mask_bit_reuses_whatever_the_result() {
    let mut context = ExposureContext::default();
    context.binding.mode = 1;
    assert!(reuse(&context, CURRENT));
    context.binding.mask = 2;
    assert!(!reuse(&context, CURRENT));
}

#[test]
fn each_compared_value_must_match() {
    let changed: [fn(&mut ExposureContext); 8] = [
        |c| c.snapshot.focus.adjustment[0] = 0,
        |c| c.outcome.adjusting = false,
        |c| c.adjustment.state = 1,
        |c| c.snapshot.focus.adjustment[1] = 6,
        |c| c.binding.subjects.count = 3,
        |c| c.snapshot.focus.adjustment[2] = 13,
        |c| c.snapshot.focus.adjustment_kind = 0x31,
        |c| c.snapshot.focus.adjustment_level = 70,
    ];
    for change in changed {
        let mut context = matching();
        change(&mut context);
        assert!(!reuse(&context, CURRENT));
    }
}

#[test]
fn kind_six_ignores_the_option_and_unchecked_subjects_are_ignored() {
    let mut context = matching();
    context.snapshot.focus.adjustment[1] = 6;
    assert!(reuse(&context, Current { kind: 6, ..CURRENT }));
    context.binding.subjects.count = 9;
    assert!(reuse(&context, Current { kind: 6, compares_subjects: false, ..CURRENT }));
}

#[test]
fn a_kind_that_qualifies_for_a_fresh_evaluation_is_not_reused() {
    let mut context = matching();
    context.levels.unmeasured = true;
    assert!(!reuse(&context, Current { kind: 7, ..CURRENT }));
    assert!(reuse(&context, Current { kind: 7, selection: 1, ..CURRENT }));
    assert!(!reuse(&context, CURRENT));
    context.binding.option = 0;
    context.snapshot.focus.adjustment[1] = 0;
    assert!(reuse(&context, CURRENT));
}

#[test]
fn the_tolerance_is_measured_either_side_of_the_limit() {
    let mut context = matching();
    context.snapshot.focus.adjustment_level = 129;
    assert!(reuse(&context, CURRENT));
    context.snapshot.focus.adjustment_level = 130;
    assert!(!reuse(&context, CURRENT));
}

#[test]
fn a_bound_frame_records_the_trace_and_the_tolerance_miss() {
    let frame = SharedAddress(0x0200_0000);
    let mut context = matching();
    context.binding.frame = Some(frame);
    context.snapshot.focus.adjustment_level = 40;
    let mut dump = Dump::default();
    assert!(!reuse_focus_result(&context, &CURRENT, &Tolerance(3), &mut dump));
    let trace = ReuseTrace { adjustment: [1, 5, 12], adjustment_kind: 0x30, subject_count: 2, adjustment_level: 540, limit: 600, tolerance: 3 };
    assert_eq!(dump.0, [Dumped::Trace(frame, trace), Dumped::OutOfTolerance(frame)]);
}

#[test]
fn no_frame_no_dump() {
    let mut dump = Dump::default();
    let mut context = matching();
    context.snapshot.focus.adjustment_level = 40;
    reuse_focus_result(&context, &CURRENT, &Tolerance(3), &mut dump);
    assert!(dump.0.is_empty());
}
