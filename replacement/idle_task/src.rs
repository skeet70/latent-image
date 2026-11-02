#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;

#[no_mangle]
pub extern "C" fn xm1_idle_task() -> ! {
    loop {
        // WFI resumes at the loop backedge after an architectural wake.
        unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) };
    }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    unsafe { asm!("udf #0", options(noreturn, nomem, nostack)) }
}
