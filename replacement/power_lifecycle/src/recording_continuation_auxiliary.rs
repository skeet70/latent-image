#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Field { Request = 0xb488, Value = 0xb48c, Response = 0xb490, Result = 0xb494, SavedValue = 0xb498, Mode = 0xb49c, Completion = 0xb4a0, Enabled = 0xb4a4 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request { Query, Start, ReadCompletion, Enable, Disable, Wait, WaitAlternate, Stop, CompletionEvent, TimerEvent, QueryOther, Other }
impl Request {
    pub fn decode(kind: u32, operation: u16) -> Self {
        match kind { 0 => Self::Query, 1 => Self::Start, 2 => Self::ReadCompletion, 3 => Self::Enable, 4 => Self::Disable, 5 => Self::Wait, 6 => Self::WaitAlternate, 7 => Self::Stop, 8 => match operation { 0x13c9 => Self::CompletionEvent, 0x13c8 => Self::TimerEvent, _ => Self::QueryOther }, _ => Self::Other }
    }
    fn index(self) -> usize { self as usize }
}

pub trait Runtime {
    fn word(&self, field: Field) -> u32;
    fn set_word(&mut self, field: Field, value: u32);
    fn state(&self) -> u16;
    fn set_state(&mut self, state: u16);
    fn set_event(&mut self, event: u16);
    fn current_operation(&self) -> u16;
    fn message_value(&self) -> u32;
    fn completion_value(&self) -> u32;
    fn defer_current(&mut self);
    fn report_missing_state(&mut self);
    fn publish_started(&mut self);
    fn wait(&mut self, mode: u32) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
enum State { Idle, Starting, Waiting, Ready, Alternate, Stopping }
impl State {
    fn decode(word: u16) -> Option<Self> {
        match word { 0 => Some(Self::Idle), 1 => Some(Self::Starting), 2 => Some(Self::Waiting), 3 => Some(Self::Ready), 4 => Some(Self::Alternate), 5 => Some(Self::Stopping), _ => None }
    }
}

fn set_response(runtime: &mut impl Runtime, value: u32) { runtime.set_word(Field::Response, value); }
fn reset(runtime: &mut impl Runtime) {
    for field in [Field::SavedValue, Field::Mode, Field::Completion, Field::Enabled, Field::Response, Field::Result] { runtime.set_word(field, 0); }
}
fn save_mode(runtime: &mut impl Runtime) {
    let value = runtime.word(Field::Value);
    runtime.set_word(Field::SavedValue, value);
    runtime.set_word(Field::Mode, match value { 1 => 2, 2 => 3, 4 => 0, _ => 1 });
}
fn wait(runtime: &mut impl Runtime) {
    if runtime.wait(runtime.word(Field::Mode)) != 0 { runtime.report_missing_state(); }
}
fn accept(runtime: &mut impl Runtime, operation: u16) {
    if runtime.current_operation() != operation { runtime.report_missing_state(); return; }
    let value = runtime.message_value();
    runtime.set_word(Field::Completion, value);
    runtime.set_word(Field::Result, value);
}

pub fn dispatch(runtime: &mut impl Runtime, request: Request) -> Result<i32, u16> {
    runtime.set_event(request.index() as u16);
    let word = runtime.state();
    let Some(state) = State::decode(word) else { return Err(word); };
    let (result, next) = match (request, state) {
        (Request::Query | Request::QueryOther, _) => { set_response(runtime, state as u32); (i32::from(matches!(state, State::Idle | State::Ready)), None) }
        (Request::Start, State::Idle) => { reset(runtime); save_mode(runtime); runtime.publish_started(); set_response(runtime, 1); (0, Some(State::Starting)) }
        (Request::ReadCompletion, State::Idle) => { let value = runtime.completion_value(); runtime.set_word(Field::Completion, value); runtime.set_word(Field::Result, value); set_response(runtime, 3); (1, Some(State::Ready)) }
        (Request::Enable, State::Ready) => { if runtime.word(Field::Enabled) == 0 { runtime.set_word(Field::Enabled, 1); } (1, None) }
        (Request::Disable, State::Ready) => { if runtime.word(Field::Enabled) == 1 { runtime.set_word(Field::Enabled, 0); } (1, None) }
        (Request::Wait, State::Idle | State::Ready) => {
            if state == State::Idle { runtime.set_state(State::Waiting as u16); }
            save_mode(runtime); wait(runtime); set_response(runtime, 2); (0, Some(State::Waiting))
        }
        (Request::WaitAlternate, State::Ready) => { save_mode(runtime); wait(runtime); set_response(runtime, 4); (0, Some(State::Alternate)) }
        (Request::Stop, State::Ready) => { runtime.set_word(Field::Mode, 0); wait(runtime); set_response(runtime, 5); (0, Some(State::Stopping)) }
        (Request::CompletionEvent, State::Starting) => { accept(runtime, 0x13c9); set_response(runtime, 3); (1, Some(State::Ready)) }
        (Request::TimerEvent, State::Waiting) => { accept(runtime, 0x13c8); set_response(runtime, 3); (1, Some(State::Ready)) }
        (Request::TimerEvent, State::Alternate) => { accept(runtime, 0x13c8); runtime.set_word(Field::Mode, 0); wait(runtime); set_response(runtime, 2); (0, Some(State::Waiting)) }
        (Request::TimerEvent, State::Stopping) => { reset(runtime); set_response(runtime, 0); (1, Some(State::Idle)) }
        _ => { runtime.defer_current(); (0, None) }
    };
    if let Some(state) = next { runtime.set_state(state as u16); }
    Ok(result)
}

#[cfg(test)]
#[path = "recording_continuation_auxiliary_tests.rs"]
mod tests;

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
    use crate::shutdown_rtos::NativeShutdownRtos;
    use core::ptr::{read_volatile, write_volatile};
    const BASE: usize = 0x01526a78;
    unsafe extern "C" {
        fn xm1_power_recording_defer_current() -> i32;
        fn xm1_power_recording_post_notification(operation: u32, value: u32) -> i32;
        fn xm1_power_recording_report_missing_state();
        fn xm1_power_recording_halt_error(status: i32) -> !;
        fn xm1_power_recording_send_task_message(operation: u32, size: u32, payload: *const u8) -> i32;
        fn xm1_power_service_error(operation: u32, id: i32, status: i32);
    }
    fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
    fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) }; }
    fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
    fn post_timer_value(mode: u32) {
        let value = mode.to_le_bytes();
        unsafe { xm1_power_recording_send_task_message(0x13c8, 4, value.as_ptr()) };
    }
    #[no_mangle]
    pub extern "C" fn xm1_power_recording_auxiliary_hardware_callback(_status: u32) {
        let callback = word(0x8e1f3c);
        if callback != 0 {
            if callback != post_timer_value as *const () as u32 { unsafe { xm1_power_recording_halt_error(-30) }; }
            post_timer_value(word(0x8e1f40));
        }
    }
    #[no_mangle]
    pub extern "C" fn xm1_power_recording_auxiliary_wait_notification_callback() { unsafe { xm1_power_recording_post_notification(0x91d, 0) }; }
    struct Native;
    impl Runtime for Native {
        fn word(&self, field: Field) -> u32 { word(BASE + field as usize) }
        fn set_word(&mut self, field: Field, value: u32) { put(BASE + field as usize, value); }
        fn state(&self) -> u16 { half(0x015458b6) }
        fn set_state(&mut self, state: u16) { unsafe { write_volatile(0x015458b6 as *mut u16, state) }; }
        fn set_event(&mut self, event: u16) { unsafe { write_volatile(0x015457f6 as *mut u16, event) }; }
        fn current_operation(&self) -> u16 { half(BASE) }
        fn message_value(&self) -> u32 { word(BASE + 12) }
        fn completion_value(&self) -> u32 { word(0x9570c0) }
        fn defer_current(&mut self) { unsafe { xm1_power_recording_defer_current() }; }
        fn report_missing_state(&mut self) { unsafe { xm1_power_recording_report_missing_state() }; }
        fn publish_started(&mut self) {
            let value = 1u32.to_le_bytes();
            unsafe { xm1_power_recording_send_task_message(0x13c9, 4, value.as_ptr()) };
        }
        fn wait(&mut self, mode: u32) -> i32 {
            if mode > 9 { return -255; }
            if !(1..=3).contains(&mode) { post_timer_value(mode); return 0; }
            put(0x8e1f3c, post_timer_value as *const () as u32);
            put(0x8e1f40, mode);
            let mut payload = [0; 48];
            payload[..4].copy_from_slice(&(xm1_power_recording_auxiliary_hardware_callback as *const () as u32).to_le_bytes());
            payload[4..6].copy_from_slice(&0x2000u16.to_le_bytes());
            payload[6] = (mode - 1) as u8;
            payload[44] = 1;
            let rtos = &mut NativeShutdownRtos;
            let sender = rtos.current_task();
            if let Err(status) = rtos.post(ServiceMessage { operation: RequestId(0x7228), sender, destination: QueueId(98), payload: &payload }) { unsafe { xm1_power_service_error(0x7228, 98, status.0) }; }
            put(0x8e1f50, xm1_power_recording_auxiliary_wait_notification_callback as *const () as u32);
            0
        }
    }
    /// # Safety
    /// A nonzero output address covers eight writable bytes.
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_recording_auxiliary_request(kind: u32, value: u32, output: u32) -> i32 {
        put(BASE + 0xb488, kind);
        put(BASE + 0xb48c, if kind < 7 && (1 << kind) & 0x62 != 0 { value } else { 0 });
        let request = Request::decode(kind, half(BASE));
        let result = match dispatch(&mut Native, request) { Ok(result) => result, Err(_) => unsafe { xm1_power_recording_halt_error(-30) } };
        if output != 0 {
            for index in 0..8 {
                let byte = unsafe { read_volatile((BASE + 0xb490 + index) as *const u8) };
                unsafe { write_volatile((output as usize + index) as *mut u8, byte) };
            }
        }
        result
    }
    #[no_mangle]
    pub extern "C" fn xm1_power_recording_auxiliary_state() -> u32 {
        unsafe { xm1_power_recording_auxiliary_request(8, 0, 0) };
        word(BASE + 0xb490)
    }
    #[no_mangle]
    pub extern "C" fn xm1_power_recording_auxiliary_finish() -> i32 { word(BASE + 0xb4a0) as i32 }
}
