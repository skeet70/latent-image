//! Global-lock A32 adapter: the three stock root extension entries over the owned core.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod global_lock;

use core::arch::{asm, global_asm};

use global_lock::{
    observe, release, try_acquire, GuestAddress, LockStamp, LockWord, Machine, ReleaseResult, TryResult,
};

struct Cpu;

impl Machine for Cpu {
    #[inline(always)]
    fn load_word(&self, at: GuestAddress) -> u32 {
        let value: u32;
        unsafe {
            asm!("ldr {value}, [{address}]", value = out(reg) value, address = in(reg) at.0, options(nostack))
        };
        value
    }

    #[inline(always)]
    fn exclusive_load(&self, at: GuestAddress) -> u32 {
        let value: u32;
        unsafe {
            asm!("ldrex {value}, [{address}]", value = out(reg) value, address = in(reg) at.0, options(nostack))
        };
        value
    }

    #[inline(always)]
    fn exclusive_store(&self, at: GuestAddress, value: u32) -> u32 {
        let status: u32;
        unsafe {
            asm!(
                "strex {status}, {value}, [{address}]",
                status = out(reg) status,
                value = in(reg) value,
                address = in(reg) at.0,
                options(nostack)
            )
        };
        status
    }

    #[inline(always)]
    fn data_memory_barrier(&self) {
        unsafe { asm!("dmb sy", options(nostack)) };
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.global_lock_try_body"]
pub extern "C" fn xm1_rtos_global_lock_try_body(word: u32, stamp: u32) -> i32 {
    match try_acquire(&Cpu, LockWord::from_argument(word), LockStamp(stamp)) {
        TryResult::Acquired => 0,
        TryResult::Held => -1,
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.global_lock_release_body"]
pub extern "C" fn xm1_rtos_global_lock_release_body(word: u32) -> i32 {
    match release(&Cpu, LockWord::from_argument(word)) {
        ReleaseResult::Released => 0,
        ReleaseResult::AlreadyFree => -1,
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.global_lock_observe_body"]
pub extern "C" fn xm1_rtos_global_lock_observe_body(word: u32) -> u32 {
    observe(&Cpu, LockWord::from_argument(word)).raw()
}

macro_rules! entry {
    ($name:ident, $body:ident) => {
        global_asm!(
            concat!(
                ".pushsection .text.global_lock_", stringify!($name), "_entry, \"ax\", %progbits\n",
                ".arm\n",
                ".global xm1_rtos_global_lock_", stringify!($name), "\n",
                ".type xm1_rtos_global_lock_", stringify!($name), ", %function\n",
                "xm1_rtos_global_lock_", stringify!($name), ":\n",
                "b xm1_rtos_global_lock_", stringify!($body), "\n",
                ".size xm1_rtos_global_lock_", stringify!($name), ", . - xm1_rtos_global_lock_", stringify!($name), "\n",
                ".popsection\n",
            )
        );
    };
}

entry!(try, try_body);
entry!(release, release_body);
entry!(observe, observe_body);

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
