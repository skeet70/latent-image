#![no_std]

#[path = "src/model.rs"]
mod model;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
#[link_section = ".text.rb_mp_subframe_count"]
pub extern "C" fn rb_mp_subframe_count(_slot: u32) -> u32 {
    model::read_u32(model::COUNT_CELL)
}
