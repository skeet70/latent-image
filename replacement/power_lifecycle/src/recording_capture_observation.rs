#![cfg(target_arch = "arm")]

use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
struct TraceOperation(u32);

fn append_trace(operation: TraceOperation) {
    unsafe {
        if read_volatile(0x4089ebec as *const u32) != 1 { return; }
        let buffer = read_volatile(0x40a5c8b0 as *const u32) as usize;
        let count_address = (buffer + 0x20) as *mut u32;
        let count = read_volatile(count_address);
        if count >= 0x1540 { return; }
        let entry = buffer + count as usize * 12;
        write_volatile(count_address, count + 1);
        let countdown = read_volatile(0xfff91000 as *const u32);
        write_volatile((entry + 0x28) as *mut u32, 30000u32.wrapping_sub(countdown));
        let ticks = read_volatile(0x400c0800 as *const u32);
        write_volatile((entry + 0x2c) as *mut u32, ticks);
        write_volatile((entry + 0x24) as *mut u32, operation.0);
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_capture_trace_begin(operation: u32) {
    append_trace(TraceOperation(operation));
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_capture_trace_end(operation: u32) {
    append_trace(TraceOperation(operation | 0x10000000));
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_capture_conversion_changed(first: u32, second: u32) -> i32 {
    let selected = match first {
        0x23 | 0x24 => true,
        0x2c => second == 1,
        0x1d | 0x1f | 0x20 => second == 1 || second == 3,
        _ => false,
    };
    if !selected { return 0; }
    let current = unsafe { read_volatile(0x0097377d as *const u8) };
    let saved = unsafe { read_volatile(0x0097377e as *const u8) };
    i32::from(current != saved)
}
