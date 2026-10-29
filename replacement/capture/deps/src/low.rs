const WAIT_COUNT: usize = 0x009a1320;
const WAIT_TRACE: usize = 0x009a1324;
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

#[no_mangle]
pub extern "C" fn capture_wait(event: u32) {
    let index = read(WAIT_COUNT) as usize;
    write(WAIT_TRACE + index * 4, event);
    write(WAIT_COUNT, index as u32 + 1);
    let signal = read(WAIT_SIGNAL);
    if signal != 0 {
        write(CONTEXT + REQUESTED_EVENTS, signal);
    }
}

#[no_mangle]
pub extern "C" fn capture_release() {
    write(RELEASE_COUNT, read(RELEASE_COUNT) + 1);
}
