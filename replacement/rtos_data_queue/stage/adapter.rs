//! Data-queue A32 stage adapter: the eleven stock entries over the owned queue core.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod data_queue;

use core::arch::{asm, global_asm};
use core::ptr::{read_volatile, write_volatile};
use data_queue::*;
use data_queue::admission::{admit_precheck, Entry, Facts, Params, Records, Request, Service};
use data_queue::create::{create_data_queue, delete_data_queue, DataQueueCreate, QueueAttributes};
use data_queue::env::{
    Deadline, DispatchTarget, Env, QueueOrder, SavedInterrupts, SavedMask, Selection, Tick,
    TimeoutCallback, TimerEntryAddress,
};
use data_queue::id::{own_data_queue_slot, resolve_data_queue, DataQueueId, EncodedId, EntryDomain};
use data_queue::layout::{
    DataQueueConfig, Domain, DomainId, GuestAddress, KernelContext, LockRecord, Task,
};
use data_queue::ops::{
    force_send, receive, resume_receive, resume_send, send, send_poll, send_timeout, ReceiveBlocking,
    ReceiveResume, SendBlocking, SendResume,
};
use data_queue::ring::{Capacity, OutWord, Payload, RingBase};
use data_queue::status::ObjectStatus;
use data_queue::timeout::Timeout;

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

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
    fn xm1_rtos_scheduler_acquire(lock: LockRecord) -> u32;
    fn xm1_rtos_scheduler_release(lock: LockRecord) -> LockRecord;
    fn xm1_rtos_scheduler_insert(record: KernelContext, task: Task) -> Reschedule;
    fn xm1_rtos_scheduler_remove(record: KernelContext, task: Task) -> Reschedule;
    fn xm1_rtos_scheduler_reset(task: Task) -> Task;
    fn xm1_rtos_scheduler_prepare(record: KernelContext, task: Task) -> Reschedule;
    fn xm1_rtos_scheduler_notify(id: RecordId) -> Notified;
    fn xm1_rtos_scheduler_save(code: i32);
    fn xm1_rtos_data_queue_stock_insert_timer(
        record: KernelContext,
        entry: GuestAddress,
        expiry: u32,
    );
    fn xm1_rtos_data_queue_stock_remove_timer(record: KernelContext, entry: GuestAddress);
}

struct Cpu;

#[inline(always)]
fn load(address: GuestAddress) -> u32 {
    unsafe { read_volatile(address.0 as *const u32) }
}

#[inline(always)]
fn store(address: GuestAddress, value: u32) {
    unsafe { write_volatile(address.0 as *mut u32, value) }
}

#[inline(always)]
fn load_byte(address: GuestAddress) -> u8 {
    unsafe { read_volatile(address.0 as *const u8) }
}

#[inline(always)]
fn store_byte(address: GuestAddress, value: u8) {
    unsafe { write_volatile(address.0 as *mut u8, value) }
}

impl Env for Cpu {
    fn load_word(&self, at: GuestAddress) -> u32 {
        load(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        store(at, value)
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        load_byte(at)
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        store_byte(at, value)
    }

    fn dmb(&self) {
        unsafe { asm!("dmb sy", options(nostack, preserves_flags)) };
    }

    fn dsb(&self) {
        unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
    }

    fn mask_if(&self) -> SavedInterrupts {
        let saved: u32;
        unsafe {
            asm!("mrs {saved}, cpsr", "cpsid if", saved = out(reg) saved, options(nostack))
        };
        SavedInterrupts(saved)
    }

    fn restore_if(&self, saved: SavedInterrupts) {
        unsafe { asm!("msr cpsr_fc, {saved}", saved = in(reg) saved.0, options(nostack)) };
    }

    fn mask_i(&self) -> SavedMask {
        let saved: u32;
        unsafe {
            asm!("mrs {saved}, cpsr", "cpsid i", saved = out(reg) saved, options(nostack))
        };
        SavedMask(saved)
    }

    fn restore_i(&self, saved: SavedMask) {
        unsafe { asm!("msr cpsr_fc, {saved}", saved = in(reg) saved.0, options(nostack)) };
    }

    fn with_object_lock_after<R>(
        &self,
        lock: LockRecord,
        body: impl FnOnce() -> R,
        after: impl FnOnce(&R),
    ) -> R {
        let saved = self.mask_i();
        unsafe { xm1_rtos_scheduler_acquire(lock) };
        let result = body();
        unsafe { xm1_rtos_scheduler_release(lock) };
        after(&result);
        self.restore_i(saved);
        result
    }

    fn with_object_lock<R>(&self, lock: LockRecord, body: impl FnOnce() -> R) -> R {
        self.with_object_lock_after(lock, body, |_| {})
    }

    fn with_scheduler_lock<R>(&self, owner: Domain, body: impl FnOnce() -> R) -> R {
        let saved = self.mask_i();
        let lock = ENTRY.words().context(owner).scheduler_lock();
        unsafe { xm1_rtos_scheduler_acquire(lock) };
        let result = body();
        unsafe { xm1_rtos_scheduler_release(lock) };
        self.restore_i(saved);
        result
    }

    fn context(&self, owner: Domain) -> KernelContext {
        ENTRY.words().context(owner)
    }

    fn current(&self, entry: Domain) -> Task {
        Task(GuestAddress(load(entry.words().running)))
    }

    fn remove_ready(&self, owner: KernelContext, task: Task) {
        unsafe { xm1_rtos_scheduler_remove(owner, task) };
    }

    fn queue_wait(&self, _owner: KernelContext, task: Task, queue: data_queue::layout::WaitQueue, order: QueueOrder) {
        let head = queue.0;
        let (mut at, mut next) = (GuestAddress(load(queue.prev())), head);
        if order == QueueOrder::Priority {
            at = head;
            next = GuestAddress(load(head));
            let candidate = load_byte(task.priority());
            while next.0 != head.0 {
                let existing = Task(next);
                if load_byte(existing.priority()) > candidate {
                    break;
                }
                at = next;
                next = GuestAddress(load(existing.next()));
            }
        }
        store(task.next(), next.0);
        store(task.prev(), at.0);
        store(GuestAddress(at.0), task.0 .0);
        store(GuestAddress(next.0).offset(0x04), task.0 .0);
    }

    fn arm_timer(&self, owner: KernelContext, entry: TimerEntryAddress, deadline: Deadline) {
        unsafe { xm1_rtos_data_queue_stock_insert_timer(owner, entry.0, deadline.0) };
    }

    fn tick(&self, owner: KernelContext) -> Tick {
        Tick(load(GuestAddress(load(owner.tick_now()))))
    }

    fn timeout_callback(&self) -> TimeoutCallback {
        TimeoutCallback(match ENTRY {
            Domain::A => 0x44318,
            Domain::B => 0xd42d8,
        })
    }

    fn reset_task(&self, task: Task) {
        unsafe { xm1_rtos_scheduler_reset(task) };
    }

    fn prepare(&self, owner: KernelContext, task: Task) -> Selection {
        match unsafe { xm1_rtos_scheduler_prepare(owner, task) } {
            Reschedule(0) => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    fn insert(&self, owner: KernelContext, task: Task) -> Selection {
        match unsafe { xm1_rtos_scheduler_insert(owner, task) } {
            Reschedule(0) => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    fn remove_timer(&self, owner: KernelContext, entry: GuestAddress) {
        unsafe { xm1_rtos_data_queue_stock_remove_timer(owner, entry) };
    }

    fn waiter_domain(&self, task: Task) -> Domain {
        match DomainId::from_word(load(task.domain())) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => panic!("waiting task has no domain tag"),
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
        store(ENTRY.words().reschedule, 1);
    }
}

#[inline(always)]
fn entry() -> Entry {
    let cpsr: u32;
    unsafe { asm!("mrs {cpsr}, cpsr", cpsr = out(reg) cpsr, options(nostack)) };
    Entry {
        handler_context: load(ENTRY.words().handler_context),
        irq_masked: cpsr & 0x80 != 0,
        dispatch_enabled: load(ENTRY.words().dispatch_enabled),
    }
}

#[inline(always)]
fn count() -> u32 {
    load(ENTRY.words().context(ENTRY).data_queue_count())
}

#[inline(always)]
fn facts(config: DataQueueConfig) -> Facts {
    Facts {
        exists: load(config.exists()) != 0,
        attributes: load(config.attributes()),
        capacity: load(config.capacity()),
    }
}

fn service(service: Service, id: i32, timeout: Option<i32>, payload: Option<u32>, out: Option<u32>) -> i32 {
    let cpu = Cpu;
    let entry = entry();
    let records = Records {
        data_queues: count(),
    };
    let resolved = match admit_precheck(
        service,
        ENTRY,
        entry,
        EncodedId(id),
        records,
        Params { timeout },
    ) {
        Ok((resolved, _, _)) => resolved,
        Err(status) => return status.code(),
    };
    let config = resolved.domain.words().data_queue_config(resolved.slot);
    let request = Request {
        service,
        entry_domain: ENTRY,
        entry,
        id: EncodedId(id),
        records,
        facts: facts(config),
        params: Params { timeout },
    };
    let admitted = match data_queue::admission::admit(&request) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let payload = Payload(payload.unwrap_or(0));
    let out = OutWord(GuestAddress(out.unwrap_or(0)));
    let blocking = matches!(
        service,
        Service::Send | Service::SendTimeout | Service::Receive | Service::ReceiveTimeout
    );
    let saved = if blocking { Some(cpu.mask_i()) } else { None };
    let result = match service {
        Service::Send => match send(&cpu, EntryDomain::new(ENTRY), admitted.object, payload) {
            Ok(SendBlocking::Ready(outcome)) => outcome.code(),
            Ok(SendBlocking::Pending(pending)) => {
                cpu.dispatch(-53);
                match resume_send(&cpu, pending) {
                    SendResume::Completed(outcome) => outcome.code(),
                    SendResume::Pending(_) | SendResume::Corrupt(_) => -49,
                }
            }
            Err(status) => status.code(),
        },
        Service::SendPoll => match send_poll(&cpu, admitted.object, payload, false) {
            Ok(outcome) => outcome.code(),
            Err(status) => status.code(),
        },
        Service::SendPollHandler => match send_poll(&cpu, admitted.object, payload, true) {
            Ok(outcome) => outcome.code(),
            Err(status) => status.code(),
        },
        Service::SendTimeout => match send_timeout(
            &cpu,
            EntryDomain::new(ENTRY),
            admitted.object,
            payload,
            admitted.timeout,
        ) {
            Ok(SendBlocking::Ready(outcome)) => outcome.code(),
            Ok(SendBlocking::Pending(pending)) => {
                cpu.dispatch(-55);
                match resume_send(&cpu, pending) {
                    SendResume::Completed(outcome) => outcome.code(),
                    SendResume::Pending(_) | SendResume::Corrupt(_) => -49,
                }
            }
            Err(status) => status.code(),
        },
        Service::ForceSend => match force_send(&cpu, admitted.object, payload, false) {
            Ok(()) => 0,
            Err(status) => status.code(),
        },
        Service::ForceSendHandler => match force_send(&cpu, admitted.object, payload, true) {
            Ok(()) => 0,
            Err(status) => status.code(),
        },
        Service::Receive => match receive(
            &cpu,
            EntryDomain::new(ENTRY),
            admitted.object,
            out,
            admitted.timeout,
        ) {
            Ok(ReceiveBlocking::Ready(outcome)) => outcome.code(),
            Ok(ReceiveBlocking::Pending(pending)) => {
                cpu.dispatch(-57);
                match resume_receive(&cpu, pending) {
                    ReceiveResume::Completed(outcome) => outcome.code(),
                    ReceiveResume::Pending(_) | ReceiveResume::Corrupt(_) => -49,
                }
            }
            Err(status) => status.code(),
        },
        Service::ReceivePoll => match receive(
            &cpu,
            EntryDomain::new(ENTRY),
            admitted.object,
            out,
            Timeout::Poll,
        ) {
            Ok(ReceiveBlocking::Ready(outcome)) => outcome.code(),
            Ok(ReceiveBlocking::Pending(_)) => -50,
            Err(status) => status.code(),
        },
        Service::ReceiveTimeout => match receive(
            &cpu,
            EntryDomain::new(ENTRY),
            admitted.object,
            out,
            admitted.timeout,
        ) {
            Ok(ReceiveBlocking::Ready(outcome)) => outcome.code(),
            Ok(ReceiveBlocking::Pending(pending)) => {
                cpu.dispatch(-59);
                match resume_receive(&cpu, pending) {
                    ReceiveResume::Completed(outcome) => outcome.code(),
                    ReceiveResume::Pending(_) | ReceiveResume::Corrupt(_) => -49,
                }
            }
            Err(status) => status.code(),
        },
    };
    if let Some(saved) = saved {
        cpu.restore_i(saved);
    }
    result
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.dq_body_create"]
pub extern "C" fn xm1_rtos_data_queue_create_body(id: i32, packet: u32) -> i32 {
    let cpu = Cpu;
    let slot = match own_data_queue_slot(EncodedId(id), count()) {
        Ok(slot) => slot,
        Err(status) => return status.code(),
    };
    let config = ENTRY.words().data_queue_config(slot);
    if load(config.exists()) != 0 {
        return ObjectStatus::AlreadyExists.code();
    }
    let create = DataQueueCreate {
        attributes: QueueAttributes(load(GuestAddress(packet))),
        capacity: Capacity(load(GuestAddress(packet + 4))),
        ring: RingBase(GuestAddress(load(GuestAddress(packet + 8)))),
    };
    match create_data_queue(&cpu, DataQueueId { domain: ENTRY, slot }, create) {
        Ok(_) => 0,
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.dq_body_delete"]
pub extern "C" fn xm1_rtos_data_queue_delete_body(id: i32) -> i32 {
    let cpu = Cpu;
    let slot = match own_data_queue_slot(EncodedId(id), count()) {
        Ok(slot) => slot,
        Err(status) => return status.code(),
    };
    let caller = if entry().handler_context == 0 {
        HandlerContext::Task
    } else {
        HandlerContext::Handler
    };
    match delete_data_queue(&cpu, DataQueueId { domain: ENTRY, slot }, caller) {
        Ok(()) => 0,
        Err(status) => status.code(),
    }
}

macro_rules! entry {
    ($section:literal, $name:literal, $body:literal) => {
        global_asm!(concat!(
            ".pushsection .text.", $section, ", \"ax\", %progbits\n",
            ".arm\n",
            ".global xm1_rtos_data_queue_", $name, "\n",
            ".type xm1_rtos_data_queue_", $name, ", %function\n",
            "xm1_rtos_data_queue_", $name, ":\n",
            "b xm1_rtos_data_queue_", $body, "_body\n",
            ".size xm1_rtos_data_queue_", $name, ", . - xm1_rtos_data_queue_", $name, "\n",
            ".popsection\n",
        ));
    };
}

entry!("dq_create", "create", "create");
entry!("dq_delete", "delete", "delete");
entry!("dq_send", "send", "send");
entry!("dq_send_poll", "send_poll", "send_poll");
entry!("dq_send_poll_handler", "send_poll_handler", "send_poll_handler");
entry!("dq_send_timeout", "send_timeout", "send_timeout");
entry!("dq_force_send", "force_send", "force_send");
entry!("dq_force_send_handler", "force_send_handler", "force_send_handler");
entry!("dq_receive", "receive", "receive");
entry!("dq_receive_poll", "receive_poll", "receive_poll");
entry!("dq_receive_timeout", "receive_timeout", "receive_timeout");

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_send_body(id: i32, data: u32) -> i32 {
    service(Service::Send, id, None, Some(data), None)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_send_poll_body(id: i32, data: u32) -> i32 {
    service(Service::SendPoll, id, None, Some(data), None)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_send_poll_handler_body(id: i32, data: u32) -> i32 {
    service(Service::SendPollHandler, id, None, Some(data), None)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_send_timeout_body(id: i32, data: u32, timeout: i32) -> i32 {
    service(Service::SendTimeout, id, Some(timeout), Some(data), None)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_force_send_body(id: i32, data: u32) -> i32 {
    service(Service::ForceSend, id, None, Some(data), None)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_force_send_handler_body(id: i32, data: u32) -> i32 {
    service(Service::ForceSendHandler, id, None, Some(data), None)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_receive_body(id: i32, out: u32) -> i32 {
    service(Service::Receive, id, None, None, Some(out))
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_receive_poll_body(id: i32, out: u32) -> i32 {
    service(Service::ReceivePoll, id, None, None, Some(out))
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn xm1_rtos_data_queue_receive_timeout_body(id: i32, out: u32, timeout: i32) -> i32 {
    service(Service::ReceiveTimeout, id, Some(timeout), None, Some(out))
}
