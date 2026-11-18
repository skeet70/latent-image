use crate::live::{LaneEvent, LaneStatus, LiveSearch, LiveStrategy, Measurement, Ports};
use crate::requests::{BridgeMotion, Request};
use std::collections::VecDeque;
use std::vec::Vec;
use xm1_saclac_controllers::search::{Direction, FocusSearch, LensPosition, SearchDecision, SearchInput, SearchStep, SearchTerminal, Travel};
use xm1_saclac_interface::search::{BridgeReport, BridgeStatus, LensClass};
use xm1_saclac_interface::LensEffect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seen {
    Initialize,
    Begin,
    Sample(Measurement<u16>),
    Suspend,
    Resume,
    ResumePending,
    Cancel,
    Reported(BridgeReport),
    Finished(SearchTerminal),
}

#[derive(Default)]
struct Script {
    decisions: VecDeque<SearchDecision<u8>>,
    unready: bool,
    frames_while_moving: bool,
    frames: u8,
}

impl FocusSearch for Script {
    type Context = Vec<Seen>;
    type Sample = Measurement<u16>;
    type Drive = u8;

    fn advance(&mut self, seen: &mut Vec<Seen>, input: SearchInput<Self::Sample>) -> SearchDecision<u8> {
        seen.push(match input {
            SearchInput::Initialize => Seen::Initialize,
            SearchInput::Begin => Seen::Begin,
            SearchInput::Sample(sample) => Seen::Sample(sample),
            SearchInput::Suspend => Seen::Suspend,
            SearchInput::Resume => Seen::Resume,
            SearchInput::ResumePending => Seen::ResumePending,
            SearchInput::Cancel => Seen::Cancel,
        });
        match input {
            SearchInput::Sample(_) | SearchInput::Cancel => self.decisions.pop_front().unwrap_or(SearchDecision::Next(SearchStep::Measure)),
            _ => SearchDecision::Next(SearchStep::Measure),
        }
    }
}

const RELEASE_DRIVE: u8 = 0xee;

impl LiveStrategy for Script {
    type Evaluation = u16;
    type Record = u8;

    fn ready(&mut self, _: &Vec<Seen>, bridge_busy: bool) -> bool {
        !self.unready && (self.frames_while_moving || !bridge_busy)
    }
    fn take_in(&mut self, _: &mut Vec<Seen>) -> u8 {
        self.frames += 1;
        self.frames
    }
    fn lens_reported(&mut self, seen: &mut Vec<Seen>, report: BridgeReport) {
        seen.push(Seen::Reported(report));
    }
    fn release(&mut self, _: &Vec<Seen>) -> u8 {
        RELEASE_DRIVE
    }
    fn accepts_begin(&self, _: &Vec<Seen>) -> bool {
        true
    }
    fn finished(&mut self, seen: &mut Vec<Seen>, outcome: SearchTerminal) {
        seen.push(Seen::Finished(outcome));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic {
    Fault,
    Request(Request<u8, u8>),
}

#[derive(Default)]
struct Peers {
    traffic: Vec<Traffic>,
}

impl Ports<u8, u8> for Peers {
    fn fault(&mut self) {
        self.traffic.push(Traffic::Fault);
    }
    fn request(&mut self, request: Request<u8, u8>) {
        self.traffic.push(Traffic::Request(request));
    }
}

const TRAVEL: Travel = Travel { direction: Direction::Increasing, steps: 40 };

fn travel(drive: u8) -> SearchDecision<u8> {
    SearchDecision::Next(SearchStep::Travel { travel: TRAVEL, drive })
}

fn moved(position: i16) -> BridgeReport {
    BridgeReport { status: BridgeStatus::Moved, position, class: LensClass(1) }
}

fn report(status: BridgeStatus) -> BridgeReport {
    BridgeReport { status, position: 0, class: LensClass(0) }
}

fn evaluate(frame: u8) -> Traffic {
    Traffic::Request(Request::Evaluate(frame))
}

fn searching(decisions: &[SearchDecision<u8>]) -> (LiveSearch<Script>, Peers) {
    let script = Script { decisions: decisions.iter().copied().collect(), frames_while_moving: true, ..Script::default() };
    let mut search = LiveSearch::new(script, Vec::new());
    let mut peers = Peers::default();
    assert_eq!(search.handle(LaneEvent::Enable, &mut peers), LaneStatus::Complete);
    (search, peers)
}

#[test]
fn enabling_begins_a_search_and_initializing_needs_an_idle_lane() {
    let mut search = LiveSearch::new(Script::default(), Vec::new());
    let mut peers = Peers::default();
    assert_eq!(search.handle(LaneEvent::Initialize, &mut peers), LaneStatus::Complete);
    assert_eq!(search.handle(LaneEvent::Enable, &mut peers), LaneStatus::Complete);
    assert_eq!(search.context(), &[Seen::Initialize, Seen::Begin]);
    assert_eq!(search.handle(LaneEvent::Enable, &mut peers), LaneStatus::Complete);
    assert_eq!(search.handle(LaneEvent::Initialize, &mut peers), LaneStatus::Refused);
    assert_eq!(peers.traffic, [Traffic::Fault]);
}

#[test]
fn an_idle_lane_completes_a_stop_and_leaves_other_events_unhandled() {
    let mut search = LiveSearch::new(Script::default(), Vec::new());
    let mut peers = Peers::default();
    assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Complete);
    assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Unhandled);
    assert_eq!(search.handle(LaneEvent::Evaluated(3), &mut peers), LaneStatus::Unhandled);
    assert!(peers.traffic.is_empty());
}

#[test]
fn a_ready_frame_is_evaluated_and_its_decision_moves_the_lens() {
    let (mut search, mut peers) = searching(&[travel(7)]);
    assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
    assert_eq!(search.handle(LaneEvent::Evaluated(500), &mut peers), LaneStatus::Idle);
    assert_eq!(peers.traffic, [evaluate(1), Traffic::Request(Request::Bridge { motion: BridgeMotion::Travel(TRAVEL), drive: 7 })]);
    assert_eq!(search.context()[1], Seen::Sample(Measurement { evaluation: 500, bridge_busy: false }));
}

#[test]
fn a_lens_report_reaches_the_strategy_as_it_arrives() {
    let (mut search, mut peers) = searching(&[travel(1)]);
    search.handle(LaneEvent::Work, &mut peers);
    search.handle(LaneEvent::Evaluated(10), &mut peers);
    search.handle(LaneEvent::Work, &mut peers);
    assert_eq!(search.handle(LaneEvent::LensReported(moved(-12)), &mut peers), LaneStatus::Idle);
    search.handle(LaneEvent::Evaluated(20), &mut peers);
    assert_eq!(
        search.context()[1..],
        [
            Seen::Sample(Measurement { evaluation: 10, bridge_busy: false }),
            Seen::Reported(moved(-12)),
            Seen::Sample(Measurement { evaluation: 20, bridge_busy: false }),
        ]
    );
}

#[test]
fn a_report_while_resuming_reaches_the_strategy_before_the_resumption() {
    let (mut search, mut peers) = searching(&[travel(1)]);
    assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Complete);
    search.handle(LaneEvent::Work, &mut peers);
    search.handle(LaneEvent::Evaluated(1), &mut peers);
    assert_eq!(search.handle(LaneEvent::Resume, &mut peers), LaneStatus::Pending);
    assert_eq!(search.handle(LaneEvent::LensReported(moved(3)), &mut peers), LaneStatus::Complete);
    assert_eq!(search.context()[3..], [Seen::ResumePending, Seen::Reported(moved(3)), Seen::Resume]);
}

#[test]
fn frames_wait_while_the_strategy_is_not_ready() {
    let (mut search, mut peers) = searching(&[]);
    let mut unready = LiveSearch::new(Script { unready: true, ..Script::default() }, Vec::new());
    assert_eq!(unready.handle(LaneEvent::Enable, &mut peers), LaneStatus::Complete);
    assert_eq!(unready.handle(LaneEvent::Work, &mut peers), LaneStatus::Idle);
    assert!(peers.traffic.is_empty());
    assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
}

#[test]
fn an_evaluation_nobody_asked_for_is_not_a_sample() {
    let (mut search, mut peers) = searching(&[]);
    assert_eq!(search.handle(LaneEvent::Evaluated(1), &mut peers), LaneStatus::Idle);
    assert_eq!(search.context(), &[Seen::Begin]);
}

#[test]
fn a_second_move_while_the_bridge_is_busy_is_dropped() {
    let (mut search, mut peers) = searching(&[travel(1), travel(2)]);
    for value in [1, 2] {
        search.handle(LaneEvent::Work, &mut peers);
        search.handle(LaneEvent::Evaluated(value), &mut peers);
    }
    let moves = peers.traffic.iter().filter(|traffic| matches!(traffic, Traffic::Request(Request::Bridge { .. }))).count();
    assert_eq!(moves, 1);
    assert_eq!(search.context().len(), 3);
}

#[test]
fn stopping_with_nothing_in_flight_releases_the_lens_before_completing() {
    let (mut search, mut peers) = searching(&[]);
    assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Pending);
    assert_eq!(peers.traffic, [Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE })]);
    assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Refused);
    assert_eq!(search.handle(LaneEvent::LensReported(report(BridgeStatus::Moved)), &mut peers), LaneStatus::Complete);
    assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Unhandled);
}

#[test]
fn a_deferred_release_is_asked_again() {
    let (mut search, mut peers) = searching(&[]);
    search.handle(LaneEvent::Stop, &mut peers);
    assert_eq!(search.handle(LaneEvent::LensReported(report(BridgeStatus::Deferred)), &mut peers), LaneStatus::Pending);
    assert_eq!(search.handle(LaneEvent::LensReported(report(BridgeStatus::Other(5))), &mut peers), LaneStatus::Complete);
    let release = Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE });
    assert_eq!(peers.traffic, [release, release]);
    assert_eq!(search.context(), &[Seen::Begin]);
}

#[test]
fn stopping_drains_the_evaluation_and_the_move_before_releasing() {
    let (mut search, mut peers) = searching(&[travel(3), SearchDecision::Next(SearchStep::Interrupt)]);
    search.handle(LaneEvent::Work, &mut peers);
    search.handle(LaneEvent::Evaluated(1), &mut peers);
    search.handle(LaneEvent::Work, &mut peers);
    peers.traffic.clear();
    assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Pending);
    assert_eq!(search.context().last(), Some(&Seen::Cancel));
    assert_eq!(peers.traffic, [Traffic::Request(Request::BridgeStop)]);
    assert_eq!(search.handle(LaneEvent::Evaluated(2), &mut peers), LaneStatus::Pending);
    assert_eq!(peers.traffic.len(), 1);
    assert_eq!(search.handle(LaneEvent::LensReported(moved(4)), &mut peers), LaneStatus::Pending);
    assert_eq!(peers.traffic[1..], [Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE })]);
    assert_eq!(search.handle(LaneEvent::LensReported(moved(4)), &mut peers), LaneStatus::Complete);
}

#[test]
fn suspension_waits_for_flights_and_resumption_searches_again() {
    let (mut search, mut peers) = searching(&[]);
    search.handle(LaneEvent::Work, &mut peers);
    assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Pending);
    assert_eq!(search.handle(LaneEvent::Evaluated(9), &mut peers), LaneStatus::Complete);
    assert_eq!(search.handle(LaneEvent::Resume, &mut peers), LaneStatus::Complete);
    assert_eq!(search.context()[1..], [Seen::Cancel, Seen::Suspend, Seen::Resume]);
    assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Complete);
    assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Refused);
}

#[test]
fn a_bridge_fault_releases_the_lens_and_halts_until_stopped() {
    let (mut search, mut peers) = searching(&[travel(1)]);
    search.handle(LaneEvent::Work, &mut peers);
    search.handle(LaneEvent::Evaluated(1), &mut peers);
    search.handle(LaneEvent::Work, &mut peers);
    peers.traffic.clear();
    assert_eq!(search.handle(LaneEvent::LensReported(report(BridgeStatus::Fault)), &mut peers), LaneStatus::Idle);
    assert!(peers.traffic.is_empty());
    assert_eq!(search.handle(LaneEvent::Evaluated(2), &mut peers), LaneStatus::Idle);
    assert_eq!(peers.traffic, [Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE })]);
    assert_eq!(search.handle(LaneEvent::LensReported(moved(0)), &mut peers), LaneStatus::Idle);
    assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Idle);
    assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Complete);
    assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Complete);
    assert_eq!(search.context().len(), 2);
}

#[test]
fn a_bridge_fault_with_nothing_evaluating_releases_at_once() {
    let (mut search, mut peers) = searching(&[travel(1)]);
    search.handle(LaneEvent::Work, &mut peers);
    search.handle(LaneEvent::Evaluated(1), &mut peers);
    peers.traffic.clear();
    assert_eq!(search.handle(LaneEvent::LensReported(report(BridgeStatus::Fault)), &mut peers), LaneStatus::Idle);
    assert_eq!(peers.traffic, [Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE })]);
}

#[test]
fn the_bridge_requests_need_the_lens_safety_contract() {
    let travel = Request::<(), ()>::Bridge { motion: BridgeMotion::Travel(TRAVEL), drive: () };
    let release = Request::<(), ()>::Bridge { motion: BridgeMotion::Release, drive: () };
    assert_eq!(travel.lens_effect(), LensEffect::DriveThroughImageBridge);
    assert_eq!(release.lens_effect(), LensEffect::DriveThroughImageBridge);
    assert_eq!(Request::<(), ()>::BridgeStop.lens_effect(), LensEffect::DriveThroughImageBridge);
    assert_eq!(Request::<(), ()>::Evaluate(()).lens_effect(), LensEffect::NoLensDrive);
}

const TERMINALS: [SearchTerminal; 3] = [SearchTerminal::Focused(LensPosition(12)), SearchTerminal::Failed, SearchTerminal::Cancelled];

#[test]
fn terminal_outcomes_are_distinct_at_the_caller_boundary() {
    for outcome in TERMINALS {
        let (mut search, mut peers) = searching(&[SearchDecision::Terminal(outcome)]);
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
        assert_eq!(search.handle(LaneEvent::Evaluated(1), &mut peers), LaneStatus::Terminal(outcome));
        assert_eq!(search.context().last(), Some(&Seen::Finished(outcome)));
        for _ in 0..2 {
            assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Terminal(outcome));
            assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Complete);
        }
        assert_eq!(search.handle(LaneEvent::Evaluated(99), &mut peers), LaneStatus::Terminal(outcome));
        assert_eq!(peers.traffic, [evaluate(1)]);
        assert_eq!(search.handle(LaneEvent::Resume, &mut peers), LaneStatus::Complete);
        assert_eq!(search.context().last(), Some(&Seen::Resume));
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
        assert_eq!(search.handle(LaneEvent::Evaluated(2), &mut peers), LaneStatus::Idle);
        assert_eq!(peers.traffic, [evaluate(1), evaluate(2)]);
        assert_eq!(search.context().iter().filter(|seen| matches!(seen, Seen::Finished(_))).count(), 1);
    }
}

#[test]
fn terminal_resume_waits_for_the_outstanding_move_without_interrupting_it() {
    for outcome in TERMINALS {
        let (mut search, mut peers) = searching(&[travel(7), SearchDecision::Terminal(outcome)]);
        search.handle(LaneEvent::Work, &mut peers);
        search.handle(LaneEvent::Evaluated(1), &mut peers);
        search.handle(LaneEvent::Work, &mut peers);
        assert_eq!(search.handle(LaneEvent::Evaluated(2), &mut peers), LaneStatus::Terminal(outcome));
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Terminal(outcome));
        assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Complete);
        assert_eq!(search.handle(LaneEvent::Resume, &mut peers), LaneStatus::Pending);
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Idle);
        assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), LaneStatus::Complete);
        assert_eq!(search.context().last(), Some(&Seen::Resume));
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
        assert_eq!(peers.traffic, [
            evaluate(1),
            Traffic::Request(Request::Bridge { motion: BridgeMotion::Travel(TRAVEL), drive: 7 }),
            evaluate(2), evaluate(3),
        ]);
    }
}

#[test]
fn terminal_stop_drains_then_releases_and_allows_a_new_search() {
    for outcome in TERMINALS {
        let (mut search, mut peers) = searching(&[travel(7), SearchDecision::Terminal(outcome)]);
        search.handle(LaneEvent::Work, &mut peers);
        search.handle(LaneEvent::Evaluated(1), &mut peers);
        search.handle(LaneEvent::Work, &mut peers);
        assert_eq!(search.handle(LaneEvent::Evaluated(2), &mut peers), LaneStatus::Terminal(outcome));
        assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Pending);
        assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), LaneStatus::Pending);
        assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), LaneStatus::Complete);
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Unhandled);
        assert_eq!(peers.traffic, [
            evaluate(1),
            Traffic::Request(Request::Bridge { motion: BridgeMotion::Travel(TRAVEL), drive: 7 }),
            evaluate(2),
            Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE }),
        ]);
        assert_eq!(search.handle(LaneEvent::Initialize, &mut peers), LaneStatus::Complete);
        assert_eq!(search.handle(LaneEvent::Enable, &mut peers), LaneStatus::Complete);
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
    }
}

#[test]
fn terminal_during_suspension_retains_flights_until_resume_or_stop_drains_them() {
    for outcome in TERMINALS {
        for evaluation_first in [false, true] {
            for stop in [false, true] {
                let (mut search, mut peers) = searching(&[travel(7), SearchDecision::Terminal(outcome)]);
                search.handle(LaneEvent::Work, &mut peers);
                search.handle(LaneEvent::Evaluated(1), &mut peers);
                search.handle(LaneEvent::Work, &mut peers);
                assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Terminal(outcome));
                assert_eq!(search.context().last(), Some(&Seen::Finished(outcome)));
                assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Terminal(outcome));
                let control = if stop { LaneEvent::Stop } else { LaneEvent::Resume };
                assert_eq!(search.handle(control, &mut peers), LaneStatus::Pending);
                let completion = if stop { LaneStatus::Pending } else { LaneStatus::Complete };
                if evaluation_first {
                    assert_eq!(search.handle(LaneEvent::Evaluated(99), &mut peers), if stop { LaneStatus::Pending } else { LaneStatus::Idle });
                    assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), completion);
                } else {
                    assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), if stop { LaneStatus::Pending } else { LaneStatus::Idle });
                    assert_eq!(search.handle(LaneEvent::Evaluated(99), &mut peers), completion);
                }
                assert!(!search.context().iter().any(|seen| matches!(seen, Seen::Sample(Measurement { evaluation: 99, .. }))));
                let mut expected = std::vec![
                    evaluate(1),
                    Traffic::Request(Request::Bridge { motion: BridgeMotion::Travel(TRAVEL), drive: 7 }),
                    evaluate(2),
                ];
                if stop {
                    expected.push(Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE }));
                    assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), LaneStatus::Complete);
                    assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Unhandled);
                } else {
                    assert_eq!(search.context().last(), Some(&Seen::Resume));
                    assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
                    expected.push(evaluate(3));
                }
                assert_eq!(peers.traffic, expected);
            }
        }
    }
}

#[test]
fn a_terminal_move_fault_releases_and_halts_until_stop() {
    for outcome in TERMINALS {
        let (mut search, mut peers) = searching(&[travel(7), SearchDecision::Terminal(outcome)]);
        search.handle(LaneEvent::Work, &mut peers);
        search.handle(LaneEvent::Evaluated(1), &mut peers);
        search.handle(LaneEvent::Work, &mut peers);
        search.handle(LaneEvent::Evaluated(2), &mut peers);
        peers.traffic.clear();
        assert_eq!(search.handle(LaneEvent::LensReported(report(BridgeStatus::Fault)), &mut peers), LaneStatus::Terminal(outcome));
        assert_eq!(peers.traffic, [Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE })]);
        assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), LaneStatus::Idle);
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Idle);
        assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Complete);
    }
}

#[test]
fn a_terminal_lane_can_drain_its_requests_before_the_next_control() {
    for outcome in TERMINALS {
        for evaluation_first in [false, true] {
            let (mut search, mut peers) = searching(&[travel(7), SearchDecision::Terminal(outcome)]);
            search.handle(LaneEvent::Work, &mut peers);
            search.handle(LaneEvent::Evaluated(1), &mut peers);
            search.handle(LaneEvent::Work, &mut peers);
            assert_eq!(search.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Terminal(outcome));
            let evaluation = LaneEvent::Evaluated(99);
            let report = LaneEvent::LensReported(moved(12));
            let events = if evaluation_first { [evaluation, report] } else { [report, evaluation] };
            for event in events {
                assert_eq!(search.handle(event, &mut peers), LaneStatus::Terminal(outcome));
            }
            assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Terminal(outcome));
            assert_eq!(search.handle(LaneEvent::Resume, &mut peers), LaneStatus::Complete);
            assert_eq!(search.context().iter().filter(|seen| matches!(seen, Seen::Sample(_))).count(), 1);
            assert_eq!(peers.traffic, [
                evaluate(1),
                Traffic::Request(Request::Bridge { motion: BridgeMotion::Travel(TRAVEL), drive: 7 }),
                evaluate(2),
            ]);
        }
    }
}

#[test]
fn a_terminal_cancel_does_not_abort_the_stop_drain() {
    for outcome in TERMINALS {
        let (mut search, mut peers) = searching(&[SearchDecision::Terminal(outcome)]);
        search.handle(LaneEvent::Work, &mut peers);
        assert_eq!(search.handle(LaneEvent::Stop, &mut peers), LaneStatus::Terminal(outcome));
        assert_eq!(search.context().last(), Some(&Seen::Finished(outcome)));
        assert_eq!(search.handle(LaneEvent::Evaluated(99), &mut peers), LaneStatus::Pending);
        assert_eq!(search.handle(LaneEvent::LensReported(moved(12)), &mut peers), LaneStatus::Complete);
        assert_eq!(search.handle(LaneEvent::Work, &mut peers), LaneStatus::Unhandled);
        assert_eq!(peers.traffic, [
            evaluate(1), Traffic::Request(Request::Bridge { motion: BridgeMotion::Release, drive: RELEASE_DRIVE }),
        ]);
    }
}
