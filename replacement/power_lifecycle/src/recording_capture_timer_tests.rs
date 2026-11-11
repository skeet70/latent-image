use crate::recording_capture_timer::*;

struct Timer {
    stopped: bool, armed: bool, enabled: bool, handle: RecordingTimerHandle, owner: RecordingTimerOwner,
    allocation: Result<RecordingTimerHandle, i32>, cancellation: i32, errors: Vec<RecordingTimerError>,
}
impl Default for Timer {
    fn default() -> Self {
        Self { stopped: false, armed: false, enabled: false, handle: RecordingTimerHandle(0), owner: RecordingTimerOwner(0), allocation: Ok(RecordingTimerHandle(123)), cancellation: 0, errors: Vec::new() }
    }
}
impl RecordingTimerRuntime for Timer {
    fn stopped(&mut self, value: bool) { self.stopped = value; }
    fn armed(&self) -> bool { self.armed }
    fn set_armed(&mut self, value: bool) { self.armed = value; }
    fn enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, value: bool) { self.enabled = value; }
    fn handle(&self) -> RecordingTimerHandle { self.handle }
    fn set_handle(&mut self, value: RecordingTimerHandle) { self.handle = value; }
    fn owner(&self) -> RecordingTimerOwner { self.owner }
    fn set_owner(&mut self, value: RecordingTimerOwner) { self.owner = value; }
    fn clear_armed_critical(&mut self) { self.armed = false; }
    fn register(&mut self) -> i32 {
        match self.allocation { Ok(handle) => { self.handle = handle; 0 }, Err(status) => status }
    }
    fn cancel(&mut self, _: RecordingTimerHandle) -> i32 { self.cancellation }
    fn report(&mut self, error: RecordingTimerError) { self.errors.push(error); }
}

#[test]
fn initialized_timer_requires_enable_and_nonzero_owner() {
    let mut timer = Timer::default();
    initialize(&mut timer, RecordingTimerOwner(5));
    start(&mut timer);
    assert!(timer.stopped);
    assert!(!timer.armed);
    assert_eq!(timer.handle, RecordingTimerHandle(0));
    set_enabled(&mut timer, true);
    start(&mut timer);
    assert!(!timer.stopped);
    assert!(timer.armed);
    assert_eq!(timer.handle, RecordingTimerHandle(123));
}

#[test]
fn failed_start_preserves_preexisting_handle_and_armed_state() {
    let mut timer = Timer { enabled: true, owner: RecordingTimerOwner(7), handle: RecordingTimerHandle(55), allocation: Err(-20), ..Timer::default() };
    start(&mut timer);
    assert!(!timer.stopped);
    assert!(timer.armed);
    assert_eq!(timer.handle, RecordingTimerHandle(55));
    assert_eq!(timer.errors, [RecordingTimerError::Start]);
}

#[test]
fn expiry_failure_keeps_handle_and_armed_flag_but_marks_stopped() {
    let mut timer = Timer { armed: true, enabled: true, owner: RecordingTimerOwner(7), handle: RecordingTimerHandle(55), cancellation: -1, ..Timer::default() };
    stop_if_armed(&mut timer);
    assert!(timer.stopped && timer.armed && timer.enabled);
    assert_eq!(timer.handle, RecordingTimerHandle(55));
    assert_eq!(timer.errors, [RecordingTimerError::Stop]);
}

#[test]
fn changing_nonzero_owner_does_not_cancel_active_timer() {
    let mut timer = Timer { armed: true, enabled: true, owner: RecordingTimerOwner(7), handle: RecordingTimerHandle(55), ..Timer::default() };
    set_owner(&mut timer, RecordingTimerOwner(8));
    assert!(!timer.stopped && timer.armed && timer.enabled);
    assert_eq!(timer.handle, RecordingTimerHandle(55));
    set_owner(&mut timer, RecordingTimerOwner(0));
    assert!(timer.stopped && !timer.armed && !timer.enabled);
    assert_eq!(timer.handle, RecordingTimerHandle(0));
}

#[test]
fn reset_failure_preserves_handle_but_disarms_and_disables() {
    let mut timer = Timer { armed: true, enabled: true, owner: RecordingTimerOwner(7), handle: RecordingTimerHandle(55), cancellation: -1, ..Timer::default() };
    reset(&mut timer);
    assert!(timer.stopped && !timer.armed && !timer.enabled);
    assert_eq!(timer.handle, RecordingTimerHandle(55));
    assert_eq!(timer.errors, [RecordingTimerError::Stop]);
}

#[test]
fn disabling_preserves_armed_timer_until_expiry() {
    let mut timer = Timer { armed: true, enabled: true, owner: RecordingTimerOwner(7), handle: RecordingTimerHandle(55), ..Timer::default() };
    set_enabled(&mut timer, false);
    assert!(!timer.enabled && timer.armed);
    assert_eq!(timer.handle, RecordingTimerHandle(55));
    stop_if_armed(&mut timer);
    assert!(timer.stopped && timer.armed);
    assert_eq!(timer.handle, RecordingTimerHandle(0));
}
