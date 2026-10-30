#![allow(dead_code)]

pub const MIRROR: usize = 0x015a_d430;
pub const TRACE_COMMANDS: usize = 0x015b_8000;
pub const TRACE_CANCELS: usize = 0x015b_8100;
pub const TRACE_FAULTS: usize = 0x015b_8200;
pub const TRACE_HANDBACK_CLASS: usize = 0x015b_8204;
pub const TRACE_HANDBACK_COUNT: usize = 0x015b_8208;
pub const TRACE_LOGS: usize = 0x015b_820c;
pub const COUNT_CELL: usize = 0x015b_8300;
pub const COUNT_ARMED: usize = 0x015b_8304;
pub const TRACE_CAPACITY: u32 = 16;

#[inline(always)]
pub fn read_u16(address: usize) -> u16 {
    unsafe { core::ptr::read_volatile(address as *const u16) }
}

#[inline(always)]
pub fn read_u32(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
pub fn write_u32(address: usize, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

#[inline(always)]
pub fn bump(count: usize) {
    write_u32(count, read_u32(count).wrapping_add(1));
}

#[inline(always)]
pub fn record(count: usize, trace: usize, first: u32, second: u32) {
    let index = read_u32(count);
    if index < TRACE_CAPACITY {
        write_u32(trace + index as usize * 8, first);
        write_u32(trace + index as usize * 8 + 4, second);
    }
    write_u32(count, index.wrapping_add(1));
}
