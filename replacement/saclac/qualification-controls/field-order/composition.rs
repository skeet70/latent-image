#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
pub struct Shared {
    pub second: u16,
    pub first: u32,
}

#[no_mangle]
pub static mut OWNER: Shared = Shared { second: 2, first: 1 };

#[no_mangle]
pub static mut RECOGNITION: Shared = Shared { second: 4, first: 3 };
