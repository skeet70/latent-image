#![no_std]

use core::ptr::{addr_of, read_volatile};
use core::sync::atomic::{AtomicU32, Ordering};

const INITIAL: u32 = 0x1357_9bdf;

#[used]
#[no_mangle]
#[link_section = ".data"]
pub static XM1_FIXED_TEST_DATA: u32 = INITIAL;
#[used]
#[no_mangle]
pub static XM1_FIXED_TEST_COUNT: AtomicU32 = AtomicU32::new(0);
#[used]
#[no_mangle]
pub static XM1_FIXED_TEST_FAILURES: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
pub extern "C" fn xm1_fixed_test_observe() {
    if unsafe { read_volatile(addr_of!(XM1_FIXED_TEST_DATA)) } != INITIAL {
        XM1_FIXED_TEST_FAILURES.store(1, Ordering::Relaxed);
    }
    XM1_FIXED_TEST_COUNT.fetch_add(1, Ordering::Relaxed);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop { core::hint::spin_loop(); }
}
