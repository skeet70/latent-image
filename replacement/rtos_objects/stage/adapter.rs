//! Family stage adapter: the generic wake bodies over the CPU and the owned S1/S2 exports.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod objects;

#[allow(unused_imports)]
pub use objects::{
    admission, block, create, id, layout, locked, matching, message, order, status, teardown,
    timeout, wake,
};

mod abi;

mod family;

use core::arch::asm;
use core::ptr::read_volatile;
use objects::create::{Machine, SavedInterrupts};
use objects::layout::{Domain, DomainId, DomainWords, GuestAddress, KernelContext, LockRecord, Task};
use objects::locked::{ObjectGuard, ObjectLock};
use objects::wake::{release, DispatchTarget, Release, SchedulerGuard, Selection, Wake};
use objects::*;

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Acquired(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Reschedule(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
struct RecordId(pub u32);

#[repr(u32)]
enum Notified {
    Remote = 0,
    Local = 1,
}

extern "C" {
    fn xm1_rtos_scheduler_acquire(lock: LockRecord) -> Acquired;
    fn xm1_rtos_scheduler_release(lock: LockRecord) -> LockRecord;
    fn xm1_rtos_scheduler_insert(record: KernelContext, task: Task) -> Reschedule;
    fn xm1_rtos_scheduler_reset(task: Task) -> Task;
    fn xm1_rtos_scheduler_prepare(record: KernelContext, task: Task) -> Reschedule;
    fn xm1_rtos_scheduler_notify(id: RecordId) -> Notified;
    fn xm1_rtos_scheduler_save(code: i32);
    /// Provided by the link script at the stock `kernel_remove_timer` entry.
    fn xm1_rtos_objects_stock_remove_timer(record: KernelContext, entry: GuestAddress);
}

struct Cpu;

#[inline(always)]
fn load(address: GuestAddress) -> u32 {
    unsafe { read_volatile(address.0 as *const u32) }
}

fn record(owner: Domain) -> KernelContext {
    ENTRY.words().context(owner)
}

impl Machine for Cpu {
    fn load_word(&self, at: GuestAddress) -> u32 {
        load(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        unsafe { core::ptr::write_volatile(at.0 as *mut u32, value) }
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        unsafe { core::ptr::read_volatile(at.0 as *const u8) }
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        unsafe { core::ptr::write_volatile(at.0 as *mut u8, value) }
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        let saved: u32;
        unsafe { asm!("mrs {saved}, cpsr", "cpsid i", saved = out(reg) saved, options(nostack)) };
        SavedInterrupts(saved)
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        unsafe { asm!("msr cpsr_fc, {saved}", saved = in(reg) saved.0, options(nostack)) };
    }

    fn dmb(&self) {
        unsafe { asm!("dmb sy", options(nostack, preserves_flags)) };
    }
}

impl ObjectLock for Cpu {
    fn lock_object(&self, lock: LockRecord) -> ObjectGuard<'_, Self> {
        let saved = self.mask_interrupts();
        unsafe { xm1_rtos_scheduler_acquire(lock) };
        ObjectGuard::new(self, lock, saved)
    }

    fn release_object(&self, lock: LockRecord) {
        unsafe { xm1_rtos_scheduler_release(lock) };
    }
}

impl Release for Cpu {
    fn context(&self, domain: Domain) -> KernelContext {
        record(domain)
    }

    fn reset_task(&self, waiter: Task) {
        unsafe { xm1_rtos_scheduler_reset(waiter) };
    }

    fn prepare(&self, owner: KernelContext, waiter: Task) -> Selection {
        match unsafe { xm1_rtos_scheduler_prepare(owner, waiter) } {
            Reschedule(0) => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    fn insert(&self, owner: KernelContext, waiter: Task) -> Selection {
        match unsafe { xm1_rtos_scheduler_insert(owner, waiter) } {
            Reschedule(0) => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    fn remove_timer(&self, owner: KernelContext, entry: GuestAddress) {
        unsafe { xm1_rtos_objects_stock_remove_timer(owner, entry) };
    }

    fn release(&self, owner: KernelContext, task: Task) -> Selection {
        match unsafe { xm1_rtos_objects_release(owner, task) } {
            0 => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }
}

impl Wake for Cpu {
    fn lock_scheduler(&self, owner: Domain) -> SchedulerGuard<'_, Self> {
        let lock = record(owner).scheduler_lock();
        unsafe { xm1_rtos_scheduler_acquire(lock) };
        SchedulerGuard::new(self, owner, None)
    }

    fn release_scheduler(&self, owner: Domain) {
        let lock = record(owner).scheduler_lock();
        unsafe { xm1_rtos_scheduler_release(lock) };
    }

    fn waiter_domain(&self, waiter: Task) -> Domain {
        match DomainId::from_word(load(waiter.domain())) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => family::xm1_rtos_objects_fault(family::KernelFault::WaiterDomain),
        }
    }

    fn record_id(&self, owner: KernelContext) -> u32 {
        load(owner.id())
    }

    fn notify(&self, id: u32) -> DispatchTarget {
        match unsafe { xm1_rtos_scheduler_notify(RecordId(id)) } {
            Notified::Local => DispatchTarget::Local,
            Notified::Remote => DispatchTarget::Remote,
        }
    }

    fn dispatch(&self, code: i32) {
        unsafe { xm1_rtos_scheduler_save(code) };
    }

    fn request_delayed_dispatch(&self) {
        self.store_word(ENTRY.words().reschedule, 1);
    }
}

#[inline(always)]
fn entry_words() -> DomainWords {
    ENTRY.words()
}

#[inline(always)]
unsafe fn cpsr_masked() -> bool {
    let cpsr: u32;
    unsafe { asm!("mrs {cpsr}, cpsr", cpsr = out(reg) cpsr, options(nostack)) };
    cpsr & 0x80 != 0
}

/// The wait release at its stock entry, A `0x44568` / B `0xd4528`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.f2_release"]
pub extern "C" fn xm1_rtos_objects_release(owner: KernelContext, task: Task) -> u32 {
    match release(&Cpu, owner, task) {
        Selection::Unchanged => 0,
        Selection::Changed => 1,
    }
}
