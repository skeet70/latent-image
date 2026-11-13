use crate::{root::{Control as RootControl, Input, Outcome}, s1::*};
use std::{collections::VecDeque, vec, vec::Vec};
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completed, Completion as C, Message, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seen { Forward, Begin, Release, Prepare, PrepareCancellable, Cancel, Resume, Reissue(K) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic {
    Shot(Seen),
    Statistics(Seen),
    Capture(Seen),
    Lens(Seen),
    Live(Seen),
    Publish(Published),
    Effect(Effect),
    SecondShot,
    Cancellable,
    ReleaseAllowed,
    Fault,
}

#[derive(Default)]
struct Peers {
    traffic: Vec<Traffic>,
    outcomes: VecDeque<Outcome>,
    second: bool,
    cancellable: bool,
    release: bool,
}

impl Peers {
    fn new(outcomes: &[Outcome]) -> Self { Self { outcomes: outcomes.iter().copied().collect(), ..Self::default() } }
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
    fn answer(&mut self, traffic: Traffic) -> Outcome {
        self.traffic.push(traffic);
        self.outcomes.pop_front().expect("child outcome")
    }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x51) }
fn command(kind: K) -> Input<'static> { Input::Message(Message::Command(Command { kind, parameters: stock::parameters(&[]) })) }
fn completed(completion: C) -> Input<'static> {
    Input::Message(Message::Completed(Completed { completion, parameters: stock::parameters(&[]) }))
}
fn other() -> Input<'static> { completed(C::Image4c) }

impl Port for Peers {
    fn shot(&mut self, event: ShotEvent<'_>, record: RecordHandle) -> Outcome {
        assert_eq!(record, self::record());
        self.answer(Traffic::Shot(match event { ShotEvent::Forward(_) => Seen::Forward, ShotEvent::Begin => Seen::Begin, ShotEvent::Release => Seen::Release }))
    }
    fn statistics(&mut self, event: StatisticsEvent<'_>, _: RecordHandle) -> Outcome {
        self.answer(Traffic::Statistics(match event {
            StatisticsEvent::Forward(_) => Seen::Forward,
            StatisticsEvent::Begin => Seen::Begin,
            StatisticsEvent::Prepare => Seen::Prepare,
            StatisticsEvent::PrepareCancellable => Seen::PrepareCancellable,
            StatisticsEvent::Cancel => Seen::Cancel,
            StatisticsEvent::Resume => Seen::Resume,
        }))
    }
    fn capture(&mut self, event: CaptureEvent<'_>, _: RecordHandle) -> Outcome {
        self.answer(Traffic::Capture(match event { CaptureEvent::Forward(_) => Seen::Forward, CaptureEvent::Begin => Seen::Begin }))
    }
    fn lens(&mut self, event: LensEvent<'_>, _: RecordHandle) -> Outcome {
        let seen = match event {
            LensEvent::Forward(Input::Control(RootControl::Cancel)) => Seen::Cancel,
            LensEvent::Forward(_) => Seen::Forward,
            LensEvent::Reissue(kind) => Seen::Reissue(kind),
        };
        self.answer(Traffic::Lens(seen))
    }
    fn live(&mut self, event: LiveEvent<'_>, _: RecordHandle) -> Outcome {
        self.answer(Traffic::Live(match event { LiveEvent::Forward(_) => Seen::Forward, LiveEvent::Reissue(kind) => Seen::Reissue(kind) }))
    }
    fn publish(&mut self, published: Published) { self.traffic.push(Traffic::Publish(published)); }
    fn effect(&mut self, effect: Effect, record: RecordHandle) {
        assert_eq!(record, self::record());
        self.traffic.push(Traffic::Effect(effect));
    }
    fn second_shot(&mut self, _: RecordHandle) -> bool { self.traffic.push(Traffic::SecondShot); self.second }
    fn lens_cancellable(&mut self, _: RecordHandle) -> bool { self.traffic.push(Traffic::Cancellable); self.cancellable }
    fn release_allowed(&mut self) -> bool { self.traffic.push(Traffic::ReleaseAllowed); self.release }
    fn fault(&mut self) { self.traffic.push(Traffic::Fault); }
}

use Outcome::{Complete as Done, Fault as Refused, Pending};

fn run(router: &mut Router, peers: &mut Peers, input: Input<'_>) -> Outcome { router.handle(input, record(), peers) }
fn waiting(shot: bool, capture: bool, bridge: bool, release: bool) -> Traffic {
    Traffic::Publish(Published::Waiting(Waiting { shot, capture, bridge, release }))
}
fn query(router: &mut Router) -> Outcome {
    let mut peers = Peers::new(&[]);
    let outcome = run(router, &mut peers, Input::Control(RootControl::QueryS1));
    assert!(peers.traffic.is_empty());
    outcome
}

/// A half press and S1 start that leave the router running statistics.
fn started(router: &mut Router) {
    let mut peers = Peers::new(&[Done, Pending]);
    assert_eq!(run(router, &mut peers, command(K::HalfPress)), Pending);
    assert_eq!(run(router, &mut peers, Input::Control(RootControl::BeginS1)), Pending);
}

#[test]
fn half_press_then_start_runs_the_shot_then_statistics() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Pending]);
    assert_eq!(run(&mut router, &mut peers, command(K::HalfPress)), Pending);
    assert_eq!(peers.take(), vec![
        waiting(true, false, false, false), Traffic::Publish(Published::ShotIdle(false)),
        Traffic::Effect(Effect::ResetShotProgress), Traffic::Effect(Effect::ApplyHalfPressOption), Traffic::Effect(Effect::ResetCaptureRecord),
    ]);
    assert_eq!(query(&mut router), Done);
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::BeginS1)), Pending);
    assert_eq!(peers.take(), vec![
        Traffic::Effect(Effect::BeginBusy), Traffic::SecondShot, Traffic::Publish(Published::Deferred(1)),
        waiting(false, false, false, false), Traffic::Publish(Published::ShotIdle(true)), Traffic::Publish(Published::Deferred(0)),
        Traffic::Shot(Seen::Begin), Traffic::Statistics(Seen::Begin),
    ]);
    assert_eq!(query(&mut router), Pending);
}

#[test]
fn release_after_statistics_prepares_and_ends_the_branch() {
    let mut router = Router::new();
    started(&mut router);
    let mut peers = Peers::new(&[Done]);
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::ReleaseS1)), Done);
    assert_eq!(peers.take(), vec![
        waiting(false, false, false, true), Traffic::Statistics(Seen::Prepare),
        waiting(false, false, false, false), Traffic::Effect(Effect::EndBusy),
    ]);
    assert_eq!(query(&mut router), Done);
}

#[test]
fn release_with_nothing_owed_takes_the_fault_reaction() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[]);
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::ReleaseS1)), Refused);
    assert_eq!(peers.traffic, vec![Traffic::Fault]);
}

#[test]
fn release_while_shooting_reaches_the_shot_only_when_allowed() {
    for allowed in [false, true] {
        let mut router = Router::new();
        let mut peers = Peers::new(&[Pending, Pending]);
        peers.release = allowed;
        run(&mut router, &mut peers, command(K::HalfPress));
        assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::BeginS1)), Pending);
        peers.take();
        assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::ReleaseS1)), Pending);
        let mut expected = vec![waiting(false, false, false, true), Traffic::ReleaseAllowed];
        if allowed { expected.push(Traffic::Shot(Seen::Release)); }
        assert_eq!(peers.take(), expected);
    }
}

#[test]
fn second_shot_waits_for_the_next_half_press() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Done]);
    peers.second = true;
    run(&mut router, &mut peers, command(K::HalfPress));
    peers.take();
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::BeginS1)), Done);
    assert_eq!(peers.take(), vec![
        Traffic::Effect(Effect::BeginBusy), Traffic::SecondShot,
        Traffic::Publish(Published::Deferred(2)), waiting(false, false, false, false), Traffic::Publish(Published::ShotIdle(true)),
        Traffic::Publish(Published::Deferred(1)), Traffic::Shot(Seen::Begin), Traffic::Statistics(Seen::Begin),
    ]);
    // The shot finishes while a second one is owed: the router waits for the next half press.
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Done, Pending]);
    peers.second = true;
    run(&mut router, &mut peers, command(K::HalfPress));
    run(&mut router, &mut peers, Input::Control(RootControl::BeginS1));
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(query(&mut router), Done);
    peers.take();
    assert_eq!(run(&mut router, &mut peers, command(K::HalfPress)), Pending);
    assert_eq!(peers.take(), vec![Traffic::Publish(Published::Deferred(0)), Traffic::Shot(Seen::Begin)]);
}

#[test]
fn capture_request_during_statistics_prepares_for_a_cancellable_lens() {
    let mut router = Router::new();
    started(&mut router);
    let mut peers = Peers::new(&[Done, Pending, Done]);
    peers.cancellable = true;
    assert_eq!(run(&mut router, &mut peers, command(K::Command07)), Pending);
    assert_eq!(query(&mut router), Done);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        waiting(false, true, false, false), Traffic::Cancellable, Traffic::Statistics(Seen::PrepareCancellable),
        waiting(false, false, false, false), Traffic::Capture(Seen::Begin),
        Traffic::Capture(Seen::Forward), Traffic::Effect(Effect::EndBusy),
    ]);
}

#[test]
fn bridge_carries_autofocus_to_live_and_ends_on_command0a() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Pending, Pending, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command09)), Pending);
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::BeginS1)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Autofocus)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Command0a)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        waiting(false, false, true, false),
        Traffic::Effect(Effect::BeginBusy), Traffic::Publish(Published::Deferred(0)),
        waiting(false, false, false, false), Traffic::Effect(Effect::NotifyBridge(K::Command09)), Traffic::Live(Seen::Reissue(K::Command09)),
        Traffic::Live(Seen::Forward), Traffic::Effect(Effect::ReplyAutofocus),
        Traffic::Live(Seen::Forward),
        Traffic::Live(Seen::Forward), Traffic::Effect(Effect::NotifyBridge(K::Command0a)), Traffic::Effect(Effect::EndBusy),
    ]);
    assert_eq!(query(&mut router), Done);
}

#[test]
fn finishing_bridge_refuses_further_commands_without_the_fault_reaction() {
    let mut bridge = Bridge::new();
    let mut peers = Peers::new(&[Done, Pending]);
    assert_eq!(bridge.begin(record(), &mut peers), Pending);
    assert_eq!(bridge.handle(command(K::Command0a), record(), &mut peers), Pending);
    peers.take();
    for kind in [K::Command17, K::Autofocus, K::Command0a] {
        assert_eq!(bridge.handle(command(kind), record(), &mut peers), Refused);
    }
    assert!(peers.traffic.is_empty());
    let mut idle = Bridge::new();
    assert_eq!(idle.handle(command(K::Command17), record(), &mut peers), Refused);
    assert!(peers.traffic.is_empty());
}

#[test]
fn lens_cancel_with_a_stop_request_reissues_command10_then_resumes_statistics() {
    let mut router = Router::new();
    started(&mut router);
    let mut peers = Peers::new(&[Pending, Done, Done, Pending, Done]);
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::Cancel)), Pending);
    assert_eq!(query(&mut router), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Command10)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(peers.take(), vec![
        Traffic::Statistics(Seen::Cancel),
        Traffic::Publish(Published::LensStopRequested(true)),
        Traffic::Statistics(Seen::Forward), Traffic::Publish(Published::LensStopRequested(false)),
        Traffic::Lens(Seen::Reissue(K::Command10)), Traffic::Statistics(Seen::Resume),
        Traffic::Statistics(Seen::Forward),
    ]);
    assert_eq!(query(&mut router), Pending);
}

#[test]
fn lens_cancel_without_a_stop_drives_the_lens_until_it_completes() {
    let mut router = Router::new();
    started(&mut router);
    let mut peers = Peers::new(&[Done, Pending, Pending, Done, Done]);
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::Cancel)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens33)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens34)), Pending);
    assert_eq!(peers.take(), vec![
        Traffic::Statistics(Seen::Cancel), Traffic::Lens(Seen::Cancel),
        Traffic::Lens(Seen::Forward), Traffic::Lens(Seen::Forward), Traffic::Statistics(Seen::Resume),
    ]);
    assert_eq!(query(&mut router), Pending);
}

#[test]
fn command08_from_idle_holds_the_capture_controller_until_it_completes() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Pending, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command08)), Pending);
    assert_eq!(query(&mut router), Done);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![Traffic::Capture(Seen::Forward); 3]);
    let mut peers = Peers::new(&[]);
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::ReleaseS1)), Refused);
}
