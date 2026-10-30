#![no_std]

#[path = "src/model.rs"]
mod model;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
#[link_section = ".text.rb_mp_cancel"]
pub extern "C" fn rb_mp_cancel(slot: u32, event: u32) {
    model::record(model::TRACE_CANCELS, model::TRACE_CANCELS + 4, slot, event);
}
