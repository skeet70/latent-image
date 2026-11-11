use super::{continue_status, publish_status4, AuxiliaryRequest, CallbackAddress, CaptureRequest, Controller, Field, FinalizeRequest, InvalidControllerState, OutputKind, Runtime, StepRequest};

struct Recording {
    words: [u32; 12],
    states: [u16; 2],
    events: [u16; 2],
    operation: u16,
    selector: u32,
    capture_ready: bool,
    step_complete: bool,
    capture_complete: bool,
    output_complete: bool,
    finalize_complete: bool,
    channels_enabled: bool,
    deferred: usize,
    callbacks: Vec<CallbackAddress>,
    saved_request: bool,
    service_requests: usize,
    emitted_outputs: Vec<OutputKind>,
}

fn index(field: Field) -> usize {
    match field {
        Field::Selector => 0, Field::SavedCallback => 1, Field::Request => 2,
        Field::ActiveSelector => 3, Field::Callback => 4, Field::OutputKind => 5,
        Field::Required => 6, Field::Started => 7, Field::Done => 8,
        Field::CaptureReady => 9, Field::OutputActive => 10,
    }
}
fn controller_index(controller: Controller) -> usize {
    match controller { Controller::Output => 0, Controller::Continuation => 1 }
}
impl Recording {
    fn new(selector: u32) -> Self {
        Self {
            words: [0; 12], states: [0; 2], events: [0; 2], operation: 0x13e7, selector,
            capture_ready: true, step_complete: true, capture_complete: true,
            output_complete: true, finalize_complete: true, channels_enabled: true,
            deferred: 0, callbacks: Vec::new(), saved_request: false,
            service_requests: 0, emitted_outputs: Vec::new(),
        }
    }
    fn receive(&mut self, operation: u16) {
        self.operation = operation;
        self.set_word(Field::Request, 6);
        continue_status(self).unwrap();
    }
}
impl Runtime for Recording {
    fn word(&self, field: Field) -> u32 {
        if field == Field::CaptureReady { u32::from(self.capture_ready) } else { self.words[index(field)] }
    }
    fn set_word(&mut self, field: Field, value: u32) { self.words[index(field)] = value; }
    fn state(&self, controller: Controller) -> u16 { self.states[controller_index(controller)] }
    fn set_state(&mut self, controller: Controller, state: u16) { self.states[controller_index(controller)] = state; }
    fn set_event(&mut self, controller: Controller, event: u16) { self.events[controller_index(controller)] = event; }
    fn current_operation(&self) -> u16 { self.operation }
    fn defer_current(&mut self) { self.deferred += 1; }
    fn channel_enabled(&mut self, enabled: bool) { self.channels_enabled = enabled; }
    fn save_current_request(&mut self) {
        self.saved_request = true;
        self.set_word(Field::Selector, self.selector);
        self.set_word(Field::SavedCallback, 0x1234);
    }
    fn clear_saved_request(&mut self) { self.saved_request = false; }
    fn step_request(&mut self, _: StepRequest) -> i32 { i32::from(self.step_complete) }
    fn capture_request(&mut self, _: CaptureRequest) -> i32 { i32::from(self.capture_complete) }
    fn output_request(&mut self, kind: OutputKind) -> i32 {
        self.emitted_outputs.push(kind);
        i32::from(self.output_complete)
    }
    fn finalize_request(&mut self, _: FinalizeRequest) -> i32 { i32::from(self.finalize_complete) }
    fn auxiliary_request(&mut self, _: AuxiliaryRequest) -> i32 { panic!("unrequested auxiliary operation") }
    fn auxiliary_state(&mut self) -> u32 { 0 }
    fn finish_auxiliary(&mut self) -> i32 { panic!("unrequested auxiliary operation") }
    fn request_service_stop(&mut self) { self.service_requests += 1; }
    fn callback(&mut self, callback: CallbackAddress) { self.callbacks.push(callback); }
}

#[test]
fn close_without_requested_engines_publishes_callback_and_releases_channel() {
    let mut recording = Recording::new(0);
    assert_eq!(publish_status4(&mut recording), Ok(()));
    assert_eq!(recording.callbacks, [CallbackAddress(0x1234)]);
    assert!(recording.channels_enabled);
    assert!(!recording.saved_request);
    assert_eq!(recording.state(Controller::Output), 5);
    assert_eq!(recording.state(Controller::Continuation), 0);
    assert!(recording.emitted_outputs.is_empty());
}

#[test]
fn snapshot_waits_for_actual_service_callback_after_other_engines_finish() {
    let mut recording = Recording::new(1);
    assert_eq!(publish_status4(&mut recording), Ok(()));
    assert!(recording.callbacks.is_empty());
    assert!(!recording.channels_enabled);
    assert!(recording.saved_request);
    assert_eq!(recording.service_requests, 1);
    recording.receive(0x13ea);
    assert_eq!(recording.callbacks, [CallbackAddress(0x1234)]);
    assert!(recording.channels_enabled);
    assert!(!recording.saved_request);
    recording.receive(0x13ea);
    assert_eq!(recording.callbacks.len(), 1);
}

#[test]
fn engine_pending_stops_later_requests_until_its_completion_event() {
    let mut recording = Recording::new(1);
    recording.step_complete = false;
    assert_eq!(publish_status4(&mut recording), Ok(()));
    assert!(recording.emitted_outputs.is_empty());
    assert_eq!(recording.service_requests, 0);
    recording.receive(0x13e6);
    assert!(recording.callbacks.is_empty());
    assert!(recording.emitted_outputs.is_empty());
    recording.step_complete = true;
    recording.receive(0x13e6);
    assert_eq!(recording.emitted_outputs, [OutputKind::Snapshot]);
    assert_eq!(recording.service_requests, 1);
    assert!(recording.callbacks.is_empty());
    recording.receive(0x13ea);
    assert_eq!(recording.callbacks, [CallbackAddress(0x1234)]);
}

#[test]
fn selector_two_needs_both_output_completions_but_no_service_callback() {
    let mut recording = Recording::new(2);
    recording.output_complete = false;
    assert_eq!(publish_status4(&mut recording), Ok(()));
    assert_eq!(recording.emitted_outputs, [OutputKind::Close]);
    assert!(recording.callbacks.is_empty());
    recording.output_complete = true;
    recording.receive(0x1399);
    assert_eq!(recording.emitted_outputs, [OutputKind::Close, OutputKind::Poll, OutputKind::Second]);
    assert_eq!(recording.callbacks, [CallbackAddress(0x1234)]);
    assert_eq!(recording.service_requests, 0);
}

#[test]
fn capture_not_ready_requires_real_capture_poll_completion() {
    let mut recording = Recording::new(2);
    recording.capture_ready = false;
    recording.capture_complete = false;
    assert_eq!(publish_status4(&mut recording), Ok(()));
    assert!(recording.callbacks.is_empty());
    assert!(recording.emitted_outputs.is_empty());
    recording.receive(0x1394);
    assert!(recording.callbacks.is_empty());
    recording.capture_complete = true;
    recording.receive(0x1394);
    assert_eq!(recording.callbacks, [CallbackAddress(0x1234)]);
}

#[test]
fn every_busy_output_state_defers_without_callback_or_state_change() {
    for state in 1..=6 {
        let mut recording = Recording::new(0);
        recording.set_state(Controller::Output, state);
        assert_eq!(publish_status4(&mut recording), Ok(()));
        assert_eq!(recording.deferred, 1);
        assert_eq!(recording.state(Controller::Output), state);
        assert!(recording.callbacks.is_empty());
        assert!(!recording.saved_request);
    }
}

#[test]
fn selectors_three_and_default_keep_asynchronous_callback_requirement() {
    for (selector, output) in [(3, OutputKind::Third), (4, OutputKind::Snapshot), (u32::MAX, OutputKind::Snapshot)] {
        let mut recording = Recording::new(selector);
        assert_eq!(publish_status4(&mut recording), Ok(()));
        assert_eq!(recording.emitted_outputs, [output]);
        assert!(recording.callbacks.is_empty());
        recording.receive(0x13ea);
        assert_eq!(recording.callbacks, [CallbackAddress(0x1234)]);
    }
}

#[test]
fn invalid_output_state_is_reported_without_completion() {
    let mut recording = Recording::new(0);
    recording.set_state(Controller::Output, 7);
    assert_eq!(publish_status4(&mut recording), Err(InvalidControllerState { controller: Controller::Output, state: 7 }));
    assert!(recording.callbacks.is_empty());
}
