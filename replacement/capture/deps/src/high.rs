#[path = "common.rs"]
mod common;

use common::{ADVANCE_COUNT, ADVANCE_TRACE, CHECK_COUNT, CHECK_STATUS, CLEAR_COUNT, CLEAR_TRACE, PLAN, read, record, write};

#[no_mangle]
pub extern "C" fn capture_plan_lookup(_first: u32, _step: u32) -> *const u32 {
    PLAN as *const u32
}

#[no_mangle]
pub extern "C" fn capture_cancel_mask(out: *mut u32, bit: u32) {
    let word = unsafe { out.add((bit >> 5) as usize) };
    let value = unsafe { core::ptr::read_volatile(word) } | (1u32 << (bit & 31));
    unsafe { core::ptr::write_volatile(word, value) }
}

#[no_mangle]
pub extern "C" fn capture_check(_context: u32, _event: u32) -> i32 {
    let index = read(CHECK_COUNT);
    write(CHECK_COUNT, index + 1);
    read(CHECK_STATUS + index as usize * 4) as i32
}

#[no_mangle]
pub extern "C" fn capture_clear(_context: u32, event: u32) {
    record(CLEAR_COUNT, CLEAR_TRACE, event)
}

#[no_mangle]
pub extern "C" fn capture_advance(_context: u32, state: u32) {
    record(ADVANCE_COUNT, ADVANCE_TRACE, state)
}

#[no_mangle]
pub extern "C" fn capture_log(_context: u32, _phase: u32) {}
