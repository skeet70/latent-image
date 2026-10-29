#![no_std]

#[path = "src/model.rs"]
mod model;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub unsafe extern "C" fn rb_mp_memcpy(dst: *mut u8, src: *const u8, len: usize) {
    let mut index = 0;
    while index < len {
        unsafe { core::ptr::write_volatile(dst.add(index), core::ptr::read_volatile(src.add(index))) };
        index += 1;
    }
}

#[no_mangle]
pub extern "C" fn rb_mp_log(_task: u32, _name: *const u8) {
    model::bump(model::TRACE_LOGS);
}
