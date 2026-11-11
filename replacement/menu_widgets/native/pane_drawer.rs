#![no_std]
#![no_main]

extern crate render_commands;
#[path = "../src/pane_drawer.rs"]
pub mod pane_drawer;
mod menu_widgets { pub use crate::pane_drawer; }
#[path = "../../render_adapter/pane.rs"]
mod services;
#[path = "../../render_adapter/calls.rs"]
mod calls;

core::arch::global_asm!(include_str!("../../render_adapter/entry.S"), options(raw));

#[no_mangle]
pub extern "C" fn owned_pane_drawer(node: u32, mode: u32) {
    pane_drawer::pane_drawer(&mut services::StockServices, render_commands::NodeId(node), pane_drawer::DrawMode(mode));
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop { core::hint::spin_loop(); } }
