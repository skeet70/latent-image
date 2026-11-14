#![no_std]

//! The bank payload's Rust crate root: one staticlib, one panic handler.

extern crate capture;
extern crate menu_engine;
extern crate owned_menu;
extern crate recipe_ui;
extern crate service;

#[cfg(rb_time_services_owned)]
extern crate xm1_time_services;

#[cfg(rb_time_services_owned)]
#[no_mangle]
#[used]
pub static XM1_KERNEL_BOUNDARY: xm1_kernel::slot_table::Boundary<7, 26> =
    xm1_kernel::slot_table::FULL_BOUNDARY;

#[cfg(all(rb_menu_engine_owned, not(rb_time_services_owned)))]
#[no_mangle]
#[used]
pub static XM1_KERNEL_BOUNDARY: xm1_kernel::slot_table::Boundary<4, 12> =
    xm1_kernel::slot_table::BASIC_BOUNDARY;

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
