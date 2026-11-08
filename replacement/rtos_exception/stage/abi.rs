//! A32 bodies for the owned exception decisions; `carrier.S` holds the entries.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod exception;

use core::arch::asm;
use exception::*;

#[cfg(all(feature = "domain-a", feature = "domain-b"))]
compile_error!("feature=\"domain-a\" and feature=\"domain-b\" are exclusive");
#[cfg(not(any(feature = "domain-a", feature = "domain-b")))]
compile_error!("select one of feature=\"domain-a\" or feature=\"domain-b\"");

#[cfg(feature = "domain-a")]
const DOMAIN: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const DOMAIN: Domain = Domain::B;

const DOMAIN_DATA: ExceptionDomain = ExceptionDomain::of(DOMAIN);

struct Cpu;

impl Machine for Cpu {
    #[inline(always)]
    fn load_word(&self, at: GuestAddress) -> u32 {
        unsafe { core::ptr::read_volatile(at.0 as *const u32) }
    }

    #[inline(always)]
    fn store_word(&self, at: GuestAddress, value: u32) {
        unsafe { core::ptr::write_volatile(at.0 as *mut u32, value) }
    }

    #[inline(always)]
    fn try_acquire(&self, word: GuestAddress, stamp: IrqTag) -> bool {
        unsafe { xm1_exception_stock_try(word.0, stamp.word()) == 0 }
    }

    #[inline(always)]
    fn release(&self, word: GuestAddress) {
        unsafe { xm1_exception_stock_release(word.0) };
    }

    #[inline(always)]
    fn primary_identity(&self) -> bool {
        unsafe { xm1_exception_stock_identity() == 0 }
    }

    #[inline(always)]
    fn set_tpidrurw_bit31(&self) {
        unsafe {
            let mut value: u32;
            asm!("mrc p15, 0, {0}, c13, c0, 2", out(reg) value, options(nostack));
            value |= 0x8000_0000;
            asm!("mcr p15, 0, {0}, c13, c0, 2", in(reg) value, options(nostack));
        }
    }

    #[inline(always)]
    fn clear_tpidrurw_bit31(&self) {
        unsafe {
            let mut value: u32;
            asm!("mrc p15, 0, {0}, c13, c0, 2", out(reg) value, options(nostack));
            value &= !0x8000_0000;
            asm!("mcr p15, 0, {0}, c13, c0, 2", in(reg) value, options(nostack));
        }
    }

    #[inline(always)]
    fn log(&self, code: u32, value: u32) {
        unsafe { xm1_exception_stock_log(code, value) };
    }

    #[inline(always)]
    fn call(&self, address: u32, first: u32, second: u32) {
        unsafe {
            let hook: extern "C" fn(u32, u32) = core::mem::transmute(address as usize);
            hook(first, second);
        }
    }
}

extern "C" {
    fn xm1_exception_stock_try(word: u32, stamp: u32) -> i32;
    fn xm1_exception_stock_release(word: u32) -> i32;
    fn xm1_exception_stock_identity() -> u32;
    fn xm1_exception_stock_log(code: u32, value: u32);
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.exception_ack"]
pub extern "C" fn xm1_exception_ack() -> u32 {
    acknowledge(&Cpu, &DOMAIN_DATA).0
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.exception_lock_acquire"]
pub extern "C" fn xm1_exception_lock_acquire() {
    lock_acquire(&Cpu, &DOMAIN_DATA)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.exception_eoi"]
pub extern "C" fn xm1_exception_eoi() {
    end_of_interrupt(&Cpu, &DOMAIN_DATA)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.exception_exit_tail"]
pub extern "C" fn xm1_exception_exit_tail() -> u32 {
    exit(&Cpu, &DOMAIN_DATA) as u32
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.exception_fault_record"]
pub extern "C" fn xm1_exception_fault_record(kind: u32, frame: u32) -> u32 {
    record_fault_raw(&Cpu, &DOMAIN_DATA, kind, FaultFrame(GuestAddress(frame)))
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
