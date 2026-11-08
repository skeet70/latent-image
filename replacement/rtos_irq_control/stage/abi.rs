//! A32 entry points for the three IRQ root slots.

#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod irq;

use core::arch::{asm, global_asm};
use irq::layout::GuestAddress;
use irq::{define, enable_raw, disable_raw, Backend, Bank, DefineOutcome, Handler, SavedInterrupts, Table};

#[cfg(feature = "domain-a")]
const TABLE: Table = Table::new(GuestAddress(0x4005_38fc), Handler::from_static(0x0004_2840));
#[cfg(feature = "domain-b")]
const TABLE: Table = Table::new(GuestAddress(0x400e_38bc), Handler::from_static(0x000d_2840));

struct Cpu;

#[inline(always)]
fn read(address: GuestAddress) -> u32 {
    unsafe { core::ptr::read_volatile(address.0 as *const u32) }
}

#[inline(always)]
fn write(address: GuestAddress, value: u32) {
    unsafe { core::ptr::write_volatile(address.0 as *mut u32, value) }
}

impl Backend for Cpu {
    #[inline(always)]
    fn load_word(&self, at: GuestAddress) -> u32 {
        read(at)
    }

    #[inline(always)]
    fn store_word(&self, at: GuestAddress, value: u32) {
        write(at, value);
    }

    #[inline(always)]
    fn mask_irq(&self) -> SavedInterrupts {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let saved = cpsr & 0x80;
        unsafe { asm!("msr cpsr_c, {}", in(reg) cpsr | 0x80, options(nostack, preserves_flags)) };
        SavedInterrupts(saved)
    }

    #[inline(always)]
    fn restore_irq(&self, saved: SavedInterrupts) {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let restored = (saved.0 & 0x80) | (cpsr & !0x80);
        unsafe { asm!("msr cpsr_c, {}", in(reg) restored, options(nostack, preserves_flags)) };
    }

    #[inline(always)]
    fn mask_irq_fiq(&self) -> SavedInterrupts {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let saved = cpsr & 0xc0;
        unsafe { asm!("msr cpsr_c, {}", in(reg) cpsr | 0xc0, options(nostack, preserves_flags)) };
        SavedInterrupts(saved)
    }

    #[inline(always)]
    fn restore_irq_fiq(&self, saved: SavedInterrupts) {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let restored = (saved.0 & 0xc0) | (cpsr & !0xc0);
        unsafe { asm!("msr cpsr_c, {}", in(reg) restored, options(nostack, preserves_flags)) };
    }

    #[inline(always)]
    fn bank(&self) -> Bank {
        let identity: u32;
        unsafe {
            asm!("mrc p14, 0, {}, c2, c0, 0", out(reg) identity, options(nomem, nostack, preserves_flags))
        };
        if identity == 0x4003 {
            Bank::Primary
        } else {
            Bank::Secondary
        }
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.irq_body_enable"]
pub extern "C" fn xm1_rtos_irq_enable_body(line: u32) -> i32 {
    match enable_raw(&Cpu, TABLE, line as i32) {
        Ok(()) => 0,
        Err(_) => -5,
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.irq_body_disable"]
pub extern "C" fn xm1_rtos_irq_disable_body(line: u32) -> i32 {
    match disable_raw(&Cpu, TABLE, line as i32) {
        Ok(()) => 0,
        Err(_) => -5,
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.irq_body_define"]
pub extern "C" fn xm1_rtos_irq_define_body(line: u32, packet: u32) -> i32 {
    let packet = if packet == 0 {
        None
    } else {
        Some(GuestAddress(packet))
    };
    match define(&Cpu, TABLE, line, packet) {
        DefineOutcome::Ok => 0,
        DefineOutcome::Refused => -17,
    }
}

global_asm!(
    ".pushsection .text.irq_enable, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_irq_enable",
    ".type xm1_rtos_irq_enable, %function",
    "xm1_rtos_irq_enable:",
    "b xm1_rtos_irq_enable_body",
    ".size xm1_rtos_irq_enable, . - xm1_rtos_irq_enable",
    ".popsection",
    ".pushsection .text.irq_disable, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_irq_disable",
    ".type xm1_rtos_irq_disable, %function",
    "xm1_rtos_irq_disable:",
    "b xm1_rtos_irq_disable_body",
    ".size xm1_rtos_irq_disable, . - xm1_rtos_irq_disable",
    ".popsection",
    ".pushsection .text.irq_define, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_irq_define",
    ".type xm1_rtos_irq_define, %function",
    "xm1_rtos_irq_define:",
    "b xm1_rtos_irq_define_body",
    ".size xm1_rtos_irq_define, . - xm1_rtos_irq_define",
    ".popsection",
);

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
