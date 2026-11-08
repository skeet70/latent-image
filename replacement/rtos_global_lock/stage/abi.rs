//! Global-lock A32 adapter: the three stock root entries and their atomic helper closure in place.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod global_lock;

use core::arch::naked_asm;

use global_lock::LockWord;

#[no_mangle]
#[inline(never)]
#[link_section = ".text.global_lock_try"]
pub extern "C" fn xm1_rtos_global_lock_try(word: u32, stamp: u32) -> i32 {
    unsafe { helper_try(LockWord::from_argument(word).address().0, stamp) }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.global_lock_release"]
pub extern "C" fn xm1_rtos_global_lock_release(word: u32) -> i32 {
    unsafe { helper_release(LockWord::from_argument(word).address().0) }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.global_lock_observe"]
pub extern "C" fn xm1_rtos_global_lock_observe(word: u32) -> u32 {
    unsafe { helper_observe(LockWord::from_argument(word).address().0) }
}

#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text.global_lock_helper_try"]
pub unsafe extern "C" fn helper_try(_word: u32, _stamp: u32) -> i32 {
    naked_asm!(
        "1:",
        "ldrex r2, [r0]",
        "cmp r2, #0",
        "mvnne r0, #0",
        "bxne lr",
        "strexeq r2, r1, [r0]",
        "cmp r2, #0",
        "bne 1b",
        "dmb sy",
        "mov r0, #0",
        "bx lr",
    )
}

#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text.global_lock_helper_release"]
pub unsafe extern "C" fn helper_release(_word: u32) -> i32 {
    naked_asm!(
        "mov r1, #0",
        "1:",
        "ldrex r2, [r0]",
        "cmp r2, #0",
        "mvneq r0, #0",
        "bxeq lr",
        "strexne r2, r1, [r0]",
        "cmp r2, #0",
        "bne 1b",
        "dmb sy",
        "mov r0, #0",
        "bx lr",
    )
}

#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text.global_lock_helper_observe"]
pub unsafe extern "C" fn helper_observe(_word: u32) -> u32 {
    naked_asm!("ldr r0, [r0]", "bx lr")
}

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
