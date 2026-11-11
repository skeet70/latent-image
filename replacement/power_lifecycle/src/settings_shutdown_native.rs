use crate::callback_registry::{registry as callbacks, CallbackWord};
use crate::settings_shutdown_actor::{self as actor, CallbackToken};
use crate::shutdown_rtos::NativeShutdownRtos;
use core::ptr::{read_volatile, write_volatile};

#[repr(C)]
pub struct RecordWrapper { _opaque: [u8; 0] }
extern "C" {
    fn record_store_register(kind: u32, bytes: u32, total: u32, sequence: u32,
        seal: Option<unsafe extern "C" fn(*mut u8, *mut u8)>, seal_context: *mut u8,
        check: Option<unsafe extern "C" fn(*mut u8, *mut u8) -> i32>, check_context: *mut u8,
        wrapper: *mut *mut RecordWrapper) -> i32;
    fn record_store_read_latest(wrapper: *mut RecordWrapper, skipped: *mut u16, output: *mut *const u8) -> i32;
    fn record_store_append(wrapper: *mut RecordWrapper, seed: u32, source: *const u8) -> i32;
    fn record_store_close(wrapper: *mut RecordWrapper);
    fn xm1_power_software_timer_cancel(handle: u32) -> i32;
    fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(id: i32) -> i32;
    fn xm1_power_diagnostic_calendar_header(output: *mut u8);
}
const CONTEXT: u32 = 0x9661f8;
const SNAPSHOT: u32 = 0x8e1920;
const WRAPPER: u32 = 0x89f870;
unsafe fn load(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn store(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }

unsafe extern "C" fn seal_record(record: *mut u8, _: *mut u8) {
    actor::seal(unsafe { &mut *(record as *mut [u8; actor::RECORD_BYTES]) });
}
unsafe extern "C" fn check_record(record: *mut u8, _: *mut u8) -> i32 {
    if actor::valid(unsafe { &*(record as *const [u8; actor::RECORD_BYTES]) }) { 0 } else { 1 }
}

#[no_mangle]
pub extern "C" fn xm1_power_settings_release_request(callback: u32) -> i32 {
    match actor::post_release(&mut NativeShutdownRtos, CallbackToken(callback)) { Ok(()) => 0, Err(_) => -255 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_load_cache() -> i32 {
    let wrapper = WRAPPER as *mut *mut RecordWrapper;
    let registration = unsafe { record_store_register(4, 0x600, 20, 0, Some(seal_record), 0x8e1f20 as *mut u8,
        Some(check_record), 0x8e1f20 as *mut u8, wrapper) };
    let mut skipped = 0u16;
    let mut record = core::ptr::null();
    let status = unsafe { record_store_read_latest(read_volatile(wrapper), &mut skipped, &mut record) };
    let snapshot = unsafe { &mut *(SNAPSHOT as *mut [u8; actor::RECORD_BYTES]) };
    if status == 0 { snapshot.copy_from_slice(unsafe { core::slice::from_raw_parts(record, actor::RECORD_BYTES) }); }
    else { snapshot.fill(0xff); }
    if registration != 0 { registration } else { status }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_dispatch_release() -> i32 {
    if actor::TransferState::from_abi(unsafe { load(CONTEXT) }).is_none() { return -255; }
    if unsafe { read_volatile((CONTEXT + 8) as *const u16) } != actor::RELEASE_COMMAND.0 { return -255; }
    if unsafe { xm1_power_semaphore_wait(97, -1) } != 0 { return -10; }
    let callback = unsafe { load(CONTEXT + 0x14) };
    unsafe { store(CONTEXT + 0x3c, callback); }
    let timer = unsafe { load(CONTEXT + 0x48) };
    if timer != 0 {
        unsafe { let _ = xm1_power_software_timer_cancel(timer); store(CONTEXT + 0x48, 0); }
    }
    let _ = crate::shutdown_requests::invoke_status_completion(callbacks(), CallbackWord(callback as usize));
    let current = unsafe { &*((CONTEXT + 0x194) as *const [u8; actor::RECORD_BYTES]) };
    let snapshot = unsafe { &*(SNAPSHOT as *const [u8; actor::RECORD_BYTES]) };
    let wrapper = unsafe { read_volatile(WRAPPER as *const *mut RecordWrapper) };
    if current[2..0x5ff] != snapshot[2..0x5ff] { unsafe { let _ = record_store_append(wrapper, 0, current.as_ptr()); } }
    unsafe { record_store_close(wrapper); store(CONTEXT, 2); }
    if unsafe { xm1_power_semaphore_signal(97) } == 0 { 0 } else { -10 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_initialize(argument: *const u32) {
    unsafe {
        store(CONTEXT, 0);
        store(CONTEXT + 0xc, 28);
        store(CONTEXT + 0x10, CONTEXT + 0x14);
        store(CONTEXT + 0x40, read_volatile(argument));
        store(CONTEXT + 0xdc, 0);
        core::ptr::write_bytes((CONTEXT + 0x94) as *mut u8, 0, 0x48);
        for index in 0..43 { store(0x9662d8 + index * 4, 0); }
        for offset in [0x138, 0x140, 0x148, 0xf8, 0x100, 0x114, 0x11c] { store(CONTEXT + offset, 1); }
        for offset in [0x108, 0x124] { store(CONTEXT + offset, 1000); }
        let _ = xm1_power_settings_load_cache();
        core::ptr::copy_nonoverlapping(SNAPSHOT as *const u8, (CONTEXT + 0x194) as *mut u8, actor::RECORD_BYTES);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_window_valid(timestamp: *const [u8; 6], minutes: u32) -> i32 {
    use crate::settings_shutdown_bounds::{classify_window, TransferTimestamp, WindowDisposition, WindowMinutes};
    let mut now = [0; 6];
    unsafe { xm1_power_diagnostic_calendar_header(now.as_mut_ptr()); }
    let Some(start) = TransferTimestamp::parse(unsafe { read_volatile(timestamp) }) else { return 0; };
    let Some(now) = TransferTimestamp::parse(now) else { return 0; };
    i32::from(classify_window(start, now, WindowMinutes(minutes)) == WindowDisposition::Within)
}
