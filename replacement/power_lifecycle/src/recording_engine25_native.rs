#![cfg(target_arch = "arm")]

use core::ptr::{read_volatile, write_volatile};
use crate::recording_engine25::{self as engine, RecordingAction, RecordingController, RecordingEngine, RecordingKind, RecordingMessage, RecordingSettings};

const RECORDING: u32 = 0x01526a78;
const ROWS: u32 = 0x01545790;
const STATES: u32 = 0x01545850;
const TIMER: u32 = 0x015b7698;
pub(crate) fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
pub(crate) fn half(address: u32) -> u16 { unsafe { read_volatile(address as *const u16) } }
pub(crate) fn byte(address: u32) -> u8 { unsafe { read_volatile(address as *const u8) } }
pub(crate) fn store(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value) }; }
pub(crate) fn store_half(address: u32, value: u16) { unsafe { write_volatile(address as *mut u16, value) }; }
fn store_byte(address: u32, value: u8) { unsafe { write_volatile(address as *mut u8, value) }; }
fn rec(offset: u32) -> u32 { word(RECORDING + offset) }
fn put(offset: u32, value: u32) { store(RECORDING + offset, value); }

#[repr(C)]
struct DispatchPacket { operation: u16, source: u8, destination: u8, length: u32, payload: u32 }
unsafe extern "C" {
    fn xm1_power_recording_defer_current() -> i32;
    fn xm1_power_recording_post_notification(operation: u32, value: u32) -> i32;
    fn xm1_power_recording_send_task_message(operation: u32, size: u32, payload: *const u8) -> i32;
    fn xm1_power_recording_halt_error(status: i32) -> !;
    fn xm1_power_recording_report_missing_state();
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_send_message(channel: u32, message: *const DispatchPacket) -> i32;
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_software_timer_cancel(handle: u32) -> i32;
    fn xm1_power_append_stream_text(task: u32, text: u32);
    fn xm1_power_application_post(event: u32, argument: u32) -> i32;
    fn xm1_power_configuration_byte(offset: u32) -> u32;
}

pub(crate) fn missing() -> ! { unsafe { xm1_power_recording_halt_error(-255) } }
fn post_changed() { unsafe { xm1_power_recording_post_notification(0x82e, 0); } }
fn post_command(command: u16, words: &[u32]) {
    let source = unsafe { xm1_pipeline_image_current_task() } as u8;
    let packet = DispatchPacket { operation: 0x3b00 + command, source: if source == 0 { 0xff } else { source }, destination: 43, length: (words.len() * 4) as u32, payload: words.as_ptr() as u32 };
    unsafe { xm1_pipeline_image_send_message(43, &packet); }
}
fn send_message(operation: u32, words: &[u32]) {
    unsafe { xm1_power_recording_send_task_message(operation, (words.len() * 4) as u32, words.as_ptr().cast()); }
}
extern "C" fn output_completion(status: u32, first: u32, second: u32, third: u32) {
    match status { 1 => send_message(0x1398, &[third]), 2 => send_message(0x139b, &[first, second]), 0 => send_message(0x1399, &[first, second, third]), _ => unsafe { xm1_power_recording_report_missing_state(); } }
}
extern "C" fn stop_completion(status: u32, value: u32) {
    if status != 0 { unsafe { xm1_power_recording_report_missing_state(); } return; }
    send_message(0x139a, &[value]);
}
extern "C" fn finalize_completion(status: u32) {
    if status != 0 { unsafe { xm1_power_recording_report_missing_state(); } return; }
    send_message(0x1395, &[]);
}
extern "C" fn output_progress(first: u32, second: u32) {
    if !matches!(rec(0x3da4), 0 | 6) { send_message(0x131c, &[first, second]); }
}
extern "C" fn output_capture_done(value: u32) { send_message(0x13c3, &[value]); }

fn table_option(kind: u32) -> u32 { word(0x00dcb4b4 + kind.wrapping_mul(12) + 8) }
fn output_selection() -> u32 {
    if rec(0x9604) != 1 || rec(0x81d8) != 1 { return 0; }
    let settings = rec(0x3540);
    if settings == 0 || byte(settings + 0xec) < byte(settings + 0xe8) { return 0; }
    u32::from(byte(settings + 0xe8))
}
fn start_output() {
    put(0x9614, 0);
    update_request_flags(0x13);
    let kind = rec(0x9604);
    let selection = if kind <= 4 && (1u32 << kind) & 0x17 != 0 { output_selection() }
        else { unsafe { xm1_power_recording_report_missing_state(); } 0 };
    put(0x960c, 0);
    post_command(4, &[output_completion as *const () as u32, word(0x00dcb4b4 + kind * 12), selection, table_option(kind), output_progress as *const () as u32, output_capture_done as *const () as u32]);
}
fn stop_output() {
    put(0x960c, 0);
    let kind = rec(0x9604);
    let mode = if rec(0x5f0c) == 1 { 1 } else { word(0x00dcb4b4 + kind.wrapping_mul(12)) };
    post_command(5, &[stop_completion as *const () as u32, mode, table_option(kind)]);
}
fn publish_output(final_output: bool) {
    if final_output { put(0x960c, 2); put(0x9610, 0); }
    let settings = rec(0x9608);
    if settings != 0 && rec(0x3540) != 0 {
        let changed = update_work_settings(if final_output { rec(0x14) } else { rec(0xc) }, settings);
        if final_output { put(0x5ff8, rec(0xc)); put(0x5ffc, rec(0x10)); }
        if changed { post_changed(); }
    }
    if final_output {
        let flags = expand_flags(rec(0x14));
        for (mask, operation) in [(2, 0x908), (1, 0x907), (0x10, 0x930)] {
            if flags & mask != 0 { unsafe { xm1_power_application_post(operation, 0); } }
        }
    }
}
fn acquire_once() {
    if rec(0x9614) == 0 { crate::recording_engine25_buffers::acquire_output_buffers(); put(0x9614, 1); }
}
fn start_finalize() {
    release_capture_handles(rec(0x3544));
    reset_soft_timer();
    post_command(1, &[finalize_completion as *const () as u32]);
}

pub struct NativeRecordingEngine;
fn offset(controller: RecordingController) -> u32 { match controller { RecordingController::Output => 50, RecordingController::Finalize => 84 } }
impl RecordingEngine for NativeRecordingEngine {
    fn message(&self) -> RecordingMessage { RecordingMessage(half(RECORDING)) }
    fn set_output_request(&mut self, kind: RecordingKind, settings: RecordingSettings) { put(0x9604, kind.0); put(0x9608, settings.0); }
    fn set_finalize_request(&mut self, kind: RecordingKind) { put(0x93c8, kind.0); }
    fn state(&self, controller: RecordingController) -> u16 { half(STATES + offset(controller)) }
    fn set_state(&mut self, controller: RecordingController, state: u16) { store_half(STATES + offset(controller), state); }
    fn set_row(&mut self, controller: RecordingController, row: u16) { store_half(ROWS + offset(controller), row); }
    fn execute(&mut self, action: RecordingAction) -> i32 {
        match action {
            RecordingAction::Ready => 1,
            RecordingAction::Pending => 0,
            RecordingAction::StartOutput => { start_output(); 0 }
            RecordingAction::Defer => { unsafe { xm1_power_recording_defer_current(); } 0 }
            RecordingAction::StopOutput => { stop_output(); 0 }
            RecordingAction::PublishOutput => { acquire_once(); publish_output(false); 0 }
            RecordingAction::FinishOutput => { acquire_once(); publish_output(true); 1 }
            RecordingAction::FinishStop => { put(0x960c, 1); publish_output(false); 1 }
            RecordingAction::StartFinalize => { start_finalize(); 0 }
            RecordingAction::FinishFinalize => { put(0x81d8, 0); 1 }
        }
    }
}
#[no_mangle]
pub extern "C" fn xm1_power_recording_output_request(kind: u32, settings: u32) -> i32 {
    match engine::request_output(&mut NativeRecordingEngine, RecordingKind(kind), RecordingSettings(settings)) { Ok(value) => value, Err(_) => missing() }
}
#[no_mangle]
pub extern "C" fn xm1_power_recording_finalize_request(kind: u32) -> i32 {
    match engine::request_finalize(&mut NativeRecordingEngine, RecordingKind(kind)) { Ok(value) => value, Err(_) => missing() }
}

fn expand_flags(flags: u32) -> u32 { (flags & 3) | ((flags & 4) << 2) }
fn update_work_flags(flags: u32, set: bool) -> bool {
    let address = 0x009168a0;
    let before = word(address);
    store(address, if set { before | flags } else { before & !flags });
    let current = rec(0x3540);
    if current != 0 { store(current, word(address)); }
    before != word(address)
}
fn update_request_flags(flags: u32) {
    let high = flags & 2 != 0 && update_work_flags(2, false);
    if flags & 1 != 0 { update_work_flags(1, false); }
    let low = flags & 0x10 != 0 && update_work_flags(0x10, false);
    if high || low { post_changed(); }
}
pub(crate) fn update_work_settings(flags: u32, settings: u32) -> bool {
    let current = rec(0x3540);
    let before = (word(current), half(current + 0x126), word(current + 0x11c));
    let flags = expand_flags(flags);
    if flags == 0 { update_work_flags(0x13, false); }
    else {
        update_work_flags(flags, true);
        if flags & 0x11 != 0 {
            let count = unsafe { xm1_power_configuration_byte(0x3e0f80) }.clamp(1, 100);
            let value = u32::from(byte(0x0091682c));
            update_work_flags(2, value < 1 || value > count);
        }
        store_half(0x009168be, half(0x009168be) | 0x200);
        if flags & 2 != 0 || byte(rec(0x3540) + 0x104) == 0 {
            let operation = word(settings);
            let mask = match operation {
                36 => 4,
                35 | 12 => 1,
                18 => u16::from(word(settings + 0x90) != 1),
                34 | 45 | 63 => 0,
                0..=74 => match word(settings + 4) { 2 => 0, 3 => 4, _ => 1 },
                _ => 0,
            };
            store_half(0x009168be, half(0x009168be) | mask | 0x10);
            store(0x009167f0, 0);
            let current = rec(0x3540);
            store_half(current + 0x126, half(0x009168be));
            let current = rec(0x3540);
            store_half(current + 0x126, half(current + 0x126) | 2);
            store(rec(0x3540) + 0x11c, 0);
        }
    }
    let current = rec(0x3540);
    before != (word(current), half(current + 0x126), word(current + 0x11c))
}

fn release_pool_handle(pool: u32, handle: i16) {
    if handle == -1 { return; }
    if !(0..320).contains(&handle) { missing(); }
    let handle = handle as u32;
    let address = pool + 8 + (handle / 32) * 4;
    store(address, word(address) & !(1 << (handle % 32)));
}
fn release_capture_handles(handles: u32) {
    for index in 1..=12 {
        let address = handles + index * 2;
        release_pool_handle(0x015b7428 + index * 0x30, half(address) as i16);
        store_half(address, 0xffff);
    }
    let handle = half(handles) as i16;
    store_half(handles, 0xffff);
    release_pool_handle(0x015b7428, handle);
}
pub(crate) fn reset_soft_timer() {
    let saved = unsafe { xm1_power_interrupt_lock() };
    store_byte(TIMER + 1, 0);
    unsafe { xm1_power_interrupt_unlock(saved); }
    let handle = word(TIMER + 4);
    if handle != 0 {
        if unsafe { xm1_power_software_timer_cancel(handle) } == 0 { store(TIMER + 4, 0); }
        else {
            let task = unsafe { xm1_pipeline_image_current_task() } as u8;
            unsafe { xm1_power_append_stream_text(u32::from(task), 0x00eb3ad0); xm1_power_append_stream_text(u32::from(task), 0x00eb3adc); }
        }
    }
    store_byte(TIMER, 1);
    store_byte(TIMER + 2, 0);
}
