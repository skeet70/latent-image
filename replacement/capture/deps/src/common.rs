pub const PLAN: usize = 0x009a1000;
pub const CHECK_COUNT: usize = 0x009a1100;
pub const CHECK_STATUS: usize = 0x009a1104;
pub const ADMITTED_STEP: usize = 0x009a1180;
pub const CLEAR_COUNT: usize = 0x009a1200;
pub const CLEAR_TRACE: usize = 0x009a1204;
pub const ADVANCE_COUNT: usize = 0x009a1300;
pub const ADVANCE_TRACE: usize = 0x009a1304;
pub const CURRENT_STATE: usize = 0x009a1340;

#[inline(always)]
pub fn read(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
pub fn write(address: usize, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

#[inline(always)]
pub fn record(count: usize, trace: usize, value: u32) {
    let index = read(count) as usize;
    write(trace + index * 4, value);
    write(count, index as u32 + 1);
}
