use core::ptr::{read_volatile, write_volatile};

// X-M1 1.50 f46b44, f46b9c, f46bb0 and f46bc8 have constant or empty bodies.
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f46b44(_: u32) -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f46b9c() {}
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f46bb0() -> i32 { 2 }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f46bc8(_: u32) -> i32 { 0 }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f46b5c() {
    let count = unsafe { read_volatile(0x16c1f0c as *const u8) }.wrapping_add(1);
    unsafe { write_volatile(0x16c1f0c as *mut u8, if count == 255 { 254 } else { count }); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f3d490() -> i32 {
    unsafe {
        if read_volatile(0x16c1f10 as *const u32) != 0 || read_volatile(0x16c1f14 as *const u16) == 0 { return 0; }
        write_volatile(0x16c1f14 as *mut u8, 0);
        write_volatile(0x16c1f15 as *mut u8, 0);
        write_volatile(0x16c0fbc as *mut u8, 0);
        write_volatile(0x16c0fe4 as *mut u8, read_volatile(0x16c0fe5 as *const u8));
        write_volatile(0x16c0fe2 as *mut u16, 0);
    }
    1
}

extern "C" { fn xm1_power_record_helper_7501cc(resource: u32) -> u32; }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f3aef8() -> u32 {
    unsafe { xm1_power_record_helper_7501cc(read_volatile(0x16c0718 as *const u32)) }
}
