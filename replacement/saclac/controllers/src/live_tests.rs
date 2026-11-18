use crate::{live::*, root::{Control as RootControl, Input, Outcome}};
use std::{collections::VecDeque, vec, vec::Vec};
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completed, Completion as C, Message, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seen { Forward, Reissue(K), Suspend, Resume, Prepare, DriveStopped }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic {
    Controller(Seen, RecordHandle),
    Lens(Seen, RecordHandle),
    Follows(Profile),
    Effect(Recorded),
    Origin,
    CheckApplies,
    Correct,
    Fault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Recorded {
    ReplyProfile(Profile),
    ReplyAutofocus,
    PrepareAutofocusLens,
    ReportLensStatus(u8),
    CaptureLensPosition,
    RecordDriveOrigin(DriveOrigin),
    RecordDriveResult(u8),
}

struct Peers {
    traffic: Vec<Traffic>,
    controller: VecDeque<Outcome>,
    lens: VecDeque<Outcome>,
    follows: bool,
    origin: DriveOrigin,
    check: bool,
    correction: Outcome,
}

impl Peers {
    fn new(controller: &[Outcome], lens: &[Outcome]) -> Self {
        Self {
            traffic: Vec::new(),
            controller: controller.iter().copied().collect(),
            lens: lens.iter().copied().collect(),
            follows: true,
            origin: DriveOrigin::Command,
            check: false,
            correction: Outcome::Complete,
        }
    }
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x1a20) }
fn command(kind: K) -> Input<'static> {
    Input::Message(Message::Command(Command { kind, parameters: stock::parameters(&[]) }))
}
fn completed(completion: C) -> Input<'static> {
    Input::Message(Message::Completed(Completed { completion, parameters: stock::parameters(&[0x5c]) }))
}
fn other() -> Input<'static> { completed(C::Image3f) }

fn first_byte(parameters: xm1_saclac_interface::Parameters<'_>) -> u8 { parameters.bytes()[0] }

impl Port for Peers {
    fn controller(&mut self, event: ControllerEvent<'_>, record: RecordHandle) -> Outcome {
        let seen = match event {
            ControllerEvent::Forward(_) => Seen::Forward,
            ControllerEvent::Reissue(kind) => Seen::Reissue(kind),
            ControllerEvent::Suspend => Seen::Suspend,
            ControllerEvent::Resume => Seen::Resume,
        };
        self.traffic.push(Traffic::Controller(seen, record));
        self.controller.pop_front().expect("controller outcome")
    }
    fn lens(&mut self, event: LensEvent<'_>, record: RecordHandle) -> Outcome {
        let seen = match event {
            LensEvent::Forward(_) => Seen::Forward,
            LensEvent::Reissue(kind) => Seen::Reissue(kind),
            LensEvent::Prepare => Seen::Prepare,
            LensEvent::DriveStopped => Seen::DriveStopped,
        };
        self.traffic.push(Traffic::Lens(seen, record));
        self.lens.pop_front().expect("lens outcome")
    }
    fn lens_follows(&mut self, profile: Profile, record: RecordHandle) -> bool {
        assert_eq!(record, self::record());
        self.traffic.push(Traffic::Follows(profile));
        self.follows
    }
    fn effect(&mut self, effect: Effect<'_>, record: RecordHandle) {
        assert_eq!(record, self::record());
        self.traffic.push(Traffic::Effect(match effect {
            Effect::ReplyProfile(profile) => Recorded::ReplyProfile(profile),
            Effect::ReplyAutofocus => Recorded::ReplyAutofocus,
            Effect::PrepareAutofocusLens => Recorded::PrepareAutofocusLens,
            Effect::ReportLensStatus(parameters) => Recorded::ReportLensStatus(first_byte(parameters)),
            Effect::CaptureLensPosition => Recorded::CaptureLensPosition,
            Effect::RecordDriveOrigin(origin) => Recorded::RecordDriveOrigin(origin),
            Effect::RecordDriveResult(parameters) => Recorded::RecordDriveResult(first_byte(parameters)),
        }));
    }
    fn drive_origin(&mut self) -> DriveOrigin { self.traffic.push(Traffic::Origin); self.origin }
    fn drive_check_applies(&mut self, _: RecordHandle) -> bool { self.traffic.push(Traffic::CheckApplies); self.check }
    fn correct_drive(&mut self, _: RecordHandle) -> Outcome { self.traffic.push(Traffic::Correct); self.correction }
    fn fault(&mut self) { self.traffic.push(Traffic::Fault); }
}

use Outcome::{Complete as Done, Fault as Refused, Inactive, Pending};

fn c(seen: Seen) -> Traffic { Traffic::Controller(seen, record()) }
fn l(seen: Seen) -> Traffic { Traffic::Lens(seen, record()) }
fn e(recorded: Recorded) -> Traffic { Traffic::Effect(recorded) }

fn run(router: &mut Router, peers: &mut Peers, input: Input<'_>) -> Outcome { router.handle(input, record(), peers) }

/// Only an idle router accepts a profile command.
fn assert_idle(router: &mut Router) {
    let mut peers = Peers::new(&[Done], &[]);
    peers.follows = false;
    assert_eq!(run(router, &mut peers, command(K::Command02)), Done);
    assert_eq!(peers.traffic, vec![Traffic::Follows(Profile::Primary), c(Seen::Forward), e(Recorded::ReplyProfile(Profile::Primary))]);
}

#[test]
fn primary_profile_suspends_live_around_the_lens_and_replies_without_resuming_when_synchronous() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Done], &[Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command02)), Done);
    assert_eq!(peers.take(), vec![
        Traffic::Follows(Profile::Primary), c(Seen::Suspend), l(Seen::Forward), c(Seen::Forward),
        e(Recorded::ReplyProfile(Profile::Primary)),
    ]);
    assert_idle(&mut router);
}

#[test]
fn primary_profile_reissues_the_command_and_resumes_after_any_wait() {
    // Suspension waits: the lens and controller then see the command again.
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Done, Done, Done], &[Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command02)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        Traffic::Follows(Profile::Primary), c(Seen::Suspend),
        c(Seen::Forward), l(Seen::Reissue(K::Command02)), c(Seen::Reissue(K::Command02)), c(Seen::Resume),
        e(Recorded::ReplyProfile(Profile::Primary)),
    ]);

    // A lens wait forwards each event to the controller first and ignores its result.
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Refused, Inactive, Done, Pending, Done], &[Pending, Inactive, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command02)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        Traffic::Follows(Profile::Primary), c(Seen::Suspend), l(Seen::Forward),
        c(Seen::Forward), l(Seen::Forward),
        c(Seen::Forward), l(Seen::Forward), c(Seen::Reissue(K::Command02)), c(Seen::Resume),
        c(Seen::Forward), e(Recorded::ReplyProfile(Profile::Primary)),
    ]);
    assert_idle(&mut router);
}

#[test]
fn primary_profile_without_the_lens_waits_on_the_controller_alone() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Inactive, Done], &[]);
    peers.follows = false;
    assert_eq!(run(&mut router, &mut peers, command(K::Command02)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Inactive);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        Traffic::Follows(Profile::Primary), c(Seen::Forward), c(Seen::Forward), c(Seen::Forward),
        e(Recorded::ReplyProfile(Profile::Primary)),
    ]);
}

#[test]
fn secondary_profile_asks_about_the_lens_after_the_controller_and_resumes_after_a_lens_wait() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Done, Done, Inactive, Pending, Pending, Done], &[Pending, Inactive, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command03)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Inactive);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        c(Seen::Forward),
        c(Seen::Forward), Traffic::Follows(Profile::Secondary), c(Seen::Suspend), l(Seen::Reissue(K::Command03)),
        c(Seen::Forward), l(Seen::Forward),
        c(Seen::Forward), l(Seen::Forward), c(Seen::Resume),
        c(Seen::Forward), e(Recorded::ReplyProfile(Profile::Secondary)),
    ]);
    assert_idle(&mut router);
}

#[test]
fn secondary_lens_wait_refuses_when_the_controller_completes_first() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Done], &[Pending]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command03)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Refused);
    assert_eq!(peers.take(), vec![c(Seen::Forward), Traffic::Follows(Profile::Secondary), l(Seen::Forward), c(Seen::Forward)]);
}

#[test]
fn autofocus_prepares_the_lens_and_replies_only_when_the_controller_finishes() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Pending, Done], &[]);
    assert_eq!(run(&mut router, &mut peers, command(K::Autofocus)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Command02)), Refused);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        e(Recorded::PrepareAutofocusLens), c(Seen::Forward), Traffic::Fault,
        c(Seen::Forward), c(Seen::Forward), e(Recorded::ReplyAutofocus),
    ]);
    assert_idle(&mut router);
}

#[test]
fn drive_from_idle_prepares_the_lens_while_suspending_then_issues_the_drive() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Done, Pending, Done], &[Inactive, Pending, Pending, Pending, Done]);
    peers.origin = DriveOrigin::Cancel;
    peers.check = true;
    peers.correction = Pending;
    assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::Cancel)), Inactive);
    // Suspension finishes before the lens has prepared: the drive is issued.
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(peers.take(), vec![
        e(Recorded::RecordDriveOrigin(DriveOrigin::Cancel)), c(Seen::Suspend), l(Seen::Prepare),
        l(Seen::Forward), c(Seen::Forward), l(Seen::Reissue(K::LensDrive)),
    ]);
    // A cancelled drive that needs correcting waits for the correction.
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens34)), Pending);
    assert_eq!(peers.take(), vec![
        e(Recorded::RecordDriveResult(0x5c)), Traffic::Origin, Traffic::CheckApplies, Traffic::Correct,
    ]);
    peers.correction = Done;
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens33)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens34)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Done);
    assert_eq!(peers.take(), vec![
        e(Recorded::ReportLensStatus(0x5c)),
        e(Recorded::RecordDriveResult(0x5c)), Traffic::Origin, Traffic::CheckApplies, Traffic::Correct,
        e(Recorded::CaptureLensPosition), c(Seen::Resume),
        l(Seen::Forward), c(Seen::Forward), l(Seen::DriveStopped),
    ]);
    assert_idle(&mut router);
}

#[test]
fn prepared_drive_that_waits_on_the_lens_is_followed_by_command10() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Pending, Done], &[Pending, Done, Inactive, Pending, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::LensDrive)), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(run(&mut router, &mut peers, other()), Pending);
    assert_eq!(peers.take(), vec![
        e(Recorded::RecordDriveOrigin(DriveOrigin::Command)), c(Seen::Suspend), l(Seen::Prepare),
        l(Seen::Forward), c(Seen::Forward),
        l(Seen::Forward), c(Seen::Forward), l(Seen::Reissue(K::LensDrive)), l(Seen::Reissue(K::Command10)),
    ]);
}

#[test]
fn stopped_drive_resumes_live_before_the_lens_sees_the_stop() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Done], &[Pending, Done]);
    peers.origin = DriveOrigin::Cancel;
    assert_eq!(run(&mut router, &mut peers, command(K::LensDrive)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens37)), Done);
    assert_eq!(peers.take(), vec![
        e(Recorded::RecordDriveOrigin(DriveOrigin::Command)), c(Seen::Suspend), l(Seen::Forward),
        e(Recorded::CaptureLensPosition), c(Seen::Resume), l(Seen::Forward),
    ]);
    assert_idle(&mut router);
}

#[test]
fn command11_held_through_a_drive_returns_to_the_hold_and_completes_on_statistics() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending, Done, Pending, Done, Done, Done], &[Pending, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command11)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::LensDrive)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Autofocus)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens34)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Statistics6e)), Done);
    assert_eq!(peers.take(), vec![
        c(Seen::Forward),
        e(Recorded::RecordDriveOrigin(DriveOrigin::Command)), c(Seen::Suspend), l(Seen::Forward),
        c(Seen::Forward), e(Recorded::ReplyAutofocus),
        e(Recorded::RecordDriveResult(0x5c)), Traffic::Origin, e(Recorded::CaptureLensPosition), c(Seen::Resume), l(Seen::Forward),
        c(Seen::Forward),
    ]);
    assert_idle(&mut router);
}

#[test]
fn command11_arriving_during_a_drive_moves_the_drive_under_the_hold() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Done, Pending, Done, Done], &[Pending, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::LensDrive)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Command11)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Statistics6e)), Pending);
    assert_eq!(run(&mut router, &mut peers, completed(C::Lens37)), Done);
    assert_idle(&mut router);
}

#[test]
fn forwarded_holds_settle_refusals_as_pending_and_end_on_their_completion() {
    for (start, end, lens) in [
        (K::Command14, C::Image47, false),
        (K::Command2b, C::Image66, false),
        (K::Command2c, C::Lens3d, true),
    ] {
        let mut router = Router::new();
        let (controller, lens_outcomes): (&[Outcome], &[Outcome]) = if lens { (&[], &[Refused]) } else { (&[Refused], &[]) };
        let mut peers = Peers::new(controller, lens_outcomes);
        assert_eq!(run(&mut router, &mut peers, command(start)), Pending);
        assert_idle(&mut router);

        let (controller, lens_outcomes): (&[Outcome], &[Outcome]) = if lens { (&[Inactive], &[Pending, Done]) } else { (&[Pending, Inactive, Done], &[]) };
        let mut peers = Peers::new(controller, lens_outcomes);
        assert_eq!(run(&mut router, &mut peers, command(start)), Pending);
        assert_eq!(run(&mut router, &mut peers, other()), Inactive);
        assert_eq!(run(&mut router, &mut peers, completed(end)), Done);
        assert_idle(&mut router);
    }
}

#[test]
fn command2d_is_answered_by_the_lens_only_when_idle_or_holding_command2c() {
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending], &[Inactive, Pending, Done]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command2d)), Inactive);
    assert_eq!(run(&mut router, &mut peers, command(K::Command2c)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Command2d)), Pending);
    assert_eq!(peers.take(), vec![l(Seen::Forward), l(Seen::Forward), l(Seen::Forward)]);

    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending], &[]);
    assert_eq!(run(&mut router, &mut peers, command(K::Command23)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Command2d)), Refused);
    assert_eq!(peers.take(), vec![c(Seen::Forward)]);
}

#[test]
fn idle_events_enter_both_children_and_complete_when_either_does() {
    for (lens, controller, outcome, entered) in [
        (Done, Pending, Done, 2),
        (Pending, Done, Done, 2),
        (Inactive, Inactive, Inactive, 2),
        (Inactive, Pending, Pending, 2),
        (Refused, Done, Refused, 1),
        (Pending, Refused, Refused, 2),
    ] {
        let mut router = Router::new();
        let mut peers = Peers::new(&[controller], &[lens]);
        assert_eq!(run(&mut router, &mut peers, Input::Control(RootControl::SuspendLive)), outcome);
        assert_eq!(peers.traffic.len(), entered);
        assert!(!peers.traffic.contains(&Traffic::Fault));
    }
}

#[test]
fn events_with_no_meaning_in_the_current_state_take_the_fault_reaction() {
    for input in [completed(C::Lens34), completed(C::Lens35), completed(C::Statistics6e), completed(C::Image47)] {
        let mut router = Router::new();
        let mut peers = Peers::new(&[], &[]);
        assert_eq!(run(&mut router, &mut peers, input), Refused);
        assert_eq!(peers.traffic.last(), Some(&Traffic::Fault));
    }
    let mut router = Router::new();
    let mut peers = Peers::new(&[Pending], &[]);
    peers.follows = false;
    assert_eq!(run(&mut router, &mut peers, command(K::Command02)), Pending);
    assert_eq!(run(&mut router, &mut peers, command(K::Command03)), Refused);
    assert_eq!(run(&mut router, &mut peers, command(K::LensDrive)), Refused);
    assert_eq!(peers.take(), vec![
        Traffic::Follows(Profile::Primary), c(Seen::Forward), Traffic::Fault,
        e(Recorded::RecordDriveOrigin(DriveOrigin::Command)), Traffic::Fault,
    ]);
}
