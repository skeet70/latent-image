use crate::children::MeasurementUpdate;
use crate::live_statistics::*;
use crate::live_view::{Signal, StatisticsCall, StatisticsControl as Ctl, StatisticsOutcome as O};
use std::vec::Vec;
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completed, Completion as C, RecordHandle};

#[derive(Debug, Eq, PartialEq)]
enum Seen {
    Request(Request),
    Reply(Shown),
    Results,
    Descriptor(u32),
    Fault,
}

#[derive(Debug, Eq, PartialEq)]
enum Shown {
    Finished(Owed),
    Accepted(Answered),
    Refused(Answered),
    Descriptor { selected: bool },
    Prepared,
    CaptureConfigured,
    CaptureResults(Vec<u8>),
    Command24 { refused: bool, completion: Vec<u8> },
}

#[derive(Default)]
struct Peers {
    seen: Vec<Seen>,
    start_needed: bool,
    selected: bool,
    primary: bool,
    cleared: bool,
    groups: Groups,
}

impl Peers {
    fn new() -> Self { Self { start_needed: true, primary: true, ..Self::default() } }
    fn take(&mut self) -> Vec<Seen> { core::mem::take(&mut self.seen) }
}

impl Port for Peers {
    fn request(&mut self, request: Request, _: RecordHandle) { self.seen.push(Seen::Request(request)) }
    fn reply(&mut self, reply: Reply<'_>, _: RecordHandle) {
        self.seen.push(Seen::Reply(match reply {
            Reply::Finished(owed) => Shown::Finished(owed),
            Reply::Accepted(answered) => Shown::Accepted(answered),
            Reply::Refused(answered) => Shown::Refused(answered),
            Reply::Descriptor { selected } => Shown::Descriptor { selected },
            Reply::Prepared => Shown::Prepared,
            Reply::CaptureConfigured => Shown::CaptureConfigured,
            Reply::CaptureResults(parameters) => Shown::CaptureResults(parameters.bytes().to_vec()),
            Reply::Command24 { refused, completion } => Shown::Command24 { refused, completion: completion.bytes().to_vec() },
        }))
    }
    fn condition(&mut self, condition: Condition, _: RecordHandle) -> bool {
        match condition {
            Condition::StartNeeded => self.start_needed,
            Condition::Selected => self.selected,
            Condition::PrimaryApplied => self.primary,
            Condition::Command22Cleared => self.cleared,
        }
    }
    fn changes(&mut self, _: RecordHandle) -> Groups { self.groups }
    fn deliver_results(&mut self, _: RecordHandle) { self.seen.push(Seen::Results) }
    fn select_descriptor(&mut self, descriptor: u32, _: RecordHandle) { self.seen.push(Seen::Descriptor(descriptor)) }
    fn fault(&mut self) { self.seen.push(Seen::Fault) }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x22) }

fn control(statistics: &mut LiveStatistics, peers: &mut Peers, control: Ctl) -> O {
    statistics.send(StatisticsCall::Control(control), record(), peers)
}

fn command(statistics: &mut LiveStatistics, peers: &mut Peers, kind: K) -> O {
    let call = StatisticsCall::Forward(Signal::Command(Command { kind, parameters: stock::parameters(&[]) }));
    statistics.send(call, record(), peers)
}

fn completed(statistics: &mut LiveStatistics, peers: &mut Peers, completion: C, payload: &[u8]) -> O {
    let call = StatisticsCall::Forward(Signal::Completed(Completed { completion, parameters: stock::parameters(payload) }));
    statistics.send(call, record(), peers)
}

fn running(peers: &mut Peers) -> LiveStatistics {
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, peers, Ctl::Begin), O::Pending);
    assert_eq!(completed(&mut statistics, peers, C::Statistics69, &[]), O::Complete);
    peers.take();
    statistics
}

fn captured(peers: &mut Peers) -> LiveStatistics {
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, peers, Ctl::Capture), O::Pending);
    assert_eq!(completed(&mut statistics, peers, C::Statistics6c, &[]), O::Complete);
    peers.take();
    statistics
}

#[test]
fn begin_starts_the_statistics_and_completes_on_the_start_completion() {
    let mut peers = Peers::new();
    peers.selected = true;
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Pending);
    assert_eq!(peers.take(), [Seen::Request(Request::Start { selected: true })]);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics69, &[]), O::Complete);
    assert_eq!(peers.take(), []);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Capture), O::Ready);
}

#[test]
fn begin_completes_without_a_request_when_the_view_does_not_need_statistics() {
    let mut peers = Peers::new();
    peers.primary = false;
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Complete);
    peers.primary = true;
    peers.start_needed = false;
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Complete);
    assert_eq!(peers.take(), []);
}

#[test]
fn stop_reports_the_shot_state_and_returns_to_idle() {
    let mut peers = Peers::new();
    let mut statistics = running(&mut peers);
    statistics.set_shot_idle(false);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Stop), O::Pending);
    assert_eq!(peers.take(), [Seen::Request(Request::Stop { pause: false, shot_idle: false })]);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6b, &[]), O::Complete);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Stop), O::Complete);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Pending);
    assert_eq!(peers.take(), [Seen::Request(Request::Start { selected: false })]);
}

#[test]
fn a_start_marks_the_shot_idle_for_the_next_stop() {
    let mut peers = Peers::new();
    let mut statistics = LiveStatistics::new();
    statistics.set_shot_idle(false);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics69, &[]), O::Complete);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Pause), O::Pending);
    assert_eq!(peers.take().last(), Some(&Seen::Request(Request::Stop { pause: true, shot_idle: true })));
}

#[test]
fn capture_configures_reports_progress_and_results_then_holds() {
    let mut peers = Peers::new();
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Capture), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics78, &[]), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics79, &[1, 2, 3, 4]), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6c, &[]), O::Complete);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Configure),
        Seen::Reply(Shown::CaptureConfigured),
        Seen::Reply(Shown::CaptureResults(std::vec![1, 2, 3, 4])),
    ]);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Fault);
    assert_eq!(peers.take(), [Seen::Fault]);
}

#[test]
fn releasing_the_capture_pauses_and_command0a_resumes_with_the_capture_reply() {
    let mut peers = Peers::new();
    let mut statistics = captured(&mut peers);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Release), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6d, &[]), O::Complete);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Release),
        Seen::Reply(Shown::Accepted(Answered::Command0a)),
        Seen::Reply(Shown::Finished(Owed::Release)),
    ]);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Pending);
    assert_eq!(peers.take(), [Seen::Request(Request::Start { selected: false })]);

    let mut statistics = captured(&mut peers);
    assert_eq!(command(&mut statistics, &mut peers, K::Command0a), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6d, &[]), O::Complete);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Release),
        Seen::Reply(Shown::Accepted(Answered::Command0a)),
        Seen::Reply(Shown::Finished(Owed::Command0a)),
    ]);
    assert_eq!(command(&mut statistics, &mut peers, K::Command0a), O::Complete);
    assert_eq!(peers.take(), [Seen::Reply(Shown::Refused(Answered::Command0a))]);
}

#[test]
fn autofocus_changes_reconfigure_measurement_first_then_the_remaining_groups() {
    let mut peers = Peers::new();
    let mut statistics = running(&mut peers);
    peers.groups = Groups { processing: true, geometry: true, measurement: true };
    assert_eq!(command(&mut statistics, &mut peers, K::Autofocus), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics80, &[]), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics73, &[]), O::Pending);
    peers.selected = true;
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics71, &[]), O::Complete);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Capture(CaptureWord::Retune)),
        Seen::Request(Request::Exposure),
        Seen::Request(Request::Format),
    ]);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Capture), O::Pending);
}

#[test]
fn autofocus_from_idle_reconfigures_only_measurement_and_keeps_the_rest_for_later() {
    let mut peers = Peers::new();
    let mut statistics = LiveStatistics::new();
    peers.groups = Groups { processing: true, geometry: false, measurement: false };
    assert_eq!(command(&mut statistics, &mut peers, K::Autofocus), O::Complete);
    peers.groups = Groups { processing: true, geometry: false, measurement: true };
    assert_eq!(command(&mut statistics, &mut peers, K::Autofocus), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics80, &[]), O::Complete);
    assert_eq!(peers.take(), [Seen::Request(Request::Capture(CaptureWord::Retune))]);

    peers.start_needed = true;
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics69, &[]), O::Complete);
    peers.take();
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Resume), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics80, &[]), O::Pending);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Capture(CaptureWord::Resume)),
        Seen::Request(Request::Exposure),
    ]);
}

#[test]
fn command11_adjusts_selects_the_chosen_descriptor_and_finishes() {
    let mut peers = Peers::new();
    let mut statistics = running(&mut peers);
    assert_eq!(command(&mut statistics, &mut peers, K::Command11), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics76, &[0, 0, 0, 0, 0x34, 0x12, 0, 0]), O::Reconfigure);
    assert_eq!(command(&mut statistics, &mut peers, K::Command13), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6e, &[]), O::Complete);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Adjust),
        Seen::Descriptor(0x1234),
        Seen::Reply(Shown::Descriptor { selected: true }),
        Seen::Request(Request::Command13),
        Seen::Reply(Shown::Accepted(Answered::Command13)),
        Seen::Reply(Shown::Finished(Owed::Command11)),
    ]);
    assert_eq!(command(&mut statistics, &mut peers, K::Command13), O::Complete);
    assert_eq!(peers.take(), [Seen::Reply(Shown::Refused(Answered::Command13))]);
}

#[test]
fn command11_while_captured_returns_to_the_capture_with_the_descriptor() {
    let mut peers = Peers::new();
    let mut statistics = captured(&mut peers);
    assert_eq!(command(&mut statistics, &mut peers, K::Command11), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6e, &[]), O::Complete);
    assert_eq!(peers.take(), [Seen::Request(Request::Adjust), Seen::Reply(Shown::Descriptor { selected: false })]);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Release), O::Pending);
}

#[test]
fn command11_while_idle_finishes_and_asks_for_measurement_to_be_reconfigured() {
    let mut peers = Peers::new();
    let mut statistics = LiveStatistics::new();
    assert_eq!(command(&mut statistics, &mut peers, K::Command11), O::Reconfigure);
    assert_eq!(peers.take(), [Seen::Reply(Shown::Finished(Owed::Command11))]);
}

#[test]
fn profile_commands_while_running_reconfigure_and_reply_when_done() {
    let mut peers = Peers::new();
    for (kind, request, owed) in [
        (K::Command1e, Request::Output, Owed::Command1e),
        (K::Command1f, Request::Window, Owed::Command1f),
        (K::Command21, Request::Format, Owed::Command21),
    ] {
        let mut statistics = running(&mut peers);
        assert_eq!(command(&mut statistics, &mut peers, kind), O::Pending);
        let done = match request { Request::Output => C::Statistics75, Request::Window => C::Statistics72, _ => C::Statistics71 };
        assert_eq!(completed(&mut statistics, &mut peers, done, &[]), O::Complete);
        assert_eq!(peers.take(), [Seen::Request(request), Seen::Reply(Shown::Finished(owed))]);
    }
    let mut statistics = running(&mut peers);
    assert_eq!(command(&mut statistics, &mut peers, K::Command20), O::Complete);
    assert_eq!(peers.take(), [Seen::Reply(Shown::Finished(Owed::Command20))]);
}

#[test]
fn command1d_without_a_needed_start_pauses_and_finishes_in_idle() {
    let mut peers = Peers::new();
    let mut statistics = running(&mut peers);
    peers.start_needed = false;
    assert_eq!(command(&mut statistics, &mut peers, K::Command1d), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6b, &[]), O::Complete);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Stop { pause: true, shot_idle: true }),
        Seen::Reply(Shown::Finished(Owed::Command1d)),
    ]);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Pause), O::Complete);
}

#[test]
fn command22_stops_into_the_pause_and_restarts_when_its_change_clears() {
    let mut peers = Peers::new();
    let mut statistics = running(&mut peers);
    assert_eq!(command(&mut statistics, &mut peers, K::Command22), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics6b, &[]), O::Complete);
    assert_eq!(command(&mut statistics, &mut peers, K::Command22), O::Complete);
    peers.cleared = true;
    assert_eq!(command(&mut statistics, &mut peers, K::Command22), O::Pending);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Stop { pause: false, shot_idle: true }),
        Seen::Reply(Shown::Finished(Owed::Command22)),
        Seen::Reply(Shown::Finished(Owed::Command22)),
        Seen::Request(Request::Start { selected: false }),
    ]);
}

#[test]
fn measurement_updates_reach_the_statistics_only_while_it_listens() {
    let mut peers = Peers::new();
    let mut statistics = LiveStatistics::new();
    let window = Ctl::Measured(MeasurementUpdate::Window);
    let source = Ctl::Measured(MeasurementUpdate::Source);
    assert_eq!(control(&mut statistics, &mut peers, window), O::Pending);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Measured(MeasurementUpdate::NoPeerRequest)), O::Pending);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Begin), O::Pending);
    assert_eq!(control(&mut statistics, &mut peers, source), O::Pending);
    assert_eq!(peers.take(), [Seen::Request(Request::MeasuredWindow), Seen::Request(Request::Start { selected: false })]);
}

#[test]
fn refresh_and_command24_post_their_requests_and_settle_by_selection() {
    let mut peers = Peers::new();
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Refresh), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics7e, &[]), O::Complete);
    assert_eq!(command(&mut statistics, &mut peers, K::Command24), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics82, &[5, 0, 0, 0, 6, 0, 0, 0]), O::Pending);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Selection { refresh: true }),
        Seen::Request(Request::Command24),
        Seen::Reply(Shown::Command24 { refused: false, completion: std::vec![5, 0, 0, 0, 6, 0, 0, 0] }),
    ]);

    let mut statistics = running(&mut peers);
    peers.selected = true;
    assert_eq!(command(&mut statistics, &mut peers, K::Command24), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics82, &[]), O::Complete);
    assert_eq!(peers.take(), [
        Seen::Request(Request::Command24),
        Seen::Reply(Shown::Command24 { refused: true, completion: std::vec![] }),
    ]);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Capture), O::Pending);
}

#[test]
fn a_selection_refresh_while_captured_keeps_the_capture_held() {
    let mut peers = Peers::new();
    let mut statistics = captured(&mut peers);
    peers.selected = true;
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Refresh), O::Pending);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics7e, &[]), O::Complete);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Release), O::Pending);
    assert_eq!(peers.take()[0], Seen::Request(Request::Selection { refresh: true }));
}

#[test]
fn results_prepare_and_unknown_events() {
    let mut peers = Peers::new();
    let mut statistics = running(&mut peers);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics7f, &[]), O::Pending);
    assert_eq!(command(&mut statistics, &mut peers, K::Prepare), O::Complete);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics77, &[]), O::Pending);
    assert_eq!(command(&mut statistics, &mut peers, K::Command17), O::Unhandled);
    assert_eq!(completed(&mut statistics, &mut peers, C::Image42, &[]), O::Unhandled);
    assert_eq!(statistics.send(StatisticsCall::Forward(Signal::SuspendLive), record(), &mut peers), O::Unhandled);
    assert_eq!(peers.take(), [Seen::Results, Seen::Request(Request::Prepare), Seen::Reply(Shown::Prepared)]);
}

#[test]
fn unexpected_controls_raise_the_fault_reaction_and_unexpected_completions_do_not() {
    let mut peers = Peers::new();
    let mut statistics = running(&mut peers);
    assert_eq!(completed(&mut statistics, &mut peers, C::Statistics69, &[]), O::Fault);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Suspend), O::Pending);
    assert_eq!(control(&mut statistics, &mut peers, Ctl::Suspend), O::Fault);
    assert_eq!(peers.take(), [Seen::Request(Request::Capture(CaptureWord::Suspend))]);
    assert_eq!(command(&mut statistics, &mut peers, K::Command02), O::Fault);
    assert_eq!(peers.take(), [Seen::Fault]);
}
