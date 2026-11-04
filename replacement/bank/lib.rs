#![no_std]

//! The bank payload's Rust crate root: one staticlib, one panic handler.

#[path = "../capture/src/lib.rs"]
mod capture;

#[path = "../service/lib.rs"]
mod service;

#[path = "../recipe_ui/src/lib.rs"]
mod recipe_ui_route;

#[path = "../menu_engine/src/lib.rs"]
mod menu_engine;

#[cfg(rb_menu_engine_owned)]
#[path = "../menu_engine/adapter/mod.rs"]
mod menu_engine_adapter;

#[cfg(all(rb_menu_engine_owned, target_arch = "arm"))]
core::arch::global_asm!(include_str!("../menu_engine/adapter/entry.S"), options(raw));

#[path = "../owned_menu/src/lib.rs"]
mod owned_menu;

#[cfg(rb_modal_scoped)]
#[path = "../modal_runtime/src/lib.rs"]
mod modal_runtime;

#[cfg(all(rb_modal_scoped, target_arch = "arm"))]
core::arch::global_asm!(include_str!("../modal_runtime/src/entry.S"), options(raw));

#[cfg(rb_lifecycle_scoped)]
#[path = "../save/src/lib.rs"]
mod save;

#[cfg(rb_lifecycle_scoped)]
#[path = "../media_permit/src/lib.rs"]
mod media_permit;

#[cfg(rb_lifecycle_scoped)]
#[path = "../media_permit/src/stock.rs"]
mod media_permit_stock;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
