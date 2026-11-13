use crate::{live::*, root::Outcome};
use std::{collections::VecDeque, vec, vec::Vec};
use xm1_saclac_interface::{stock, CommandKind, Completed, Completion, Message, RecordHandle};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Traffic {
    Live(Request<'static>, RecordHandle),
    Lens(Request<'static>, RecordHandle),
    Available(Profile, RecordHandle),
    Publish(Profile, RecordHandle),
    Fault,
}

struct Peers { traffic: Vec<Traffic>, outcomes: VecDeque<Outcome>, available: bool }

fn record() -> RecordHandle { stock::record_handle_from_a2(73) }
fn completion() -> Message<'static> {
    Message::Completed(Completed { completion: Completion::Image3f, parameters: stock::parameters(&[7, 3]) })
}

fn owned(request: Request<'_>) -> Request<'static> {
    match request {
        Request::Control(control) => Request::Control(control),
        Request::Command(command) => Request::Command(command),
        Request::Message(Message::Completed(done)) => {
            assert_eq!(done.parameters.bytes(), &[7, 3]);
            Request::Message(completion())
        }
        Request::Message(Message::Command(_)) => panic!("unexpected command"),
    }
}

impl Port for Peers {
    fn live(&mut self, request: Request<'_>, record: RecordHandle) -> Outcome {
        self.traffic.push(Traffic::Live(owned(request), record));
        self.outcomes.pop_front().expect("live response")
    }
    fn lens(&mut self, request: Request<'_>, record: RecordHandle) -> Outcome {
        self.traffic.push(Traffic::Lens(owned(request), record));
        self.outcomes.pop_front().expect("lens response")
    }
    fn profile_available(&mut self, profile: Profile, record: RecordHandle) -> bool {
        self.traffic.push(Traffic::Available(profile, record)); self.available
    }
    fn publish_profile(&mut self, profile: Profile, record: RecordHandle) {
        self.traffic.push(Traffic::Publish(profile, record));
    }
    fn fault(&mut self) { self.traffic.push(Traffic::Fault); }
}

#[test]
fn primary_pair_publishes_after_both_participants_complete() {
    let mut profiles = Profiles::new();
    let mut peers = Peers { traffic: Vec::new(), outcomes: [Outcome::Complete; 3].into(), available: true };
    assert_eq!(profiles.begin(Profile::Primary, record(), &mut peers), Outcome::Complete);
    assert_eq!(peers.traffic, vec![
        Traffic::Available(Profile::Primary, record()),
        Traffic::Live(Request::Control(Control::Suspend), record()),
        Traffic::Lens(Request::Command(CommandKind::Command02), record()),
        Traffic::Live(Request::Command(CommandKind::Command02), record()),
        Traffic::Publish(Profile::Primary, record()),
    ]);
    assert_eq!(profiles.handle(completion(), &mut peers), Outcome::Inactive);
}

#[test]
fn primary_pending_stages_resume_before_publishing() {
    for pending_stage in 0..3 {
        let mut profiles = Profiles::new();
        let mut outcomes = VecDeque::new();
        for stage in 0..3 {
            if stage == pending_stage { outcomes.push_back(Outcome::Pending); }
            outcomes.push_back(Outcome::Complete);
        }
        outcomes.push_back(Outcome::Complete);
        let mut peers = Peers { traffic: Vec::new(), outcomes, available: true };
        assert_eq!(profiles.begin(Profile::Primary, record(), &mut peers), Outcome::Pending);
        assert!(!peers.traffic.iter().any(|event| matches!(event, Traffic::Publish(..))));
        assert_eq!(profiles.handle(completion(), &mut peers), Outcome::Complete);
        let mut expected = vec![
            Traffic::Available(Profile::Primary, record()),
            Traffic::Live(Request::Control(Control::Suspend), record()),
            Traffic::Lens(Request::Command(CommandKind::Command02), record()),
            Traffic::Live(Request::Command(CommandKind::Command02), record()),
            Traffic::Live(Request::Control(Control::Resume), record()),
            Traffic::Publish(Profile::Primary, record()),
        ];
        let callback = if pending_stage == 1 { Traffic::Lens(Request::Message(completion()), record()) }
            else { Traffic::Live(Request::Message(completion()), record()) };
        expected.insert(pending_stage + 2, callback);
        assert_eq!(peers.traffic, expected);
    }
}

#[test]
fn absent_primary_never_suspends_or_resumes_the_lens_pair() {
    let mut profiles = Profiles::new();
    let mut peers = Peers { traffic: Vec::new(), outcomes: [Outcome::Pending, Outcome::Complete].into(), available: false };
    assert_eq!(profiles.begin(Profile::Primary, record(), &mut peers), Outcome::Pending);
    assert_eq!(profiles.handle(completion(), &mut peers), Outcome::Complete);
    assert_eq!(peers.traffic, vec![
        Traffic::Available(Profile::Primary, record()),
        Traffic::Live(Request::Command(CommandKind::Command02), record()),
        Traffic::Live(Request::Message(completion()), record()),
        Traffic::Publish(Profile::Primary, record()),
    ]);
}

#[test]
fn secondary_checks_availability_after_live_finishes_and_suspends_after_waiting() {
    let mut profiles = Profiles::new();
    let mut peers = Peers { traffic: Vec::new(), outcomes: [Outcome::Pending, Outcome::Complete, Outcome::Complete, Outcome::Pending, Outcome::Complete].into(), available: true };
    assert_eq!(profiles.begin(Profile::Secondary, record(), &mut peers), Outcome::Pending);
    assert_eq!(profiles.handle(completion(), &mut peers), Outcome::Pending);
    assert_eq!(profiles.handle(completion(), &mut peers), Outcome::Complete);
    assert_eq!(peers.traffic, vec![
        Traffic::Live(Request::Command(CommandKind::Command03), record()),
        Traffic::Live(Request::Message(completion()), record()),
        Traffic::Available(Profile::Secondary, record()),
        Traffic::Live(Request::Control(Control::Suspend), record()),
        Traffic::Lens(Request::Command(CommandKind::Command03), record()),
        Traffic::Lens(Request::Message(completion()), record()),
        Traffic::Publish(Profile::Secondary, record()),
    ]);
}

#[test]
fn occupied_profile_refuses_reentry_without_replacing_the_record() {
    let mut profiles = Profiles::new();
    let mut peers = Peers { traffic: Vec::new(), outcomes: [Outcome::Pending, Outcome::Inactive, Outcome::Complete].into(), available: false };
    assert_eq!(profiles.begin(Profile::Primary, record(), &mut peers), Outcome::Pending);
    assert_eq!(profiles.begin(Profile::Secondary, stock::record_handle_from_a2(74), &mut peers), Outcome::Fault);
    assert_eq!(profiles.handle(completion(), &mut peers), Outcome::Inactive);
    assert_eq!(profiles.handle(completion(), &mut peers), Outcome::Complete);
    assert_eq!(peers.traffic.last(), Some(&Traffic::Publish(Profile::Primary, record())));
}
