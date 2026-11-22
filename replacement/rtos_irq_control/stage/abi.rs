//! A32 entry points for the three IRQ root slots.
//!
//! `bank` builds every retained IRQ, CPSR mask and cache entry as a body exported as
//! `xm1_rtos_<family>_<entry>_<domain>`, with no stock-address veneer. `probe` adds a
//! `b` veneer at each stock entry to a bank build for a standalone link.

#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod irq;

use core::arch::asm;
#[cfg(not(feature = "bank"))]
use core::arch::global_asm;
use irq::layout::GuestAddress;
use irq::{define, enable_raw, disable_raw, Backend, Bank, DefineOutcome, Handler, SavedInterrupts, Table};

#[cfg(feature = "domain-a")]
const TABLE: Table = Table::new(GuestAddress(0x4005_38fc), Handler::from_static(0x0004_2840));
#[cfg(feature = "domain-b")]
const TABLE: Table = Table::new(GuestAddress(0x400e_38bc), Handler::from_static(0x000d_2840));

#[cfg(all(feature = "bank", feature = "domain-a"))]
macro_rules! bank_name {
    ($entry:literal) => {
        concat!("xm1_rtos_", $entry, "_a")
    };
}
#[cfg(all(feature = "bank", feature = "domain-b"))]
macro_rules! bank_name {
    ($entry:literal) => {
        concat!("xm1_rtos_", $entry, "_b")
    };
}

struct Cpu;

#[inline(always)]
fn read(address: GuestAddress) -> u32 {
    unsafe { core::ptr::read_volatile(address.0 as *const u32) }
}

#[inline(always)]
fn write(address: GuestAddress, value: u32) {
    unsafe { core::ptr::write_volatile(address.0 as *mut u32, value) }
}

#[inline(always)]
fn identity() -> u32 {
    let identity: u32;
    unsafe { asm!("mrc p14, 0, {}, c2, c0, 0", out(reg) identity, options(nomem, nostack, preserves_flags)) };
    identity
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

    #[cfg(not(feature = "bank"))]
    #[inline(always)]
    fn mask_irq(&self) -> SavedInterrupts {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let saved = cpsr & 0x80;
        unsafe { asm!("msr cpsr_c, {}", in(reg) cpsr | 0x80, options(nostack, preserves_flags)) };
        SavedInterrupts(saved)
    }

    #[cfg(not(feature = "bank"))]
    #[inline(always)]
    fn restore_irq(&self, saved: SavedInterrupts) {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let restored = (saved.0 & 0x80) | (cpsr & !0x80);
        unsafe { asm!("msr cpsr_c, {}", in(reg) restored, options(nostack, preserves_flags)) };
    }

    #[cfg(not(feature = "bank"))]
    #[inline(always)]
    fn mask_irq_fiq(&self) -> SavedInterrupts {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let saved = cpsr & 0xc0;
        unsafe { asm!("msr cpsr_c, {}", in(reg) cpsr | 0xc0, options(nostack, preserves_flags)) };
        SavedInterrupts(saved)
    }

    #[cfg(not(feature = "bank"))]
    #[inline(always)]
    fn restore_irq_fiq(&self, saved: SavedInterrupts) {
        let cpsr: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
        let restored = (saved.0 & 0xc0) | (cpsr & !0xc0);
        unsafe { asm!("msr cpsr_c, {}", in(reg) restored, options(nostack, preserves_flags)) };
    }

    // The fixed-bank stack admission refuses a CPSR write that is not a restored snapshot.
    #[cfg(feature = "bank")]
    #[inline(always)]
    fn mask_irq(&self) -> SavedInterrupts {
        mask::save(self, MaskBits::Irq)
    }

    #[cfg(feature = "bank")]
    #[inline(always)]
    fn restore_irq(&self, saved: SavedInterrupts) {
        mask::restore(self, MaskBits::Irq, saved);
    }

    #[cfg(feature = "bank")]
    #[inline(always)]
    fn mask_irq_fiq(&self) -> SavedInterrupts {
        mask::save(self, MaskBits::IrqFiq)
    }

    #[cfg(feature = "bank")]
    #[inline(always)]
    fn restore_irq_fiq(&self, saved: SavedInterrupts) {
        mask::restore(self, MaskBits::IrqFiq, saved);
    }

    #[inline(always)]
    fn bank(&self) -> Bank {
        if identity() == 0x4003 {
            Bank::Primary
        } else {
            Bank::Secondary
        }
    }
}

#[cfg_attr(not(feature = "bank"), no_mangle)]
#[cfg_attr(feature = "bank", export_name = bank_name!("irq_slot_enable"))]
#[inline(never)]
#[link_section = ".text.irq_body_enable"]
pub extern "C" fn xm1_rtos_irq_enable_body(line: u32) -> i32 {
    match enable_raw(&Cpu, TABLE, line as i32) {
        Ok(()) => 0,
        Err(_) => -5,
    }
}

#[cfg_attr(not(feature = "bank"), no_mangle)]
#[cfg_attr(feature = "bank", export_name = bank_name!("irq_slot_disable"))]
#[inline(never)]
#[link_section = ".text.irq_body_disable"]
pub extern "C" fn xm1_rtos_irq_disable_body(line: u32) -> i32 {
    match disable_raw(&Cpu, TABLE, line as i32) {
        Ok(()) => 0,
        Err(_) => -5,
    }
}

#[cfg_attr(not(feature = "bank"), no_mangle)]
#[cfg_attr(feature = "bank", export_name = bank_name!("irq_define"))]
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

#[cfg(not(feature = "bank"))]
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

#[cfg(feature = "bank")]
use bank::{mask, MaskBits};

#[cfg(feature = "bank")]
#[path = "../../rtos_exception/src/lib.rs"]
mod exception;

#[cfg(feature = "bank")]
mod bank {
    use super::{exception, identity, irq, Cpu, TABLE};
    use core::arch::asm;
    use core::cell::Cell;
    use irq::cache::{self, Cache, Cp15, Maintenance, RangeOp};
    use irq::controller::{self, Controller, Core};
    use exception::{ExceptionDomain, IrqTag};
    use irq::layout::GuestAddress;
    pub use irq::mask::{self, Cpsr, MaskBits};
    use irq::{disable_raw, enable_raw};

    #[cfg(feature = "domain-a")]
    const EXCEPTION: ExceptionDomain = ExceptionDomain::of(exception::Domain::A);
    #[cfg(feature = "domain-b")]
    const EXCEPTION: ExceptionDomain = ExceptionDomain::of(exception::Domain::B);

    #[cfg(feature = "domain-a")]
    const STOCK: u32 = 0;
    #[cfg(feature = "domain-b")]
    const STOCK: u32 = 0x9_0000;

    /// Retained stock kernel log.
    const LOG: u32 = 0x41668 + STOCK;

    const CONTROLLER: Controller = Controller {
        table: TABLE,
        active: GuestAddress(EXCEPTION.active_irqs.0),
        unhandled: GuestAddress(EXCEPTION.unhandled.0),
    };

    const _: () = assert!(EXCEPTION.handler_table.0 == TABLE.base().0);
    const _: () = assert!(EXCEPTION.unhandled_vector.0 == TABLE.default_handler().get());

    impl Cpsr for Cpu {
        #[inline(always)]
        fn read_cpsr(&self) -> u32 {
            let cpsr: u32;
            unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nostack, preserves_flags)) };
            cpsr
        }

        #[inline(always)]
        fn mask(&self, bits: MaskBits) {
            unsafe {
                match bits {
                    MaskBits::IrqFiq => asm!("cpsid if", options(nostack, preserves_flags)),
                    MaskBits::Irq => asm!("cpsid i", options(nostack, preserves_flags)),
                    MaskBits::Fiq => asm!("cpsid f", options(nostack, preserves_flags)),
                }
            }
        }

        #[inline(always)]
        fn unmask(&self, bits: MaskBits) {
            unsafe {
                match bits {
                    MaskBits::IrqFiq => asm!("cpsie if", options(nostack, preserves_flags)),
                    MaskBits::Irq => asm!("cpsie i", options(nostack, preserves_flags)),
                    MaskBits::Fiq => asm!("cpsie f", options(nostack, preserves_flags)),
                }
            }
        }
    }

    impl Cp15 for Cpu {
        #[inline(always)]
        fn read_sctlr(&self) -> u32 {
            let value: u32;
            unsafe { asm!("mrc p15, 0, {}, c1, c0, 0", out(reg) value, options(nostack, preserves_flags)) };
            value
        }

        #[inline(always)]
        fn write_sctlr(&self, value: u32) {
            unsafe { asm!("mcr p15, 0, {}, c1, c0, 0", in(reg) value, options(nostack, preserves_flags)) };
        }

        #[inline(always)]
        fn read_clidr(&self) -> u32 {
            let value: u32;
            unsafe { asm!("mrc p15, 1, {}, c0, c0, 1", out(reg) value, options(nostack, preserves_flags)) };
            value
        }

        #[inline(always)]
        fn write_csselr(&self, value: u32) {
            unsafe { asm!("mcr p15, 2, {}, c0, c0, 0", in(reg) value, options(nostack, preserves_flags)) };
        }

        #[inline(always)]
        fn read_ccsidr(&self) -> u32 {
            let value: u32;
            unsafe { asm!("mrc p15, 1, {}, c0, c0, 0", out(reg) value, options(nostack, preserves_flags)) };
            value
        }

        #[inline(always)]
        fn maintain(&self, op: Maintenance, operand: u32) {
            unsafe {
                match op {
                    Maintenance::InvalidateInstructionAll => {
                        asm!("mcr p15, 0, {}, c7, c5, 0", in(reg) operand, options(nostack, preserves_flags))
                    }
                    Maintenance::InvalidateInstructionLine => {
                        asm!("mcr p15, 0, {}, c7, c5, 1", in(reg) operand, options(nostack, preserves_flags))
                    }
                    Maintenance::InvalidateDataLine => {
                        asm!("mcr p15, 0, {}, c7, c6, 1", in(reg) operand, options(nostack, preserves_flags))
                    }
                    Maintenance::CleanDataLine => {
                        asm!("mcr p15, 0, {}, c7, c10, 1", in(reg) operand, options(nostack, preserves_flags))
                    }
                    Maintenance::CleanDataSetWay => {
                        asm!("mcr p15, 0, {}, c7, c10, 2", in(reg) operand, options(nostack, preserves_flags))
                    }
                    Maintenance::CleanInvalidateDataLine => {
                        asm!("mcr p15, 0, {}, c7, c14, 1", in(reg) operand, options(nostack, preserves_flags))
                    }
                    Maintenance::CleanInvalidateDataSetWay => {
                        asm!("mcr p15, 0, {}, c7, c14, 2", in(reg) operand, options(nostack, preserves_flags))
                    }
                    Maintenance::InvalidateDataAll => {
                        asm!("mcr p15, 0, {}, c15, c5, 0", in(reg) operand, options(nostack, preserves_flags))
                    }
                }
            }
        }

        #[inline(always)]
        fn dsb(&self) {
            unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
        }

        #[inline(always)]
        fn isb(&self) {
            unsafe { asm!("isb sy", options(nostack, preserves_flags)) };
        }
    }

    /// Records the identity its exception decision read, for leave's bank result.
    struct ExceptionCpu {
        primary: Cell<bool>,
    }

    impl exception::Machine for ExceptionCpu {
        #[inline(always)]
        fn load_word(&self, at: exception::GuestAddress) -> u32 {
            unsafe { core::ptr::read_volatile(at.0 as *const u32) }
        }

        #[inline(always)]
        fn store_word(&self, at: exception::GuestAddress, value: u32) {
            unsafe { core::ptr::write_volatile(at.0 as *mut u32, value) }
        }

        #[inline(always)]
        fn try_acquire(&self, word: exception::GuestAddress, stamp: IrqTag) -> bool {
            // Retained stock global lock.
            let acquire = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, u32) -> i32>((0x427d4 + STOCK) as usize) };
            acquire(word.0, stamp.word()) == 0
        }

        #[inline(always)]
        fn release(&self, word: exception::GuestAddress) {
            let release = unsafe { core::mem::transmute::<usize, extern "C" fn(u32) -> i32>((0x427fc + STOCK) as usize) };
            release(word.0);
        }

        #[inline(always)]
        fn primary_identity(&self) -> bool {
            let primary = identity() == 0x4003;
            self.primary.set(primary);
            primary
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
            let log = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, u32)>(LOG as usize) };
            log(code, value);
        }

        #[inline(always)]
        fn call(&self, address: u32, first: u32, second: u32) {
            let hook = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, u32)>(address as usize) };
            hook(first, second);
        }
    }

    #[export_name = bank_name!("mask_save_irq_fiq")]
    #[inline(never)]
    #[link_section = ".text.irq_body_mask_save_irq_fiq"]
    pub extern "C" fn mask_save_irq_fiq() -> u32 {
        mask::save(&Cpu, MaskBits::IrqFiq).0
    }

    #[export_name = bank_name!("mask_restore_irq_fiq")]
    #[inline(never)]
    #[link_section = ".text.irq_body_mask_restore_irq_fiq"]
    pub extern "C" fn mask_restore_irq_fiq(saved: u32) -> u32 {
        mask::restore(&Cpu, MaskBits::IrqFiq, irq::SavedInterrupts(saved))
    }

    #[export_name = bank_name!("mask_save_irq")]
    #[inline(never)]
    #[link_section = ".text.irq_body_mask_save_irq"]
    pub extern "C" fn mask_save_irq() -> u32 {
        mask::save(&Cpu, MaskBits::Irq).0
    }

    #[export_name = bank_name!("mask_restore_irq")]
    #[inline(never)]
    #[link_section = ".text.irq_body_mask_restore_irq"]
    pub extern "C" fn mask_restore_irq(saved: u32) -> u32 {
        mask::restore(&Cpu, MaskBits::Irq, irq::SavedInterrupts(saved))
    }

    #[export_name = bank_name!("mask_save_fiq")]
    #[inline(never)]
    #[link_section = ".text.irq_body_mask_save_fiq"]
    pub extern "C" fn mask_save_fiq() -> u32 {
        mask::save(&Cpu, MaskBits::Fiq).0
    }

    #[export_name = bank_name!("mask_restore_fiq")]
    #[inline(never)]
    #[link_section = ".text.irq_body_mask_restore_fiq"]
    pub extern "C" fn mask_restore_fiq(saved: u32) -> u32 {
        mask::restore(&Cpu, MaskBits::Fiq, irq::SavedInterrupts(saved))
    }

    #[export_name = bank_name!("cache_enable_instruction")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_enable_instruction"]
    pub extern "C" fn cache_enable_instruction() -> u32 {
        cache::enable(&Cpu, Cache::Instruction)
    }

    #[export_name = bank_name!("cache_enable_data")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_enable_data"]
    pub extern "C" fn cache_enable_data() -> u32 {
        cache::enable(&Cpu, Cache::Data)
    }

    #[export_name = bank_name!("cache_disable_instruction")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_disable_instruction"]
    pub extern "C" fn cache_disable_instruction() -> u32 {
        cache::disable(&Cpu, Cache::Instruction)
    }

    #[export_name = bank_name!("cache_disable_data")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_disable_data"]
    pub extern "C" fn cache_disable_data() -> u32 {
        cache::disable(&Cpu, Cache::Data)
    }

    #[export_name = bank_name!("cache_invalidate_instruction_range")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_invalidate_instruction_range"]
    pub extern "C" fn cache_invalidate_instruction_range(start: u32, length: u32) -> u32 {
        cache::range(&Cpu, RangeOp::InvalidateInstruction, start, length)
    }

    #[export_name = bank_name!("cache_clean_invalidate_data_range")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_clean_invalidate_data_range"]
    pub extern "C" fn cache_clean_invalidate_data_range(start: u32, length: u32) -> u32 {
        cache::range(&Cpu, RangeOp::CleanInvalidateData, start, length)
    }

    #[export_name = bank_name!("cache_clean_data_range")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_clean_data_range"]
    pub extern "C" fn cache_clean_data_range(start: u32, length: u32) -> u32 {
        cache::range(&Cpu, RangeOp::CleanData, start, length)
    }

    #[export_name = bank_name!("cache_invalidate_data_range")]
    #[inline(never)]
    #[link_section = ".text.irq_body_cache_invalidate_data_range"]
    pub extern "C" fn cache_invalidate_data_range(start: u32, length: u32) -> u32 {
        cache::range(&Cpu, RangeOp::InvalidateData, start, length)
    }

    #[export_name = bank_name!("irq_identity")]
    #[inline(never)]
    #[link_section = ".text.irq_body_identity"]
    pub extern "C" fn irq_identity() -> u32 {
        controller::core_index(&Cpu)
    }

    #[export_name = bank_name!("irq_core_index")]
    #[inline(never)]
    #[link_section = ".text.irq_body_core_index"]
    pub extern "C" fn irq_core_index() -> u32 {
        controller::core_index(&Cpu)
    }

    #[export_name = bank_name!("irq_initialize")]
    #[inline(never)]
    #[link_section = ".text.irq_body_initialize"]
    pub extern "C" fn irq_initialize() -> u32 {
        controller::initialize(&Cpu, CONTROLLER)
    }

    #[export_name = bank_name!("irq_configure")]
    #[inline(never)]
    #[link_section = ".text.irq_body_configure"]
    pub extern "C" fn irq_configure(line: i32, descriptor: u32) -> i32 {
        controller::configure(&Cpu, TABLE, line, GuestAddress(descriptor)) as i32
    }

    #[export_name = bank_name!("irq_reset")]
    #[inline(never)]
    #[link_section = ".text.irq_body_reset"]
    pub extern "C" fn irq_reset(line: i32) -> i32 {
        controller::reset(&Cpu, TABLE, line) as i32
    }

    #[export_name = bank_name!("irq_handler")]
    #[inline(never)]
    #[link_section = ".text.irq_body_handler"]
    pub extern "C" fn irq_handler(line: i32) -> u32 {
        controller::handler(&Cpu, TABLE, line)
    }

    #[export_name = bank_name!("irq_enable")]
    #[inline(never)]
    #[link_section = ".text.irq_body_controller_enable"]
    pub extern "C" fn irq_enable(line: i32) -> i32 {
        match enable_raw(&Cpu, TABLE, line) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }

    #[export_name = bank_name!("irq_disable")]
    #[inline(never)]
    #[link_section = ".text.irq_body_controller_disable"]
    pub extern "C" fn irq_disable(line: i32) -> i32 {
        match disable_raw(&Cpu, TABLE, line) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }

    #[export_name = bank_name!("irq_enter")]
    #[inline(never)]
    #[link_section = ".text.irq_body_enter"]
    pub extern "C" fn irq_enter() -> u32 {
        let cpu = ExceptionCpu { primary: Cell::new(false) };
        exception::acknowledge(&cpu, &EXCEPTION).0
    }

    /// Returns the bank base, stock's r0.
    #[export_name = bank_name!("irq_leave")]
    #[inline(never)]
    #[link_section = ".text.irq_body_leave"]
    pub extern "C" fn irq_leave() -> u32 {
        let cpu = ExceptionCpu { primary: Cell::new(false) };
        exception::end_of_interrupt(&cpu, &EXCEPTION);
        exception::Bank::of(cpu.primary.get()).address().0
    }

    #[export_name = bank_name!("irq_set_enable")]
    #[inline(never)]
    #[link_section = ".text.irq_body_set_enable"]
    pub extern "C" fn irq_set_enable(core: u32, line: i32) -> u32 {
        controller::set_enable(&Cpu, Core(core), line)
    }

    #[export_name = bank_name!("irq_clear")]
    #[inline(never)]
    #[link_section = ".text.irq_body_clear"]
    pub extern "C" fn irq_clear(core: u32, line: i32) -> u32 {
        controller::clear(&Cpu, Core(core), line)
    }

    #[export_name = bank_name!("irq_raise")]
    #[inline(never)]
    #[link_section = ".text.irq_body_raise"]
    pub extern "C" fn irq_raise(core: u32, line: i32) -> u32 {
        controller::raise(&Cpu, Core(core), line)
    }

    #[export_name = bank_name!("irq_clear_pending")]
    #[inline(never)]
    #[link_section = ".text.irq_body_clear_pending"]
    pub extern "C" fn irq_clear_pending(line: i32) -> u32 {
        controller::clear_pending(&Cpu, line)
    }

    #[export_name = bank_name!("irq_copy_active")]
    #[inline(never)]
    #[link_section = ".text.irq_body_copy_active"]
    pub extern "C" fn irq_copy_active(buffer: u32) -> u32 {
        controller::copy_active(&Cpu, CONTROLLER.active, GuestAddress(buffer))
    }
}

#[cfg(feature = "probe")]
macro_rules! probe_veneer {
    ($entry:literal, $domain:literal) => {
        concat!(
            ".pushsection .text.irq_veneer_", $entry, ", \"ax\", %progbits\n",
            ".arm\n",
            ".global xm1_rtos_", $entry, "\n",
            ".type xm1_rtos_", $entry, ", %function\n",
            "xm1_rtos_", $entry, ":\n",
            "b xm1_rtos_", $entry, "_", $domain, "\n",
            ".size xm1_rtos_", $entry, ", . - xm1_rtos_", $entry, "\n",
            ".popsection\n",
        )
    };
}

#[cfg(feature = "probe")]
macro_rules! probe_veneers {
    ($domain:literal) => {
        concat!(
            probe_veneer!("irq_slot_enable", $domain),
            probe_veneer!("irq_slot_disable", $domain),
            probe_veneer!("mask_save_irq_fiq", $domain),
            probe_veneer!("mask_restore_irq_fiq", $domain),
            probe_veneer!("mask_save_irq", $domain),
            probe_veneer!("mask_restore_irq", $domain),
            probe_veneer!("mask_save_fiq", $domain),
            probe_veneer!("mask_restore_fiq", $domain),
            probe_veneer!("cache_enable_instruction", $domain),
            probe_veneer!("cache_enable_data", $domain),
            probe_veneer!("cache_disable_instruction", $domain),
            probe_veneer!("cache_disable_data", $domain),
            probe_veneer!("cache_invalidate_instruction_range", $domain),
            probe_veneer!("cache_clean_invalidate_data_range", $domain),
            probe_veneer!("cache_clean_data_range", $domain),
            probe_veneer!("cache_invalidate_data_range", $domain),
            probe_veneer!("irq_identity", $domain),
            probe_veneer!("irq_core_index", $domain),
            probe_veneer!("irq_initialize", $domain),
            probe_veneer!("irq_configure", $domain),
            probe_veneer!("irq_reset", $domain),
            probe_veneer!("irq_handler", $domain),
            probe_veneer!("irq_enable", $domain),
            probe_veneer!("irq_disable", $domain),
            probe_veneer!("irq_enter", $domain),
            probe_veneer!("irq_leave", $domain),
            probe_veneer!("irq_set_enable", $domain),
            probe_veneer!("irq_clear", $domain),
            probe_veneer!("irq_raise", $domain),
            probe_veneer!("irq_clear_pending", $domain),
            probe_veneer!("irq_copy_active", $domain),
            probe_veneer!("irq_define", $domain),
        )
    };
}

#[cfg(all(feature = "probe", feature = "domain-a"))]
core::arch::global_asm!(probe_veneers!("a"));
#[cfg(all(feature = "probe", feature = "domain-b"))]
core::arch::global_asm!(probe_veneers!("b"));

#[cfg(all(target_arch = "arm", not(test), any(not(feature = "bank"), feature = "probe")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
