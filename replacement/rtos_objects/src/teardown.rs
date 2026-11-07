use crate::create::Machine;
use crate::id::{FlagId, MailboxId};
use crate::layout::{GuestAddress, Message, Task, WaitQueue};
use crate::locked::ObjectLock;
use crate::status::ObjectStatus;
use crate::wake::{release_with, unlink, DispatchTarget, Selection, WaitCompletion, Wake};

/// The queued messages of a deleted mailbox in delivery order; the messages are caller-owned.
pub struct MessageDrain<'a, M: Machine> {
    machine: &'a M,
    next: GuestAddress,
}

impl<'a, M: Machine> Iterator for MessageDrain<'a, M> {
    type Item = Message;

    fn next(&mut self) -> Option<Message> {
        if self.next.0 == 0 {
            return None;
        }
        let message = Message(self.next);
        self.next = GuestAddress(self.machine.load_word(message.next()));
        Some(message)
    }
}

#[cfg_attr(feature = "family", inline(never))]
fn release_all<M: Wake>(m: &M, queue: WaitQueue) -> bool {
    let mut dispatch = false;
    loop {
        let waiter = m.load_word(queue.next());
        if waiter == queue.0 .0 {
            break;
        }
        let task = Task(GuestAddress(waiter));
        let owner_domain = m.waiter_domain(task);
        let owner = m.context(owner_domain);
        let changed = m.with_scheduler_lock(owner_domain, || {
            unlink(m, task);
            match release_with(m, owner, task, WaitCompletion::Released) {
                Selection::Changed => m.notify(m.record_id(owner)) == DispatchTarget::Local,
                Selection::Unchanged => false,
            }
        });
        dispatch |= changed;
    }
    dispatch
}

/// Slot `+0x84`.
#[cfg_attr(feature = "family", inline(never))]
pub fn delete_flag<M: Wake>(m: &M, id: FlagId) -> Result<(), ObjectStatus> {
    let words = id.domain.words();
    let control = words.flag_control(id.slot);
    let config = words.flag_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock_after(
        lock,
        || {
            let dispatch = release_all(m, control.queue());
            m.store_word(config.attributes(), 0);
            m.store_word(config.initial(), 0);
            m.store_word(config.exists(), 0);
            m.store_word(control.queue().next(), control.0 .0);
            m.store_word(control.queue().prev(), control.0 .0);
            m.store_word(control.pattern(), 0);
            dispatch
        },
        |dispatch| if *dispatch { m.dispatch(0) },
    );
    Ok(())
}

/// Slot `+0xd0`.
#[cfg_attr(feature = "family", inline(never))]
pub fn delete_mailbox<'a, M: Wake>(
    m: &'a M,
    id: MailboxId,
) -> Result<MessageDrain<'a, M>, ObjectStatus> {
    let words = id.domain.words();
    let control = words.mailbox_control(id.slot);
    let config = words.mailbox_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    let mut head = GuestAddress(0);
    m.with_object_lock_after(
        lock,
        || {
            let dispatch = release_all(m, control.queue());
            head = GuestAddress(m.load_word(control.head()));
            m.store_word(config.attributes(), 2);
            m.store_word(config.priority_limit(), 32);
            m.store_word(config.exists(), 0);
            m.store_word(control.queue().next(), control.0 .0);
            m.store_word(control.queue().prev(), control.0 .0);
            m.store_word(control.head(), 0);
            m.store_word(control.tail(), 0);
            dispatch
        },
        |dispatch| if *dispatch { m.dispatch(0) },
    );
    Ok(MessageDrain { machine: m, next: head })
}
