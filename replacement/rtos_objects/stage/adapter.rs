//! F2 stage adapter over the CPU and the owned S1/S2 bodies; unbuilt until the scheduler exports
//! the C-ABI shims named below.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod objects;

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use objects::create::{Machine, SavedInterrupts};
use objects::layout::{Domain, GuestAddress, LockRecord};
use objects::locked::ObjectLock;
use objects::wake::{DispatchTarget, Selection, Wake};
use objects::*;

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

extern "C" {
    fn xm1_rtos_scheduler_insert(record: GuestAddress, task: u32) -> u32;
    fn xm1_rtos_scheduler_reset(task: u32);
    fn xm1_rtos_scheduler_prepare(record: GuestAddress, task: u32) -> u32;
    fn xm1_rtos_scheduler_notify(record: GuestAddress) -> u32;
    fn xm1_rtos_scheduler_save(code: i32);
    fn xm1_rtos_scheduler_acquire(lock: GuestAddress);
    fn xm1_rtos_scheduler_release(lock: GuestAddress);
    /// Provided by the link script at the stock `kernel_remove_timer` entry.
    fn xm1_rtos_objects_stock_remove_timer(record: GuestAddress, entry: GuestAddress);
}

struct Cpu;

impl Machine for Cpu {
    fn load_word(&self, at: GuestAddress) -> u32 {
        unsafe { read_volatile(at.0 as *const u32) }
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        unsafe { write_volatile(at.0 as *mut u32, value) }
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
        let saved: u32;
        unsafe { asm!("mrs {saved}, cpsr", "cpsid i", saved = out(reg) saved, options(nostack)) };
        let word = GuestAddress(read_volatile(lock.word().0 as *const u32));
        xm1_rtos_scheduler_acquire(word);
        let result = body();
        xm1_rtos_scheduler_release(word);
        unsafe { asm!("msr cpsr_fc, {saved}", saved = in(reg) saved, options(nostack)) };
        result
    }
}

impl Wake for Cpu {
    fn with_scheduler_lock<R>(&self, domain: Domain, body: impl FnOnce() -> R) -> R {
        let lock = ENTRY.words().context(domain).scheduler_lock();
        let word = GuestAddress(read_volatile(lock.word().0 as *const u32));
        xm1_rtos_scheduler_acquire(word);
        let result = body();
        xm1_rtos_scheduler_release(word);
        result
    }

    fn waiter_domain(&self, task: Task) -> Domain {
        match DomainId::from_word(read_volatile(task.domain().0 as *const u32)) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => Domain::A,
        }
    }

    fn remove_timer(&self, owner: Domain, entry: GuestAddress) {
        unsafe { xm1_rtos_objects_stock_remove_timer(ENTRY.words().context(owner).0, entry) };
    }

    fn reset_task(&self, task: Task) {
        xm1_rtos_scheduler_reset(task.0 .0);
    }

    fn prepare(&self, owner: Domain, task: Task) -> Selection {
        let record = ENTRY.words().context(owner);
        if xm1_rtos_scheduler_prepare(record.0, task.0 .0) != 0 {
            Selection::Changed
        } else {
            Selection::Unchanged
        }
    }

    fn insert(&self, owner: Domain, task: Task) -> Selection {
        let record = ENTRY.words().context(owner);
        if xm1_rtos_scheduler_insert(record.0, task.0 .0) != 0 {
            Selection::Changed
        } else {
            Selection::Unchanged
        }
    }

    fn notify(&self, owner: Domain) -> DispatchTarget {
        let record = ENTRY.words().context(owner);
        if xm1_rtos_scheduler_notify(record.0) != 0 {
            DispatchTarget::Local
        } else {
            DispatchTarget::Remote
        }
    }

    fn dispatch(&self) {
        xm1_rtos_scheduler_save(-24);
    }
}

fn flag_facts(domain: Domain, slot: FlagSlot) -> Facts {
    let config = domain.words().flag_config(slot);
    Facts {
        exists: read_volatile(config.exists().0 as *const u32) != 0,
        attributes: read_volatile(config.attributes().0 as *const u32),
        priority_limit: 0,
    }
}

fn mailbox_facts(domain: Domain, slot: MailboxSlot) -> Facts {
    let config = domain.words().mailbox_config(slot);
    Facts {
        exists: read_volatile(config.exists().0 as *const u32) != 0,
        attributes: read_volatile(config.attributes().0 as *const u32),
        priority_limit: read_volatile(config.priority_limit().0 as *const u32),
    }
}

/// Slot `+0x88`.
#[no_mangle]
pub extern "C" fn xm1_rtos_objects_f2_set_flag(id: i32, bits: u32) -> i32 {
    let cpu = Cpu;
    let entry = EntryDomain::new(ENTRY);
    let slot = match resolve_flag(entry, EncodedId(id), 120) {
        Ok(resolved) => resolved,
        Err(status) => return status.code(),
    };
    let request = Request {
        service: Service::FlagSet,
        entry: Entry {
            handler_context: 0,
            irq_masked: false,
            dispatch_enabled: 1,
        },
        entry_domain: entry,
        id: EncodedId(id),
        records: Records {
            flags: 120,
            mailboxes: 1,
        },
        facts: flag_facts(slot.domain, slot.slot),
        params: Params::NONE,
    };
    match admit(&request) {
        Ok(admitted) => match admitted.object {
            Object::Flag(id) => {
                set_flag(&cpu, id, Pattern(bits));
                0
            }
            Object::Mailbox(_) => -18,
        },
        Err(status) => status.code(),
    }
}

/// Slot `+0xd4`.
#[no_mangle]
pub extern "C" fn xm1_rtos_objects_f2_send(id: i32, message: u32) -> i32 {
    let cpu = Cpu;
    let entry = EntryDomain::new(ENTRY);
    let slot = match resolve_mailbox(entry, EncodedId(id), 1) {
        Ok(resolved) => resolved,
        Err(status) => return status.code(),
    };
    let priority = read_volatile(GuestAddress(message + 4) as *const u32);
    let request = Request {
        service: Service::MailboxSend,
        entry: Entry {
            handler_context: 0,
            irq_masked: false,
            dispatch_enabled: 0,
        },
        entry_domain: entry,
        id: EncodedId(id),
        records: Records {
            flags: 120,
            mailboxes: 1,
        },
        facts: mailbox_facts(slot.domain, slot.slot),
        params: Params {
            pattern: Pattern(0),
            mode: 0,
            timeout: None,
            message_priority: Some(priority),
        },
    };
    match admit(&request) {
        Ok(admitted) => match admitted.object {
            Object::Mailbox(id) => {
                send(&cpu, id, Message(GuestAddress(message)), admitted.priority);
                0
            }
            Object::Flag(_) => -18,
        },
        Err(status) => status.code(),
    }
}
