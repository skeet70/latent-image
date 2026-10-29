#![no_std]

#[path = "src/model.rs"]
mod model;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
#[link_section = ".text.rb_mp_command"]
pub extern "C" fn rb_mp_command(slot: u32, command: u32) {
    model::record(model::TRACE_COMMANDS, model::TRACE_COMMANDS + 4, slot, command);
    if model::read_u32(model::COUNT_ARMED) == 1 {
        model::write_u32(model::COUNT_CELL, 1);
    }
}
