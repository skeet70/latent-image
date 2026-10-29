#![no_std]

#[path = "src/low.rs"]
mod low;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
