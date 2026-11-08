//! A32 adapter for the six S3d lifecycle entries over the shared Rust core.
#![cfg_attr(not(test), no_std)]

#[path = "../src/s3d.rs"]
mod s3d;

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

use s3d::layout::{Domain, GuestAddress, KernelContext, LockRecord, Task};
use s3d::{
    change_priority, delay, interrupt_release_wait, release_wait, terminate, timed_sleep,
    timer_heap_active, EncodedId, InterruptState, Machine, PriorityArgument, ServiceError,
    SwitchReason, TimerEntry, Ticks,
};

#[cfg(feature = "domain-a")]
const EXECUTING: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const EXECUTING: Domain = Domain::B;

#[cfg(all(feature = "domain-a", feature = "domain-b"))]
compile_error!("feature=\"domain-a\" and feature=\"domain-b\" are exclusive");
#[cfg(all(not(feature = "domain-a"), not(feature = "domain-b")))]
compile_error!("select one of feature=\"domain-a\" or feature=\"domain-b\"");

extern "C" {
    fn s3d_remove_ready(context: u32, task: u32) -> u32;
    fn s3d_insert_ready(context: u32, task: u32) -> u32;
    fn s3d_reset_task(task: u32);
    fn s3d_restart_task(context: u32, task: u32) -> u32;
    fn s3d_notify_dispatch(context: u32) -> u32;
    fn s3d_switch(reason: i32);
    fn s3d_timer_insert(context: u32, entry: u32, expiry: u32);
    fn s3d_timer_remove(context: u32, entry: u32);
}

struct Cpu;

#[inline(always)]
fn load(at: GuestAddress) -> u32 {
    unsafe { read_volatile(at.0 as *const u32) }
}

#[inline(always)]
fn store(at: GuestAddress, value: u32) {
    unsafe { write_volatile(at.0 as *mut u32, value) }
}

#[inline(always)]
fn load_byte(at: GuestAddress) -> u8 {
    unsafe { read_volatile(at.0 as *const u8) }
}

#[inline(always)]
fn store_byte(at: GuestAddress, value: u8) {
    unsafe { write_volatile(at.0 as *mut u8, value) }
}

#[inline(always)]
fn cpsr() -> u32 {
    let value;
    unsafe { asm!("mrs {}, cpsr", out(reg) value, options(nostack, preserves_flags)) };
    value
}

impl Machine for Cpu {
    #[inline(always)]
    fn load_word(&self, at: GuestAddress) -> u32 {
        load(at)
    }

    #[inline(always)]
    fn store_word(&self, at: GuestAddress, value: u32) {
        store(at, value)
    }

    #[inline(always)]
    fn load_byte(&self, at: GuestAddress) -> u8 {
        load_byte(at)
    }

    #[inline(always)]
    fn store_byte(&self, at: GuestAddress, value: u8) {
        store_byte(at, value)
    }

    #[inline(always)]
    fn save_and_mask(&self) -> InterruptState {
        let state = InterruptState(cpsr() & InterruptState::IRQ_MASKED);
        unsafe { asm!("cpsid i", options(nostack, preserves_flags)) };
        state
    }

    #[inline(always)]
    fn restore(&self, state: InterruptState) {
        if state.masked() {
            unsafe { asm!("cpsid i", options(nostack, preserves_flags)) };
        } else {
            unsafe { asm!("cpsie i", options(nostack, preserves_flags)) };
        }
    }

    #[inline(always)]
    fn masked(&self) -> bool {
        cpsr() & InterruptState::IRQ_MASKED != 0
    }

    #[inline(always)]
    fn lock_acquire(&self, lock: LockRecord) -> bool {
        let acquire: unsafe extern "C" fn(u32) -> u32 =
            unsafe { core::mem::transmute(load(lock.acquire())) };
        unsafe { acquire(load(lock.word())) == 0 }
    }

    #[inline(always)]
    fn lock_release(&self, lock: LockRecord) {
        let release: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(load(lock.release())) };
        unsafe { release(load(lock.word())) };
    }

    #[inline(always)]
    fn remove_ready(&self, context: KernelContext, task: Task) -> bool {
        unsafe { s3d_remove_ready(context.0 .0, task.0 .0) != 0 }
    }

    #[inline(always)]
    fn insert_ready(&self, context: KernelContext, task: Task) -> bool {
        unsafe { s3d_insert_ready(context.0 .0, task.0 .0) != 0 }
    }

    #[inline(always)]
    fn reset_task(&self, task: Task) {
        unsafe { s3d_reset_task(task.0 .0) };
    }

    #[inline(always)]
    fn restart_task(&self, context: KernelContext, task: Task) -> bool {
        unsafe { s3d_restart_task(context.0 .0, task.0 .0) != 0 }
    }

    #[inline(always)]
    fn notify_dispatch(&self, context: KernelContext) -> bool {
        unsafe { s3d_notify_dispatch(context.0 .0) != 0 }
    }

    #[inline(always)]
    fn switch(&self, reason: SwitchReason) {
        unsafe { s3d_switch(reason.code()) };
    }

    #[inline(always)]
    fn timer_insert(&self, context: KernelContext, entry: TimerEntry, expiry: u32) {
        unsafe { s3d_timer_insert(context.0 .0, entry.0 .0, expiry) };
    }

    #[inline(always)]
    fn timer_remove(&self, context: KernelContext, entry: TimerEntry) {
        unsafe { s3d_timer_remove(context.0 .0, entry.0 .0) };
    }

    #[inline(always)]
    fn timer_active(&self, context: KernelContext, entry: TimerEntry) -> bool {
        s3d_timer_active(context.0 .0, entry.0 .0)
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_timer_active"]
pub extern "C" fn s3d_timer_active(context: u32, entry: u32) -> bool {
    timer_heap_active(
        &Cpu,
        KernelContext(GuestAddress(context)),
        TimerEntry(GuestAddress(entry)),
    )
}

#[inline(always)]
fn status(result: Result<(), ServiceError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => error.status(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_terminate_body"]
pub extern "C" fn s3d_terminate_body(id: i32) -> i32 {
    status(terminate(&Cpu, EXECUTING, EncodedId(id)))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_change_priority_body"]
pub extern "C" fn s3d_change_priority_body(id: i32, priority: u32) -> i32 {
    status(change_priority(
        &Cpu,
        EXECUTING,
        EncodedId(id),
        PriorityArgument(priority),
    ))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_timed_sleep_body"]
pub extern "C" fn s3d_timed_sleep_body(timeout: i32) -> i32 {
    timed_sleep(&Cpu, EXECUTING, Ticks(timeout))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_release_wait_body"]
pub extern "C" fn s3d_release_wait_body(id: i32) -> i32 {
    status(release_wait(&Cpu, EXECUTING, EncodedId(id)))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_interrupt_release_wait_body"]
pub extern "C" fn s3d_interrupt_release_wait_body(id: i32) -> i32 {
    status(interrupt_release_wait(&Cpu, EXECUTING, EncodedId(id)))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_delay_body"]
pub extern "C" fn s3d_delay_body(delay_ticks: i32) -> i32 {
    delay(&Cpu, EXECUTING, Ticks(delay_ticks))
}

core::arch::global_asm!(
    ".section .text.s3d_terminate_entry,\"ax\",%progbits",
    ".global s3d_terminate",
    ".type s3d_terminate, %function",
    "s3d_terminate: b s3d_terminate_body",
    ".section .text.s3d_change_priority_entry,\"ax\",%progbits",
    ".global s3d_change_priority",
    ".type s3d_change_priority, %function",
    "s3d_change_priority: b s3d_change_priority_body",
    ".section .text.s3d_timed_sleep_entry,\"ax\",%progbits",
    ".global s3d_timed_sleep",
    ".type s3d_timed_sleep, %function",
    "s3d_timed_sleep: b s3d_timed_sleep_body",
    ".section .text.s3d_release_wait_entry,\"ax\",%progbits",
    ".global s3d_release_wait",
    ".type s3d_release_wait, %function",
    "s3d_release_wait: b s3d_release_wait_body",
    ".section .text.s3d_interrupt_release_wait_entry,\"ax\",%progbits",
    ".global s3d_interrupt_release_wait",
    ".type s3d_interrupt_release_wait, %function",
    "s3d_interrupt_release_wait: b s3d_interrupt_release_wait_body",
    ".section .text.s3d_delay_entry,\"ax\",%progbits",
    ".global s3d_delay",
    ".type s3d_delay, %function",
    "s3d_delay: b s3d_delay_body",
);

#[cfg(all(target_arch = "arm", not(test), not(feature = "component")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
