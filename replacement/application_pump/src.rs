#![no_std]

use core::panic::PanicInfo;

#[repr(transparent)]
pub struct PumpResult(u32);

extern "C" {
    fn xm1_application_prehook();
    fn xm1_application_normalizer() -> PumpResult;
    fn xm1_application_posthook();
}

/// # Safety
/// Entry requires the admitted task1 context and initialized retained application services.
#[no_mangle]
#[link_section = ".text.application_pump"]
pub unsafe extern "C" fn xm1_application_pump() -> PumpResult {
    unsafe {
        xm1_application_prehook();
        let result = xm1_application_normalizer();
        xm1_application_posthook();
        result
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
