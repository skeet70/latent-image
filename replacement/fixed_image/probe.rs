#![no_std]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

const INITIAL: u32 = 0x1357_9bdf;

#[used]
#[no_mangle]
pub static mut XM1_FIXED_TEST_DATA: u32 = INITIAL;
#[used]
#[no_mangle]
pub static mut XM1_FIXED_TEST_COUNT: u32 = 0;
#[used]
#[no_mangle]
pub static mut XM1_FIXED_TEST_FAILURES: u32 = 0;

#[no_mangle]
pub extern "C" fn xm1_fixed_test_observe() {
    unsafe {
        let count = read_volatile(addr_of!(XM1_FIXED_TEST_COUNT));
        let data = read_volatile(addr_of!(XM1_FIXED_TEST_DATA));
        if data != INITIAL.wrapping_add(count) {
            write_volatile(addr_of_mut!(XM1_FIXED_TEST_FAILURES), 1);
        }
        write_volatile(addr_of_mut!(XM1_FIXED_TEST_DATA), data.wrapping_add(1));
        write_volatile(addr_of_mut!(XM1_FIXED_TEST_COUNT), count.wrapping_add(1));
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop { core::hint::spin_loop(); }
}
