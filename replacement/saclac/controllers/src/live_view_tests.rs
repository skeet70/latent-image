use crate::children::{ChildEvent, ChildOutcome, MeasurementControl, MeasurementOutcome, MeasurementUpdate, Restart, ShotControl, ShotEvent};
use crate::live_view::*;
use crate::root::Outcome;
use std::{collections::VecDeque, vec, vec::Vec};
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completed, Completion as C, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seen {
    Statistics(Stat),
    Measurement(Meas),
    Sequence(Seq),
    Prepare(Option<K>),
    Apply(K),
    View(ViewRequest),
    Effect(Effect),
    Condition(Condition),
    Motion,
    Reply(Option<K>, ReplyStatus),
    Fault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stat { Control(StatisticsControl), Forward, Reissue(K) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Meas { Control(MeasurementControl), Command(K), Completed(C) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seq { Start, Forward }

struct Peers {
    seen: Vec<Seen>,
    statistics: VecDeque<StatisticsOutcome>,
    measurement: VecDeque<ChildOutcome>,
    sequence: VecDeque<Outcome>,
    applications: VecDeque<Application>,
    preparation: Preparation,
    true_conditions: Vec<Condition>,
}

impl Peers {
    fn new() -> Self {
        Self {
            seen: Vec::new(),
            statistics: VecDeque::new(),
            measurement: VecDeque::new(),
            sequence: VecDeque::new(),
            applications: VecDeque::new(),
            preparation: Preparation::default(),
            true_conditions: vec![Condition::DirectStart],
        }
    }
    fn take(&mut self) -> Vec<Seen> { core::mem::take(&mut self.seen) }
}

impl Port for Peers {
    fn statistics(&mut self, call: StatisticsCall<'_>, record: RecordHandle) -> StatisticsOutcome {
        assert_eq!(record, self::record());
        self.seen.push(Seen::Statistics(match call {
            StatisticsCall::Control(control) => Stat::Control(control),
            StatisticsCall::Forward(_) => Stat::Forward,
            StatisticsCall::Reissue(kind) => Stat::Reissue(kind),
        }));
        self.statistics.pop_front().unwrap_or(StatisticsOutcome::Complete)
    }
    fn measurement(&mut self, event: ChildEvent<'_, MeasurementControl>) -> ChildOutcome {
        let (seen, record) = match event {
            ChildEvent::Control { control, record, .. } => (Meas::Control(control), record),
            ChildEvent::Command { command, record } => (Meas::Command(command.kind), record),
            ChildEvent::Completed { completed, record } => (Meas::Completed(completed.completion), record),
        };
        assert_eq!(record, self::record());
        self.seen.push(Seen::Measurement(seen));
        self.measurement.pop_front().unwrap_or(ChildOutcome::Complete)
    }
    fn sequence(&mut self, event: ShotEvent<'_>) -> Outcome {
        self.seen.push(Seen::Sequence(match event {
            ShotEvent::Control { control: ShotControl::StartLiveSequence, .. } => Seq::Start,
            _ => Seq::Forward,
        }));
        self.sequence.pop_front().unwrap_or(Outcome::Complete)
    }
    fn prepare(&mut self, command: Option<Command<'_>>, _: RecordHandle) -> Preparation {
        self.seen.push(Seen::Prepare(command.map(|command| command.kind)));
        self.preparation
    }
    fn apply(&mut self, command: Command<'_>, _: RecordHandle) -> Application {
        self.seen.push(Seen::Apply(command.kind));
        self.applications.pop_front().unwrap_or(Application::Applied)
    }
    fn view(&mut self, request: ViewRequest, _: RecordHandle) { self.seen.push(Seen::View(request)) }
    fn effect(&mut self, effect: Effect, _: RecordHandle) { self.seen.push(Seen::Effect(effect)) }
    fn condition(&mut self, condition: Condition, _: Option<Command<'_>>, _: RecordHandle) -> bool {
        self.seen.push(Seen::Condition(condition));
        self.true_conditions.contains(&condition)
    }
    fn motion_setting(&mut self) -> MotionSetting { self.seen.push(Seen::Motion); MotionSetting(0x33) }
    fn reply(&mut self, command: Option<K>, status: ReplyStatus, _: RecordHandle) { self.seen.push(Seen::Reply(command, status)) }
    fn fault(&mut self) { self.seen.push(Seen::Fault) }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16a0_0490) }

fn command(kind: K, parameters: &'static [u8]) -> Signal<'static> {
    Signal::Command(Command { kind, parameters: stock::parameters(parameters) })
}

fn completed(completion: C) -> Signal<'static> {
    Signal::Completed(Completed { completion, parameters: stock::parameters(&[0, 0, 0, 0]) })
}

/// Command02 parameters whose fifth word asks for a measured start (zero) or not.
const PRIMARY_MEASURED: &[u8] = &[0; 20];
const PRIMARY_STILL: &[u8] = &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0];

struct Live { controller: Controller, preparation: PreparationOverride }

impl Live {
    fn new() -> Self { Self { controller: Controller::new(), preparation: PreparationOverride::None } }
    fn send(&mut self, signal: Signal<'_>, peers: &mut Peers) -> Outcome {
        self.controller.handle(signal, record(), &mut self.preparation, peers)
    }
}

/// Brings the view up for live measurement and returns it running.
fn running(peers: &mut Peers) -> Live {
    let mut live = Live::new();
    assert_eq!(live.send(command(K::Command02, PRIMARY_MEASURED), peers), Outcome::Pending);
    assert_eq!(live.send(completed(C::Image3f), peers), Outcome::Pending);
    assert_eq!(live.send(completed(C::Image41), peers), Outcome::Pending);
    assert_eq!(live.send(completed(C::Accessor4a), peers), Outcome::Complete);
    peers.take();
    live
}

#[test]
fn idle_accessor_tick_is_consumed_without_starting_measurement() {
    let mut peers = Peers::new();
    let mut live = Live::new();
    assert_eq!(live.send(completed(C::Accessor4b), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), []);
}

#[test]
fn resumed_live_services_initialize_measurement_on_the_first_accessor_tick() {
    let mut peers = Peers::new();
    peers.measurement.extend([ChildOutcome::Complete, ChildOutcome::Pending]);
    let mut live = Live::new();
    assert_eq!(live.send(Signal::ResumeLive, &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), []);
    assert_eq!(live.send(completed(C::Accessor4b), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), [
        Seen::Statistics(Stat::Control(StatisticsControl::BindRunning)),
        Seen::Measurement(Meas::Control(MeasurementControl::Initialize)),
        Seen::Measurement(Meas::Completed(C::Accessor4b)),
    ]);
}

#[test]
fn primary_profile_starts_the_view_then_configures_measurement_and_statistics() {
    let mut peers = Peers::new();
    let mut live = Live::new();
    assert_eq!(live.send(command(K::Command02, PRIMARY_MEASURED), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Prepare(Some(K::Command02)), Seen::Apply(K::Command02), Seen::Condition(Condition::DirectStart),
        Seen::Effect(Effect::Hold), Seen::View(ViewRequest::Start { measured: true }),
    ]);
    assert_eq!(live.send(completed(C::Image3f), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Effect(Effect::StoreAdjustment), Seen::Measurement(Meas::Control(MeasurementControl::Initialize)),
        Seen::Effect(Effect::BeginRecordMode), Seen::View(ViewRequest::Configure(MotionSetting(0))),
    ]);
    assert_eq!(live.send(completed(C::Image41), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![]);
    assert_eq!(live.send(completed(C::Accessor4a), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![
        Seen::Statistics(Stat::Control(StatisticsControl::Begin)), Seen::Condition(Condition::SelectionPending),
        Seen::Reply(Some(K::Command02), ReplyStatus::Accepted), Seen::Effect(Effect::ReleaseHold),
    ]);
    assert_eq!(live.send(command(K::HalfRelease, &[]), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Prepare(Some(K::HalfRelease)), Seen::Reply(Some(K::HalfRelease), ReplyStatus::Accepted)]);
}

#[test]
fn a_still_start_replies_once_the_view_shows_without_measurement() {
    let mut peers = Peers::new();
    peers.true_conditions.clear();
    let mut live = Live::new();
    assert_eq!(live.send(command(K::Command02, PRIMARY_STILL), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Prepare(Some(K::Command02)), Seen::Apply(K::Command02), Seen::Condition(Condition::DirectStart),
        Seen::Effect(Effect::Hold), Seen::View(ViewRequest::Refresh(MotionSetting(0))),
    ]);
    assert_eq!(live.send(completed(C::Image40), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Effect(Effect::ReleaseHold), Seen::Reply(Some(K::Command02), ReplyStatus::Accepted)]);
    assert_eq!(live.send(Signal::Suspend, &mut peers), Outcome::Complete, "idle again");
}

#[test]
fn the_live_sequence_runs_on_the_started_view_before_configuration() {
    let mut peers = Peers::new();
    peers.preparation.sequence = true;
    peers.sequence.extend([Outcome::Pending, Outcome::Pending, Outcome::Complete]);
    let mut live = Live::new();
    live.send(command(K::Command02, PRIMARY_MEASURED), &mut peers);
    peers.take();
    assert_eq!(live.send(completed(C::Image3f), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Effect(Effect::QueryRecordMode), Seen::Sequence(Seq::Start)]);
    assert_eq!(live.send(completed(C::Calculation90), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Sequence(Seq::Forward)]);
    assert_eq!(live.send(completed(C::Image4c), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Sequence(Seq::Forward), Seen::Effect(Effect::StoreAdjustment),
        Seen::Measurement(Meas::Control(MeasurementControl::Initialize)), Seen::Effect(Effect::BeginRecordMode),
        Seen::View(ViewRequest::Configure(MotionSetting(0))),
    ]);
}

#[test]
fn suspending_live_stops_both_children_then_the_view_and_replies_when_it_stopped() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    assert_eq!(live.send(Signal::SuspendLive, &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Prepare(None), Seen::Statistics(Stat::Control(StatisticsControl::Stop)),
        Seen::Measurement(Meas::Control(MeasurementControl::Stop)), Seen::Motion, Seen::View(ViewRequest::Stop(StopMode::Plain)),
    ]);
    assert_eq!(live.send(completed(C::Image42), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Effect(Effect::FinishRecordMode), Seen::Reply(None, ReplyStatus::Accepted)]);
}

#[test]
fn the_view_stops_only_after_a_pending_child_reports_its_stop() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.statistics.push_back(StatisticsOutcome::Pending);
    assert_eq!(live.send(Signal::SuspendLive, &mut peers), Outcome::Pending);
    assert!(!peers.take().iter().any(|seen| matches!(seen, Seen::View(_))));
    assert_eq!(live.send(completed(C::Statistics71), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Statistics(Stat::Forward), Seen::Motion, Seen::View(ViewRequest::Stop(StopMode::Plain))]);
}

#[test]
fn a_secondary_profile_stops_the_view_with_its_options_and_restores_it() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.preparation.stop = StopReason::SecondaryProfile;
    let negative_first: &'static [u8] = &[0, 0, 0, 0, 1, 0, 0, 0x80];
    assert_eq!(live.send(command(K::Command03, negative_first), &mut peers), Outcome::Pending);
    let options = RestoreOptions { first: true, second: false, negative: true };
    assert_eq!(peers.take(), vec![
        Seen::Apply(K::Command03), Seen::Prepare(Some(K::Command03)), Seen::Statistics(Stat::Control(StatisticsControl::Pause)),
        Seen::Measurement(Meas::Control(MeasurementControl::Stop)), Seen::Motion, Seen::View(ViewRequest::Stop(StopMode::SecondaryNegative)),
    ]);
    assert_eq!(live.send(completed(C::Image42), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Effect(Effect::FinishRecordMode), Seen::View(ViewRequest::Restore(options))]);
    assert_eq!(live.send(completed(C::Image40), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Reply(Some(K::Command03), ReplyStatus::Accepted)]);
}

#[test]
fn a_secondary_profile_without_options_goes_to_statistics_while_a_selection_is_active() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.statistics.push_back(StatisticsOutcome::Pending);
    peers.true_conditions.push(Condition::SelectionActive);
    assert_eq!(live.send(command(K::Command03, &[0; 8]), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Apply(K::Command03), Seen::Condition(Condition::SelectionActive), Seen::Statistics(Stat::Forward)]);
    assert_eq!(live.send(completed(C::Statistics82), &mut peers), Outcome::Complete);
    assert_eq!(live.send(command(K::HalfRelease, &[]), &mut peers), Outcome::Complete, "running again");
}

#[test]
fn a_pause_suspends_measurement_then_the_view_and_its_end_resumes_both() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    assert_eq!(live.send(Signal::Suspend, &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Measurement(Meas::Control(MeasurementControl::Suspend)), Seen::View(ViewRequest::Suspend)]);
    assert_eq!(live.send(completed(C::Image43), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Condition(Condition::StatisticsRetune)]);
    assert_eq!(live.send(Signal::Resume, &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Condition(Condition::StatisticsRetune), Seen::Measurement(Meas::Control(MeasurementControl::Resume)),
        Seen::View(ViewRequest::Resume),
    ]);
    assert_eq!(live.send(completed(C::Image44), &mut peers), Outcome::Complete);
}

#[test]
fn a_statistics_retune_precedes_the_resume_when_the_statistics_record_asks_for_it() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.statistics.push_back(StatisticsOutcome::Pending);
    peers.true_conditions.push(Condition::StatisticsRetune);
    assert_eq!(live.send(Signal::Resume, &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Condition(Condition::StatisticsRetune), Seen::Statistics(Stat::Control(StatisticsControl::Resume))]);
    assert_eq!(live.send(completed(C::Statistics6e), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Statistics(Stat::Forward), Seen::Measurement(Meas::Control(MeasurementControl::Resume)), Seen::View(ViewRequest::Resume),
    ]);
}

#[test]
fn measurement_window_updates_reach_statistics_while_running() {
    let update = ChildOutcome::Measurement(MeasurementOutcome::Update { update: MeasurementUpdate::Source, restart: Restart::Pending });
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.measurement.push_back(update);
    peers.statistics.push_back(StatisticsOutcome::Pending);
    assert_eq!(live.send(completed(C::Calculation8b), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Measurement(Meas::Completed(C::Calculation8b)),
        Seen::Statistics(Stat::Control(StatisticsControl::Measured(MeasurementUpdate::Source))),
    ]);
}

#[test]
fn a_refused_command17_is_answered_without_reaching_measurement() {
    let mut peers = Peers::new();
    peers.applications.push_back(Application::Refused);
    let mut live = Live::new();
    assert_eq!(live.send(command(K::Command17, &[]), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Prepare(Some(K::Command17)), Seen::Apply(K::Command17), Seen::Reply(Some(K::Command17), ReplyStatus::Rejected)]);
}

#[test]
fn a_restarting_change_stops_children_and_starts_the_view_again() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.applications.push_back(Application::Restart);
    assert_eq!(live.send(command(K::Command21, &[]), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Apply(K::Command21), Seen::Prepare(Some(K::Command21)), Seen::Statistics(Stat::Control(StatisticsControl::Stop)),
        Seen::Measurement(Meas::Control(MeasurementControl::Stop)), Seen::Motion, Seen::View(ViewRequest::Stop(StopMode::Plain)),
    ]);
    assert_eq!(live.send(completed(C::Image42), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Effect(Effect::FinishRecordMode), Seen::Effect(Effect::Hold), Seen::View(ViewRequest::Start { measured: true }),
    ]);
}

#[test]
fn the_preparation_flag_is_cleared_across_a_stop_and_set_again_once_configured() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.true_conditions.extend([Condition::PreparationRequested, Condition::PreparationRestorable]);
    assert_eq!(live.send(command(K::Prepare, &[]), &mut peers), Outcome::Complete);
    assert_eq!(live.preparation, PreparationOverride::Set);
    peers.take();
    peers.applications.push_back(Application::Restart);
    live.send(command(K::Command20, &[]), &mut peers);
    peers.take();
    live.send(completed(C::Image42), &mut peers);
    assert_eq!(peers.take(), vec![
        Seen::Effect(Effect::FinishRecordMode), Seen::Effect(Effect::StatisticsPreparation(false)),
        Seen::Statistics(Stat::Reissue(K::Prepare)), Seen::Effect(Effect::Hold), Seen::View(ViewRequest::Start { measured: true }),
    ]);
    assert_eq!(live.preparation, PreparationOverride::Pending);
    live.send(completed(C::Image3f), &mut peers);
    peers.take();
    live.send(completed(C::Image41), &mut peers);
    assert_eq!(peers.take(), vec![
        Seen::Condition(Condition::PreparationRestorable), Seen::Effect(Effect::StatisticsPreparation(true)),
        Seen::Statistics(Stat::Reissue(K::Prepare)),
    ]);
    assert_eq!(live.preparation, PreparationOverride::Set);
}

#[test]
fn the_s1_capture_stops_the_view_only_once_statistics_is_held() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    assert_eq!(live.send(command(K::Command09, &[]), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Statistics(Stat::Control(StatisticsControl::Capture))]);
    peers.statistics.push_back(StatisticsOutcome::Ready);
    assert_eq!(live.send(command(K::Command09, &[]), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Statistics(Stat::Control(StatisticsControl::Capture)), Seen::Prepare(Some(K::Command09)),
        Seen::Statistics(Stat::Control(StatisticsControl::Stop)), Seen::Measurement(Meas::Control(MeasurementControl::Stop)),
        Seen::Motion, Seen::View(ViewRequest::Stop(StopMode::Plain)),
    ]);
}

#[test]
fn unexpected_commands_fault_while_foreign_completions_and_lens_controls_are_dropped() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    assert_eq!(live.send(command(K::Command29, &[]), &mut peers), Outcome::Fault);
    assert_eq!(peers.take(), vec![Seen::Fault]);
    assert_eq!(live.send(command(K::LensBringUp, &[]), &mut peers), Outcome::Fault);
    assert_eq!(live.send(completed(C::Image65), &mut peers), Outcome::Fault);
    assert_eq!(peers.take(), vec![]);
    assert_eq!(live.send(command(K::HalfPress, &[]), &mut peers), Outcome::Inactive);
    assert_eq!(live.send(completed(C::Lens33), &mut peers), Outcome::Inactive);
    assert_eq!(peers.take(), vec![]);
}

#[test]
fn the_window_request_drops_everything_but_its_completion() {
    let mut peers = Peers::new();
    let mut live = Live::new();
    assert_eq!(live.send(command(K::Command29, &[0, 0, 0, 0, 7, 0, 0, 0]), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::Prepare(Some(K::Command29)), Seen::View(ViewRequest::Window(7))]);
    assert_eq!(live.send(Signal::Cancel, &mut peers), Outcome::Fault);
    assert_eq!(live.send(command(K::Command24, &[]), &mut peers), Outcome::Fault);
    assert_eq!(peers.take(), vec![]);
    assert_eq!(live.send(completed(C::Image65), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Reply(Some(K::Command29), ReplyStatus::Accepted)]);
}

#[test]
fn an_autofocus_mode_change_retunes_statistics_and_adjusts_the_view() {
    let mut peers = Peers::new();
    let mut live = running(&mut peers);
    peers.true_conditions.extend([Condition::AutofocusModeChanged, Condition::AutofocusRetune]);
    assert_eq!(live.send(command(K::Autofocus, &[]), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Apply(K::Autofocus), Seen::Condition(Condition::ViewGeometryChanged), Seen::Measurement(Meas::Command(K::Autofocus)),
        Seen::Statistics(Stat::Forward), Seen::Condition(Condition::AutofocusAreaChanged), Seen::Condition(Condition::AutofocusModeChanged),
        Seen::Condition(Condition::AutofocusRetune), Seen::Statistics(Stat::Control(StatisticsControl::Autofocus)),
    ]);
    assert_eq!(live.send(completed(C::Image63), &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![Seen::View(ViewRequest::Adjust { selection: true })]);
    assert_eq!(live.send(completed(C::Image63), &mut peers), Outcome::Complete);
    assert_eq!(peers.take(), vec![Seen::Condition(Condition::SelectionPending)]);
}

#[test]
fn the_deferred_option_configures_measurement_without_starting_the_view() {
    let mut peers = Peers::new();
    let mut live = Live::new();
    assert_eq!(live.send(Signal::RefreshDeferred, &mut peers), Outcome::Pending);
    assert_eq!(peers.take(), vec![
        Seen::Prepare(None), Seen::Effect(Effect::Hold), Seen::Effect(Effect::StoreAdjustment),
        Seen::Measurement(Meas::Control(MeasurementControl::Initialize)), Seen::Effect(Effect::BeginRecordMode),
        Seen::View(ViewRequest::Configure(MotionSetting(0))),
    ]);
}
