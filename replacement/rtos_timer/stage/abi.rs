//! TIMER A32 stage adapter: the stock entries over the owned heap core.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod timer;

use core::arch::{asm, global_asm};
use timer::layout::{Domain, GuestAddress, KernelContext};
use timer::{
    insert, remove, tick, Capacity, Expiry, Machine, Scheduler, TimerEntry, TimerError, TimerHeap,
};

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

/// The heap span at `g_timer_heap` is 0x5c0 bytes, 184 eight-byte nodes.
const FIRMWARE_CAPACITY: u32 = 184;

#[inline(always)]
fn read(address: GuestAddress) -> u32 {
    unsafe { core::ptr::read_volatile(address.0 as *const u32) }
}

#[inline(always)]
fn write(address: GuestAddress, value: u32) {
    unsafe { core::ptr::write_volatile(address.0 as *mut u32, value) }
}

struct Cpu;

impl Machine for Cpu {
    fn load_word(&self, at: GuestAddress) -> u32 {
        read(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        write(at, value)
    }
}

#[inline(always)]
fn current_context() -> KernelContext {
    KernelContext(GuestAddress(read(ENTRY.words().current_context)))
}

#[inline(always)]
fn irq_masked() -> bool {
    let cpsr: u32;
    unsafe { asm!("mrs {cpsr}, cpsr", cpsr = out(reg) cpsr) };
    cpsr & 0x80 != 0
}

#[inline(always)]
fn irq_set() {
    unsafe {
        asm!(
            "mrs r3, cpsr",
            "orr r3, r3, #0x80",
            "msr cpsr_fc, r3",
            out("r3") _,
        )
    };
}

#[inline(always)]
fn irq_clear() {
    unsafe {
        asm!(
            "mrs r3, cpsr",
            "bic r3, r3, #0x80",
            "msr cpsr_fc, r3",
            out("r3") _,
        )
    };
}

#[inline(always)]
fn lock_entry(at: GuestAddress) -> extern "C" fn(u32) -> i32 {
    unsafe { core::mem::transmute::<u32, extern "C" fn(u32) -> i32>(read(at)) }
}

#[inline(always)]
fn acquire_lock() {
    let context = current_context();
    let lock = context.scheduler_lock();
    let acquire = lock_entry(lock.acquire());
    loop {
        irq_set();
        if acquire(lock.0 .0) == 0 {
            break;
        }
    }
}

#[inline(always)]
fn release_lock() {
    let context = current_context();
    let lock = context.scheduler_lock();
    let release = lock_entry(lock.release());
    let _ = release(lock.0 .0);
}

impl Scheduler for Cpu {
    fn handler_context(&self) -> u32 {
        read(ENTRY.words().handler_context)
    }

    fn irq_masked(&self) -> bool {
        irq_masked()
    }

    fn release_lock(&self) {
        release_lock()
    }

    fn acquire_lock(&self) {
        acquire_lock()
    }

    fn irq_clear(&self) {
        irq_clear()
    }

    fn irq_set(&self) {
        irq_set()
    }
}

#[inline(always)]
fn heap(context: KernelContext) -> TimerHeap {
    TimerHeap::from_context(
        &Cpu,
        context,
        Capacity::from_static(FIRMWARE_CAPACITY),
    )
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.timer_body_insert"]
pub extern "C" fn xm1_rtos_timer_insert_body(context: KernelContext, entry: TimerEntry, expiry: u32) {
    let _ = insert(&Cpu, heap(context), entry, Expiry(expiry));
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.timer_body_remove"]
pub extern "C" fn xm1_rtos_timer_remove_body(context: KernelContext, entry: TimerEntry) {
    let _ = remove(&Cpu, heap(context), entry);
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.timer_body_tick"]
pub extern "C" fn xm1_rtos_timer_tick_body() -> i32 {
    let context = current_context();
    match tick(&Cpu, heap(context), |_m, callback, argument| unsafe {
        let invoke: extern "C" fn(u32) = core::mem::transmute::<u32, extern "C" fn(u32)>(callback.0);
        invoke(argument.0);
    }) {
        Ok(()) => 0,
        Err(TimerError::Context) | Err(TimerError::InterruptMasked) => -25,
        Err(_) => -25,
    }
}

global_asm!(
    ".pushsection .text.timer_insert, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_timer_insert",
    ".type xm1_rtos_timer_insert, %function",
    "xm1_rtos_timer_insert:",
    "b xm1_rtos_timer_insert_body",
    ".size xm1_rtos_timer_insert, . - xm1_rtos_timer_insert",
    ".popsection",
    ".pushsection .text.timer_remove, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_timer_remove",
    ".type xm1_rtos_timer_remove, %function",
    "xm1_rtos_timer_remove:",
    "b xm1_rtos_timer_remove_body",
    ".size xm1_rtos_timer_remove, . - xm1_rtos_timer_remove",
    ".popsection",
    ".pushsection .text.timer_tick, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_timer_tick",
    ".type xm1_rtos_timer_tick, %function",
    "xm1_rtos_timer_tick:",
    "b xm1_rtos_timer_tick_body",
    ".size xm1_rtos_timer_tick, . - xm1_rtos_timer_tick",
    ".popsection",
);

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
