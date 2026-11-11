use core::ptr::{read_volatile, write_volatile};

pub(crate) const FRAME: usize = 0x0094_4580;
pub(crate) const INPUT: usize = 0x0095_6838;

pub(crate) unsafe fn read_word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
pub(crate) unsafe fn write_word(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
pub(crate) unsafe fn read_half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
pub(crate) unsafe fn write_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value); } }
pub(crate) unsafe fn read_byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
pub(crate) unsafe fn write_byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value); } }

pub(crate) unsafe fn copy_bytes(destination: usize, source: usize, length: usize) {
    for offset in 0..length {
        unsafe { write_byte(destination + offset, read_byte(source + offset)); }
    }
}

pub(crate) unsafe fn fill_bytes(destination: usize, value: u8, length: usize) {
    for offset in 0..length {
        unsafe { write_byte(destination + offset, value); }
    }
}
