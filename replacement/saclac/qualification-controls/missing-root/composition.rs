#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
pub struct Shared {
    pub first: u32,
    pub second: u16,
}
