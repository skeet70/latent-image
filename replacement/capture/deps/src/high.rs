#[path = "common.rs"]
mod common;

use common::{
    ADMITTED_STEP, ADVANCE_COUNT, ADVANCE_TRACE, CHECK_COUNT, CHECK_STATUS, CLEAR_COUNT, CLEAR_TRACE,
    CURRENT_STATE, PLAN, context_is_fixture, live_plan_word, read, record, write,
};

#[link_section = ".text.capture_plan_lookup"]
#[no_mangle]
pub extern "C" fn capture_plan_lookup(first: u32, step: u32) -> *const u32 {
    if first != 0x80 || step != read(ADMITTED_STEP) {
        loop {}
    }
    PLAN as *const u32
}

#[link_section = ".text.capture_cancel_mask"]
#[no_mangle]
pub extern "C" fn capture_cancel_mask(out: *mut u32, bit: u32) {
    let word = unsafe { out.add((bit >> 5) as usize) };
    let value = read(word as usize) | (1u32 << (bit & 31));
    write(word as usize, value)
}

#[link_section = ".text.capture_check"]
#[no_mangle]
pub extern "C" fn capture_check(context: u32, event: u32) -> i32 {
    if !context_is_fixture(context) || event != live_plan_word(0) {
        loop {}
    }
    let index = read(CHECK_COUNT);
    write(CHECK_COUNT, index + 1);
    read(CHECK_STATUS + index as usize * 4) as i32
}

#[link_section = ".text.capture_clear"]
#[no_mangle]
pub extern "C" fn capture_clear(context: u32, event: u32, r2: u32, r3: u32) {
    if !context_is_fixture(context)
        || r2 != 0
        || r3 != 0
        || (event != live_plan_word(1) && event != live_plan_word(2))
    {
        loop {}
    }
    record(CLEAR_COUNT, CLEAR_TRACE, event)
}

#[link_section = ".text.capture_advance"]
#[no_mangle]
pub extern "C" fn capture_advance(context: u32, state: u32) {
    if !context_is_fixture(context)
        || (state != live_plan_word(3) && state != live_plan_word(4))
    {
        loop {}
    }
    write(CURRENT_STATE, state);
    record(ADVANCE_COUNT, ADVANCE_TRACE, state)
}

#[link_section = ".text.capture_log"]
#[no_mangle]
pub extern "C" fn capture_log(_context: u32, _phase: u32) {}
