use crate::create::Machine;
use crate::id::{FlagId, MailboxId};
use crate::layout::{Domain, FlagConfig, FlagWait, GuestAddress, MailboxControl, MailboxWait, Message, Task};
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

pub trait Wake: ObjectLock {
    fn with_scheduler_lock<R>(&self, domain: Domain, body: impl FnOnce() -> R) -> R;
    /// T+0x20.
    fn waiter_domain(&self, task: Task) -> Domain;
    /// A 0x44568 / B 0xd4528: cancel the waiter's timer, clear its result, then reset or insert it.
    fn release_wait(&self, owner: Domain, task: Task) -> Selection;
    /// A 0x44018 / B 0xd3fd4: ring IRQ 120 on the peer when the target is not this kernel.
    fn notify_dispatch(&self, owner: Domain) -> DispatchTarget;
    /// A 0x43640 / B 0xd3640: switch to the selected task.
    fn dispatch(&self);
}

fn unlink<M: Machine>(m: &M, task: Task) {
    let prev = m.load_word(task.prev());
    let next = m.load_word(task.next());
    m.store_word(GuestAddress(prev), next);
    m.store_word(GuestAddress(next).offset(0x04), prev);
}

fn dispatch_after<M: Wake>(m: &M, owner: Domain, released: Selection) -> bool {
    match released {
        Selection::Unchanged => false,
        Selection::Changed => m.notify_dispatch(owner) == DispatchTarget::Local,
    }
}

/// Slot `+0x88`.
pub fn set_flag<M: Wake>(m: &M, id: FlagId, bits: Pattern) {
    let words = id.domain.words();
    let control = words.flag_control(id.slot);
    let config = words.flag_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    let dispatch = m.with_object_lock(lock, || {
        let waiter = m.load_word(control.queue().next());
        if waiter == control.0 .0 {
            let pattern = m.load_word(control.pattern()) | bits.0;
            m.store_word(control.pattern(), pattern);
            return false;
        }
        let task = Task(GuestAddress(waiter));
        let owner = m.waiter_domain(task);
        m.with_scheduler_lock(owner, || {
            let pattern = m.load_word(control.pattern()) | bits.0;
            m.store_word(control.pattern(), pattern);
            let wait = FlagWait(m.load_word(task.wait()));
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
                    dispatch_after(m, owner, m.release_wait(owner, task))
                }
                None => false,
            }
        })
    });
    if dispatch {
        m.dispatch();
    }
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
    priority: u32,
) {
    let mut link = control.head();
    let mut next = m.load_word(control.head());
    while next != 0 {
        let entry = Message(GuestAddress(next));
        if priority < m.load_word(entry.priority()) {
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
    let dispatch = m.with_object_lock(lock, || {
        let waiter = m.load_word(control.queue().next());
        if waiter == control.0 .0 {
            match priority {
                Some(priority) => insert_by_priority(m, control, message, priority.get()),
                None => append_fifo(m, control, message),
            }
            return false;
        }
        let task = Task(GuestAddress(waiter));
        let owner = m.waiter_domain(task);
        let wait = MailboxWait(m.load_word(task.wait()));
        m.store_word(wait.message(), message.0 .0);
        m.with_scheduler_lock(owner, || {
            unlink(m, task);
            dispatch_after(m, owner, m.release_wait(owner, task))
        })
    });
    if dispatch {
        m.dispatch();
    }
}
