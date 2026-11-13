use crate::{lifecycle::{Effects, Request, Startup}, root::*};
use std::{collections::VecDeque, vec::Vec};
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completed, Completion, Message, Parameters, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Initialize,
    Context(Context, Allocation),
    Release(Context, Generation),
    Services,
    Stop,
    Lifecycle(Request),
    Child(Branch, Input<'static>, RecordHandle),
    Utility(Utility, RecordHandle),
    Fault,
}

struct Peers {
    events: Vec<Event>,
    replies: VecDeque<Outcome>,
    startup: Startup,
    lens_route: bool,
    still: bool,
    live_option: bool,
}

impl Default for Peers {
    fn default() -> Self {
        Self { events: Vec::new(), replies: VecDeque::new(), startup: Startup::ResumeLive, lens_route: false, still: true, live_option: true }
    }
}

fn handle(context: Context) -> RecordHandle {
    stock::record_handle_from_a2(match context { Context::Root => 10, Context::Lens => 11, Context::Live => 12, Context::S1 => 13 })
}

fn input(kind: K) -> Input<'static> { Input::Message(command(kind)) }
fn command(kind: K) -> Message<'static> {
    Message::Command(Command { kind, parameters: stock::parameters(&[]) })
}
fn completion() -> Message<'static> {
    Message::Completed(Completed { completion: Completion::Calculation87, parameters: stock::parameters(&[]) })
}

impl Effects for Peers {
    fn emit(&mut self, request: Request) { self.events.push(Event::Lifecycle(request)); }
}

impl Port for Peers {
    fn startup(&mut self) -> Startup { self.startup }
    fn initialize_contexts(&mut self) { self.events.push(Event::Initialize); }
    fn context(&mut self, context: Context, allocation: Allocation, _: Input<'_>) -> RecordHandle {
        self.events.push(Event::Context(context, allocation)); handle(context)
    }
    fn release(&mut self, context: Context, generation: Generation) { self.events.push(Event::Release(context, generation)); }
    fn initialize_services(&mut self, _: RecordHandle, _: Parameters<'_>) { self.events.push(Event::Services); }
    fn stop_services(&mut self) { self.events.push(Event::Stop); }
    fn child(&mut self, branch: Branch, event: Input<'_>, record: RecordHandle) -> Outcome {
        let event = match event {
            Input::Control(control) => Input::Control(control),
            Input::Message(Message::Command(cmd)) => input(cmd.kind),
            Input::Message(Message::Completed(done)) => Input::Message(Message::Completed(Completed { completion: done.completion, parameters: stock::parameters(&[]) })),
        };
        self.events.push(Event::Child(branch, event, record));
        self.replies.pop_front().unwrap_or(Outcome::Complete)
    }
    fn utility(&mut self, utility: Utility, record: RecordHandle) { self.events.push(Event::Utility(utility, record)); }
    fn autofocus_admitted(&mut self) -> bool { true }
    fn s1_lens_route(&mut self, _: RecordHandle) -> bool { self.lens_route }
    fn still_record(&mut self, _: RecordHandle) -> bool { self.still }
    fn live_option_allowed(&mut self) -> bool { self.live_option }
    fn fault(&mut self) { self.events.push(Event::Fault); }
}

fn live() -> (Root, Peers) {
    let mut root = Root::new();
    let mut peers = Peers::default();
    assert_eq!(root.handle(command(K::Start), &mut peers), Outcome::Pending);
    peers.events.clear();
    (root, peers)
}

#[test]
fn startup_replies_before_releasing_the_lens_context() {
    let mut root = Root::new();
    let mut peers = Peers::default();
    root.handle(command(K::Start), &mut peers);
    assert_eq!(peers.events, [Event::Initialize, Event::Context(Context::Lens, Allocation::Replace),
        Event::Services, Event::Lifecycle(Request::ReplyStarted),
        Event::Release(Context::Lens, Generation::Current), Event::Release(Context::Lens, Generation::Previous)]);
}

#[test]
fn stopped_rejects_every_queue_command_other_than_start() {
    for descriptor in stock::COMMANDS {
        if descriptor.kind == K::Start { continue; }
        let mut root = Root::new();
        let mut peers = Peers::default();
        assert_eq!(root.handle(command(descriptor.kind), &mut peers), Outcome::Fault);
        assert_eq!(peers.events, [Event::Fault]);
    }
}

#[test]
fn live_forwards_parameter_and_lens_commands_in_order() {
    for kind in [K::Command02, K::Command03, K::Command17, K::Prepare, K::Command1d, K::Command1e,
        K::Command1f, K::Command20, K::Command21, K::Command22, K::LensDrive, K::Cancel, K::Command10,
        K::Command11, K::Command13, K::Command14, K::Command15, K::Command16, K::Command2b, K::Command2c,
        K::Command2d, K::Prepare26, K::Command29] {
        let (mut root, mut peers) = live();
        root.handle(command(kind), &mut peers);
        assert_eq!(peers.events, [Event::Context(Context::Live, Allocation::Reuse),
            Event::Child(Branch::Live, input(kind), handle(Context::Live))], "{kind:?}");
    }
}

#[test]
fn half_press_quiesces_live_before_starting_s1_with_the_live_record() {
    let (mut root, mut peers) = live();
    root.handle(command(K::HalfPress), &mut peers);
    assert_eq!(peers.events, [
        Event::Context(Context::S1, Allocation::Reuse), Event::Child(Branch::S1, input(K::HalfPress), handle(Context::S1)),
        Event::Context(Context::Live, Allocation::Reuse), Event::Child(Branch::Live, Input::Control(Control::SuspendLive), handle(Context::Live)),
        Event::Child(Branch::S1, Input::Control(Control::BeginS1), handle(Context::Live)),
    ]);
}

#[test]
fn pending_half_press_remembers_release_until_s1_finishes() {
    let (mut root, mut peers) = live();
    peers.replies.extend([Outcome::Complete, Outcome::Pending]);
    root.handle(command(K::HalfPress), &mut peers);
    root.handle(command(K::HalfRelease), &mut peers);
    peers.events.clear();
    root.handle(completion(), &mut peers);
    assert_eq!(peers.events, [
        Event::Context(Context::Live, Allocation::Reuse), Event::Child(Branch::Live, Input::Message(completion()), handle(Context::Live)),
        Event::Context(Context::S1, Allocation::Reuse), Event::Child(Branch::S1, Input::Control(Control::BeginS1), handle(Context::S1)),
    ]);
    peers.events.clear();
    root.handle(completion(), &mut peers);
    assert_eq!(peers.events, [
        Event::Context(Context::Live, Allocation::Reuse), Event::Child(Branch::S1, Input::Control(Control::QueryS1), handle(Context::Live)),
        Event::Context(Context::S1, Allocation::Reuse), Event::Child(Branch::S1, Input::Message(completion()), handle(Context::S1)),
        Event::Release(Context::S1, Generation::Current), Event::Context(Context::Live, Allocation::Reuse),
        Event::Child(Branch::Live, Input::Control(Control::ReleaseS1), handle(Context::Live)),
    ]);
}

#[test]
fn pending_autofocus_releases_previous_context_after_completion() {
    let (mut root, mut peers) = live();
    peers.replies.push_back(Outcome::Pending);
    root.handle(command(K::Autofocus), &mut peers);
    assert_eq!(peers.events, [Event::Context(Context::Live, Allocation::Replace),
        Event::Utility(Utility::PrepareAutofocus, handle(Context::Live)),
        Event::Child(Branch::Live, input(K::Autofocus), handle(Context::Live))]);
    peers.events.clear();
    root.handle(completion(), &mut peers);
    assert_eq!(peers.events, [Event::Context(Context::Live, Allocation::Reuse),
        Event::Child(Branch::Live, Input::Message(completion()), handle(Context::Live)),
        Event::Release(Context::Live, Generation::Previous)]);
}

#[test]
fn ordinary_stop_replies_before_releasing_current_and_previous_contexts() {
    let mut root = Root::new();
    let mut peers = Peers { startup: Startup::Normal, ..Peers::default() };
    root.handle(command(K::Start), &mut peers);
    peers.events.clear();
    root.handle(command(K::Stop), &mut peers);
    assert_eq!(peers.events, [Event::Context(Context::Lens, Allocation::Reuse), Event::Stop,
        Event::Lifecycle(Request::ReplyStopped),
        Event::Release(Context::Lens, Generation::Current), Event::Release(Context::Lens, Generation::Previous),
        Event::Release(Context::Live, Generation::Current), Event::Release(Context::Live, Generation::Previous),
        Event::Release(Context::S1, Generation::Current), Event::Release(Context::S1, Generation::Previous)]);
}

#[test]
fn deferred_option_is_forwarded_only_after_live_quiesces() {
    let (mut root, mut peers) = live();
    peers.replies.push_back(Outcome::Pending);
    root.handle(command(K::Command24), &mut peers);
    peers.events.clear();
    root.handle(completion(), &mut peers);
    assert_eq!(peers.events, [Event::Context(Context::Live, Allocation::Reuse),
        Event::Child(Branch::Live, Input::Message(completion()), handle(Context::Live)),
        Event::Child(Branch::LiveSequence, Input::Control(Control::RefreshDeferred), handle(Context::Live))]);
}

#[test]
fn live_admits_all_queue_commands_except_start_stop_and_capture_value() {
    for descriptor in stock::COMMANDS {
        let (mut root, mut peers) = live();
        let outcome = root.handle(command(descriptor.kind), &mut peers);
        let refused = matches!(descriptor.kind, K::Start | K::Stop | K::Command0a);
        assert_eq!(outcome == Outcome::Fault, refused, "{:?}", descriptor.kind);
        assert_eq!(peers.events.contains(&Event::Fault), refused, "{:?}", descriptor.kind);
    }
}

#[test]
fn capture_followup_routes_to_s1_without_quiescing_live_again() {
    for kind in [K::Command0c, K::Command28] {
        let (mut root, mut peers) = live();
        root.handle(command(kind), &mut peers);
        assert_eq!(peers.events, [Event::Context(Context::S1, Allocation::Reuse),
            Event::Child(Branch::S1, input(kind), handle(Context::S1))]);
    }
}

#[test]
fn lens_status_completion_restores_started_before_stop() {
    let (mut root, mut peers) = live();
    let message = Message::Completed(Completed { completion: Completion::Lens3c, parameters: stock::parameters(&[]) });
    root.handle(message, &mut peers);
    root.handle(command(K::Stop), &mut peers);
    assert_eq!(&peers.events[..3], &[
        Event::Context(Context::Lens, Allocation::Reuse), Event::Child(Branch::Lens, Input::Message(message), handle(Context::Lens)),
        Event::Release(Context::Lens, Generation::Current),
    ]);
    assert!(peers.events.contains(&Event::Lifecycle(Request::ReplyStopped)));
    assert!(!peers.events.contains(&Event::Fault));
}

#[test]
fn lens_start_and_parameter_completion_join_in_either_order() {
    for lens_first in [false, true] {
        let mut root = Root::new();
        let mut peers = Peers { startup: Startup::Normal, ..Peers::default() };
        root.handle(command(K::Start), &mut peers);
        peers.replies.extend([Outcome::Pending, Outcome::Pending]);
        root.handle(command(K::LensBringUp), &mut peers);
        root.handle(command(K::Command02), &mut peers);
        peers.replies.extend(if lens_first { [Outcome::Complete, Outcome::Pending] } else { [Outcome::Pending, Outcome::Complete] });
        root.handle(completion(), &mut peers);
        if lens_first {
            peers.replies.push_back(Outcome::Complete);
        } else {
            peers.replies.extend([Outcome::Complete, Outcome::Inactive]);
        }
        root.handle(completion(), &mut peers);
        peers.events.clear();
        root.handle(command(K::HalfPress), &mut peers);
        assert_eq!(peers.events[0], Event::Context(Context::S1, Allocation::Reuse));
        assert!(!peers.events.contains(&Event::Fault));
    }
}
