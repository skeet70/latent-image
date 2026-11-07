use super::{cpsr_masked, entry_words, load, target, xm1_rtos_scheduler_save, Cpu, Reschedule, ENTRY};
use objects::block::{
    receive, resume_flag, resume_receive, wait_flag, Block, Blocking, Deadline, FlagBlocking, FlagResume,
    FlagWaitPending, IrqScope, ReceiveBlocking, ReceivePending, ReceiveResume, SavedMask, Tick,
    TimerEntryAddress, TimeoutCallback,
};
use objects::create::{create_flag, create_mailbox, FlagAttributes, MailboxAttributes};
use objects::id::{own_flag_slot, own_mailbox_slot, resolve_flag, resolve_mailbox, EncodedId, EntryDomain, FlagId, MailboxId};
use objects::layout::{Domain, GuestAddress, Message, Task};
use objects::locked::{clear_flag, poll_flag, poll_receive, OutWord};
use objects::matching::{KeepMask, MatchMode, Pattern, WantedMask};
use objects::message::PriorityLimit;
use objects::status::{ObjectStatus, WaitOutcome};
use objects::teardown::{delete_flag, delete_mailbox};
use objects::timeout::Timeout;
use objects::wake::{send, set_flag, set_flag_handler};
use objects::admission::{admit, Entry, Facts, Object, Params, Records, Request, Service};

extern "C" {
    fn xm1_rtos_scheduler_remove(record: objects::layout::KernelContext, task: Task) -> Reschedule;
    fn xm1_rtos_objects_stock_insert_timer(record: objects::layout::KernelContext, entry: GuestAddress, expiry: u32);
    static xm1_rtos_objects_stock_timeout: u32;
}

/// The S2 save returns only after a completion; a still-waiting or corrupt record is an invariant
/// violation.
const INVARIANT: i32 = -25;

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

fn facts_flag(id: i32) -> Facts {
    let words = entry_words();
    let resolved = target(ENTRY, id >> 16).and_then(|domain| {
        resolve_flag(EntryDomain::new(ENTRY), EncodedId(id), load(words.context(ENTRY).event_flag_count()))
            .map(|flag| (domain, flag))
    });
    match resolved {
        Ok((domain, flag)) => {
            let config = domain.words().flag_config(flag.slot);
            Facts { exists: load(config.exists()) != 0, attributes: load(config.attributes()), priority_limit: 0 }
        }
        Err(_) => Facts { exists: false, attributes: 0, priority_limit: 0 },
    }
}

fn facts_mailbox(id: i32) -> Facts {
    let words = entry_words();
    let resolved = target(ENTRY, id >> 16).and_then(|domain| {
        resolve_mailbox(EntryDomain::new(ENTRY), EncodedId(id), load(words.context(ENTRY).mailbox_count()))
            .map(|mailbox| (domain, mailbox))
    });
    match resolved {
        Ok((domain, mailbox)) => {
            let config = domain.words().mailbox_config(mailbox.slot);
            Facts {
                exists: load(config.exists()) != 0,
                attributes: load(config.attributes()),
                priority_limit: load(config.priority_limit()),
            }
        }
        Err(_) => Facts { exists: false, attributes: 0, priority_limit: 0 },
    }
}

fn request(service: Service, id: i32, params: Params, facts: Facts) -> Request {
    let words = entry_words();
    let context = words.context(ENTRY);
    Request {
        service,
        entry: Entry {
            handler_context: load(words.handler_context),
            irq_masked: cpsr_masked(),
            dispatch_enabled: load(GuestAddress(load(context.dispatch_enabled()))),
        },
        entry_domain: EntryDomain::new(ENTRY),
        id: EncodedId(id),
        records: Records {
            flags: load(context.event_flag_count()),
            mailboxes: load(context.mailbox_count()),
        },
        facts,
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

fn blocking_flag(result: Result<FlagBlocking, ObjectStatus>) -> i32 {
    match result {
        Ok(FlagBlocking::Ready(outcome)) => outcome.code(),
        Ok(FlagBlocking::Pending(pending)) => {
            unsafe { xm1_rtos_scheduler_save(0) };
            match resume_flag(&Cpu, pending) {
                FlagResume::Completed(outcome) => outcome.code(),
                FlagResume::Pending(_) | FlagResume::Corrupt(_) => INVARIANT,
            }
        }
        Err(status) => status.code(),
    }
}

/// Slot `+0x80`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_create"]
pub extern "C" fn xm1_rtos_objects_family_flag_create(id: i32, request_pointer: u32) -> i32 {
    let words = entry_words();
    let slot = match own_flag_slot(EncodedId(id), load(words.context(ENTRY).event_flag_count())) {
        Ok(slot) => slot,
        Err(status) => return status.code(),
    };
    let create = objects::create::FlagCreate {
        attributes: FlagAttributes(load(GuestAddress(request_pointer))),
        initial: Pattern(load(GuestAddress(request_pointer + 4))),
    };
    match create_flag(&Cpu, FlagId { domain: ENTRY, slot }, create) {
        Ok(_) => 0,
        Err(status) => status.code(),
    }
}

/// Slot `+0x84`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_delete"]
pub extern "C" fn xm1_rtos_objects_family_flag_delete(id: i32) -> i32 {
    let words = entry_words();
    let slot = match own_flag_slot(EncodedId(id), load(words.context(ENTRY).event_flag_count())) {
        Ok(slot) => slot,
        Err(status) => return status.code(),
    };
    match delete_flag(&Cpu, FlagId { domain: ENTRY, slot }) {
        Ok(()) => 0,
        Err(status) => status.code(),
    }
}

/// Slot `+0x88`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_set"]
pub extern "C" fn xm1_rtos_objects_family_flag_set(id: i32, bits: u32) -> i32 {
    let admitted = match admit(&request(Service::FlagSet, id, Params::NONE, facts_flag(id))) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match flag(&admitted) {
        Some(id) => {
            set_flag(&Cpu, id, Pattern(bits));
            0
        }
        None => INVARIANT,
    }
}

/// Slot `+0x8c`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_set_handler"]
pub extern "C" fn xm1_rtos_objects_family_flag_set_handler(id: i32, bits: u32) -> i32 {
    let admitted = match admit(&request(Service::FlagSetHandler, id, Params::NONE, facts_flag(id))) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match flag(&admitted) {
        Some(id) => {
            set_flag_handler(&Cpu, id, Pattern(bits));
            0
        }
        None => INVARIANT,
    }
}

/// Slot `+0x90`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_clear"]
pub extern "C" fn xm1_rtos_objects_family_flag_clear(id: i32, keep: u32) -> i32 {
    let admitted = match admit(&request(Service::FlagClear, id, Params::NONE, facts_flag(id))) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match flag(&admitted) {
        Some(id) => match clear_flag(&Cpu, id, KeepMask(keep)) {
            Ok(()) => 0,
            Err(status) => status.code(),
        },
        None => INVARIANT,
    }
}

fn flag_wait_entry(id: i32, pattern: u32, mode: u32, result: u32, timeout: Option<i32>, service: Service) -> i32 {
    let params = Params { pattern: Pattern(pattern), mode, timeout, message_priority: None };
    let admitted = match admit(&request(service, id, params, facts_flag(id))) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match flag(&admitted) {
        Some(id) => id,
        None => return INVARIANT,
    };
    let wanted = match want(&admitted) {
        Ok(wanted) => wanted,
        Err(status) => return status.code(),
    };
    blocking_flag(wait_flag(
        &Cpu,
        EntryDomain::new(ENTRY),
        id,
        wanted,
        admitted.mode,
        OutWord(GuestAddress(result)),
        admitted.timeout.unwrap_or(Timeout::Forever),
    ))
}

/// Slot `+0x94`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_wait"]
pub extern "C" fn xm1_rtos_objects_family_flag_wait(id: i32, pattern: u32, mode: u32, result: u32) -> i32 {
    flag_wait_entry(id, pattern, mode, result, None, Service::FlagWait)
}

/// Slot `+0x9c`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_timed_wait"]
pub extern "C" fn xm1_rtos_objects_family_flag_timed_wait(id: i32, pattern: u32, mode: u32, result: u32, timeout: i32) -> i32 {
    flag_wait_entry(id, pattern, mode, result, Some(timeout), Service::FlagTimedWait)
}

/// Slot `+0x98`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_flag_poll"]
pub extern "C" fn xm1_rtos_objects_family_flag_poll(id: i32, pattern: u32, mode: u32, result: u32) -> i32 {
    let params = Params { pattern: Pattern(pattern), mode, timeout: None, message_priority: None };
    let admitted = match admit(&request(Service::FlagPoll, id, params, facts_flag(id))) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match flag(&admitted) {
        Some(id) => id,
        None => return INVARIANT,
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
    let words = entry_words();
    let slot = match own_mailbox_slot(EncodedId(id), load(words.context(ENTRY).mailbox_count())) {
        Ok(slot) => slot,
        Err(status) => return status.code(),
    };
    let create = objects::create::MailboxCreate {
        attributes: MailboxAttributes(load(GuestAddress(request_pointer))),
        priority_limit: PriorityLimit(load(GuestAddress(request_pointer + 4))),
    };
    match create_mailbox(&Cpu, MailboxId { domain: ENTRY, slot }, create) {
        Ok(_) => 0,
        Err(status) => status.code(),
    }
}

/// Slot `+0xd0`; the raw ABI discards the drained pointers as stock does.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_delete"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_delete(id: i32) -> i32 {
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
}

/// Slot `+0xd4`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_send"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_send(id: i32, message: u32) -> i32 {
    let facts = facts_mailbox(id);
    let priority = if facts.attributes & objects::layout::MailboxConfig::MESSAGE_PRIORITY != 0 { Some(load(GuestAddress(message + 4))) } else { None };
    let params = Params { pattern: Pattern(0), mode: 0, timeout: None, message_priority: priority };
    let admitted = match admit(&request(Service::MailboxSend, id, params, facts)) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    match mailbox(&admitted) {
        Some(id) => {
            send(&Cpu, id, Message(GuestAddress(message)), admitted.priority);
            0
        }
        None => INVARIANT,
    }
}

fn receive_entry(id: i32, message_out: u32, timeout: Option<i32>, service: Service) -> i32 {
    let params = Params { pattern: Pattern(0), mode: 0, timeout, message_priority: None };
    let admitted = match admit(&request(service, id, params, facts_mailbox(id))) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match mailbox(&admitted) {
        Some(id) => id,
        None => return INVARIANT,
    };
    match receive(
        &Cpu,
        EntryDomain::new(ENTRY),
        id,
        OutWord(GuestAddress(message_out)),
        admitted.timeout.unwrap_or(Timeout::Forever),
    ) {
        Ok(ReceiveBlocking::Ready(outcome)) => outcome.code(),
        Ok(ReceiveBlocking::Pending(pending)) => {
            unsafe { xm1_rtos_scheduler_save(0) };
            match resume_receive(&Cpu, pending) {
                ReceiveResume::Completed(outcome) => outcome.code(),
                ReceiveResume::Pending(_) | ReceiveResume::Corrupt(_) => INVARIANT,
            }
        }
        Err(status) => status.code(),
    }
}

/// Slot `+0xd8`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_receive"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_receive(id: i32, message_out: u32) -> i32 {
    receive_entry(id, message_out, None, Service::MailboxReceive)
}

/// Slot `+0xe0`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_timed_receive"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_timed_receive(id: i32, message_out: u32, timeout: i32) -> i32 {
    receive_entry(id, message_out, Some(timeout), Service::MailboxTimedReceive)
}

/// Slot `+0xdc`.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.family_mailbox_poll"]
pub extern "C" fn xm1_rtos_objects_family_mailbox_poll(id: i32, message_out: u32) -> i32 {
    let params = Params { pattern: Pattern(0), mode: 0, timeout: None, message_priority: None };
    let admitted = match admit(&request(Service::MailboxPoll, id, params, facts_mailbox(id))) {
        Ok(admitted) => admitted,
        Err(status) => return status.code(),
    };
    let id = match mailbox(&admitted) {
        Some(id) => id,
        None => return INVARIANT,
    };
    match poll_receive(&Cpu, id, OutWord(GuestAddress(message_out))) {
        Ok(outcome) => outcome.code(),
        Err(status) => status.code(),
    }
}
