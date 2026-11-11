use core::ptr::{read_volatile, write_volatile};

pub type GeometryCompletion = unsafe extern "C" fn(i32);
#[repr(C)]
struct GeometryStart { first: u32, second: u32, third: u32 }

extern "C" {
    fn xm1_power_record_helper_eebe70() -> u32;
    fn xm1_power_record_helper_eebb6c(mode: u32) -> u32;
    fn xm1_power_record_helper_eec89c();
    fn xm1_power_record_helper_106631c(request: *const GeometryStart, completion: GeometryCompletion) -> u32;
    fn xm1_power_record_helper_106637c() -> u32;
}
unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eeccc0(first: u32, second: u32, third: u32, completion: GeometryCompletion) -> u32 {
    if unsafe { xm1_power_record_helper_eebe70() } == 0 {
        unsafe { completion(-14); }
        return 0x11;
    }
    if unsafe { word(0x4171_7764) } != 0 {
        unsafe { completion(-14); }
        return 0x81;
    }
    let first = if first <= 1 { unsafe { word(0x00ee_e4ac + first * 4) } } else { 0 };
    let second_value = unsafe { word(0x00ee_e4a4u32.wrapping_add(second.wrapping_mul(4))) };
    unsafe { write_volatile(0x0169_b938 as *mut u32, second); }
    let kind = unsafe { xm1_power_record_helper_eebb6c(third) };
    let request = GeometryStart { first, second: second_value, third: unsafe { word(0x00ee_e4b4u32.wrapping_add(kind.wrapping_mul(4))) } };
    if unsafe { xm1_power_record_helper_106631c(&request, completion) } == 0 {
        unsafe { write_volatile(0x4171_7764 as *mut u32, 1); }
        0
    } else { 0x86 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eecddc() -> u32 {
    if !matches!(unsafe { word(0x4171_7764) }, 1 | 2) { return 0x81; }
    unsafe { xm1_power_record_helper_eec89c(); }
    if unsafe { xm1_power_record_helper_106637c() } == 0 { 0 } else { 0x86 }
}
