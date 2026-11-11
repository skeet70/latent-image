#![allow(unsafe_op_in_unsafe_fn)]
use crate::record_task43_aux_head_ffi::*;

core::arch::global_asm!(include_str!("record_task43_aux_head_residue.S"), options(raw));

pub(crate) const RESULT: u32 = 0x016b_f954;
pub(crate) fn handle() -> u32 { word(0x016b_fe48) }
pub(crate) fn word(address: u32) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
pub(crate) fn half(address: u32) -> u16 { unsafe { core::ptr::read_volatile(address as *const u16) } }
pub(crate) fn signed(address: u32) -> u32 { i32::from(half(address) as i16) as u32 }
pub(crate) fn byte(address: u32) -> u8 { unsafe { core::ptr::read_volatile(address as *const u8) } }
pub(crate) fn put(address: u32, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u32, value) }; }
pub(crate) fn put_half(address: u32, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u16, value as u16) }; }
pub(crate) fn put_byte(address: u32, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u8, value as u8) }; }
pub(crate) fn copy(destination: u32, source: u32, size: usize) { unsafe { core::ptr::copy_nonoverlapping(source as *const u8, destination as *mut u8, size) }; }
pub(crate) fn clear(destination: u32, size: usize) { unsafe { core::ptr::write_bytes(destination as *mut u8, 0, size) }; }

#[derive(Clone, Copy)]
pub(crate) struct Frame(pub u32);
impl Frame {
    pub(crate) fn at(self, offset: u32) -> u32 { self.0 - offset }
    pub(crate) fn word(self, offset: u32) -> u32 { word(self.at(offset)) }
    pub(crate) fn signed(self, offset: u32) -> u32 { signed(self.at(offset)) }
    pub(crate) fn half(self, offset: u32) -> u32 { u32::from(half(self.at(offset))) }
    pub(crate) fn byte(self, offset: u32) -> u32 { u32::from(byte(self.at(offset))) }
    pub(crate) fn put(self, offset: u32, value: u32) { put(self.at(offset), value); }
    pub(crate) fn put_half(self, offset: u32, value: u32) { put_half(self.at(offset), value); }
    pub(crate) fn put_byte(self, offset: u32, value: u32) { put_byte(self.at(offset), value); }
}

pub(crate) unsafe fn snapshot(destination: u32, value: u32) {
    let mut record = core::mem::MaybeUninit::<[u32; 19]>::uninit();
    xm1_power_record_helper_7588e8(record.as_mut_ptr() as u32);
    copy(destination, record.as_ptr() as u32, 76);
    put(destination + 0x38, xm1_power_record_helper_76d9c8());
    put(destination + 0x3c, xm1_power_record_helper_746834(value));
    xm1_power_record_helper_75890c(handle(), destination);
}


pub(crate) unsafe fn publish_result(frame: Frame, index: u32, value: u32, scale: u32, category: u32, first: u32, second: u32, adjustment: u32, bound: u32, flags: u32) {
    put_half(RESULT, index);
    put(RESULT + 0x14, category);
    put_half(RESULT + 0x10, scale);
    put_half(RESULT + 0xe, xm1_power_record_helper_ef5310(category));
    put(RESULT + 4, value);
    put_half(RESULT + 8, frame.half(first));
    put_half(RESULT + 0xa, frame.half(second));
    put_half(RESULT + 0xc, frame.half(adjustment));
    put_half(RESULT + 0x18, frame.half(bound));
    put_byte(RESULT + 0x1a, frame.byte(flags));
    put(RESULT + 0x1c, u32::MAX);
    put(RESULT + 0x20, u32::MAX);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f339f8() { put(0x016b_fe4c, xm1_power_record_helper_7569ec(handle())); }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f3313c() {
    put(0x4171_7f6c, 0);
    put(0x4171_7f70, 0);
    put(0x4171_7f74, xm1_power_record_helper_76d9c8());
}
