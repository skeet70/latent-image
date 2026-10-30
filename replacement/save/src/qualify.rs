#![no_std]
#![allow(unexpected_cfgs)]

#[path = "lib.rs"]
mod save;

#[cfg(xm1_freestanding)]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
