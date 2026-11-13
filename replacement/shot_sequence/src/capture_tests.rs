use crate::ports::*;
use crate::tests::{record, Call, Recorder};
use crate::{Outcome, Shot};
use std::vec::Vec;
use xm1_saclac_controllers::children::{ShotControl, ShotEvent, ShotTarget as Target};
use xm1_saclac_interface::{stock, Command, CommandKind, Completion, CompletionStatus};

fn primary_readout(ports: &mut Recorder) {
    ports.answer(Query::Primary(PrimaryQuery::ImageFirst), true);
    ports.answer(Query::Primary(PrimaryQuery::ImageVariant), true);
    ports.answer(Query::Primary(PrimaryQuery::BlockReady), true);
    ports.answer(Query::Calculations(CalculationsQuery::Requested), true);
    for job in [CalculationsJob::Second, CalculationsJob::Third, CalculationsJob::Fourth] {
        ports.answer(Query::Calculations(CalculationsQuery::Enabled(job)), true);
    }
}

fn started(shot: &mut Shot, ports: &mut Recorder) {
    assert_eq!(shot.control(ShotControl::InitializeCapture, record(), ports), Outcome::Pending);
    assert_eq!(shot.completed(Target::Capture, Completion::Statistics83, record(), ports), Outcome::Pending);
}

fn calculation_requests(ports: &Recorder) -> Vec<CalculationsJob> {
    ports.calls.iter().filter_map(|call| match call { Call::Effect(Effect::Calculations(CalculationsEffect::Request(job))) => Some(*job), _ => None }).collect()
}

#[test]
fn capture_setup_then_primary_job_replies_job_started() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    primary_readout(&mut ports);
    shot.control(ShotControl::InitializeCapture, record(), &mut ports);
    assert_eq!(ports.take(), [
        Call::Effect(Effect::Capture(CaptureEffect::BindRecord(record()))),
        Call::Effect(Effect::Capture(CaptureEffect::RequestStatisticsSetup)),
    ]);
    shot.completed(Target::Capture, Completion::Statistics83, record(), &mut ports);
    assert!(ports.calls.contains(&Call::Effect(Effect::Primary(PrimaryEffect::Request))));
    assert_eq!(ports.replies(), [(CommandKind::Command07, 7)]);
    assert!(ports.lens_moves().is_empty());
}

#[test]
fn first_ready_block_runs_the_enabled_calculations_in_order() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    primary_readout(&mut ports);
    started(&mut shot, &mut ports);
    assert_eq!(shot.completed(Target::Capture, Completion::Image61, record(), &mut ports), Outcome::Pending);
    for completion in [Completion::Calculation97, Completion::Calculation98] {
        assert_eq!(shot.completed(Target::Capture, completion, record(), &mut ports), Outcome::Pending);
    }
    assert_eq!(calculation_requests(&ports), [CalculationsJob::Second, CalculationsJob::Third, CalculationsJob::Fourth]);
    ports.take();
    assert_eq!(shot.completed(Target::Capture, Completion::Calculation9a, record(), &mut ports), Outcome::Pending);
    assert!(ports.calls.contains(&Call::Effect(Effect::Primary(PrimaryEffect::PublishProgress))));
    ports.take();
    assert_eq!(shot.completed(Target::Capture, Completion::Image61, record(), &mut ports), Outcome::Pending);
    assert!(calculation_requests(&ports).is_empty());
}

#[test]
fn untracked_calculations_wait_without_requests() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    primary_readout(&mut ports);
    ports.answer(Query::Calculations(CalculationsQuery::Requested), false);
    started(&mut shot, &mut ports);
    shot.completed(Target::Capture, Completion::Image61, record(), &mut ports);
    assert!(calculation_requests(&ports).is_empty());
    assert_eq!(shot.completed(Target::Capture, Completion::Calculation97, record(), &mut ports), Outcome::Pending);
}

#[test]
fn capture_completion_finalizes_and_replies_success() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    primary_readout(&mut ports);
    started(&mut shot, &mut ports);
    shot.completed(Target::Capture, Completion::Image61, record(), &mut ports);
    for completion in [Completion::Calculation97, Completion::Calculation98, Completion::Calculation9a, Completion::Image60] {
        shot.completed(Target::Capture, completion, record(), &mut ports);
    }
    ports.take();
    assert_eq!(shot.completed(Target::Capture, Completion::Image5e, record(), &mut ports), Outcome::Complete);
    assert_eq!(ports.replies(), [(CommandKind::Command07, 0)]);
    let finalize = ports.calls.iter().position(|call| *call == Call::Effect(Effect::Capture(CaptureEffect::Finalize))).unwrap();
    let reply = ports.calls.iter().position(|call| matches!(call, Call::Reply(_))).unwrap();
    assert!(finalize < reply);
    assert_eq!(shot.control(ShotControl::InitializeCapture, record(), &mut ports), Outcome::Pending);
}

#[test]
fn capture_error_status_aborts_with_its_reply_and_no_success_reply() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    primary_readout(&mut ports);
    started(&mut shot, &mut ports);
    ports.take();
    ports.abort = Some(AbortStatus(CompletionStatus(13)));
    assert_eq!(shot.completed(Target::Capture, Completion::Image5e, record(), &mut ports), Outcome::Complete);
    assert_eq!(ports.replies(), [(CommandKind::Command07, 13)]);
    assert!(ports.calls.contains(&Call::Effect(Effect::Capture(CaptureEffect::Finalize))));
    assert!(!ports.calls.contains(&Call::Fault));
}

#[test]
fn lens_only_capture_completes_both_moves_itself_when_none_is_needed() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    started(&mut shot, &mut ports);
    let completions: Vec<_> = ports.calls.iter().filter_map(|call| match call { Call::SelfComplete(completion) => Some(*completion), _ => None }).collect();
    assert_eq!(completions, [Completion::Lens35, Completion::Lens3a]);
    assert!(ports.lens_moves().is_empty());
}

#[test]
fn lens_only_capture_requests_the_needed_relative_move() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.answer(Query::Primary(PrimaryQuery::NeedsRelative), true);
    started(&mut shot, &mut ports);
    assert_eq!(ports.lens_moves(), [LensMove::CaptureRelative]);
    ports.answer(Query::Primary(PrimaryQuery::NeedsRelative), false);
    ports.answer(Query::Primary(PrimaryQuery::NeedsAbsolute), true);
    shot.completed(Target::Capture, Completion::Lens35, record(), &mut ports);
    assert_eq!(ports.lens_moves(), [LensMove::CaptureRelative, LensMove::CaptureAbsolute]);
}

#[test]
fn lens_return_runs_before_the_job_and_reports_each_step() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    ports.answer(Query::Capture(CaptureQuery::LensReturn), true);
    ports.answer(Query::Return(ReturnQuery::NeedsCalculation), true);
    ports.answer(Query::Return(ReturnQuery::NeedsAbsoluteMove), true);
    shot.control(ShotControl::InitializeCapture, record(), &mut ports);
    assert_eq!(shot.completed(Target::Capture, Completion::Statistics83, record(), &mut ports), Outcome::Pending);
    assert!(ports.replies().is_empty());
    for completion in [Completion::Calculation90, Completion::Lens3a, Completion::Lens35] {
        assert_eq!(shot.completed(Target::Capture, completion, record(), &mut ports), Outcome::Pending);
    }
    assert_eq!(ports.lens_moves(), [LensMove::ReturnAbsolute, LensMove::ReturnRelative]);
    assert_eq!(ports.replies(), [(CommandKind::Command07, 7); 3]);
    assert!(ports.calls.contains(&Call::Effect(Effect::Primary(PrimaryEffect::Begin(record())))));
}

#[test]
fn idle_capture_answers_commands_and_runs_a_setting() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    assert_eq!(shot.command(Target::Capture, CommandKind::Command0c, record(), &mut ports), Outcome::Pending);
    assert_eq!(shot.command(Target::Capture, CommandKind::Command28, record(), &mut ports), Outcome::Pending);
    assert_eq!(shot.command(Target::Capture, CommandKind::Command08, record(), &mut ports), Outcome::Pending);
    assert_eq!(shot.command(Target::Capture, CommandKind::Command08, record(), &mut ports), Outcome::Fault);
    assert_eq!(shot.completed(Target::Capture, Completion::Statistics85, record(), &mut ports), Outcome::Complete);
    assert_eq!(ports.replies(), [(CommandKind::Command0c, 21), (CommandKind::Command28, 21), (CommandKind::Command08, 0)]);
}

#[test]
fn an_event_reaches_the_entry_its_target_names() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    let command = Command { kind: CommandKind::Command0c, parameters: stock::parameters(&[]) };
    assert_eq!(shot.handle(ShotEvent::Command { target: Target::HalfPress, command, record: record() }, &mut ports), Outcome::Unhandled);
    assert!(ports.calls.is_empty());
    assert_eq!(shot.handle(ShotEvent::Command { target: Target::Capture, command, record: record() }, &mut ports), Outcome::Pending);
    assert_eq!(ports.replies(), [(CommandKind::Command0c, 21)]);
}

#[test]
fn capture_commands_during_a_job_go_to_the_job() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    primary_readout(&mut ports);
    started(&mut shot, &mut ports);
    ports.take();
    assert_eq!(shot.command(Target::Capture, CommandKind::Command0c, record(), &mut ports), Outcome::Pending);
    assert_eq!(ports.take(), [Call::Effect(Effect::Primary(PrimaryEffect::RequestCommandC))]);
}

#[test]
fn statistics_7f_while_idle_is_absorbed_and_others_are_not() {
    let mut shot = Shot::new();
    let mut ports = Recorder::new();
    assert_eq!(shot.completed(Target::Capture, Completion::Statistics7f, record(), &mut ports), Outcome::Pending);
    assert_eq!(shot.completed(Target::Capture, Completion::Statistics7e, record(), &mut ports), Outcome::Unhandled);
    assert!(ports.calls.is_empty());
}
