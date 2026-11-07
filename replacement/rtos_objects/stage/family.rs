use super::{cpsr_masked, entry_words, load, xm1_rtos_scheduler_save, Cpu, Reschedule, ENTRY};
use crate::objects;
use crate::objects::admission::{admit_op, Entry, Object, Params, Request, Service};
use crate::objects::block::{
    receive, resume_flag, resume_receive, wait_flag, Block, Deadline, FlagBlocking, FlagResume,
    IrqScope, ReceiveBlocking, ReceiveResume, SavedMask, Tick, TimerEntryAddress, TimeoutCallback,
};
use crate::objects::create::{create_flag, create_mailbox, FlagAttributes, MailboxAttributes};
use crate::objects::id::{own_flag_slot, own_mailbox_slot, EncodedId, EntryDomain, FlagId, MailboxId};
use crate::objects::layout::{Domain, GuestAddress, Message, Task};
use crate::objects::locked::{clear_flag, poll_flag, poll_receive, OutWord};
use crate::objects::matching::{KeepMask, Pattern, WantedMask};
use crate::objects::message::PriorityLimit;
use crate::objects::status::ObjectStatus;
use crate::objects::teardown::{delete_flag, delete_mailbox};
use crate::objects::timeout::Timeout;
use crate::objects::wake::{send, set_flag, set_flag_handler};

extern "C" {
    fn xm1_rtos_scheduler_remove(record: objects::layout::KernelContext, task: Task) -> Reschedule;
    fn xm1_rtos_objects_stock_insert_timer(record: objects::layout::KernelContext, entry: GuestAddress, expiry: u32);
    static xm1_rtos_objects_stock_timeout: u32;
}

fn wrong_object() -> ! {
    panic!("admission returned the wrong object family")
}

fn still_waiting() -> ! {
    panic!("the S2 save returned with the wait still queued")
}

fn unknown_result() -> ! {
    panic!("the S2 save returned an unknown wait result")
}

impl IrqScope for Cpu {
    fn mask_i(&self) -> SavedMask {
        let saved: u32;
        unsafe { core::arch::asm!("mrs {saved}, cpsr", "cpsid i", saved = out(reg) saved, options(nostack)) };
        SavedMask(saved)
    }

    fn restore_i(&self, saved: SavedMask) {
        unsafe { core::arch::asm!("msr cpsr_fc, {saved}", saved = in(reg) saved.0, options(nostack)) };
    }
}

impl Block for Cpu {
    fn current(&self, entry: Domain) -> Task {
        Task(GuestAddress(load(GuestAddress(load(super::record(entry).running())))))
    }

    fn remove_ready(&self, owner: objects::layout::KernelContext, task: Task) {
        unsafe { xm1_rtos_scheduler_remove(owner, task) };
    }

    fn queue_wait(&self, _owner: objects::layout::KernelContext, task: Task, queue: objects::layout::WaitQueue, order: objects::order::QueueOrder) {
        objects::wake::queue_insert(self, task, queue, order);
    }

    fn arm_timer(&self, owner: objects::layout::KernelContext, entry: TimerEntryAddress, deadline: Deadline) {
        unsafe { xm1_rtos_objects_stock_insert_timer(owner, entry.0, deadline.0) };
    }

    fn tick(&self, owner: objects::layout::KernelContext) -> Tick {
        Tick(load(GuestAddress(load(owner.tick_now()))))
    }

    fn timeout_callback(&self) -> TimeoutCallback {
        TimeoutCallback(core::ptr::addr_of!(xm1_rtos_objects_stock_timeout) as u32)
    }
}

/// Stock create/delete set I+F through `0x42560` and restore only those bits through `0x42568`.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct LifecycleMask(u32);

fn mask_lifecycle() -> LifecycleMask {
    let saved: u32;
    let set: u32;
    unsafe {
        core::arch::asm!(
            "mrs {saved}, cpsr",
            "orr {set}, {saved}, #0xc0",
            "msr cpsr_c, {set}",
            saved = out(reg) saved,
            set = out(reg) set,
            options(nostack),
        )
    };
    LifecycleMask(saved & 0xc0)
}

fn restore_lifecycle(saved: LifecycleMask) {
    let current: u32;
    let restored: u32;
    unsafe {
        core::arch::asm!(
            "mrs {current}, cpsr",
            "bic {restored}, {current}, #0xc0",
            "orr {restored}, {restored}, {saved}",
            "msr cpsr_c, {restored}",
            current = out(reg) current,
            restored = out(reg) restored,
            saved = in(reg) saved.0,
            options(nostack),
        )
    };
}

/// Stock retry addresses `0x488e8` / `0x4892c`.
fn unmask_i() {
    let current: u32;
    let cleared: u32;
    unsafe {
        core::arch::asm!(
            "mrs {current}, cpsr",
            "bic {cleared}, {current}, #0x80",
            "msr cpsr_c, {cleared}",
            current = out(reg) current,
            cleared = out(reg) cleared,
            options(nostack),
        )
    };
}

struct StageTables;

impl objects::admission::Records for StageTables {
    fn flags(&self, domain: Domain) -> u32 {
        load(entry_words().context(domain).event_flag_count())
    }

    fn mailboxes(&self, domain: Domain) -> u32 {
        load(entry_words().context(domain).mailbox_count())
    }
}

impl objects::admission::Facts for StageTables {
    fn exists(&self, object: Object) -> bool {
        match object {
            Object::Flag(id) => load(id.domain.words().flag_config(id.slot).exists()) != 0,
            Object::Mailbox(id) => load(id.domain.words().mailbox_config(id.slot).exists()) != 0,
        }
    }

    fn attributes(&self, object: Object) -> u32 {
        match object {
            Object::Flag(id) => load(id.domain.words().flag_config(id.slot).attributes()),
            Object::Mailbox(id) => load(id.domain.words().mailbox_config(id.slot).attributes()),
        }
    }

    fn priority_limit(&self, object: Object) -> u32 {
        match object {
            Object::Mailbox(id) => load(id.domain.words().mailbox_config(id.slot).priority_limit()),
            Object::Flag(_) => 0,
        }
    }

    fn message_priority(&self, message: Message) -> u32 {
        load(message.priority())
    }
}

#[inline(always)]
fn request(service: Service, id: i32, params: Params) -> Request {
    let words = entry_words();
    Request {
        service,
        entry: Entry {
            handler_context: load(words.handler_context),
            irq_masked: unsafe { cpsr_masked() },
            dispatch_enabled: if service.blocks() { load(words.dispatch_enabled) } else { 0 },
        },
        entry_domain: EntryDomain::new(ENTRY),
        id: EncodedId(id),
        params,
    }
}

fn flag(admitted: &objects::admission::Admitted) -> Option<FlagId> {
    match admitted.object {
        Object::Flag(id) => Some(id),
        Object::Mailbox(_) => None,
    }
}

fn mailbox(admitted: &objects::admission::Admitted) -> Option<MailboxId> {
    match admitted.object {
        Object::Mailbox(id) => Some(id),
        Object::Flag(_) => None,
    }
}

fn want(admitted: &objects::admission::Admitted) -> Result<WantedMask, ObjectStatus> {
    WantedMask::new(admitted.want)
}

/// Slot `+0x80`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_create"]
pub extern "C" fn xm1_rtos_objects_family_flag_create(id: i32, request_pointer: u32) -> i32 {
    let saved = mask_lifecycle();
    let code = (|| -> i32 {
        let words = entry_words();
        let slot = match own_flag_slot(EncodedId(id), load(words.context(ENTRY).event_flag_count())) {
            Ok(slot) => slot,
            Err(status) => return status.code(),
        };
        if load(ENTRY.words().flag_config(slot).exists()) != 0 {
            return ObjectStatus::AlreadyExists.code();
        }
        let create = objects::create::FlagCreate {
            attributes: FlagAttributes(load(GuestAddress(request_pointer))),
            initial: Pattern(load(GuestAddress(request_pointer + 4))),
        };
        match create_flag(&Cpu, FlagId { domain: ENTRY, slot }, create) {
            Ok(_) => 0,
            Err(status) => status.code(),
        }
    })();
    restore_lifecycle(saved);
    code
}

/// Slot `+0x84`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_delete"]
pub extern "C" fn xm1_rtos_objects_family_flag_delete(id: i32) -> i32 {
    let saved = mask_lifecycle();
    let code = (|| -> i32 {
        let words = entry_words();
        let slot = match own_flag_slot(EncodedId(id), load(words.context(ENTRY).event_flag_count())) {
            Ok(slot) => slot,
            Err(status) => return status.code(),
        };
        match delete_flag(&Cpu, FlagId { domain: ENTRY, slot }) {
            Ok(()) => 0,
            Err(status) => status.code(),
        }
    })();
    restore_lifecycle(saved);
    code
}

/// Slot `+0x88`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_set"]
pub extern "C" fn xm1_rtos_objects_family_flag_set(id: i32, bits: u32) -> i32 {
    let admitted = match admit_op::<{ Service::FlagSet as u8 }, _, _>(
        &request(Service::FlagSet, id, Params::NONE),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match flag(&admitted) {
        Some(id) => {
            set_flag(&Cpu, id, Pattern(bits));
            0
        }
        None => wrong_object(),
    }
}

/// Slot `+0x8c`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_set_handler"]
pub extern "C" fn xm1_rtos_objects_family_flag_set_handler(id: i32, bits: u32) -> i32 {
    let admitted = match admit_op::<{ Service::FlagSetHandler as u8 }, _, _>(
        &request(Service::FlagSetHandler, id, Params::NONE),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match flag(&admitted) {
        Some(id) => {
            set_flag_handler(&Cpu, id, Pattern(bits));
            0
        }
        None => wrong_object(),
    }
}

/// Slot `+0x90`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_clear"]
pub extern "C" fn xm1_rtos_objects_family_flag_clear(id: i32, keep: u32) -> i32 {
    let admitted = match admit_op::<{ Service::FlagClear as u8 }, _, _>(
        &request(Service::FlagClear, id, Params::NONE),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match flag(&admitted) {
        Some(id) => match clear_flag(&Cpu, id, KeepMask(keep)) {
            Ok(()) => 0,
            Err(status) => status.code(),
        },
        None => wrong_object(),
    }
}

fn flag_wait_entry<const SERVICE: u8>(id: i32, pattern: u32, mode: u32, result: u32, timeout: Option<i32>) -> i32 {
    let params = Params { pattern: Pattern(pattern), mode, timeout, message: None };
    let admitted = match admit_op::<SERVICE, _, _>(
        &request(Service::from_u8(SERVICE), id, params),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match flag(&admitted) {
        Some(id) => id,
        None => wrong_object(),
    };
    let wanted = match want(&admitted) {
        Ok(wanted) => wanted,
        Err(status) => return status.code(),
    };
    let saved = Cpu.mask_i();
    let code = loop {
        match wait_flag(
            &Cpu,
            EntryDomain::new(ENTRY),
            id,
            wanted,
            admitted.mode,
            OutWord(GuestAddress(result)),
            admitted.timeout.unwrap_or(Timeout::Forever),
        ) {
            Ok(FlagBlocking::Ready(outcome)) => break outcome.code(),
            Ok(FlagBlocking::Pending(pending)) => {
                unsafe { xm1_rtos_scheduler_save(0) };
                break match resume_flag(&Cpu, pending) {
                    FlagResume::Completed(outcome) => outcome.code(),
                    FlagResume::Pending(_) => still_waiting(),
                    FlagResume::Corrupt(_) => unknown_result(),
                };
            }
            Ok(FlagBlocking::Retry) => {
                unmask_i();
                Cpu.mask_i();
            }
            Err(status) => break status.code(),
        }
    };
    Cpu.restore_i(saved);
    code
}

/// Slot `+0x94`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_wait"]
pub extern "C" fn xm1_rtos_objects_family_flag_wait(id: i32, pattern: u32, mode: u32, result: u32) -> i32 {
    flag_wait_entry::<{ Service::FlagWait as u8 }>(id, pattern, mode, result, None)
}

/// Slot `+0x9c`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_timed_wait"]
pub extern "C" fn xm1_rtos_objects_family_flag_timed_wait(id: i32, pattern: u32, mode: u32, result: u32, timeout: i32) -> i32 {
    flag_wait_entry::<{ Service::FlagTimedWait as u8 }>(id, pattern, mode, result, Some(timeout))
}

/// Slot `+0x98`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_poll"]
pub extern "C" fn xm1_rtos_objects_family_flag_poll(id: i32, pattern: u32, mode: u32, result: u32) -> i32 {
    let params = Params { pattern: Pattern(pattern), mode, timeout: None, message: None };
    let admitted = match admit_op::<{ Service::FlagPoll as u8 }, _, _>(
        &request(Service::FlagPoll, id, params),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match flag(&admitted) {
        Some(id) => id,
        None => wrong_object(),
    };
    let wanted = match want(&admitted) {
        Ok(wanted) => wanted,
        Err(status) => return status.code(),
    };
    match poll_flag(&Cpu, id, wanted, admitted.mode, OutWord(GuestAddress(result))) {
        Ok(outcome) => outcome.code(),
        Err(status) => status.code(),
    }
}

/// Slot `+0xcc`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_create"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_create(id: i32, request_pointer: u32) -> i32 {
    let saved = mask_lifecycle();
    let code = (|| -> i32 {
        let words = entry_words();
        let slot = match own_mailbox_slot(EncodedId(id), load(words.context(ENTRY).mailbox_count())) {
            Ok(slot) => slot,
            Err(status) => return status.code(),
        };
        if load(ENTRY.words().mailbox_config(slot).exists()) != 0 {
            return ObjectStatus::AlreadyExists.code();
        }
        let create = objects::create::MailboxCreate {
            attributes: MailboxAttributes(load(GuestAddress(request_pointer))),
            priority_limit: PriorityLimit(load(GuestAddress(request_pointer + 4))),
        };
        match create_mailbox(&Cpu, MailboxId { domain: ENTRY, slot }, create) {
            Ok(_) => 0,
            Err(status) => status.code(),
        }
    })();
    restore_lifecycle(saved);
    code
}

/// Slot `+0xd0`; the raw ABI discards the drained pointers as stock does.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_delete"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_delete(id: i32) -> i32 {
    let saved = mask_lifecycle();
    let code = (|| -> i32 {
        let words = entry_words();
        let slot = match own_mailbox_slot(EncodedId(id), load(words.context(ENTRY).mailbox_count())) {
            Ok(slot) => slot,
            Err(status) => return status.code(),
        };
        match delete_mailbox(&Cpu, MailboxId { domain: ENTRY, slot }) {
            Ok(drain) => {
                core::mem::drop(drain);
                0
            }
            Err(status) => status.code(),
        }
    })();
    restore_lifecycle(saved);
    code
}

/// Slot `+0xd4`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_send"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_send(id: i32, message: u32) -> i32 {
    let params = Params {
        pattern: Pattern(0),
        mode: 0,
        timeout: None,
        message: Some(Message(GuestAddress(message))),
    };
    let admitted = match admit_op::<{ Service::MailboxSend as u8 }, _, _>(
        &request(Service::MailboxSend, id, params),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match mailbox(&admitted) {
        Some(id) => {
            send(&Cpu, id, Message(GuestAddress(message)), admitted.priority);
            0
        }
        None => wrong_object(),
    }
}

fn receive_entry<const SERVICE: u8>(id: i32, message_out: u32, timeout: Option<i32>) -> i32 {
    let params = Params { pattern: Pattern(0), mode: 0, timeout, message: None };
    let admitted = match admit_op::<SERVICE, _, _>(
        &request(Service::from_u8(SERVICE), id, params),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match mailbox(&admitted) {
        Some(id) => id,
        None => wrong_object(),
    };
    let saved = Cpu.mask_i();
    let code = loop {
        match receive(
            &Cpu,
            EntryDomain::new(ENTRY),
            id,
            OutWord(GuestAddress(message_out)),
            admitted.timeout.unwrap_or(Timeout::Forever),
        ) {
            Ok(ReceiveBlocking::Ready(outcome)) => break outcome.code(),
            Ok(ReceiveBlocking::Pending(pending)) => {
                unsafe { xm1_rtos_scheduler_save(0) };
                break match resume_receive(&Cpu, pending) {
                    ReceiveResume::Completed(outcome) => outcome.code(),
                    ReceiveResume::Pending(_) => still_waiting(),
                    ReceiveResume::Corrupt(_) => unknown_result(),
                };
            }
            Ok(ReceiveBlocking::Retry) => {
                unmask_i();
                Cpu.mask_i();
            }
            Err(status) => break status.code(),
        }
    };
    Cpu.restore_i(saved);
    code
}

/// Slot `+0xd8`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_receive"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_receive(id: i32, message_out: u32) -> i32 {
    receive_entry::<{ Service::MailboxReceive as u8 }>(id, message_out, None)
}

/// Slot `+0xe0`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_timed_receive"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_timed_receive(id: i32, message_out: u32, timeout: i32) -> i32 {
    receive_entry::<{ Service::MailboxTimedReceive as u8 }>(id, message_out, Some(timeout))
}

/// Slot `+0xdc`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_poll"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_poll(id: i32, message_out: u32) -> i32 {
    let params = Params { pattern: Pattern(0), mode: 0, timeout: None, message: None };
    let admitted = match admit_op::<{ Service::MailboxPoll as u8 }, _, _>(
        &request(Service::MailboxPoll, id, params),
        &StageTables,
        &StageTables,
    ) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match mailbox(&admitted) {
        Some(id) => id,
        None => wrong_object(),
    };
    match poll_receive(&Cpu, id, OutWord(GuestAddress(message_out))) {
        Ok(outcome) => outcome.code(),
        Err(status) => status.code(),
    }
}
