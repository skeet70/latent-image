use crate::create::Machine;
use crate::id::{EntryDomain, FlagId, MailboxId};
use crate::layout::{
    Domain, FlagConfig, GuestAddress, KernelContext, Message, Task, TaskState, WaitBlock, WaitQueue,
};
use crate::locked::ObjectLock;
use crate::matching::{check, MatchMode, Pattern, WantedMask};
use crate::order::QueueOrder;
use crate::status::{ObjectStatus, WaitOutcome};
use crate::timeout::Timeout;
use crate::wake::Wake;

/// The wake writes the result into the wait record after the task resumes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockOutcome<T> {
    Ready(WaitOutcome<T>),
    Suspended,
}

pub trait Block: Wake {
    fn current(&self) -> Task;
    fn remove_ready(&self, owner: KernelContext, task: Task);
    fn queue_wait(&self, owner: KernelContext, task: Task, queue: WaitQueue, order: QueueOrder);
    fn arm_timer(&self, owner: KernelContext, entry: GuestAddress, deadline: u32);
    fn now(&self, owner: KernelContext) -> u32;
}

fn block<M: Block>(
    m: &M,
    entry: Domain,
    object: Domain,
    task: Task,
    wait: GuestAddress,
    queue: WaitQueue,
    order: QueueOrder,
    timeout: Timeout,
) {
    let owner = m.context(object);
    m.with_scheduler_lock(entry, || {
        m.store_byte(
            task.state(),
            TaskState::WAIT
                .union(TaskState::OBJECT_QUEUED)
                .union(TaskState::OWNER_LOCK_QUEUED)
                .bits(),
        );
        m.remove_ready(m.context(entry), task);
        m.store_word(task.wait(), wait.0);
        m.store_word(wait, 0);
        m.store_word(WaitBlock(wait).queue(), queue.0 .0);
        m.store_word(WaitBlock(wait).owner(), owner.0 .0);
        m.queue_wait(owner, task, queue, order);
        if let Timeout::Units(units) = timeout {
            let waiter = m.waiter_domain(task);
            let waiter_context = m.context(waiter);
            let deadline = m.now(waiter_context).wrapping_add(units);
            m.arm_timer(waiter_context, GuestAddress(wait.0), deadline);
            m.store_word(wait, wait.0);
        }
    });
}

/// Slots `+0x94` and `+0x9c`.
pub fn wait_flag<M: Block>(
    m: &M,
    entry: EntryDomain,
    id: FlagId,
    want: WantedMask,
    mode: MatchMode,
    out: crate::locked::OutWord,
    timeout: Timeout,
) -> Result<BlockOutcome<Pattern>, ObjectStatus> {
    let words = id.domain.words();
    let control = words.flag_control(id.slot);
    let config = words.flag_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock(lock, || {
        if m.load_word(control.queue().next()) != control.0 .0 {
            return Err(ObjectStatus::QueueOverflow);
        }
        let mut pattern = Pattern(m.load_word(control.pattern()));
        let clear_on_match = m.load_word(config.attributes()) & FlagConfig::CLEAR_ON_MATCH != 0;
        match check(&mut pattern, want.pattern(), mode, clear_on_match) {
            Some(reported) => {
                m.store_word(out.0, reported.0);
                if clear_on_match {
                    m.store_word(control.pattern(), 0);
                }
                Ok(BlockOutcome::Ready(WaitOutcome::Satisfied(reported)))
            }
            None => match timeout {
                Timeout::Poll => Ok(BlockOutcome::Ready(WaitOutcome::TimedOut)),
                Timeout::Forever | Timeout::Units(_) => {
                    let task = m.current();
                    let slot = entry
                        .domain()
                        .words()
                        .task_slot(task)
                        .ok_or(ObjectStatus::Param)?;
                    let wait = entry.domain().words().flag_wait(slot);
                    m.store_word(wait.pattern(), want.pattern().0);
                    m.store_word(wait.mode(), if mode == MatchMode::Any { 1 } else { 0 });
                    block(
                        m,
                        entry.domain(),
                        id.domain,
                        task,
                        wait.0,
                        control.queue(),
                        QueueOrder::from_attribute(m.load_word(config.attributes())),
                        timeout,
                    );
                    Ok(BlockOutcome::Suspended)
                }
            },
        }
    })
}

/// Slots `+0xd8` and `+0xe0`.
pub fn receive<M: Block>(
    m: &M,
    entry: EntryDomain,
    id: MailboxId,
    out: crate::locked::OutWord,
    timeout: Timeout,
) -> Result<BlockOutcome<Message>, ObjectStatus> {
    let words = id.domain.words();
    let control = words.mailbox_control(id.slot);
    let config = words.mailbox_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock(lock, || {
        let head = m.load_word(control.head());
        if head != 0 {
            let message = Message(GuestAddress(head));
            let successor = m.load_word(message.next());
            m.store_word(out.0, head);
            m.store_word(control.head(), successor);
            return Ok(BlockOutcome::Ready(WaitOutcome::Satisfied(message)));
        }
        match timeout {
            Timeout::Poll => Ok(BlockOutcome::Ready(WaitOutcome::TimedOut)),
            Timeout::Forever | Timeout::Units(_) => {
                let task = m.current();
                let slot = entry
                    .domain()
                    .words()
                    .task_slot(task)
                    .ok_or(ObjectStatus::Param)?;
                let wait = entry.domain().words().mailbox_wait(slot);
                block(
                    m,
                    entry.domain(),
                    id.domain,
                    task,
                    wait.0,
                    control.queue(),
                    QueueOrder::from_attribute(m.load_word(config.attributes())),
                    timeout,
                );
                Ok(BlockOutcome::Suspended)
            }
        }
    })
}
