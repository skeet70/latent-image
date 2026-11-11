use super::*;
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
use crate::shutdown_rtos::NativeShutdownRtos;
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_recording_defer_current() -> i32;
    fn xm1_power_recording_send_task_message(operation: u32, size: u32, payload: *const u8) -> i32;
    fn xm1_power_recording_halt_error(status: i32) -> !;
    fn xm1_power_recording_report_missing_state();
}

pub struct NativeRecordingEngine;

impl Runtime for NativeRecordingEngine {
    fn read16(&self, address: Address) -> u16 { unsafe { read_volatile(address.0 as *const u16) } }
    fn read32(&self, address: Address) -> u32 { unsafe { read_volatile(address.0 as *const u32) } }
    fn write16(&mut self, address: Address, value: u16) { unsafe { write_volatile(address.0 as *mut u16, value); } }
    fn write32(&mut self, address: Address, value: u32) { unsafe { write_volatile(address.0 as *mut u32, value); } }
    fn submit_engine(&mut self, operation: EngineOperation) {
        let (operation, callback) = match operation {
            EngineOperation::Resume => (RequestId(0x7308), xm1_power_recording_engine_resumed as *const () as u32),
            EngineOperation::Suspend => (RequestId(0x7309), xm1_power_recording_engine_suspended as *const () as u32),
        };
        let mut payload = [0u8; 24];
        payload[..4].copy_from_slice(&callback.to_le_bytes());
        let mut rtos = NativeShutdownRtos;
        let sender = rtos.current_task();
        let _ = rtos.post(ServiceMessage { operation, sender, destination: QueueId(0x63), payload: &payload });
    }
    fn defer_current(&mut self) { unsafe { let _ = xm1_power_recording_defer_current(); } }
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_engine_resumed() {
    unsafe { let _ = xm1_power_recording_send_task_message(0x13e5, 0, core::ptr::null()); }
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_engine_suspended() {
    unsafe { let _ = xm1_power_recording_send_task_message(0x13e6, 0, core::ptr::null()); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_recording_step_request(input: *const u32, output: *mut u8) -> i32 {
    let result = if input.is_null() {
        unsafe { xm1_power_recording_report_missing_state(); }
        1
    } else {
        let words = unsafe { [read_volatile(input), read_volatile(input.add(1))] };
        match request(&mut NativeRecordingEngine, words) {
            Ok(result) => result,
            Err(_) => unsafe { xm1_power_recording_halt_error(-255) },
        }
    };
    if !output.is_null() {
        unsafe { write_volatile(output, read_volatile(OUTPUT.0 as *const u8)); }
    }
    result
}
