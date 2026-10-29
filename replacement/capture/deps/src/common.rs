pub const PLAN: usize = 0x009a1000;
pub const CHECK_COUNT: usize = 0x009a1100;
pub const CHECK_STATUS: usize = 0x009a1104;
pub const TRACE: usize = 0x009a1200;
pub const TRACE_COUNT: usize = 0x009a1300;

pub const CLEAR: u32 = 0x0100_0000;
pub const ADVANCE: u32 = 0x0200_0000;
pub const LOG: u32 = 0x0300_0000;

#[inline(always)]
pub fn read(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
pub fn write(address: usize, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

#[inline(always)]
pub fn record(entry: u32) {
    let index = read(TRACE_COUNT) as usize;
    write(TRACE + index * 4, entry);
    write(TRACE_COUNT, index as u32 + 1);
}
