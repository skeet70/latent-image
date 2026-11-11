use crate::recording_capture_timer::{self as timer, RecordingTimerError, RecordingTimerHandle, RecordingTimerOwner, RecordingTimerRuntime};

const STATE: u32 = 0x015b_7698;

extern "C" {
    fn xm1_power_software_timer_start(period: u32, mode: u32, callback: u32, token: *mut u32) -> i32;
    fn xm1_power_software_timer_cancel(handle: u32) -> i32;
    fn xm1_power_owned_callback_register(word: u32, shape: u32) -> i32;
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_current_task() -> u32;
    fn xm1_power_configuration_byte(key: u32) -> u32;
    fn xm1_power_append_stream_text(task: u32, text: u32);
}

fn byte(offset: u32) -> u8 { unsafe { core::ptr::read_volatile((STATE + offset) as *const u8) } }
fn put_byte(offset: u32, value: u8) { unsafe { core::ptr::write_volatile((STATE + offset) as *mut u8, value) }; }
fn word(offset: u32) -> u32 { unsafe { core::ptr::read_volatile((STATE + offset) as *const u32) } }
fn put_word(offset: u32, value: u32) { unsafe { core::ptr::write_volatile((STATE + offset) as *mut u32, value) }; }

struct NativeRecordingTimer;
impl RecordingTimerRuntime for NativeRecordingTimer {
    fn stopped(&mut self, stopped: bool) { put_byte(0, u8::from(stopped)); }
    fn armed(&self) -> bool { byte(1) == 1 }
    fn set_armed(&mut self, armed: bool) { put_byte(1, u8::from(armed)); }
    fn enabled(&self) -> bool { byte(2) == 1 }
    fn set_enabled(&mut self, enabled: bool) { put_byte(2, u8::from(enabled)); }
    fn handle(&self) -> RecordingTimerHandle { RecordingTimerHandle(word(4)) }
    fn set_handle(&mut self, handle: RecordingTimerHandle) { put_word(4, handle.0); }
    fn owner(&self) -> RecordingTimerOwner { RecordingTimerOwner(word(8)) }
    fn set_owner(&mut self, owner: RecordingTimerOwner) { put_word(8, owner.0); }
    fn clear_armed_critical(&mut self) {
        let saved = unsafe { xm1_power_interrupt_lock() };
        put_byte(1, 0);
        unsafe { xm1_power_interrupt_unlock(saved) };
    }
    fn register(&mut self) -> i32 {
        let seconds = unsafe { xm1_power_configuration_byte(0x947e) };
        let registered = unsafe { xm1_power_owned_callback_register(xm1_power_recording_timer_expired as *const () as u32, crate::callback_registry::COMPLETION_SHAPE) };
        if registered != 0 {
            return registered;
        }
        unsafe { xm1_power_software_timer_start(seconds.wrapping_mul(1000), 0, xm1_power_recording_timer_expired as *const () as u32, (STATE + 4) as *mut u32) }
    }
    fn cancel(&mut self, handle: RecordingTimerHandle) -> i32 {
        unsafe { xm1_power_software_timer_cancel(handle.0) }
    }
    fn report(&mut self, error: RecordingTimerError) {
        let message: &[u8] = match error {
            RecordingTimerError::Start => b"SoftTimerStart\0",
            RecordingTimerError::Stop => b"SoftTimerStop\0",
        };
        let task = unsafe { xm1_power_current_task() };
        unsafe {
            xm1_power_append_stream_text(task, b"rec0p_err\0".as_ptr() as u32);
            xm1_power_append_stream_text(task, message.as_ptr() as u32);
        }
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_timer_initialize(owner: u32) { timer::initialize(&mut NativeRecordingTimer, RecordingTimerOwner(owner)); }
#[no_mangle]
pub extern "C" fn xm1_power_recording_timer_set_owner(owner: u32) { timer::set_owner(&mut NativeRecordingTimer, RecordingTimerOwner(owner)); }
#[no_mangle]
pub extern "C" fn xm1_power_recording_timer_reset() { timer::reset(&mut NativeRecordingTimer); }
#[no_mangle]
pub extern "C" fn xm1_power_recording_timer_start() { timer::start(&mut NativeRecordingTimer); }
#[no_mangle]
pub extern "C" fn xm1_power_recording_timer_stop() { timer::reset(&mut NativeRecordingTimer); }
#[no_mangle]
pub extern "C" fn xm1_power_recording_timer_expired() { timer::stop_if_armed(&mut NativeRecordingTimer); }
#[no_mangle]
pub extern "C" fn xm1_power_recording_timer_set_enabled(enabled: u32) { timer::set_enabled(&mut NativeRecordingTimer, enabled == 1); }
