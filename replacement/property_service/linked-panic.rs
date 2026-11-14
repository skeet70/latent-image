#![no_std]

#[no_mangle]
#[used]
pub static XM1_KERNEL_BOUNDARY: xm1_kernel::slot_table::Boundary<4, 12> =
    xm1_kernel::slot_table::BASIC_BOUNDARY;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
