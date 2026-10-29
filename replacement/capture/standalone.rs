#![no_std]

#[path = "src/lib.rs"]
mod capture;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
