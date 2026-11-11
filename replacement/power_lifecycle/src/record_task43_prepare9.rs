use core::ptr::{read_volatile, write_volatile};

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_prepare9(event: u32, request: u32) {
    unsafe {
        write_volatile(0x16c06ec as *mut u32, request);
        match event {
            0x14 => write_volatile(0x16c06fc as *mut u32, read_volatile(0x169d218 as *const u32)),
            0x2b => write_volatile(0x16c06fc as *mut u32, read_volatile(0x169d358 as *const u32)),
            _ => {}
        }
    }
}

// The only pre-dispatch call at efe480 targets the empty f3a3e4 function.
#[no_mangle]
pub extern "C" fn xm1_power_record_task43_prepare10(_: u32, _: u32) {}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_prepare11(event: u32, _: u32) {
    unsafe {
        match event {
            0x0e | 0xa7 => write_volatile(0x16b5b14 as *mut u32, u32::from(event == 0xa7)),
            0x34 => write_volatile(0x954dbc as *mut u32, read_volatile(0x169d384 as *const u32)),
            _ => {}
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_prepare12(_: u32, request: u32) {
    unsafe { write_volatile(0x16be01c as *mut u32, request); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_prepare13(_: u32, request: u32) {
    unsafe { write_volatile(0x16be054 as *mut u32, request); }
}
