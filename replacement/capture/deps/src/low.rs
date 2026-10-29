const CLEAR_COUNT: usize = 0x009a1200;
const CLEAR_TRACE: usize = 0x009a1204;
const ADVANCE_COUNT: usize = 0x009a1300;
const ADVANCE_TRACE: usize = 0x009a1304;
const CURRENT_STATE: usize = 0x009a1340;
const WAIT_FLAG: usize = 0x009a1344;
const WAIT_STATE: usize = 0x009a1348;
const WAIT_CLEAR_COUNT: usize = 0x009a134c;
const WAIT_CLEAR_TRACE: usize = 0x009a1350;
const WAIT_EVENT: usize = 0x009a1370;
const WAIT_ADVANCE_COUNT: usize = 0x009a13a0;
const WAIT_ADVANCE_TRACE: usize = 0x009a13a4;
const FINAL_FLAG: usize = 0x009a1374;
const FINAL_STATE: usize = 0x009a1378;
const FINAL_CLEAR_COUNT: usize = 0x009a137c;
const FINAL_CLEAR_TRACE: usize = 0x009a1380;
const FINAL_ADVANCE_COUNT: usize = 0x009a13c8;
const FINAL_ADVANCE_TRACE: usize = 0x009a13cc;
const WAIT_SIGNAL: usize = 0x009a1400;
const RELEASE_COUNT: usize = 0x009a1600;
const CONTEXT: usize = 0x009a0000;
const REQUESTED_EVENTS: usize = 0x4c;

#[inline(always)]
fn read(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
fn write(address: usize, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

#[inline(always)]
fn copy_cleared(count: usize, trace: usize) {
    let number = read(CLEAR_COUNT);
    write(count, number);
    let mut index = 0;
    while index < number {
        write(trace + index as usize * 4, read(CLEAR_TRACE + index as usize * 4));
        index += 1;
    }
}

#[inline(always)]
fn copy_advanced(count: usize, trace: usize) {
    let number = read(ADVANCE_COUNT);
    write(count, number);
    let mut index = 0;
    while index < number {
        write(trace + index as usize * 4, read(ADVANCE_TRACE + index as usize * 4));
        index += 1;
    }
}

#[no_mangle]
pub extern "C" fn capture_wait(event: u32) {
    write(WAIT_FLAG, 1);
    write(WAIT_STATE, read(CURRENT_STATE));
    copy_cleared(WAIT_CLEAR_COUNT, WAIT_CLEAR_TRACE);
    copy_advanced(WAIT_ADVANCE_COUNT, WAIT_ADVANCE_TRACE);
    write(WAIT_EVENT, event);
    let signal = read(WAIT_SIGNAL);
    if signal != 0 {
        write(CONTEXT + REQUESTED_EVENTS, signal);
    }
}

#[no_mangle]
pub extern "C" fn capture_release() {
    write(FINAL_FLAG, 1);
    write(FINAL_STATE, read(CURRENT_STATE));
    copy_cleared(FINAL_CLEAR_COUNT, FINAL_CLEAR_TRACE);
    copy_advanced(FINAL_ADVANCE_COUNT, FINAL_ADVANCE_TRACE);
    write(RELEASE_COUNT, read(RELEASE_COUNT) + 1);
}
