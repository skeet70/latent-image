#![no_std]

#[path = "src/model.rs"]
mod model;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
#[link_section = ".text.rb_mp_fatal2"]
pub extern "C" fn rb_mp_fatal2() {
    model::bump(model::TRACE_FAULTS);
}
