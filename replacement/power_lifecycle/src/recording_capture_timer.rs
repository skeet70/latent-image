#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingTimerOwner(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingTimerHandle(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingTimerError { Start, Stop }

pub trait RecordingTimerRuntime {
    fn stopped(&mut self, stopped: bool);
    fn armed(&self) -> bool;
    fn set_armed(&mut self, armed: bool);
    fn enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);
    fn handle(&self) -> RecordingTimerHandle;
    fn set_handle(&mut self, handle: RecordingTimerHandle);
    fn owner(&self) -> RecordingTimerOwner;
    fn set_owner(&mut self, owner: RecordingTimerOwner);
    fn clear_armed_critical(&mut self);
    fn register(&mut self) -> i32;
    fn cancel(&mut self, handle: RecordingTimerHandle) -> i32;
    fn report(&mut self, error: RecordingTimerError);
}

pub fn initialize(runtime: &mut impl RecordingTimerRuntime, owner: RecordingTimerOwner) {
    runtime.stopped(true);
    runtime.set_armed(false);
    runtime.set_enabled(false);
    runtime.set_handle(RecordingTimerHandle(0));
    runtime.set_owner(owner);
}

fn cancel_handle(runtime: &mut impl RecordingTimerRuntime) {
    let handle = runtime.handle();
    if handle.0 != 0 {
        if runtime.cancel(handle) == 0 {
            runtime.set_handle(RecordingTimerHandle(0));
        } else {
            runtime.report(RecordingTimerError::Stop);
        }
    }
}

pub fn reset(runtime: &mut impl RecordingTimerRuntime) {
    runtime.clear_armed_critical();
    cancel_handle(runtime);
    runtime.stopped(true);
    runtime.set_enabled(false);
}

pub fn set_owner(runtime: &mut impl RecordingTimerRuntime, owner: RecordingTimerOwner) {
    let previous = runtime.owner();
    if previous == owner { return; }
    if previous.0 == 0 || owner.0 == 0 { reset(runtime); }
    runtime.set_owner(owner);
}

pub fn stop_if_armed(runtime: &mut impl RecordingTimerRuntime) {
    if !runtime.armed() { return; }
    let handle = runtime.handle();
    if runtime.cancel(handle) == 0 {
        runtime.set_handle(RecordingTimerHandle(0));
    } else {
        runtime.report(RecordingTimerError::Stop);
    }
    runtime.stopped(true);
}

pub fn start(runtime: &mut impl RecordingTimerRuntime) {
    if !runtime.enabled() || runtime.owner().0 == 0 { return; }
    runtime.stopped(false);
    runtime.set_armed(true);
    if runtime.register() != 0 { runtime.report(RecordingTimerError::Start); }
}

pub fn set_enabled(runtime: &mut impl RecordingTimerRuntime, enabled: bool) {
    if enabled && runtime.owner().0 != 0 {
        runtime.clear_armed_critical();
        cancel_handle(runtime);
    }
    runtime.set_enabled(enabled);
}
