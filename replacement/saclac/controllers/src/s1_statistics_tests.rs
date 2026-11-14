use crate::children::{AcquisitionControl, ChildEvent, ChildOutcome, SearchControl};
use crate::root::{Input, Outcome};
use crate::s1::StatisticsEvent;
use crate::s1_statistics::*;
use std::{collections::VecDeque, vec::Vec};
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completed, Completion as C, Message, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seen {
    Acquisition(AcquisitionControl),
    AcquisitionForward(C),
    Search(SearchControl),
    SearchForward(C),
    Snapshot,
    RecordMode(bool),
    Request(Request),
    Reply(Reply),
    Apply(Apply),
    MoveLens,
    Fault,
}

#[derive(Default)]
struct Fake {
    seen: Vec<Seen>,
    acquisition: VecDeque<ChildOutcome>,
    search: VecDeque<ChildOutcome>,
    view_active: bool,
    output_needed: bool,
    restart: bool,
    change: Option<AutofocusChange>,
    lens_in_place: bool,
    reporting: Option<bool>,
    reports_on_requests: Vec<(Request, bool)>,
}

impl Fake {
    fn take(&mut self) -> Vec<Seen> { core::mem::take(&mut self.seen) }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x17) }

impl Port for Fake {
    fn acquisition(&mut self, event: ChildEvent<'_, AcquisitionControl>) -> ChildOutcome {
        match event {
            ChildEvent::Control { control, record, .. } => {
                assert_eq!(record, self::record());
                self.seen.push(Seen::Acquisition(control));
            }
            ChildEvent::Completed { completed, .. } => self.seen.push(Seen::AcquisitionForward(completed.completion)),
            ChildEvent::Command { .. } => panic!("no command reaches the acquisition"),
        }
        self.acquisition.pop_front().unwrap_or(ChildOutcome::Pending)
    }
    fn search(&mut self, event: ChildEvent<'_, SearchControl>) -> ChildOutcome {
        match event {
            ChildEvent::Control { control, .. } => self.seen.push(Seen::Search(control)),
            ChildEvent::Completed { completed, .. } => self.seen.push(Seen::SearchForward(completed.completion)),
            ChildEvent::Command { .. } => panic!("no command reaches the search"),
        }
        self.search.pop_front().unwrap_or(ChildOutcome::Pending)
    }
    fn snapshot(&mut self, _: RecordHandle) -> Snapshot {
        self.seen.push(Seen::Snapshot);
        Snapshot { view_active: self.view_active }
    }
    fn record_mode(&mut self, held: bool) { self.seen.push(Seen::RecordMode(held)) }
    fn report_results(&mut self, open: bool) { self.reporting = Some(open); }
    fn request(&mut self, request: Request, _: RecordHandle) {
        self.reports_on_requests.push((request, self.reporting.expect("begin publishes the callback gate")));
        self.seen.push(Seen::Request(request));
    }
    fn reply(&mut self, reply: Reply, _: RecordHandle) { self.seen.push(Seen::Reply(reply)) }
    fn apply(&mut self, apply: Apply, _: RecordHandle) { self.seen.push(Seen::Apply(apply)) }
    fn condition(&mut self, condition: Condition, _: RecordHandle) -> bool {
        match condition {
            Condition::OutputNeeded => self.output_needed,
            Condition::RestartRequired => self.restart,
        }
    }
    fn autofocus(&mut self, _: RecordHandle) -> AutofocusChange { self.change.unwrap_or(AutofocusChange::None) }
    fn move_lens(&mut self, _: RecordHandle) -> bool {
        self.seen.push(Seen::MoveLens);
        self.lens_in_place
    }
    fn fault(&mut self) { self.seen.push(Seen::Fault) }
}

fn command(kind: K) -> StatisticsEvent<'static> {
    StatisticsEvent::Forward(Input::Message(Message::Command(Command { kind, parameters: stock::parameters(&[]) })))
}

fn completed(completion: C) -> StatisticsEvent<'static> {
    StatisticsEvent::Forward(Input::Message(Message::Completed(Completed { completion, parameters: stock::parameters(&[]) })))
}

fn send(statistics: &mut S1Statistics, fake: &mut Fake, event: StatisticsEvent<'_>) -> Outcome { statistics.send(event, record(), fake) }

fn running(fake: &mut Fake) -> S1Statistics {
    let mut statistics = S1Statistics::new();
    assert_eq!(send(&mut statistics, fake, StatisticsEvent::Begin), Outcome::Pending);
    assert_eq!(send(&mut statistics, fake, completed(C::Image57)), Outcome::Pending);
    assert_eq!(send(&mut statistics, fake, completed(C::Statistics7b)), Outcome::Pending);
    fake.take();
    statistics
}

#[test]
fn begin_brings_up_the_children_record_mode_and_imaging_then_the_measurement() {
    let mut fake = Fake::default();
    let mut statistics = S1Statistics::new();
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Begin), Outcome::Pending);
    assert_eq!(fake.take(), [
        Seen::Acquisition(AcquisitionControl::Initialize),
        Seen::Search(SearchControl::Initialize),
        Seen::RecordMode(true),
        Seen::Snapshot,
        Seen::Request(Request::BeginImaging),
    ]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image57)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Start)]);
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Begin), Outcome::Fault);
    assert_eq!(fake.take(), [Seen::Fault]);
}

#[test]
fn a_half_press_whose_children_finish_at_once_stops_and_ends_the_imaging() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    fake.acquisition.push_back(ChildOutcome::Complete);
    fake.search.push_back(ChildOutcome::Complete);
    assert!(statistics.results_open());
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Prepare), Outcome::Pending);
    assert_eq!(fake.take(), [
        Seen::Acquisition(AcquisitionControl::Start),
        Seen::Search(SearchControl::Stop),
        Seen::Request(Request::Stop { pause: false }),
    ]);
    assert!(!statistics.results_open());
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7c)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::EndImaging)]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image58)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::RecordMode(false)]);
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Begin), Outcome::Pending);
}

#[test]
fn a_half_press_stops_once_the_forwarded_results_finish_both_children() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::PrepareCancellable), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Acquisition(AcquisitionControl::Start), Seen::Search(SearchControl::Conclude)]);
    fake.acquisition.push_back(ChildOutcome::Complete);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Calculation9b)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::AcquisitionForward(C::Calculation9b)]);
    assert!(statistics.results_open());
    fake.search.push_back(ChildOutcome::Complete);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image5a)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::SearchForward(C::Image5a), Seen::Request(Request::Stop { pause: false })]);
    assert!(!statistics.results_open());
}

#[test]
fn before_the_half_press_the_search_samples_return_the_search_outcome() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    fake.search.push_back(ChildOutcome::Complete);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Calculation9d)), Outcome::Complete);
    fake.search.push_back(ChildOutcome::Inactive);
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Cancel), Outcome::Inactive);
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Resume), Outcome::Pending);
    assert_eq!(fake.take(), [
        Seen::SearchForward(C::Calculation9d),
        Seen::Search(SearchControl::Cancel),
        Seen::Apply(Apply::Resume),
        Seen::Search(SearchControl::Resume),
    ]);
}

#[test]
fn an_active_view_takes_a_selection_before_running() {
    let mut fake = Fake { view_active: true, ..Fake::default() };
    let mut statistics = S1Statistics::new();
    send(&mut statistics, &mut fake, StatisticsEvent::Begin);
    send(&mut statistics, &mut fake, completed(C::Image57));
    fake.take();
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7b)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Selection)]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7f)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7e)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7f)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Apply(Apply::Results)]);
}

#[test]
fn a_half_press_during_the_selection_closes_the_results() {
    let mut fake = Fake { view_active: true, ..Fake::default() };
    let mut statistics = S1Statistics::new();
    send(&mut statistics, &mut fake, StatisticsEvent::Begin);
    send(&mut statistics, &mut fake, completed(C::Image57));
    send(&mut statistics, &mut fake, completed(C::Statistics7b));
    fake.take();
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Prepare), Outcome::Pending);
    assert!(!statistics.results_open());
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7e)), Outcome::Pending);
    assert_eq!(fake.take(), [
        Seen::Acquisition(AcquisitionControl::Start),
        Seen::Search(SearchControl::Stop),
        Seen::Request(Request::Stop { pause: false }),
    ]);
}

#[test]
fn a_half_press_before_the_imaging_is_configured_ends_it() {
    let mut fake = Fake::default();
    let mut statistics = S1Statistics::new();
    send(&mut statistics, &mut fake, StatisticsEvent::Begin);
    fake.take();
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::PrepareCancellable), Outcome::Pending);
    assert_eq!(fake.take(), [
        Seen::Acquisition(AcquisitionControl::Start),
        Seen::Search(SearchControl::Stop),
        Seen::Request(Request::EndImaging),
    ]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image57)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image58)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::RecordMode(false)]);
}

#[test]
fn an_autofocus_retune_while_running_replies_once_the_capture_block_is_sent() {
    let mut fake = Fake { change: Some(AutofocusChange::Retune), ..Fake::default() };
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Retune)]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics80)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::Reply(Reply::Autofocus)]);
}

#[test]
fn an_autofocus_retune_while_starting_waits_for_the_start() {
    let mut fake = Fake { change: Some(AutofocusChange::Retune), ..Fake::default() };
    let mut statistics = S1Statistics::new();
    send(&mut statistics, &mut fake, StatisticsEvent::Begin);
    send(&mut statistics, &mut fake, completed(C::Image57));
    fake.take();
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Pending);
    assert_eq!(fake.take(), []);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7b)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Retune)]);
}

#[test]
fn an_autofocus_lens_change_replies_after_the_lens_moves() {
    let mut fake = Fake { change: Some(AutofocusChange::Lens), ..Fake::default() };
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::MoveLens]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Lens35)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Reply(Reply::Autofocus)]);

    fake.lens_in_place = true;
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::MoveLens, Seen::Reply(Reply::Autofocus)]);
}

#[test]
fn an_unchanged_autofocus_replies_at_once() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::Reply(Reply::Autofocus)]);
}

#[test]
fn a_restarting_change_reimages_after_the_stop_and_the_acquisition() {
    let mut fake = Fake { restart: true, ..Fake::default() };
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Stop { pause: false })]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7c)), Outcome::Pending);
    assert_eq!(fake.take(), []);
    fake.acquisition.push_back(ChildOutcome::Complete);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Calculation9c)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::AcquisitionForward(C::Calculation9c), Seen::Request(Request::EndImaging)]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image58)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Acquisition(AcquisitionControl::Initialize), Seen::Snapshot, Seen::Request(Request::BeginImaging)]);
}

#[test]
fn a_second_restart_reimages_on_the_stop_alone() {
    let mut fake = Fake { restart: true, ..Fake::default() };
    let mut statistics = running(&mut fake);
    send(&mut statistics, &mut fake, command(K::Autofocus));
    send(&mut statistics, &mut fake, completed(C::Statistics7c));
    fake.acquisition.push_back(ChildOutcome::Complete);
    send(&mut statistics, &mut fake, completed(C::Calculation9b));
    send(&mut statistics, &mut fake, completed(C::Image58));
    fake.restart = false;
    send(&mut statistics, &mut fake, completed(C::Image57));
    send(&mut statistics, &mut fake, completed(C::Statistics7b));
    fake.restart = true;
    fake.take();
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7c)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Stop { pause: false }), Seen::Request(Request::EndImaging)]);
}

#[test]
fn a_restart_asked_for_while_imaging_starts_then_stops_and_restarts_the_acquisition() {
    let mut fake = Fake { restart: true, ..Fake::default() };
    let mut statistics = S1Statistics::new();
    send(&mut statistics, &mut fake, StatisticsEvent::Begin);
    fake.take();
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Pending);
    assert_eq!(fake.take(), []);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image57)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Start)]);
    fake.acquisition.push_back(ChildOutcome::Complete);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7b)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::Stop { pause: false }), Seen::Acquisition(AcquisitionControl::Start)]);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7c)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Request(Request::EndImaging)]);
}

#[test]
fn command1d_sends_the_output_or_pauses_before_its_reply() {
    let mut fake = Fake { output_needed: true, ..Fake::default() };
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Command1d)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics75)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::Apply(Apply::Command1d), Seen::Request(Request::Output), Seen::Reply(Reply::Command1d)]);

    fake.output_needed = false;
    assert_eq!(send(&mut statistics, &mut fake, command(K::Command1d)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7c)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::Apply(Apply::Command1d), Seen::Request(Request::Stop { pause: true }), Seen::Reply(Reply::Command1d)]);
}

#[test]
fn command02_restarts_from_a_new_snapshot_and_command03_pauses() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Command02)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7b)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::Snapshot, Seen::Request(Request::Start), Seen::Reply(Reply::Command02)]);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Command03)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7c)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::Request(Request::Stop { pause: true }), Seen::Reply(Reply::Command03)]);
}

#[test]
fn profile_commands_apply_while_the_measurement_is_up() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Command17)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Command27)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Prepare)), Outcome::Complete);
    assert_eq!(fake.take(), [Seen::Apply(Apply::Command17), Seen::Request(Request::LensOption), Seen::Apply(Apply::Prepare)]);
}

#[test]
fn an_imaging_report_stops_the_acquisition_and_starts_the_search() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image59)), Outcome::Pending);
    assert_eq!(fake.take(), [Seen::Acquisition(AcquisitionControl::Stop), Seen::Search(SearchControl::Start)]);
}

#[test]
fn unknown_events_are_inactive_and_empty_cells_fault_without_the_reaction() {
    let mut fake = Fake::default();
    let mut statistics = S1Statistics::new();
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Lens33)), Outcome::Inactive);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Command11)), Outcome::Inactive);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Calculation9b)), Outcome::Fault);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Lens35)), Outcome::Fault);
    assert_eq!(fake.take(), []);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image57)), Outcome::Fault);
    assert_eq!(send(&mut statistics, &mut fake, command(K::Autofocus)), Outcome::Fault);
    assert_eq!(fake.take(), [Seen::Fault, Seen::Fault]);
}

#[test]
fn results_are_delivered_only_while_running() {
    let mut fake = Fake::default();
    let mut statistics = S1Statistics::new();
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7f)), Outcome::Pending);
    send(&mut statistics, &mut fake, StatisticsEvent::Begin);
    fake.take();
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7f)), Outcome::Fault);
    assert_eq!(fake.take(), [Seen::Fault]);
}

#[test]
fn stop_closes_the_callback_gate_before_the_peer_can_reply_and_begin_reopens_it() {
    let mut fake = Fake::default();
    let mut statistics = running(&mut fake);
    fake.acquisition.push_back(ChildOutcome::Complete);
    fake.search.push_back(ChildOutcome::Complete);
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Prepare), Outcome::Pending);
    assert_eq!(fake.reports_on_requests.last(), Some(&(Request::Stop { pause: false }, false)));
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Statistics7c)), Outcome::Pending);
    assert_eq!(send(&mut statistics, &mut fake, completed(C::Image58)), Outcome::Complete);
    assert_eq!(send(&mut statistics, &mut fake, StatisticsEvent::Begin), Outcome::Pending);
    assert_eq!(fake.reports_on_requests.last(), Some(&(Request::BeginImaging, true)));
}
