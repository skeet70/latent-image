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

#[no_mangle]
pub static mut OWNER: Shared = Shared { first: 1, second: 2 };

#[no_mangle]
pub static mut RECOGNITION: Shared = Shared { first: 3, second: 4 };
