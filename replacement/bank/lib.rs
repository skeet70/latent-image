#![no_std]

//! The bank payload's Rust crate root: one staticlib, one panic handler.

#[path = "../capture/src/lib.rs"]
mod capture;

#[path = "../service/lib.rs"]
mod service;

#[path = "../recipe_ui/src/lib.rs"]
mod recipe_ui_route;

#[path = "../owned_menu/src/lib.rs"]
mod owned_menu;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
