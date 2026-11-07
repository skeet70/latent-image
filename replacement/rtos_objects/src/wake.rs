use crate::create::Machine;
use crate::id::{FlagId, MailboxId};
use crate::layout::{
    Domain, FlagConfig, FlagWait, GuestAddress, KernelContext, MailboxControl, MailboxWait, Message,
    Task, TaskFlags, TaskState,
};
use crate::locked::ObjectLock;
use crate::matching::{check, MatchMode, Pattern};
use crate::message::MessagePriority;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    Unchanged,
    Changed,
}

/// `Local` is this kernel; `Remote` rang the peer's IRQ 120.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispatchTarget {
    Local,
    Remote,
}

/// The release's primitives, keyed by the owner record the ABI supplies.
pub trait Release: Machine {
    fn context(&self, domain: Domain) -> KernelContext;
    fn reset_task(&self, task: Task);
    fn prepare(&self, owner: KernelContext, task: Task) -> Selection;
    fn insert(&self, owner: KernelContext, task: Task) -> Selection;
    fn remove_timer(&self, owner: KernelContext, entry: GuestAddress);
    fn release(&self, owner: KernelContext, task: Task) -> Selection;
}

pub trait Wake: ObjectLock + Release {
    fn with_scheduler_lock<R>(&self, domain: Domain, body: impl FnOnce() -> R) -> R;
    /// T+0x20.
    fn waiter_domain(&self, task: Task) -> Domain;
    /// The owner record's `+0x00` id word.
    fn record_id(&self, owner: KernelContext) -> u32;
    /// A `0x44018` / B `0xd3fd4`.
    fn notify(&self, id: u32) -> DispatchTarget;
    /// A `0x43640` / B `0xd3640`, with the stock body's code in r0.
    fn dispatch(&self, code: i32);
}

fn unlink<M: Machine>(m: &M, task: Task) {
    let prev = m.load_word(task.prev());
    let next = m.load_word(task.next());
    m.store_word(GuestAddress(prev), next);
    m.store_word(GuestAddress(next).offset(0x04), prev);
}

/// The stock wait release, A `0x44568` / B `0xd4528`, over the S1 primitives.
pub fn release<M: Release>(m: &M, owner: KernelContext, task: Task) -> Selection {
    let wait = GuestAddress(m.load_word(task.wait()));
    let entry = m.load_word(wait);
    if entry != 0 {
        m.remove_timer(owner, GuestAddress(entry));
    }
    m.store_word(wait, 0);
    if m.load_word(task.terminate_pending()) != 0 {
        m.reset_task(task);
        m.store_word(task.context_discarded(), 1);
        let flags = m.load_byte(task.flags());
        if flags & TaskFlags::QUEUED_ACTIVATION.0 == 0 {
            return Selection::Unchanged;
        }
        m.store_byte(task.flags(), flags & !TaskFlags::QUEUED_ACTIVATION.0);
        return m.prepare(owner, task);
    }
    if m.load_word(task.priority_change_pending()) != 0 {
        m.store_byte(task.priority(), m.load_word(task.pending_priority()) as u8);
        m.store_word(task.priority_change_pending(), 0);
    }
    m.store_word(task.cancel_pending(), 0);
    if m.load_byte(task.state()) & TaskState::SUSPENDED.bits() != 0 {
        m.store_byte(task.state(), TaskState::SUSPENDED.bits());
        return Selection::Unchanged;
    }
    m.insert(owner, task)
}

fn dispatch_after<M: Wake>(m: &M, owner: KernelContext, released: Selection) -> bool {
    match released {
        Selection::Unchanged => false,
        Selection::Changed => m.notify(m.record_id(owner)) == DispatchTarget::Local,
    }
}

/// Slot `+0x88`.
pub fn set_flag<M: Wake>(m: &M, id: FlagId, bits: Pattern) {
    let words = id.domain.words();
    let control = words.flag_control(id.slot);
    let config = words.flag_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock_after(lock, || {
        let waiter = m.load_word(control.queue().next());
        if waiter == control.0 .0 {
            let pattern = m.load_word(control.pattern()) | bits.0;
            m.store_word(control.pattern(), pattern);
            return false;
        }
        let task = Task(GuestAddress(waiter));
        let owner_domain = m.waiter_domain(task);
        let owner = m.context(owner_domain);
        m.with_scheduler_lock(owner_domain, || {
            let pattern = m.load_word(control.pattern()) | bits.0;
            m.store_word(control.pattern(), pattern);
            let wait = FlagWait(GuestAddress(m.load_word(task.wait())));
            let want = Pattern(m.load_word(wait.pattern()));
            let mode = MatchMode::from_stored(m.load_word(wait.mode()));
            let mut current = Pattern(pattern);
            match check(&mut current, want, mode, false) {
                Some(reported) => {
                    let clear_on_match =
                        m.load_word(config.attributes()) & FlagConfig::CLEAR_ON_MATCH != 0;
                    m.store_word(wait.result(), reported.0);
                    if clear_on_match {
                        m.store_word(control.pattern(), 0);
                    }
                    unlink(m, task);
                    dispatch_after(m, owner, m.release(owner, task))
                }
                None => false,
            }
        })
    }, |dispatch| if *dispatch { m.dispatch(-43) });
}

fn append_fifo<M: Machine>(m: &M, control: MailboxControl, message: Message) {
    m.store_word(message.next(), 0);
    let head = m.load_word(control.head());
    if head != 0 {
        let tail = m.load_word(control.tail());
        m.store_word(Message(GuestAddress(tail)).next(), message.0 .0);
    } else {
        m.store_word(control.head(), message.0 .0);
    }
    m.store_word(control.tail(), message.0 .0);
}

fn insert_by_priority<M: Machine>(
    m: &M,
    control: MailboxControl,
    message: Message,
    priority: MessagePriority,
) {
    let mut link = control.head();
    let mut next = m.load_word(control.head());
    while next != 0 {
        let entry = Message(GuestAddress(next));
        if priority.get() < m.load_word(entry.priority()) {
            break;
        }
        link = entry.next();
        next = m.load_word(entry.next());
    }
    m.store_word(message.next(), next);
    m.store_word(link, message.0 .0);
}

/// Slot `+0xd4`.
pub fn send<M: Wake>(m: &M, id: MailboxId, message: Message, priority: Option<MessagePriority>) {
    let words = id.domain.words();
    let control = words.mailbox_control(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock_after(lock, || {
        let waiter = m.load_word(control.queue().next());
        if waiter == control.0 .0 {
            match priority {
                Some(priority) => insert_by_priority(m, control, message, priority),
                None => append_fifo(m, control, message),
            }
            return false;
        }
        let task = Task(GuestAddress(waiter));
        let owner_domain = m.waiter_domain(task);
        let owner = m.context(owner_domain);
        let wait = MailboxWait(GuestAddress(m.load_word(task.wait())));
        m.store_word(wait.message(), message.0 .0);
        m.with_scheduler_lock(owner_domain, || {
            unlink(m, task);
            dispatch_after(m, owner, m.release(owner, task))
        })
    }, |dispatch| if *dispatch { m.dispatch(-63) });
}
