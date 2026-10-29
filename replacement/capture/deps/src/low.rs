const TRACE: usize = 0x009a1200;
const TRACE_COUNT: usize = 0x009a1300;
const WAIT_SIGNAL: usize = 0x009a1400;
const RELEASE_COUNT: usize = 0x009a1600;
const CONTEXT: usize = 0x009a0000;
const REQUESTED_EVENTS: usize = 0x4c;
const WAIT: u32 = 0x0400_0000;

#[inline(always)]
fn read(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
fn write(address: usize, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

#[inline(always)]
fn record(entry: u32) {
    let index = read(TRACE_COUNT) as usize;
    write(TRACE + index * 4, entry);
    write(TRACE_COUNT, index as u32 + 1);
}

#[no_mangle]
pub extern "C" fn capture_wait(event: u32) {
    record(WAIT | event);
    let signal = read(WAIT_SIGNAL);
    if signal != 0 {
        write(CONTEXT + REQUESTED_EVENTS, signal);
    }
}

#[no_mangle]
pub extern "C" fn capture_release() {
    write(RELEASE_COUNT, read(RELEASE_COUNT) + 1);
}
