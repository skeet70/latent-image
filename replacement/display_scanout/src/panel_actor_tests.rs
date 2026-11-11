use crate::panel::{Completion, PanelCommand, PanelRequest, TaskId};
use crate::panel_actor::*;
use crate::panel_device::{DeviceState, SequencePlan, SequenceStep};
use std::vec::Vec;

#[derive(Default)]
struct Runtime {
    callbacks: Vec<u32>,
    callback_observations: Vec<(bool, Activity)>,
    clears: usize,
    errors: Vec<u32>,
    signals: Vec<Event>,
    receive_status: i32,
    wait_status: i32,
    signal_status: i32,
    incoming: Option<Message>,
    observed: Option<EventFlags>,
    timer_calls: usize,
    stock_plan: bool,
}
impl ActorRuntime for Runtime {
    fn build_plan(&mut self, current: TransitionTarget, target: TransitionTarget) -> SequencePlan {
        if self.stock_plan { crate::panel_device::plan(current.code(), target.code()) } else { SequencePlan::empty() }
    }
    fn submit(&mut self, _: &SequencePlan, _: usize, _: &mut OperationToken) -> i32 { 0 }
    fn execute(&mut self, _: &mut DeviceState, _: SequenceStep) -> bool { true }
    fn invoke(&mut self, callback: Completion, state: &mut PanelState) {
        self.callbacks.push(callback.address());
        self.callback_observations.push((state.completion == Some(callback), state.activity));
        state.completion = Completion::parse(0x2220);
    }
    fn clear_completion(&mut self) { self.clears += 1; }
    fn error(&mut self, callback: Completion) { self.errors.push(callback.address()); }
    fn signal(&mut self, event: Event) -> i32 { self.signals.push(event); self.signal_status }
    fn post_initial(&mut self) -> i32 { 0 }
    fn receive(&mut self, message: &mut Message) -> i32 {
        if let Some(incoming) = self.incoming.take() { *message = incoming; }
        self.receive_status
    }
    fn wait(&mut self, events: &mut EventFlags) -> i32 {
        if let Some(observed) = self.observed.take() { *events = observed; }
        self.wait_status
    }
    fn timer_command(&mut self, _: &mut PanelState) { self.timer_calls += 1; }
}
fn request(command: PanelCommand) -> PanelRequest {
    PanelRequest { sender: TaskId(9), command, completion: Completion::parse(0x1110) }
}
#[test]
fn completion_invokes_before_clearing_callback_and_busy() {
    let mut state = PanelState::new(None);
    state.activity = Activity::Busy;
    state.completion = Completion::parse(0x1110);
    let mut runtime = Runtime::default();
    state.complete(&mut runtime);
    assert_eq!(runtime.callbacks, [0x1110]);
    assert_eq!(runtime.callback_observations, [(true, Activity::Busy)]);
    assert_eq!(runtime.clears, 1);
    assert_eq!(state.completion, None);
    assert_eq!(state.activity, Activity::Idle);
}
#[test]
fn immediate_transition_does_not_clear_callback_after_return() {
    let mut state = PanelState::new(None);
    let mut runtime = Runtime::default();
    state.begin(request(PanelCommand::Command6), SourceClass::Internal, &mut runtime);
    assert_eq!(runtime.callbacks, [0x1110]);
    assert_eq!(state.completion, Completion::parse(0x2220));
    assert_eq!(state.activity, Activity::Idle);
    assert_eq!(state.target.code(), 0);
}
#[test]
fn completion_event_has_priority_over_both_continuations() {
    let mut state = PanelState::new(None);
    state.activity = Activity::Busy;
    state.completion = Completion::parse(0x1110);
    state.events = EventFlags::new(7);
    let mut runtime = Runtime::default();
    assert_eq!(state.dispatch(SourceClass::Internal, &mut runtime), Ok(()));
    assert_eq!(runtime.callbacks, [0x1110]);
    assert_eq!(state.activity, Activity::Idle);
}
#[test]
fn receive_failure_reports_error_and_dispatches_preserved_message() {
    let mut state = PanelState::new(Completion::parse(0x3330));
    state.message = Message::Timer;
    let mut runtime = Runtime { receive_status: -1, ..Runtime::default() };
    state.task_iteration(SourceClass::Internal, &mut runtime);
    assert_eq!(runtime.errors, [0x3330]);
    assert_eq!(runtime.timer_calls, 1);
}
#[test]
fn failed_wait_still_dispatches_received_completion_bits() {
    let mut state = PanelState::new(Completion::parse(0x3330));
    state.activity = Activity::Busy;
    state.completion = Completion::parse(0x1110);
    let mut runtime = Runtime { wait_status: -1, observed: Some(EventFlags::new(1)), ..Runtime::default() };
    state.task_iteration(SourceClass::Internal, &mut runtime);
    assert_eq!(runtime.errors, [0x3330]);
    assert_eq!(runtime.callbacks, [0x1110]);
}
#[test]
fn selector_uses_exact_command_and_source_classes() {
    let commands = [PanelCommand::Command0, PanelCommand::Command1, PanelCommand::Command2, PanelCommand::Command3, PanelCommand::Command4, PanelCommand::Command5, PanelCommand::Command6];
    let expected = [1, 2, 3, 2, 1, 1, 0];
    for (command, expected) in commands.into_iter().zip(expected) {
        assert_eq!(select_target(PanelMode::Normal, true, SourceClass::Internal, command).code(), expected);
    }
    assert_eq!(select_target(PanelMode::Alternate, false, SourceClass::External, PanelCommand::Command2).code(), 11);
    assert_eq!(select_target(PanelMode::Alternate, false, SourceClass::Internal, PanelCommand::Command1).code(), 8);
    assert_eq!(SourceClass::parse(1), SourceClass::Internal);
    assert_eq!(SourceClass::parse(2), SourceClass::External);
    assert_eq!(SourceClass::parse(9), SourceClass::External);
    assert_eq!(SourceClass::parse(10), SourceClass::Internal);
}
#[test]
fn request_decode_rejects_unknown_transition_command() {
    assert_eq!(decode_request(0x3200, TaskId(9), &[7,0,0,0,0,0,0,0]), Err(ActorError::InvalidRequest));
    assert_eq!(decode_request(0x3200, TaskId(9), &[6,0,0,0,0x10,0x11,0,0]), Ok(Message::Transition(request(PanelCommand::Command6))));
    assert_eq!(decode_request(0x3201, TaskId(9), &[]), Ok(Message::Timer));
}

#[test]
fn asynchronous_transition_does_not_complete_on_submission_or_software_event() {
    let mut state = PanelState::new(None);
    state.target = TransitionTarget::parse(1).unwrap();
    let mut runtime = Runtime { stock_plan: true, ..Runtime::default() };
    state.begin(request(PanelCommand::Command1), SourceClass::Internal, &mut runtime);
    assert!(runtime.callbacks.is_empty());
    state.events = EventFlags::new(4);
    assert_eq!(state.dispatch(SourceClass::Internal, &mut runtime), Ok(()));
    assert!(runtime.callbacks.is_empty());
    state.events = EventFlags::new(1);
    assert_eq!(state.dispatch(SourceClass::Internal, &mut runtime), Ok(()));
    assert_eq!(runtime.callbacks, [0x1110]);
}

#[test]
fn bounded_host_plan_refuses_unbounded_recipe_without_completion() {
    let mut state = PanelState::new(Completion::parse(0x3330));
    state.target = TransitionTarget::parse(1).unwrap();
    state.mode = PanelMode::Alternate;
    let mut runtime = Runtime { stock_plan: true, ..Runtime::default() };
    state.begin(request(PanelCommand::Command1), SourceClass::Internal, &mut runtime);
    assert_eq!(runtime.errors, [0x3330]);
    assert!(runtime.callbacks.is_empty());
    state.events = EventFlags::new(1);
    assert_eq!(state.dispatch(SourceClass::Internal, &mut runtime), Err(ActorError::Dispatch));
    assert!(runtime.callbacks.is_empty());
}

#[test]
fn failed_signal_notifies_error_without_caller_completion() {
    let state = PanelState::new(Completion::parse(0x3330));
    let mut runtime = Runtime { signal_status: -17, ..Runtime::default() };
    state.signal(Event::Continue, &mut runtime);
    assert_eq!(runtime.signals, [Event::Continue]);
    assert_eq!(runtime.errors, [0x3330]);
    assert!(runtime.callbacks.is_empty());
}
