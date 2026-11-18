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
    S1LensRoute(RecordHandle),
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
    fn s1_lens_route(&mut self, record: RecordHandle) -> bool { self.events.push(Event::S1LensRoute(record)); self.lens_route }
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
        Event::Services, Event::Child(Branch::Live, Input::Control(Control::ResumeLive), handle(Context::Lens)),
        Event::Lifecycle(Request::ReplyStarted),
        Event::Release(Context::Lens, Generation::Current), Event::Release(Context::Lens, Generation::Previous)]);
}

#[test]
fn normal_start_does_not_bind_the_resume_live_path() {
    let mut root = Root::new();
    let mut peers = Peers { startup: Startup::Normal, ..Peers::default() };
    root.handle(command(K::Start), &mut peers);
    assert!(!peers.events.iter().any(|event| matches!(event, Event::Child(Branch::Live, Input::Control(Control::ResumeLive), _))));
}

#[test]
fn normal_lens_bring_up_starts_only_the_lens_path() {
    let mut root = Root::new();
    let mut peers = Peers { startup: Startup::Normal, ..Peers::default() };
    root.handle(command(K::Start), &mut peers);
    peers.events.clear();
    assert_eq!(root.handle(command(K::LensBringUp), &mut peers), Outcome::Pending);
    assert_eq!(peers.events, [
        Event::Context(Context::Lens, Allocation::Reuse),
        Event::Utility(Utility::RefreshLens, handle(Context::Lens)),
        Event::Child(Branch::Lens, input(K::LensBringUp), handle(Context::Lens)),
        Event::Release(Context::Lens, Generation::Current),
    ]);
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
        Event::S1LensRoute(handle(Context::Live)),
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

mod consumers {
    use super::*;
    use crate::{children::*, live, live_statistics as statistics, live_view as view, s1};

    #[derive(Default)]
    struct Boundary {
        view: view::Controller,
        preparation: view::PreparationOverride,
        requests: Vec<(view::ViewRequest, RecordHandle)>,
        replies: Vec<(Option<K>, view::ReplyStatus, RecordHandle)>,
        faults: usize,
        restored: Vec<(i16, u32)>,
        root_payload: Vec<u8>,
        statistics: statistics::LiveStatistics,
        statistics_requests: Vec<(statistics::Request, RecordHandle)>,
        statistics_replies: Vec<(bool, Vec<u8>, RecordHandle)>,
        released: Vec<(Context, Generation)>,
        direct_start: bool,
    }

    impl view::Port for Boundary {
        fn statistics(&mut self, call: view::StatisticsCall<'_>, record: RecordHandle) -> view::StatisticsOutcome {
            let mut statistics = core::mem::take(&mut self.statistics);
            let result = statistics.send(call, record, self);
            self.statistics = statistics;
            result
        }
        fn measurement(&mut self, _: ChildEvent<'_, MeasurementControl>) -> ChildOutcome { ChildOutcome::Complete }
        fn sequence(&mut self, _: ShotEvent<'_>) -> Outcome { Outcome::Complete }
        fn prepare(&mut self, _: Option<Command<'_>>, _: RecordHandle) -> view::Preparation { view::Preparation::default() }
        fn apply(&mut self, _: Command<'_>, _: RecordHandle) -> view::Application { view::Application::Applied }
        fn view(&mut self, request: view::ViewRequest, record: RecordHandle) { self.requests.push((request, record)); }
        fn effect(&mut self, _: view::Effect, _: RecordHandle) {}
        fn condition(&mut self, condition: view::Condition, _: Option<Command<'_>>, _: RecordHandle) -> bool {
            self.direct_start && condition == view::Condition::DirectStart
        }
        fn motion_setting(&mut self) -> view::MotionSetting { view::MotionSetting::default() }
        fn reply(&mut self, command: Option<K>, status: view::ReplyStatus, record: RecordHandle) { self.replies.push((command, status, record)); }
        fn fault(&mut self) { self.faults += 1; }
    }

    impl statistics::Port for Boundary {
        fn request(&mut self, request: statistics::Request, record: RecordHandle) { self.statistics_requests.push((request, record)); }
        fn reply(&mut self, reply: statistics::Reply<'_>, record: RecordHandle) {
            if let statistics::Reply::Command24 { refused, completion } = reply {
                self.statistics_replies.push((refused, completion.bytes().to_vec(), record));
            }
        }
        fn condition(&mut self, _: statistics::Condition, _: RecordHandle) -> bool { false }
        fn changes(&mut self, _: RecordHandle) -> statistics::Groups { statistics::Groups::default() }
        fn deliver_results(&mut self, _: RecordHandle) {}
        fn select_descriptor(&mut self, _: u32, _: RecordHandle) {}
        fn fault(&mut self) { self.faults += 1; }
    }

    impl Boundary {
        fn enter_view(&mut self, signal: view::Signal<'_>, record: RecordHandle) -> Outcome {
            let mut controller = core::mem::take(&mut self.view);
            let mut preparation = self.preparation;
            let result = controller.handle(signal, record, &mut preparation, self);
            self.preparation = preparation;
            self.view = controller;
            result
        }
        fn forward(&mut self, input: Input<'_>, record: RecordHandle) -> Outcome {
            let signal = match input {
                Input::Message(Message::Command(command)) => view::Signal::Command(command),
                Input::Message(Message::Completed(done)) => view::Signal::Completed(done),
                Input::Control(Control::ResumeLive) => view::Signal::ResumeLive,
                Input::Control(Control::SuspendLive) => return Outcome::Complete,
                _ => return Outcome::Fault,
            };
            self.enter_view(signal, record)
        }
    }

    impl live::Port for Boundary {
        fn controller(&mut self, event: live::ControllerEvent<'_>, record: RecordHandle) -> Outcome {
            match event {
                live::ControllerEvent::Forward(input) => self.forward(input, record),
                live::ControllerEvent::Reissue(kind) => self.forward(input(kind), record),
                _ => Outcome::Complete,
            }
        }
        fn lens(&mut self, _: live::LensEvent<'_>, _: RecordHandle) -> Outcome { Outcome::Inactive }
        fn lens_follows(&mut self, _: live::Profile, _: RecordHandle) -> bool { false }
        fn effect(&mut self, _: live::Effect<'_>, _: RecordHandle) {}
        fn drive_origin(&mut self) -> live::DriveOrigin { live::DriveOrigin::Command }
        fn drive_check_applies(&mut self, _: RecordHandle) -> bool { false }
        fn correct_drive(&mut self, _: RecordHandle) -> Outcome { Outcome::Complete }
        fn fault(&mut self) { self.faults += 1; }
    }

    impl s1::Port for Boundary {
        fn shot(&mut self, _: s1::ShotEvent<'_>, _: RecordHandle) -> Outcome { Outcome::Complete }
        fn statistics(&mut self, _: s1::StatisticsEvent<'_>, _: RecordHandle) -> Outcome { Outcome::Complete }
        fn capture(&mut self, _: s1::CaptureEvent<'_>, _: RecordHandle) -> Outcome { Outcome::Complete }
        fn lens(&mut self, _: s1::LensEvent<'_>, _: RecordHandle) -> Outcome { Outcome::Complete }
        fn live(&mut self, event: s1::LiveEvent<'_>, record: RecordHandle) -> Outcome {
            match event {
                s1::LiveEvent::Forward(input) => self.forward(input, record),
                s1::LiveEvent::Reissue(kind) => self.forward(input(kind), record),
            }
        }
        fn publish(&mut self, _: s1::Published) {}
        fn effect(&mut self, _: s1::Effect, _: RecordHandle) {}
        fn second_shot(&mut self, _: RecordHandle) -> bool { false }
        fn lens_cancellable(&mut self, _: RecordHandle) -> bool { false }
        fn release_allowed(&mut self) -> bool { true }
        fn fault(&mut self) { self.faults += 1; }
    }

    struct Composition { boundary: Boundary, live: live::Router, s1: s1::Router, startup: Startup }
    impl Composition {
        fn new(startup: Startup) -> Self { Self { boundary: Boundary::default(), live: live::Router::new(), s1: s1::Router::new(), startup } }
    }
    impl Effects for Composition { fn emit(&mut self, _: Request) {} }
    impl Port for Composition {
        fn startup(&mut self) -> Startup { self.startup }
        fn initialize_contexts(&mut self) {}
        fn context(&mut self, context: Context, _: Allocation, input: Input<'_>) -> RecordHandle {
            if context == Context::Root {
                if let Input::Message(Message::Completed(done)) = input { self.boundary.root_payload = done.parameters.bytes().to_vec(); }
            }
            handle(context)
        }
        fn release(&mut self, context: Context, generation: Generation) { self.boundary.released.push((context, generation)); }
        fn initialize_services(&mut self, _: RecordHandle, _: Parameters<'_>) {}
        fn stop_services(&mut self) {}
        fn child(&mut self, branch: Branch, input: Input<'_>, record: RecordHandle) -> Outcome {
            match branch {
                Branch::Live => self.live.handle(input, record, &mut self.boundary),
                Branch::S1 => self.s1.handle(input, record, &mut self.boundary),
                _ => Outcome::Complete,
            }
        }
        fn utility(&mut self, utility: Utility, record: RecordHandle) {
            if utility == Utility::RestoreLensStatus {
                assert_eq!(record, handle(Context::Root));
                let bytes = &self.boundary.root_payload;
                self.boundary.restored.push((i16::from_le_bytes(bytes[0..2].try_into().unwrap()), u32::from_le_bytes(bytes[4..8].try_into().unwrap())));
            }
        }
        fn autofocus_admitted(&mut self) -> bool { true }
        fn s1_lens_route(&mut self, _: RecordHandle) -> bool { false }
        fn still_record(&mut self, _: RecordHandle) -> bool { true }
        fn live_option_allowed(&mut self) -> bool { true }
        fn fault(&mut self) { self.boundary.faults += 1; }
    }

    #[test]
    fn lens_notice_restores_context_status_without_stopping_live() {
        let mut root = Root::new();
        let mut port = Composition::new(Startup::ResumeLive);
        root.handle(command(K::Start), &mut port);
        port.boundary.released.clear();
        let bytes = [0xfe, 0xff, 0, 0, 0x78, 0x56, 0x34, 0x12];
        let notice = Message::Completed(Completed { completion: Completion::Lens3c, parameters: stock::parameters(&bytes) });
        assert_eq!(root.handle(notice, &mut port), Outcome::Complete);
        assert_eq!(port.boundary.restored, [(-2, 0x12345678)]);
        assert_eq!(port.boundary.released, [(Context::Root, Generation::Current)]);
        assert_eq!(root.handle(command(K::Stop), &mut port), Outcome::Fault);
        assert_eq!(port.boundary.faults, 1);
    }

    #[test]
    fn started_window_completion_replies_to_client_and_allows_next_command() {
        let mut root = Root::new();
        let mut port = Composition::new(Startup::Normal);
        root.handle(command(K::Start), &mut port);
        let bytes = [0, 0, 0, 0, 0x39, 0, 0, 0];
        let window = Message::Command(Command { kind: K::Command29, parameters: stock::parameters(&bytes) });
        assert_eq!(root.handle(window, &mut port), Outcome::Pending);
        assert_eq!(port.boundary.requests, [(view::ViewRequest::Window(0x39), handle(Context::Live))]);
        assert!(port.boundary.replies.is_empty());
        let done = Message::Completed(Completed { completion: Completion::Image65, parameters: stock::parameters(&[]) });
        assert_eq!(root.handle(done, &mut port), Outcome::Pending);
        assert_eq!(port.boundary.replies, [(Some(K::Command29), view::ReplyStatus::Accepted, handle(Context::Live))]);
        assert_eq!(root.handle(command(K::Command23), &mut port), Outcome::Pending);
        assert_eq!(port.boundary.replies.last(), Some(&(Some(K::Command23), view::ReplyStatus::Accepted, handle(Context::Live))));
        assert_eq!(root.handle(command(K::Stop), &mut port), Outcome::Pending);
        assert_eq!(port.boundary.faults, 0);
    }

    #[test]
    fn started_command23_replies_and_releases_its_context() {
        let mut root = Root::new();
        let mut port = Composition::new(Startup::Normal);
        root.handle(command(K::Start), &mut port);
        port.boundary.released.clear();
        assert_eq!(root.handle(command(K::Command23), &mut port), Outcome::Pending);
        assert_eq!(port.boundary.replies, [(Some(K::Command23), view::ReplyStatus::Accepted, handle(Context::Live))]);
        assert_eq!(port.boundary.released, [(Context::Live, Generation::Current)]);
        assert_eq!(port.boundary.faults, 0);
    }

    #[test]
    fn normal_live_ignores_an_accessor_tick_before_a_profile_starts() {
        let mut root = Root::new();
        let mut port = Composition::new(Startup::Normal);
        root.handle(command(K::Start), &mut port);
        root.handle(command(K::LensBringUp), &mut port);
        assert_eq!(root.handle(Message::Completed(Completed { completion: Completion::Accessor4b, parameters: stock::parameters(&[]) }), &mut port), Outcome::Pending);
        assert!(port.boundary.statistics_requests.is_empty());
        assert!(port.boundary.requests.is_empty());
        assert_eq!(port.boundary.faults, 0);
    }

    #[test]
    fn normal_lens_start_then_primary_publishes_view_start_then_configure() {
        let mut root = Root::new();
        let mut port = Composition::new(Startup::Normal);
        port.boundary.direct_start = true;
        root.handle(command(K::Start), &mut port);
        root.handle(command(K::LensBringUp), &mut port);
        let parameters = [0; 20];
        let primary = Message::Command(Command { kind: K::Command02, parameters: stock::parameters(&parameters) });
        assert_eq!(root.handle(primary, &mut port), Outcome::Pending);
        assert_eq!(port.boundary.requests, [(view::ViewRequest::Start { measured: true }, handle(Context::Live))]);
        let started = Message::Completed(Completed { completion: Completion::Image3f, parameters: stock::parameters(&[]) });
        assert_eq!(root.handle(started, &mut port), Outcome::Pending);
        assert_eq!(port.boundary.requests.last(), Some(&(view::ViewRequest::Configure(view::MotionSetting(0)), handle(Context::Live))));
        assert_eq!(port.boundary.faults, 0);
    }

    #[test]
    fn pending_bridge_command17_is_silently_refused() {
        let mut root = Root::new();
        let mut port = Composition::new(Startup::ResumeLive);
        root.handle(command(K::Start), &mut port);
        root.handle(Message::Completed(Completed { completion: Completion::Accessor4b, parameters: stock::parameters(&[]) }), &mut port);
        root.handle(command(K::Command09), &mut port);
        let requests = port.boundary.requests.len();
        let replies = port.boundary.replies.len();
        assert_eq!(root.handle(command(K::Command17), &mut port), Outcome::Pending);
        assert_eq!(port.boundary.requests.len(), requests);
        assert_eq!(port.boundary.replies.len(), replies);
        assert_eq!(port.boundary.faults, 0);
        assert_eq!(root.handle(command(K::Autofocus), &mut port), Outcome::Fault);
        assert_eq!(port.boundary.faults, 1);
    }

    #[test]
    fn command24_reaches_statistics_and_waits_in_autofocus_route() {
        let mut root = Root::new();
        let mut port = Composition::new(Startup::ResumeLive);
        root.handle(command(K::Start), &mut port);
        root.handle(Message::Completed(Completed { completion: Completion::Accessor4b, parameters: stock::parameters(&[]) }), &mut port);
        assert_eq!(root.handle(command(K::Command24), &mut port), Outcome::Pending);
        assert_eq!(port.boundary.statistics_requests, [(statistics::Request::Command24, handle(Context::Live))]);
        assert!(port.boundary.requests.is_empty());
        assert_eq!(root.handle(command(K::Command24), &mut port), Outcome::Fault);
        assert_eq!(port.boundary.faults, 1);
        let bytes = [0xfa, 0xff, 0xff, 0xff, 0x78, 0x56, 0x34, 0x12];
        let completion = Message::Completed(Completed { completion: Completion::Statistics82, parameters: stock::parameters(&bytes) });
        assert_eq!(root.handle(completion, &mut port), Outcome::Pending);
        assert_eq!(port.boundary.statistics_replies, [(false, bytes.to_vec(), handle(Context::Live))]);
        assert_eq!(port.boundary.faults, 1);
    }
}
