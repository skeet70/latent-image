#![no_std]

#[path = "src/high.rs"]
mod high;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
