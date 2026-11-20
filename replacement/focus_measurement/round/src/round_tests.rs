use crate::lanes::{Lane, Selection};
use crate::planner::{Cadence, Mode, Work};
use crate::round::{Environment, LaneSignal, LaneStatus, ParameterClient, RecordEffect, Round, SearchStart};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl, MeasurementOutcome, MeasurementUpdate, Restart, RetainedRoundStatus};
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, RecordHandle};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind { Initialize, Enable, Stop, Suspend, Resume, Work(Work), Command(CommandKind), Completed(Completion) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic { Signal(Lane, Kind), Effect(RecordEffect), Fault }

struct Env {
    traffic: Vec<Traffic>,
    answers: Vec<(Lane, Kind, LaneStatus)>,
    available: Option<Selection>,
    alternate: bool,
    adjustment_pending: bool,
}

impl Env {
    fn offering(available: Option<Selection>) -> Self {
        Self { traffic: Vec::new(), answers: Vec::new(), available, alternate: false, adjustment_pending: false }
    }
    fn answer(&mut self, lane: Lane, kind: Kind, status: LaneStatus) { self.answers.push((lane, kind, status)); }
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
    fn signals(&mut self) -> Vec<(Lane, Kind)> {
        self.take().into_iter().filter_map(|traffic| match traffic { Traffic::Signal(lane, kind) => Some((lane, kind)), _ => None }).collect()
    }
}

impl Environment for Env {
    fn signal(&mut self, lane: Lane, signal: LaneSignal<'_>, _: RecordHandle) -> LaneStatus {
        let kind = match signal {
            LaneSignal::Initialize => Kind::Initialize,
            LaneSignal::Enable => Kind::Enable,
            LaneSignal::Stop => Kind::Stop,
            LaneSignal::Suspend => Kind::Suspend,
            LaneSignal::Resume => Kind::Resume,
            LaneSignal::Work(work) => Kind::Work(work),
            LaneSignal::Command(command) => Kind::Command(command.kind),
            LaneSignal::Completed(completed) => Kind::Completed(completed.completion),
        };
        self.traffic.push(Traffic::Signal(lane, kind));
        if let Some(index) = self.answers.iter().position(|&(l, k, _)| l == lane && k == kind) {
            return self.answers.remove(index).2;
        }
        match kind {
            Kind::Work(_) => LaneStatus::Pending,
            Kind::Completed(_) => LaneStatus::Idle,
            _ => LaneStatus::Complete,
        }
    }
    fn available(&mut self, _: RecordHandle) -> Option<Selection> { self.available }
    fn search_permitted(&mut self) -> bool { true }
    fn alternate_search(&mut self, _: RecordHandle) -> bool { self.alternate }
    fn autofocus_geometry(&mut self, _: RecordHandle) -> bool { true }
    fn planning_mode(&mut self, _: RecordHandle) -> Mode { Mode::Default }
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: true, third: true, fourth: true } }
    fn record_effect(&mut self, _: RecordHandle, effect: RecordEffect) { self.traffic.push(Traffic::Effect(effect)); }
    fn fault(&mut self) { self.traffic.push(Traffic::Fault); }
    fn adjustment_pending(&mut self) -> bool { self.adjustment_pending }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }
fn control(control: MeasurementControl) -> ChildEvent<'static, MeasurementControl> {
    ChildEvent::Control { control, parameters: stock::parameters(&[]), record: record() }
}
fn command(kind: CommandKind) -> ChildEvent<'static, MeasurementControl> {
    ChildEvent::Command { command: Command { kind, parameters: stock::parameters(&[]) }, record: record() }
}
fn completion(completion: Completion) -> ChildEvent<'static, MeasurementControl> {
    ChildEvent::Completed { completed: Completed { completion, parameters: stock::parameters(&[]) }, record: record() }
}
/// Command 0x17's block: client, requested selection word, offered selection word.
fn parameter_change(words: [u32; 3]) -> Vec<u8> { words.iter().flat_map(|word| word.to_le_bytes()).collect() }
fn changing(bytes: &[u8]) -> ChildEvent<'_, MeasurementControl> {
    ChildEvent::Command { command: Command { kind: CommandKind::Command17, parameters: stock::parameters(bytes) }, record: record() }
}

const ACQUISITION: Selection = Selection { acquisition: true, search: false, auxiliary: false, image: false, source: false };

fn armed(available: Selection) -> (Round, Env) {
    let mut round = Round::new();
    let mut env = Env::offering(Some(available));
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Complete);
    env.take();
    (round, env)
}

#[test]
fn initialization_enables_the_offered_lanes_in_order_and_refuses_a_second_start() {
    let mut round = Round::new();
    let mut env = Env::offering(Some(ACQUISITION));
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Complete);
    assert_eq!(env.take(), [
        Traffic::Effect(RecordEffect::ResetSelection),
        Traffic::Effect(RecordEffect::ClearSelection),
        Traffic::Signal(Lane::Acquisition, Kind::Enable),
        Traffic::Signal(Lane::Search, Kind::Initialize),
        Traffic::Signal(Lane::Auxiliary, Kind::Initialize),
        Traffic::Signal(Lane::Image, Kind::Initialize),
        Traffic::Signal(Lane::Window, Kind::Enable),
        Traffic::Signal(Lane::Source, Kind::Initialize),
        Traffic::Signal(Lane::Bookkeeping, Kind::Initialize),
    ]);
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Fault);
    assert_eq!(env.take(), [Traffic::Fault]);
}

#[test]
fn a_pending_adjustment_skips_the_capture_buffer_detach() {
    // 0xf33094 writes the mask and clears the pair statuses, then returns before 0x756b74
    // (the shared group-20 +0x154 CAPTURE_BUFFER detach) while 0x016d8750 is 1
    // (`id5.dis:846183`). Root's detach is `RecordEffect::ResetSelection`.
    let mut round = Round::new();
    let mut env = Env::offering(Some(ACQUISITION));
    env.adjustment_pending = true;
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Complete);
    let traffic = env.take();
    assert!(!traffic.contains(&Traffic::Effect(RecordEffect::ResetSelection)));
    assert!(traffic.contains(&Traffic::Effect(RecordEffect::ClearSelection)));
    assert!(traffic.contains(&Traffic::Signal(Lane::Acquisition, Kind::Enable)));
}

#[test]
fn a_record_without_selection_initializes_every_lane_and_stays_idle() {
    let mut round = Round::new();
    let mut env = Env::offering(None);
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Complete);
    assert!(env.signals().iter().all(|&(_, kind)| kind == Kind::Initialize));
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Complete);
    assert!(env.take().is_empty());
    assert_eq!(round.handle(completion(Completion::Accessor4b), &mut env), ChildOutcome::Pending);
    assert!(env.take().is_empty());
}

#[test]
fn a_failed_lane_aborts_initialization_with_a_zero_status() {
    let mut round = Round::new();
    let mut env = Env::offering(Some(ACQUISITION));
    env.answer(Lane::Auxiliary, Kind::Initialize, LaneStatus::Refused);
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Fault);
    assert_eq!(env.signals().last(), Some(&(Lane::Auxiliary, Kind::Initialize)));
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Complete);
}

#[test]
fn the_accessor_tick_fills_both_work_slots_and_completions_join_the_round() {
    let (mut round, mut env) = armed(ACQUISITION);
    assert_eq!(round.handle(completion(Completion::Accessor4b), &mut env), ChildOutcome::Pending);
    let traffic = env.take();
    assert_eq!(traffic[0], Traffic::Effect(RecordEffect::CollectReady));
    let signals: Vec<_> = traffic.iter().filter_map(|t| match *t { Traffic::Signal(lane, kind) => Some((lane, kind)), _ => None }).collect();
    assert_eq!(signals, [(Lane::Window, Kind::Work(Work::Window)), (Lane::Acquisition, Kind::Work(Work::PrimaryAcquisition))]);

    assert_eq!(round.handle(completion(Completion::Accessor4b), &mut env), ChildOutcome::Pending);
    assert_eq!(env.take(), [Traffic::Effect(RecordEffect::SelectionHistory)]);

    assert_eq!(round.handle(completion(Completion::Calculation8d), &mut env), ChildOutcome::Pending);
    assert_eq!(env.signals(), [(Lane::Window, Kind::Completed(Completion::Calculation8d))]);
    assert_eq!(round.handle(completion(Completion::Calculation87), &mut env), ChildOutcome::Pending);
    assert_eq!(env.signals(), [(Lane::Acquisition, Kind::Completed(Completion::Calculation87))]);

    assert_eq!(round.handle(completion(Completion::Accessor4b), &mut env), ChildOutcome::Pending);
    assert_eq!(env.take()[0], Traffic::Effect(RecordEffect::CollectReady));
}

#[test]
fn a_lane_that_finishes_synchronously_lets_the_slot_take_the_next_work() {
    let all = Selection { acquisition: true, search: true, auxiliary: true, image: true, source: true };
    let (mut round, mut env) = armed(all);
    env.answer(Lane::Window, Kind::Work(Work::Window), LaneStatus::Idle);
    round.handle(completion(Completion::Accessor4b), &mut env);
    assert_eq!(env.signals(), [
        (Lane::Window, Kind::Work(Work::Window)),
        (Lane::Acquisition, Kind::Work(Work::PrimaryAcquisition)),
        (Lane::Search, Kind::Work(Work::Search)),
    ]);
}

#[test]
fn completions_outside_a_running_round_are_not_forwarded() {
    let (mut round, mut env) = armed(ACQUISITION);
    assert_eq!(round.handle(completion(Completion::Calculation87), &mut env), ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)));
    assert!(env.take().is_empty());
}

#[test]
fn stop_waits_for_a_pending_lane_and_completes_on_its_completion() {
    let (mut round, mut env) = armed(ACQUISITION);
    env.answer(Lane::Image, Kind::Stop, LaneStatus::Pending);
    assert_eq!(round.handle(control(MeasurementControl::Stop), &mut env), ChildOutcome::Pending);
    assert_eq!(env.signals().len(), 7);
    assert_eq!(round.handle(control(MeasurementControl::Reconfigure), &mut env), ChildOutcome::Fault);
    assert_eq!(env.take(), [Traffic::Fault]);
    env.answer(Lane::Image, Kind::Completed(Completion::Image47), LaneStatus::Complete);
    assert_eq!(round.handle(completion(Completion::Image47), &mut env), ChildOutcome::Complete);
    assert_eq!(round.handle(control(MeasurementControl::Initialize), &mut env), ChildOutcome::Complete);
}

#[test]
fn image_commands_reach_the_image_lane_only_while_measuring() {
    let mut round = Round::new();
    let mut env = Env::offering(Some(ACQUISITION));
    assert_eq!(round.handle(command(CommandKind::Command14), &mut env), ChildOutcome::Fault);
    assert_eq!(env.take(), [Traffic::Fault]);
    round.handle(control(MeasurementControl::Initialize), &mut env);
    env.take();
    env.answer(Lane::Image, Kind::Command(CommandKind::Command2b), LaneStatus::Pending);
    assert_eq!(round.handle(command(CommandKind::Command2b), &mut env), ChildOutcome::Pending);
    assert_eq!(env.signals(), [(Lane::Image, Kind::Command(CommandKind::Command2b))]);
}

#[test]
fn suspension_stops_search_unless_the_record_uses_the_alternate_search() {
    let (mut round, mut env) = armed(ACQUISITION);
    round.handle(control(MeasurementControl::Suspend), &mut env);
    assert_eq!(env.signals(), [(Lane::Search, Kind::Stop)]);
    env.alternate = true;
    round.handle(control(MeasurementControl::Suspend), &mut env);
    round.handle(control(MeasurementControl::Resume), &mut env);
    assert_eq!(env.signals(), [(Lane::Search, Kind::Suspend), (Lane::Search, Kind::Resume)]);
    env.alternate = false;
    round.handle(control(MeasurementControl::Resume), &mut env);
    assert_eq!(env.signals(), [(Lane::Search, Kind::Initialize)]);
}

#[test]
fn a_parameter_change_waits_for_the_round_then_restarts_with_the_update() {
    let source = Selection { acquisition: true, source: true, ..Selection::default() };
    let (mut round, mut env) = armed(source);
    round.handle(completion(Completion::Accessor4b), &mut env);
    env.take();

    let change = parameter_change([0xf0a0, 32, 0]);
    assert_eq!(round.handle(changing(&change), &mut env), ChildOutcome::Pending);
    assert!(env.take().is_empty());

    assert_eq!(round.handle(completion(Completion::Calculation8d), &mut env), ChildOutcome::Pending);
    assert_eq!(env.signals(), [(Lane::Window, Kind::Completed(Completion::Calculation8d)), (Lane::Source, Kind::Work(Work::Source))]);
    assert_eq!(round.handle(completion(Completion::Calculation9a), &mut env), ChildOutcome::Pending);
    env.take();
    env.answer(Lane::Acquisition, Kind::Completed(Completion::Calculation87), LaneStatus::Updated(MeasurementUpdate::NoPeerRequest));
    let outcome = round.handle(completion(Completion::Calculation87), &mut env);
    assert_eq!(outcome, ChildOutcome::Measurement(MeasurementOutcome::Update { update: MeasurementUpdate::NoPeerRequest, restart: Restart::Complete }));
    let traffic = env.take();
    let tail: Vec<_> = traffic.iter().copied().filter(|t| !matches!(t, Traffic::Effect(RecordEffect::Planned { .. }))).collect();
    assert_eq!(&tail[tail.len() - 3..], [
        Traffic::Effect(RecordEffect::ResetSelection),
        Traffic::Signal(Lane::Source, Kind::Stop),
        Traffic::Effect(RecordEffect::FinishSelectionClear(ParameterClient(stock::optional_callback(0xf0a0)))),
    ]);
}

#[test]
fn a_parameter_change_that_enables_search_announces_the_search_mode() {
    let (mut round, mut env) = armed(ACQUISITION);
    let change = parameter_change([0, 2, 2]);
    assert_eq!(round.handle(changing(&change), &mut env), ChildOutcome::Pending);
    assert_eq!(env.take(), [Traffic::Effect(RecordEffect::SearchMode(SearchStart::Search))]);
}

#[test]
fn a_parameter_change_with_nothing_to_change_completes_the_pending_clear() {
    let (mut round, mut env) = armed(ACQUISITION);
    assert_eq!(round.handle(changing(&parameter_change([0, 0, 0])), &mut env), ChildOutcome::Complete);
    assert_eq!(env.take(), [Traffic::Effect(RecordEffect::FinishSelectionClear(ParameterClient(None)))]);
    let mut dormant = Round::new();
    let change = parameter_change([0xf0a0, 1, 1]);
    assert_eq!(dormant.handle(changing(&change), &mut env), ChildOutcome::Complete);
    assert_eq!(env.take(), [Traffic::Effect(RecordEffect::FinishSelectionClear(ParameterClient(stock::optional_callback(0xf0a0))))]);
}

#[test]
fn reconfiguration_and_autofocus_send_geometry() {
    let mut round = Round::new();
    let mut env = Env::offering(None);
    assert_eq!(round.handle(control(MeasurementControl::Reconfigure), &mut env), ChildOutcome::Pending);
    assert_eq!(round.handle(command(CommandKind::Autofocus), &mut env), ChildOutcome::Complete);
    assert_eq!(env.take(), [Traffic::Effect(RecordEffect::Geometry), Traffic::Effect(RecordEffect::Geometry)]);
}
