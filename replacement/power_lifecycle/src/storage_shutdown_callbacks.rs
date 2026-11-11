#![cfg(target_arch = "arm")]

extern "C" { fn xm1_power_set_task_flags(task: u32, bits: u32) -> i32; }
unsafe fn put_word(offset: u32, value: u32) { unsafe { core::ptr::write_volatile((0x956774 + offset) as *mut u32, value) }; }
unsafe fn put_half(offset: u32, value: u16) { unsafe { core::ptr::write_volatile((0x956774 + offset) as *mut u16, value) }; }
unsafe fn record_status(status: i32) {
    unsafe { put_word(0x14, if status == 0 { 0 } else { (-255i32) as u32 }) };
}
unsafe fn signal(bits: u32) {
    if unsafe { xm1_power_set_task_flags(100, bits) } != 0 { unsafe { put_word(0x14, (-255i32) as u32) }; }
}
unsafe fn complete(status: i32, bits: u32) {
    unsafe { record_status(status); signal(bits); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_storage_shutdown_ready(status: i32) { unsafe { complete(status, 1) }; }
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_shutdown_auxiliary(status: i32) { unsafe { complete(status, 2) }; }
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_shutdown_dma() { unsafe { complete(0, 4) }; }
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_shutdown_extended(status: i32) { unsafe { complete(status, 0x40) }; }

#[no_mangle]
pub unsafe extern "C" fn xm1_storage_shutdown_information(status: i32, descriptor: *const u8) {
    if status == 0 {
        unsafe {
            put_half(0x30, core::ptr::read_unaligned(descriptor.cast::<u16>()));
            core::ptr::write_volatile((0x956774 + 0x3a) as *mut u8, core::ptr::read(descriptor.add(2)));
        }
    }
    unsafe { complete(status, 8) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_storage_shutdown_page_read(status: i32, descriptor: *const u8) -> u32 {
    let page = unsafe { core::ptr::read_unaligned(descriptor.cast::<u16>()) };
    if status == 0 {
        unsafe {
            put_word(0x20, core::ptr::read_unaligned(descriptor.add(4).cast::<u32>()));
            put_half(0x1c, page);
        }
    } else { unsafe { put_half(0x1a, page) }; }
    unsafe { complete(status, 0x20) };
    if status != 0 { 2 } else { u32::from(page == 0xffff) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_storage_shutdown_page_write(status: i32, descriptor: *const u8) -> u32 {
    let page = unsafe { core::ptr::read_unaligned(descriptor.cast::<u16>()) };
    unsafe { record_status(status); put_half(0x1a, page); signal(0x10); }
    u32::from(status == 0 && page == 0xff00)
}
