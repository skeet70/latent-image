//! F2 stage adapter: the generic wake bodies over the CPU and the owned S1/S2 exports.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod objects;

use core::arch::asm;
use core::ptr::read_volatile;
use objects::create::{Machine, SavedInterrupts};
use objects::layout::{Domain, DomainId, DomainWords, GuestAddress, KernelContext, LockRecord, Task};
use objects::locked::ObjectLock;
use objects::message::{MessagePriority, PriorityLimit};
use objects::wake::{DispatchTarget, Selection, Wake};
use objects::*;

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

    fn mask_interrupts(&self) -> SavedInterrupts {
        let saved: u32;
        unsafe { asm!("mrs {saved}, cpsr", "cpsid if", saved = out(reg) saved, options(nostack)) };
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
    fn with_object_lock<R>(&self, lock: LockRecord, body: impl FnOnce() -> R) -> R {
        let saved = self.mask_interrupts();
        unsafe { xm1_rtos_scheduler_acquire(lock) };
        let result = body();
        unsafe { xm1_rtos_scheduler_release(lock) };
        self.restore_interrupts(saved);
        result
    }
}

impl Wake for Cpu {
    fn with_scheduler_lock<R>(&self, owner: Domain, body: impl FnOnce() -> R) -> R {
        let lock = record(owner).scheduler_lock();
        unsafe { xm1_rtos_scheduler_acquire(lock) };
        let result = body();
        unsafe { xm1_rtos_scheduler_release(lock) };
        result
    }

    fn waiter_domain(&self, waiter: Task) -> Domain {
        match DomainId::from_word(load(waiter.domain())) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => panic!("waiting task has no domain tag"),
        }
    }

    fn remove_timer(&self, owner: Domain, entry: GuestAddress) {
        unsafe { xm1_rtos_objects_stock_remove_timer(record(owner), entry) };
    }

    fn reset_task(&self, waiter: Task) {
        unsafe { xm1_rtos_scheduler_reset(waiter) };
    }

    fn prepare(&self, owner: Domain, waiter: Task) -> Selection {
        match unsafe { xm1_rtos_scheduler_prepare(record(owner), waiter) } {
            Reschedule(0) => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    fn insert(&self, owner: Domain, waiter: Task) -> Selection {
        match unsafe { xm1_rtos_scheduler_insert(record(owner), waiter) } {
            Reschedule(0) => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    fn record_id(&self, owner: Domain) -> u32 {
        load(record(owner).id())
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
}

fn entry_words() -> DomainWords {
    ENTRY.words()
}

fn entry_state() -> Entry {
    Entry {
        handler_context: load(entry_words().handler_context),
        irq_masked: unsafe { cpsr_masked() },
        dispatch_enabled: 0,
    }
}

unsafe fn cpsr_masked() -> bool {
    let cpsr: u32;
    unsafe { asm!("mrs {cpsr}, cpsr", cpsr = out(reg) cpsr, options(nostack)) };
    cpsr & 0x80 != 0
}

fn target(domain: Domain, class: i32) -> Result<Domain, ObjectStatus> {
    match class {
        0 => Ok(domain),
        1 => Ok(Domain::A),
        2 => Ok(Domain::B),
        _ => Err(ObjectStatus::InvalidClass),
    }
}

/// Slot `+0x88`, in stock admission order.
#[no_mangle]
pub extern "C" fn xm1_rtos_objects_f2_set_flag(id: i32, bits: u32) -> i32 {
    let cpu = Cpu;
    let words = entry_words();
    let entry = entry_state();
    if entry.handler_context != 0 || entry.irq_masked {
        return ObjectStatus::Context.code();
    }
    let domain = match target(ENTRY, id >> 16) {
        Ok(domain) => domain,
        Err(status) => return status.code(),
    };
    let slot = match resolve_flag(EntryDomain::new(ENTRY), EncodedId(id), load(words.context(domain).event_flag_count())) {
        Ok(resolved) => resolved,
        Err(status) => return status.code(),
    };
    if load(domain.words().flag_config(slot.slot).exists()) == 0 {
        return ObjectStatus::NotCreated.code();
    }
    set_flag(&cpu, slot, Pattern(bits));
    0
}

/// Slot `+0xd4`, in stock admission order: the creation record is read before the message priority.
#[no_mangle]
pub extern "C" fn xm1_rtos_objects_f2_send(id: i32, message: u32) -> i32 {
    let cpu = Cpu;
    let words = entry_words();
    let entry = entry_state();
    if entry.handler_context != 0 || entry.irq_masked {
        return ObjectStatus::Context.code();
    }
    let domain = match target(ENTRY, id >> 16) {
        Ok(domain) => domain,
        Err(status) => return status.code(),
    };
    let slot = match resolve_mailbox(
        EntryDomain::new(ENTRY),
        EncodedId(id),
        load(words.context(domain).mailbox_count()),
    ) {
        Ok(resolved) => resolved,
        Err(status) => return status.code(),
    };
    let config = domain.words().mailbox_config(slot.slot);
    if load(config.exists()) == 0 {
        return ObjectStatus::NotCreated.code();
    }
    let priority = if load(config.attributes()) & 2 != 0 {
        let raw = load(GuestAddress(message + 4));
        match MessagePriority::new(raw, PriorityLimit(load(config.priority_limit()))) {
            Ok(priority) => Some(priority),
            Err(status) => return status.code(),
        }
    } else {
        None
    };
    send(&cpu, slot, objects::layout::Message(GuestAddress(message)), priority);
    0
}
