use crate::recording_engine25::{request_finalize, request_output, RecordingAction, RecordingController, RecordingEngine, RecordingEngineError, RecordingKind, RecordingMessage, RecordingSettings};

struct Recording {
    message: RecordingMessage,
    output_state: u16,
    finalize_state: u16,
    kind: RecordingKind,
    settings: RecordingSettings,
    output_status: u32,
    finalization_pending: bool,
}
impl Recording {
    fn new() -> Self { Self { message: RecordingMessage(0x13e7), output_state: 0, finalize_state: 0, kind: RecordingKind(0), settings: RecordingSettings(0), output_status: 0, finalization_pending: false } }
    fn output(&mut self, kind: u32) -> i32 { request_output(self, RecordingKind(kind), RecordingSettings(0)).unwrap() }
    fn finalize(&mut self, kind: u32) -> i32 { request_finalize(self, RecordingKind(kind)).unwrap() }
}
impl RecordingEngine for Recording {
    fn message(&self) -> RecordingMessage { self.message }
    fn set_output_request(&mut self, kind: RecordingKind, settings: RecordingSettings) { self.kind = kind; self.settings = settings; }
    fn set_finalize_request(&mut self, kind: RecordingKind) { self.kind = kind; }
    fn state(&self, controller: RecordingController) -> u16 { match controller { RecordingController::Output => self.output_state, RecordingController::Finalize => self.finalize_state } }
    fn set_state(&mut self, controller: RecordingController, state: u16) { match controller { RecordingController::Output => self.output_state = state, RecordingController::Finalize => self.finalize_state = state } }
    fn set_row(&mut self, _controller: RecordingController, _row: u16) {}
    fn execute(&mut self, action: RecordingAction) -> i32 {
        match action {
            RecordingAction::Ready => 1,
            RecordingAction::Pending | RecordingAction::Defer | RecordingAction::StartOutput | RecordingAction::StopOutput | RecordingAction::PublishOutput => 0,
            RecordingAction::StartFinalize => { self.finalization_pending = true; 0 }
            RecordingAction::FinishFinalize => { self.finalization_pending = false; 1 }
            RecordingAction::FinishOutput => { self.output_status = 2; 1 }
            RecordingAction::FinishStop => { self.output_status = 1; 1 }
        }
    }
}

#[test]
fn output_shutdown_waits_for_capture_then_stop_completion() {
    let mut recording = Recording::new();
    assert_eq!(recording.output(4), 0);
    assert_eq!(recording.output(13), 0);
    recording.message = RecordingMessage(0x1398);
    assert_eq!(recording.output(14), 0);
    recording.message = RecordingMessage(0x1399);
    assert_eq!(recording.output(14), 1);
    assert_eq!(recording.output_status, 2);
    assert_eq!(recording.output(13), 1);
    assert_eq!(recording.output(6), 0);
    assert_eq!(recording.output(13), 0);
    recording.message = RecordingMessage(0x139a);
    assert_eq!(recording.output(14), 1);
    assert_eq!(recording.output_status, 1);
    assert_eq!(recording.output(13), 1);
}

#[test]
fn finalize_completes_only_on_its_own_completion_message() {
    let mut recording = Recording::new();
    assert_eq!(recording.finalize(2), 0);
    assert!(recording.finalization_pending);
    assert_eq!(recording.finalize(0), 0);
    recording.message = RecordingMessage(0x1399);
    assert_eq!(recording.finalize(25), 0);
    assert!(recording.finalization_pending);
    recording.message = RecordingMessage(0x1395);
    assert_eq!(recording.finalize(25), 1);
    assert!(!recording.finalization_pending);
    assert_eq!(recording.finalize(0), 1);
}

#[test]
fn output_terminal_queries_do_not_claim_busy_recording_is_ready() {
    let mut recording = Recording::new();
    assert_eq!(recording.output(13), 1);
    assert_eq!(recording.output(5), 1);
    assert_eq!(recording.output(4), 0);
    recording.message = RecordingMessage(0x1397);
    assert_eq!(recording.output(14), 0);
    recording.message = RecordingMessage(0x13e7);
    assert_eq!(recording.output(14), 0);
    assert_eq!(recording.output(13), 0);
}

#[test]
fn corrupt_controller_state_is_reported() {
    let mut recording = Recording::new();
    recording.finalize_state = 2;
    assert_eq!(request_finalize(&mut recording, RecordingKind(2)), Err(RecordingEngineError::InvalidState { controller: RecordingController::Finalize, state: 2 }));
}
