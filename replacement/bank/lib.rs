#![no_std]

//! The bank payload's Rust crate root: one staticlib, one panic handler.

extern crate capture;
extern crate menu_engine;
extern crate owned_menu;
extern crate recipe_ui;
extern crate service;

#[cfg(rb_menu_engine_owned)]
extern crate menu_engine_adapter;

#[cfg(rb_menu_value_owned)]
extern crate menu_value;

#[cfg(rb_render_owned)]
extern crate render_adapter;

#[cfg(rb_modal_scoped)]
extern crate modal_runtime;

#[cfg(rb_lifecycle_scoped)]
extern crate media_permit;

#[cfg(rb_lifecycle_scoped)]
extern crate save;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
