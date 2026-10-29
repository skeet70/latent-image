#![no_std]

#[path = "src/faulty_low.rs"]
mod faulty_low;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
