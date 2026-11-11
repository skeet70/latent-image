#![no_std]

#[path = "../src/lib.rs"]
mod menu_value;

#[path = "mod.rs"]
mod menu_value_adapter;

core::arch::global_asm!(include_str!("entry.S"), options(raw));

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
