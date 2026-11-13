use crate::ports::*;
use crate::{Outcome, Shot};
use std::collections::VecDeque;
use std::vec::Vec;
use xm1_saclac_controllers::children::{ShotControl, ShotTarget as Target};
use xm1_saclac_interface::{stock, BlockingRequest, CommandKind, Completed, Completion, CompletionStatus, LensEffect, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Call {
    Effect(Effect),
    Reply(Reply),
    Blocking(BlockingRequest),
    SelfComplete(Completion),
    Received(Completion),
    Fault,
}

pub(crate) struct Recorder {
    pub calls: Vec<Call>,
    pub answers: Vec<(Query, bool)>,
    pub mode: HalfPressMode,
    pub notifications: HalfPressNotifications,
    pub plan: CalculationPlan,
    pub evaluations: VecDeque<EvaluationIndex>,
    pub abort: Option<AbortStatus>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            calls: Vec::new(),
            answers: Vec::new(),
            mode: HalfPressMode(6),
            notifications: HalfPressNotifications { frame_released: false, relative_lens_done: false },
            plan: CalculationPlan::Request,
            evaluations: VecDeque::new(),
            abort: None,
        }
    }

    pub fn answer(&mut self, query: Query, value: bool) {
        self.answers.retain(|(known, _)| *known != query);
        self.answers.push((query, value));
    }

    pub fn take(&mut self) -> Vec<Call> { core::mem::take(&mut self.calls) }

    pub fn lens_moves(&self) -> Vec<LensMove> {
        self.calls.iter().filter_map(|call| match call { Call::Effect(Effect::Lens(lens)) => Some(*lens), _ => None }).collect()
    }

    pub fn replies(&self) -> Vec<(CommandKind, i32)> {
        self.calls.iter().filter_map(|call| match call { Call::Reply(reply) => Some((reply.command, reply.status.0)), _ => None }).collect()
    }
}

impl Ports for Recorder {
    fn effect(&mut self, effect: Effect) { self.calls.push(Call::Effect(effect)); }
    fn query(&mut self, query: Query) -> bool { self.answers.iter().find(|(known, _)| *known == query).is_some_and(|(_, value)| *value) }
    fn half_press_mode(&mut self) -> HalfPressMode { self.mode }
    fn half_press_notifications(&mut self) -> HalfPressNotifications { self.notifications }
    fn calculation_plan(&mut self, _record: RecordHandle) -> CalculationPlan { self.plan }
    fn next_evaluation(&mut self) -> EvaluationIndex { self.evaluations.pop_front().unwrap_or(EvaluationIndex::FINAL) }
    fn capture_abort(&mut self) -> Option<AbortStatus> { self.abort }
    fn blocking(&mut self, request: BlockingRequest) { self.calls.push(Call::Blocking(request)); }
    fn reply(&mut self, reply: Reply) { self.calls.push(Call::Reply(reply)); }
    fn self_complete(&mut self, completion: Completion) { self.calls.push(Call::SelfComplete(completion)); }
    fn receive(&mut self, completed: Completed<'_>) { self.calls.push(Call::Received(completed.completion)); }
    fn fault(&mut self) { self.calls.push(Call::Fault); }
}

pub(crate) fn record() -> RecordHandle { stock::record_handle_from_a2(0x16a_0000) }

fn s1_with_work(ports: &mut Recorder) {
    ports.answer(Query::HasWork, true);
    ports.answer(Query::Pipeline(PipelineQuery::FinishOnly), true);
    ports.answer(Query::Pipeline(PipelineQuery::ImageBusy), true);
}

/// Statistics accepted once, no evaluation, released: the exposure step completes on 0x4c.
fn exposure_completes(shot: &mut Shot, target: Target, ports: &mut Recorder) {
    ports.answer(Query::Exposure(ExposureQuery::StatisticsAccepted), true);
    assert_eq!(shot.completed(target, Completion::Image4d, record(), ports), Outcome::Pending);
    assert_eq!(shot.completed(target, Completion::Image4c, record(), ports), Outcome::Pending);
}

#[test]
fn a_completion_event_delivers_its_result_before_the_step_handles_it() {
    use xm1_saclac_controllers::children::ShotEvent;
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    ports.take();
    ports.answer(Query::Exposure(ExposureQuery::StatisticsAccepted), true);
    let completed = Completed { completion: Completion::Image4d, parameters: stock::parameters(&[4]) };
    assert_eq!(shot.handle(ShotEvent::Completed { target: Target::HalfPress, completed, record: record() }, &mut ports), Outcome::Pending);
    assert_eq!(ports.calls.first(), Some(&Call::Received(Completion::Image4d)));
    assert!(ports.calls.contains(&Call::Effect(Effect::Exposure(ExposureEffect::AbsorbStatistics))));
}

#[test]
fn half_press_mode_zero_synchronizes_adjustment_then_lens_before_preparing() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.mode = HalfPressMode(0);
    s1_with_work(&mut ports);
    assert_eq!(shot.control(ShotControl::StartS1, record(), &mut ports), Outcome::Pending);
    let position = |wanted: Call| ports.calls.iter().position(|call| *call == wanted).unwrap();
    let adjustment = position(Call::Blocking(BlockingRequest::AdjustmentSynchronize));
    let lens = position(Call::Blocking(BlockingRequest::LensSynchronize));
    let prepare = position(Call::Effect(Effect::Sequence(SequenceEffect::PrepareRecords(Branch::HalfPress))));
    assert!(adjustment < lens && lens < prepare);
}

#[test]
fn half_press_lens_synchronization_follows_mode_and_record() {
    for (mode, skip, adjustment, lens) in [(6, false, false, false), (5, false, true, false), (7, false, true, false), (0, true, true, false)] {
        let mut shot = Shot::new();
        let mut ports = Recorder::new();
        ports.mode = HalfPressMode(mode);
        ports.answer(Query::SkipLensSynchronization, skip);
        s1_with_work(&mut ports);
        shot.control(ShotControl::StartS1, record(), &mut ports);
        assert_eq!(ports.calls.contains(&Call::Blocking(BlockingRequest::AdjustmentSynchronize)), adjustment, "mode {mode}");
        assert_eq!(ports.calls.contains(&Call::Blocking(BlockingRequest::LensSynchronize)), lens, "mode {mode}");
    }
}

#[test]
fn half_press_without_work_replies_at_once() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    assert_eq!(shot.control(ShotControl::StartS1, record(), &mut ports), Outcome::Complete);
    assert_eq!(ports.replies(), [(CommandKind::HalfPress, 0)]);
    assert!(ports.lens_moves().is_empty());
    assert_eq!(shot.control(ShotControl::StartS1, record(), &mut ports), Outcome::Complete);
}

#[test]
fn half_press_runs_exposure_calculation_and_lens_finish_before_its_reply() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    s1_with_work(&mut ports);
    assert_eq!(shot.control(ShotControl::StartS1, record(), &mut ports), Outcome::Pending);
    assert!(ports.calls.contains(&Call::Effect(Effect::Exposure(ExposureEffect::RequestStatistics))));
    exposure_completes(&mut shot, Target::HalfPress, &mut ports);
    assert!(ports.calls.contains(&Call::Effect(Effect::Calculation(CalculationEffect::Request))));
    ports.take();
    assert_eq!(shot.completed(Target::HalfPress, Completion::Calculation90, record(), &mut ports), Outcome::Pending);
    assert_eq!(ports.lens_moves(), [LensMove::FinishAbsolute, LensMove::FinishRelative]);
    assert!(ports.replies().is_empty());
    assert_eq!(shot.completed(Target::HalfPress, Completion::Lens35, record(), &mut ports), Outcome::Pending);
    assert!(ports.replies().is_empty());
    assert_eq!(shot.completed(Target::HalfPress, Completion::Lens3a, record(), &mut ports), Outcome::Complete);
    assert_eq!(ports.replies(), [(CommandKind::HalfPress, 0)]);
    assert!(!ports.calls.contains(&Call::Fault));
}

#[test]
fn relative_finish_move_first_when_the_lens_asks_for_it() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    s1_with_work(&mut ports);
    ports.answer(Query::Finish(FinishQuery::RelativeFirst), true);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    exposure_completes(&mut shot, Target::HalfPress, &mut ports);
    shot.completed(Target::HalfPress, Completion::Calculation90, record(), &mut ports);
    assert_eq!(ports.lens_moves(), [LensMove::FinishRelative, LensMove::FinishAbsolute]);
}

#[test]
fn mode_one_completes_the_relative_move_itself() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.mode = HalfPressMode(1);
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    exposure_completes(&mut shot, Target::HalfPress, &mut ports);
    ports.take();
    shot.completed(Target::HalfPress, Completion::Calculation90, record(), &mut ports);
    assert_eq!(ports.lens_moves(), [LensMove::FinishAbsolute]);
    assert!(ports.calls.contains(&Call::SelfComplete(Completion::Lens35)));
}

#[test]
fn skipped_half_press_calculation_completes_itself() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.plan = CalculationPlan::Skip;
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    exposure_completes(&mut shot, Target::HalfPress, &mut ports);
    assert!(ports.calls.contains(&Call::SelfComplete(Completion::Calculation90)));
    assert!(!ports.calls.contains(&Call::Effect(Effect::Calculation(CalculationEffect::Request))));
}

#[test]
fn relative_done_notification_replies_status_six_before_completion() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.notifications.relative_lens_done = true;
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    exposure_completes(&mut shot, Target::HalfPress, &mut ports);
    shot.completed(Target::HalfPress, Completion::Calculation90, record(), &mut ports);
    shot.completed(Target::HalfPress, Completion::Lens35, record(), &mut ports);
    assert_eq!(ports.replies(), [(CommandKind::HalfPress, 6)]);
    shot.completed(Target::HalfPress, Completion::Lens3a, record(), &mut ports);
    assert_eq!(ports.replies(), [(CommandKind::HalfPress, 6), (CommandKind::HalfPress, 0)]);
}

#[test]
fn two_pass_exposure_runs_exposure_and_calculation_twice() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.mode = HalfPressMode(5);
    ports.answer(Query::TwoPassExposure, true);
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    for _ in 0..2 {
        exposure_completes(&mut shot, Target::HalfPress, &mut ports);
        assert_eq!(shot.completed(Target::HalfPress, Completion::Calculation90, record(), &mut ports), Outcome::Pending);
    }
    let passes: Vec<_> = ports.calls.iter().filter_map(|call| match call { Call::Effect(Effect::Sequence(SequenceEffect::SetRefinementPass(pass))) => Some(*pass), _ => None }).collect();
    assert_eq!(passes, [false, true, false]);
    assert_eq!(ports.lens_moves(), [LensMove::FinishAbsolute, LensMove::FinishRelative]);
}

#[test]
fn exposure_evaluates_until_the_calculation_has_no_index() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    s1_with_work(&mut ports);
    ports.evaluations.extend([EvaluationIndex(3), EvaluationIndex(4)]);
    ports.answer(Query::Exposure(ExposureQuery::StatisticsAccepted), true);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    shot.completed(Target::HalfPress, Completion::Image4d, record(), &mut ports);
    shot.completed(Target::HalfPress, Completion::Calculation8e, record(), &mut ports);
    shot.completed(Target::HalfPress, Completion::Image4c, record(), &mut ports);
    let evaluations: Vec<_> = ports.calls.iter().filter_map(|call| match call { Call::Effect(Effect::Exposure(ExposureEffect::RequestEvaluation(index))) => Some(index.0), _ => None }).collect();
    assert_eq!(evaluations, [3, 4]);
    assert_eq!(shot.completed(Target::HalfPress, Completion::Calculation8e, record(), &mut ports), Outcome::Pending);
    assert!(!ports.calls.contains(&Call::Effect(Effect::Calculation(CalculationEffect::Request))));
    assert_eq!(shot.completed(Target::HalfPress, Completion::Image4c, record(), &mut ports), Outcome::Pending);
    assert!(ports.calls.contains(&Call::Effect(Effect::Calculation(CalculationEffect::Request))));
}

#[test]
fn autofocus_during_half_press_faults_and_while_idle_replies() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    assert_eq!(shot.command(Target::HalfPress, CommandKind::Autofocus, record(), &mut ports), Outcome::Pending);
    assert_eq!(ports.replies(), [(CommandKind::Autofocus, 0)]);
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    assert_eq!(shot.command(Target::HalfPress, CommandKind::Autofocus, record(), &mut ports), Outcome::Fault);
    assert_eq!(ports.calls.last(), Some(&Call::Fault));
}

#[test]
fn completions_for_an_idle_shot_are_not_consumed() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    for target in [Target::HalfPress, Target::Live, Target::Capture] {
        assert_eq!(shot.completed(target, Completion::Calculation90, record(), &mut ports), Outcome::Unhandled);
    }
    assert!(ports.calls.iter().all(|call| matches!(call, Call::Effect(Effect::Sequence(SequenceEffect::NoteLiveRecord(_))))));
}

#[test]
fn a_completion_the_running_step_does_not_expect_faults() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    assert_eq!(shot.completed(Target::HalfPress, Completion::Calculation8f, record(), &mut ports), Outcome::Fault);
    assert_eq!(shot.completed(Target::HalfPress, Completion::Statistics83, record(), &mut ports), Outcome::Unhandled);
}

#[test]
fn second_half_press_start_while_running_faults() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    s1_with_work(&mut ports);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    assert_eq!(shot.control(ShotControl::StartS1, record(), &mut ports), Outcome::Fault);
}

#[test]
fn live_sequence_finishes_with_an_absolute_move_only() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    assert_eq!(shot.control(ShotControl::StartLiveSequence, record(), &mut ports), Outcome::Pending);
    assert!(!ports.calls.iter().any(|call| matches!(call, Call::Blocking(_))));
    exposure_completes(&mut shot, Target::Live, &mut ports);
    assert!(ports.calls.contains(&Call::Effect(Effect::Calculation(CalculationEffect::RequestLive(record())))));
    assert_eq!(shot.completed(Target::Live, Completion::Calculation90, record(), &mut ports), Outcome::Pending);
    assert_eq!(ports.lens_moves(), [LensMove::FinishAbsolute]);
    ports.take();
    assert_eq!(shot.completed(Target::Live, Completion::Lens3a, record(), &mut ports), Outcome::Complete);
    assert_eq!(ports.calls.last(), Some(&Call::Effect(Effect::Sequence(SequenceEffect::ReleaseLiveResources))));
    assert!(ports.replies().is_empty());
}

#[test]
fn relative_completion_before_absolute_reports_progress() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    shot.control(ShotControl::StartLiveSequence, record(), &mut ports);
    exposure_completes(&mut shot, Target::Live, &mut ports);
    shot.completed(Target::Live, Completion::Calculation90, record(), &mut ports);
    assert_eq!(shot.completed(Target::Live, Completion::Lens35, record(), &mut ports), Outcome::Progress);
    assert_eq!(shot.completed(Target::Live, Completion::Lens3a, record(), &mut ports), Outcome::Complete);
}

#[test]
fn advance_during_the_raw_request_finishes_after_the_raw_completion() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.answer(Query::HasWork, true);
    ports.answer(Query::Pipeline(PipelineQuery::ImageBusy), true);
    ports.answer(Query::Pipeline(PipelineQuery::FrameReady), true);
    ports.answer(Query::Pipeline(PipelineQuery::NeedsRaw), true);
    shot.control(ShotControl::StartS1, record(), &mut ports);
    assert_eq!(shot.control(ShotControl::AdvanceS1, record(), &mut ports), Outcome::Pending);
    assert!(ports.calls.contains(&Call::Effect(Effect::Pipeline(PipelineEffect::MarkRestart))));
    exposure_completes(&mut shot, Target::HalfPress, &mut ports);
    ports.take();
    shot.completed(Target::HalfPress, Completion::Calculation90, record(), &mut ports);
    assert_eq!(ports.calls.iter().filter(|call| **call == Call::Effect(Effect::Pipeline(PipelineEffect::Request(PipelineRequest::Image1d)))).count(), 1);
    assert_eq!(shot.control(ShotControl::AdvanceS1, record(), &mut ports), Outcome::Pending);
    ports.take();
    assert_eq!(shot.completed(Target::HalfPress, Completion::Image4e, record(), &mut ports), Outcome::Pending);
    assert!(ports.calls.contains(&Call::Effect(Effect::Pipeline(PipelineEffect::ClearFrameReady))));
    assert_eq!(ports.lens_moves(), [LensMove::FinishAbsolute, LensMove::FinishRelative]);
}

#[test]
fn every_lens_move_and_the_half_press_reply_need_the_lens_safety_contract() {
    for lens in [LensMove::FinishRelative, LensMove::FinishAbsolute, LensMove::CaptureRelative, LensMove::CaptureAbsolute, LensMove::ReturnRelative, LensMove::ReturnAbsolute] {
        assert!(Effect::Lens(lens).lens_effect().requires_lens_safety_contract());
    }
    assert!(blocking_lens_effect(BlockingRequest::LensSynchronize).requires_lens_safety_contract());
    assert!(!blocking_lens_effect(BlockingRequest::AdjustmentSynchronize).requires_lens_safety_contract());
    let half_press = Reply { command: CommandKind::HalfPress, record: None, status: CompletionStatus(0) };
    assert!(half_press.lens_effect().requires_lens_safety_contract());
    assert_eq!(Reply { command: CommandKind::Command07, ..half_press }.lens_effect(), LensEffect::NoLensDrive);
    assert_eq!(Effect::Pipeline(PipelineEffect::Request(PipelineRequest::Image1d)).lens_effect(), LensEffect::NoLensDrive);
}
