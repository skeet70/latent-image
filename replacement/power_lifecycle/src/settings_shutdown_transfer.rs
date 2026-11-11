use crate::settings_shutdown_actor::{self as actor, TransferState};
use crate::settings_shutdown_native::{xm1_power_settings_dispatch_release, xm1_power_settings_initialize, xm1_power_settings_load_cache, xm1_power_settings_window_valid};
use crate::settings_shutdown_parser::{self as parser, CoordinatePair, MetadataDescriptor};
use crate::shutdown::{post_application_record, QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId, UiEvents, UiRouting};
use crate::shutdown_receive::{ReceiveDescriptor, ReceiveTimeout};
use crate::shutdown_rtos::NativeShutdownRtos;
use core::{mem::MaybeUninit, ptr::{read_volatile, write_volatile}};
core::arch::global_asm!(include_str!("settings_shutdown_residue.S"), options(raw));
const CONTEXT: u32 = 0x9661f8;
const DESCRIPTOR: u32 = CONTEXT + 0xe0;
const WORKING: u32 = CONTEXT + 0x194;
extern "C" {
    fn xm1_power_configuration_byte(offset: u32) -> u32;
    fn xm1_power_diagnostic_calendar_header(output: *mut u8);
    fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(id: i32) -> i32;
    fn xm1_power_software_timer_start(period: u32, mode: u32, callback: u32, token: *mut u32) -> i32;
    fn xm1_power_queue_create(queue: u32, configuration: *const QueueConfiguration) -> i32;
    fn xm1_task_submit(descriptor: u32) -> i32;
    fn xm1_power_settings_output(source: *const u8);
    fn xm1_power_settings_reconstruct();
}
unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn put(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
unsafe fn byte(address: u32) -> u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn callback(address: u32) {
    if address != 0 { let call: unsafe extern "C" fn() = unsafe { core::mem::transmute(address as usize) }; unsafe { call(); } }
}
fn halt() -> ! { loop { core::hint::spin_loop(); } }
unsafe fn clear(address: u32, bytes: usize) { unsafe { core::ptr::write_bytes(address as *mut u8, 0, bytes); } }
unsafe fn copy(destination: u32, source: u32, bytes: usize) { unsafe { core::ptr::copy(source as *const u8, destination as *mut u8, bytes); } }
unsafe fn reset_descriptor() {
    unsafe {
        clear(DESCRIPTOR, 43 * 4);
        for offset in [0x138, 0x140, 0x148, 0xf8, 0x100, 0x114, 0x11c] { put(CONTEXT + offset, 1); }
        for offset in [0x108, 0x124] { put(CONTEXT + offset, 1000); }
    }
}
unsafe fn cached_entries(reset_invalid: bool) {
    unsafe {
        if reset_invalid { clear(CONTEXT + 0x94, 72); }
        let valid = xm1_power_settings_window_valid((WORKING + 4) as *const [u8; 6], xm1_power_configuration_byte(0x97ed));
        if valid == 1 {
            copy(CONTEXT + 0x94, WORKING + 10, 72);
            xm1_power_settings_reconstruct();
            let recent = xm1_power_settings_window_valid((WORKING + 4) as *const [u8; 6], xm1_power_configuration_byte(0x97f1).wrapping_mul(10));
            let flag = if recent == 1 { 2 } else { 1 };
            put(DESCRIPTOR, flag); put(CONTEXT + 0x188, flag);
        } else {
            if reset_invalid { reset_descriptor(); }
            put(DESCRIPTOR, 0); put(CONTEXT + 0x188, 0);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_timer_notify() {
    let mut rtos = NativeShutdownRtos;
    let sender = rtos.current_task();
    let _ = rtos.post(ServiceMessage { operation: RequestId(0x6303), sender, destination: QueueId(83), payload: &[] });
}
unsafe fn start_transfer() {
    unsafe {
        put(CONTEXT + 0x30, word(CONTEXT + 0x18));
        put(CONTEXT + 0x34, word(CONTEXT + 0x1c));
        put(CONTEXT + 0x18c, 1);
        put(CONTEXT + 0x38, word(CONTEXT + 0x2c));
        put(CONTEXT + 0x3c, word(CONTEXT + 0x14));
        let _ = xm1_power_software_timer_start(30000, 1, xm1_power_settings_timer_notify as usize as u32, (CONTEXT + 0x48) as *mut u32);
        if word(CONTEXT + 0x34) != 0 { cached_entries(false); xm1_power_settings_output(DESCRIPTOR as *const u8); }
        put(CONTEXT, 1);
    }
}
unsafe fn update_transfer() {
    unsafe {
        put(CONTEXT + 0x30, word(CONTEXT + 0x18));
        put(CONTEXT + 0x34, word(CONTEXT + 0x1c));
        let mode = word(CONTEXT + 0x28); put(CONTEXT + 0x18c, mode);
        put(CONTEXT + 0x38, word(CONTEXT + 0x2c));
        if mode == 1 {
            if word(CONTEXT + 0x34) != 0 { cached_entries(true); }
            else { reset_descriptor(); put(DESCRIPTOR, 0); put(CONTEXT + 0x188, 0); }
            xm1_power_settings_output(DESCRIPTOR as *const u8);
        } else { put(CONTEXT, 1); }
        callback(word(CONTEXT + 0x14));
    }
}
unsafe fn restart_transfer() {
    unsafe {
        put(CONTEXT, 0); clear(CONTEXT + 0x4c, 72); clear(CONTEXT + 0x94, 72);
        reset_descriptor(); let _ = xm1_power_settings_load_cache(); copy(WORKING, 0x8e1920, actor::RECORD_BYTES); start_transfer();
    }
}
unsafe fn refresh_transfer() {
    unsafe {
        if word(CONTEXT + 0x34) != 0 { cached_entries(true); }
        xm1_power_settings_output(DESCRIPTOR as *const u8);
    }
}
fn digits(destination: &mut [u8], value: u32) {
    let mut divisor = 1u32;
    for _ in 0..destination.len() { divisor *= 10; }
    if value >= divisor { return; }
    let mut value = value;
    for out in destination.iter_mut().rev() { *out = b'0' + (value % 10) as u8; value /= 10; }
}
unsafe fn configured_text() {
    unsafe {
        clear(CONTEXT + 0x4c, 72);
        copy(CONTEXT + 0x4c, 0x7819e8, 68);
        let text = &mut *((CONTEXT + 0x4c) as *mut [u8; 72]);
        digits(&mut text[0..2], xm1_power_configuration_byte(0x97d7));
        digits(&mut text[2..4], xm1_power_configuration_byte(0x97d8));
        let latitude_fraction = xm1_power_configuration_byte(0x97da) * 256 + xm1_power_configuration_byte(0x97d9);
        digits(&mut text[5..9], latitude_fraction * 100 / 60);
        text[12] = if xm1_power_configuration_byte(0x97d6) == 0 { b'N' } else { b'S' };
        digits(&mut text[14..17], xm1_power_configuration_byte(0x97dc));
        digits(&mut text[17..19], xm1_power_configuration_byte(0x97dd));
        let longitude_fraction = xm1_power_configuration_byte(0x97df) * 256 + xm1_power_configuration_byte(0x97de);
        digits(&mut text[20..24], longitude_fraction * 100 / 60);
        text[27] = if xm1_power_configuration_byte(0x97db) == 0 { b'E' } else { b'W' };
    }
}
unsafe fn same_string(left: u32, right: u32) -> bool {
    let mut index = 0;
    loop {
        let left = unsafe { byte(left + index) }; let right = unsafe { byte(right + index) };
        if left != right { return false; }
        if left == 0 { return true; }
        index += 1;
    }
}
unsafe fn periodic_transfer() {
    unsafe {
        let producer = word(CONTEXT + 0xdc);
        if producer == 0 { return; }
        let producer: unsafe extern "C" fn(*mut u8, u32) = core::mem::transmute(producer as usize);
        producer((CONTEXT + 0x4c) as *mut u8, 72);
        let mode = word(CONTEXT + 0x18c);
        if xm1_power_configuration_byte(0x97d5) != 0 || mode == 2 { configured_text(); }
        if mode == 2 {
            if word(CONTEXT + 0x190) == 1 { put(CONTEXT + 0x190, 2); }
            clear(DESCRIPTOR + 12, 160);
        } else if word(CONTEXT + 0x190) == 2 {
            xm1_power_settings_output(DESCRIPTOR as *const u8); put(CONTEXT + 0x190, 1);
        }
        let mut pair = CoordinatePair([0; 2]);
        if parser::coordinates(&*((CONTEXT + 0x4c) as *const [u8; 72]), &mut pair).is_err() { return; }
        copy(CONTEXT + 0x94, CONTEXT + 0x4c, 72);
        if mode == 1 && !same_string(WORKING + 10, CONTEXT + 0x94) {
            xm1_power_diagnostic_calendar_header((WORKING + 4) as *mut u8); copy(WORKING + 10, CONTEXT + 0x94, 72);
        }
        xm1_power_settings_timer_notify();
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_dispatch() -> i32 {
    let Some(state) = TransferState::from_abi(unsafe { word(CONTEXT) }) else { return -255; };
    let operation = RequestId(unsafe { read_volatile((CONTEXT + 8) as *const u16) });
    let Some(action) = actor::dispatch_route(operation, state) else { return -255; };
    if action == actor::TransferAction::Release { return unsafe { xm1_power_settings_dispatch_release() }; }
    if unsafe { xm1_power_semaphore_wait(97, -1) } != 0 { return -10; }
    unsafe {
        match action {
            actor::TransferAction::Start => start_transfer(),
            actor::TransferAction::Update => update_transfer(),
            actor::TransferAction::Restart => restart_transfer(),
            actor::TransferAction::Callback => callback(word(CONTEXT + 0x14)),
            actor::TransferAction::Refresh => refresh_transfer(),
            actor::TransferAction::Poll => periodic_transfer(),
            actor::TransferAction::Release | actor::TransferAction::NoAction => {}
        }
    }
    if unsafe { xm1_power_semaphore_signal(97) } == 0 { 0 } else { -10 }
}
#[repr(C)]
struct QueueConfiguration { flags: u32, message_size: u32 }
#[repr(C)]
struct TaskDescriptor { identifier: u32, name: u32, priority: u32, stack: u32, entry: unsafe extern "C" fn(*const u32) -> !, argument: *const u32, reserved: MaybeUninit<u32> }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_start(error_callback: Option<unsafe extern "C" fn()>) -> i32 {
    if unsafe { xm1_power_queue_create(83, &QueueConfiguration { flags: 48, message_size: 36 }) } != 0 { return -255; }
    unsafe { put(0x8e1f24, error_callback.map_or(0, |callback| callback as usize as u32)); }
    let descriptor = TaskDescriptor { identifier: 83, name: 0x781a38, priority: 4, stack: 3072, entry: xm1_power_settings_entry, argument: 0x8e1f24 as *const u32, reserved: MaybeUninit::uninit() };
    if unsafe { xm1_task_submit(&descriptor as *const TaskDescriptor as u32) } == 0 { 0 } else { -255 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_entry(argument: *const u32) -> ! {
    unsafe { xm1_power_settings_initialize(argument); }
    loop {
        let received = unsafe { NativeShutdownRtos.receive(QueueId(83), &mut *((CONTEXT + 8) as *mut ReceiveDescriptor), ReceiveTimeout::Forever) };
        if received.is_err() { unsafe { callback(word(CONTEXT + 0x40)); } }
        if unsafe { xm1_power_settings_dispatch() } != 0 { unsafe { callback(word(CONTEXT + 0x40)); } }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_reconstruct_backed(scratch: *mut CoordinatePair) {
    unsafe {
        parser::reconstruct(&mut *(DESCRIPTOR as *mut MetadataDescriptor), &*((CONTEXT + 0x94) as *const [u8; 72]), word(CONTEXT + 0x18c), &mut *scratch);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_output_copy(source: *const u8) {
    if unsafe { xm1_power_semaphore_wait(57, -1) } != 0 { halt(); }
    let destination = unsafe { word(0x956834) };
    unsafe { core::ptr::copy_nonoverlapping(source, destination as *mut u8, 172); }
    if unsafe { xm1_power_semaphore_signal(57) } != 0 { halt(); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_output_post(application: *const [u8; 76]) {
    let routing = if unsafe { word(0x9134f8) } == 1 { UiRouting::Primary } else { UiRouting::Other };
    let mut events = UiEvents::from_runtime(routing, TaskId(unsafe { byte(0x954884) }));
    if post_application_record(&mut NativeShutdownRtos, &mut events, unsafe { &*application }).is_err() { halt(); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_transfer_request(request: *const actor::TransferRequest) -> i32 {
    match actor::post_transfer(&mut NativeShutdownRtos, unsafe { &*request }) { Ok(()) => 0, Err(_) => -255 }
}
#[no_mangle]
pub extern "C" fn xm1_power_settings_poll_request() {
    let mut rtos = NativeShutdownRtos;
    let sender = rtos.current_task();
    let _ = rtos.post(ServiceMessage { operation: RequestId(0x6304), sender, destination: QueueId(83), payload: &[] });
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_settings_set_text_producer(producer: Option<unsafe extern "C" fn(*mut u8, u32)>) {
    unsafe { put(CONTEXT + 0xdc, producer.map_or(0, |producer| producer as usize as u32)); }
}
