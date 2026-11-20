#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
pub enum Shared {
    Alpha,
    Beta,
}

#[no_mangle]
pub static mut OWNER: Shared = Shared::Alpha;

#[no_mangle]
pub static mut RECOGNITION: Shared = Shared::Beta;
