use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use super::s3d::layout::{Domain, GuestAddress, KernelContext, LockRecord, Task};
use super::s3d::{InterruptState, Machine, SwitchReason, TimerEntry};
extern "C" {
    fn s3d_remove_ready(context: u32, task: u32) -> u32;
    fn s3d_insert_ready(context: u32, task: u32) -> u32;
    fn s3d_reset_task(task: u32);
    fn s3d_restart_task(context: u32, task: u32) -> u32;
    fn s3d_notify_dispatch(context: u32) -> u32;
    fn s3d_switch(reason: i32);
    fn s3d_timer_insert(context: u32, entry: u32, expiry: u32);
    fn s3d_timer_remove(context: u32, entry: u32);
    fn s3d_timer_active(context: u32, entry: u32) -> bool;
}

pub struct Cpu {
    domain: Domain,
}

impl Cpu {
    /// # Safety
    /// The executing core uses `domain`'s published root and owned scheduler, timer, and context-switch bindings.
    pub const unsafe fn claim(domain: Domain) -> Self { Self { domain } }

    pub const fn domain(&self) -> Domain { self.domain }
}

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
        let state = InterruptState(cpsr() & (InterruptState::IRQ_MASKED | InterruptState::FIQ_MASKED));
        unsafe { asm!("cpsid if", options(nostack, preserves_flags)) };
        state
    }

    #[inline(always)]
    fn restore(&self, state: InterruptState) {
        let bits = state.0;
        let mut current: u32;
        unsafe {
            asm!("mrs {}, cpsr", out(reg) current, options(nostack));
            asm!("bic {0}, {0}, #0xc0", inout(reg) current, options(nostack));
            asm!("orr {0}, {0}, {1}", inout(reg) current, in(reg) bits, options(nostack));
            asm!("msr cpsr_fc, {0}", in(reg) current, options(nostack));
        }
    }

    #[inline(always)]
    fn masked(&self) -> bool {
        cpsr() & InterruptState::IRQ_MASKED != 0
    }

    #[inline(always)]
    fn irq_enable(&self) {
        unsafe {
            asm!(
                "mrs r3, cpsr",
                "bic r3, r3, #0x80",
                "msr cpsr_fc, r3",
                out("r3") _,
                options(nostack, preserves_flags),
            )
        };
    }

    #[inline(always)]
    fn irq_disable(&self) {
        unsafe {
            asm!(
                "mrs r3, cpsr",
                "orr r3, r3, #0x80",
                "msr cpsr_fc, r3",
                out("r3") _,
                options(nostack, preserves_flags),
            )
        };
    }

    #[inline(always)]
    fn lock_acquire(&self, lock: LockRecord) -> bool {
        let acquire: unsafe extern "C" fn(u32) -> u32 =
            unsafe { core::mem::transmute(load(lock.acquire())) };
        unsafe { acquire(lock.0 .0) == 0 }
    }

    #[inline(always)]
    fn lock_release(&self, lock: LockRecord) {
        let release: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(load(lock.release())) };
        unsafe { release(lock.0 .0) };
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
        unsafe { s3d_notify_dispatch(load(context.id())) != 0 }
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
        unsafe { s3d_timer_active(context.0 .0, entry.0 .0) }
    }
}
