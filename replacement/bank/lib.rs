#![no_std]

//! The bank payload's Rust crate root: one staticlib, one panic handler.

#[path = "../service/lib.rs"]
mod service;

#[path = "../recipe_ui/src/lib.rs"]
mod recipe_ui_route;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
