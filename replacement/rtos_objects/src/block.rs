use crate::create::Machine;
use crate::id::{EntryDomain, FlagId, MailboxId};
use crate::layout::{
    Domain, FlagConfig, FlagWait, GuestAddress, KernelContext, MailboxWait, Message, Task, TaskState,
    WaitBlock, WaitQueue,
};
use crate::locked::OutWord;
use crate::matching::{check, MatchMode, Pattern, WantedMask};
use crate::order::QueueOrder;
use crate::status::{ObjectStatus, WaitOutcome};
use crate::timeout::Timeout;
use crate::wake::Wake;

/// The per-task timer entry (`g_flag_timers`/`g_mailbox_timers`, `struct timer_entry`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerEntryAddress(pub GuestAddress);

/// An absolute expiry in kernel clock units, `timeout + *context->tick`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Deadline(pub u32);

/// The executing context's clock, `*context->tick`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tick(pub u32);

/// The stock timeout callback stored into the entry, `kernel_timeout_task`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeoutCallback(pub u32);

const TIMER_ENTRY_CALLBACK: u32 = 0x04;
const TIMER_ENTRY_ARGUMENT: u32 = 0x08;

pub trait Block: Wake {
    fn current(&self, entry: Domain) -> Task;
    fn remove_ready(&self, owner: KernelContext, task: Task);
    fn queue_wait(&self, owner: KernelContext, task: Task, queue: WaitQueue, order: QueueOrder);
    fn arm_timer(&self, owner: KernelContext, entry: TimerEntryAddress, deadline: Deadline);
    fn tick(&self, owner: KernelContext) -> Tick;
    fn timeout_callback(&self) -> TimeoutCallback;
}

/// The resume tail's I-only mask; the F1a creation mask pair is a separate contract.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SavedMask(pub u32);

pub trait IrqScope: Machine {
    fn mask_i(&self) -> SavedMask;
    fn restore_i(&self, saved: SavedMask);
}

/// A single-use continuation: the armed wait and the caller's output word.
#[derive(Debug, PartialEq, Eq)]
pub struct FlagWaitPending {
    task: Task,
    wait: FlagWait,
    out: OutWord,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReceivePending {
    task: Task,
    wait: MailboxWait,
    out: OutWord,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FlagBlocking {
    Ready(WaitOutcome<Pattern>),
    Pending(FlagWaitPending),
    /// The running task was not READY; the caller drops the object lock and retries.
    Retry,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveBlocking {
    Ready(WaitOutcome<Message>),
    Pending(ReceivePending),
    /// The running task was not READY; the caller drops the object lock and retries.
    Retry,
}

/// The wake writes the wait block's completion word before the task becomes runnable; the
/// caller output is written by the resumed entry.
fn block<M: Block>(
    m: &M,
    entry: Domain,
    object: Domain,
    task: Task,
    wait: GuestAddress,
    queue: WaitQueue,
    order: QueueOrder,
    timer: Option<(TimerEntryAddress, Deadline)>,
) -> bool {
    let owner = m.context(object);
    m.with_scheduler_lock(entry, || {
        if m.load_byte(task.state()) & TaskState::READY.bits() == 0 {
            return false;
        }
        m.store_byte(
            task.state(),
            TaskState::WAIT
                .union(TaskState::OBJECT_QUEUED)
                .union(TaskState::OWNER_LOCK_QUEUED)
                .bits(),
        );
        m.remove_ready(m.context(entry), task);
        m.store_word(task.wait(), wait.0);
        match timer {
            Some((timer_entry, deadline)) => {
                m.store_word(wait, timer_entry.0 .0);
                m.store_word(timer_entry.0.offset(TIMER_ENTRY_CALLBACK), m.timeout_callback().0);
                m.store_word(timer_entry.0.offset(TIMER_ENTRY_ARGUMENT), task.0 .0);
                m.arm_timer(m.context(entry), timer_entry, deadline);
            }
            None => m.store_word(wait, 0),
        }
        m.store_word(WaitBlock(wait).queue(), queue.0 .0);
        m.store_word(WaitBlock(wait).owner(), owner.0 .0);
        m.queue_wait(owner, task, queue, order);
        true
    })
}

/// Slots `+0x94` and `+0x9c`.
pub fn wait_flag<M: Block>(
    m: &M,
    entry: EntryDomain,
    id: FlagId,
    want: WantedMask,
    mode: MatchMode,
    out: OutWord,
    timeout: Timeout,
) -> Result<FlagBlocking, ObjectStatus> {
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
                if clear_on_match {
                    m.store_word(control.pattern(), 0);
                }
                m.store_word(out.0, reported.0);
                Ok(FlagBlocking::Ready(WaitOutcome::Satisfied(reported)))
            }
            None => match timeout {
                Timeout::Poll => Ok(FlagBlocking::Ready(WaitOutcome::TimedOut)),
                Timeout::Forever | Timeout::Units(_) => {
                    let task = m.current(entry.domain());
                    let slot = entry
                        .domain()
                        .words()
                        .task_slot(task)
                        .ok_or(ObjectStatus::Param)?;
                    let wait = entry.domain().words().flag_wait(slot);
                    m.store_word(wait.pattern(), want.pattern().0);
                    m.store_word(wait.mode(), if mode == MatchMode::Any { 1 } else { 0 });
                    let timer = match timeout {
                        Timeout::Units(units) => Some((
                            TimerEntryAddress(entry.domain().words().flag_timer(slot)),
                            Deadline(m.tick(m.context(entry.domain())).0.wrapping_add(units)),
                        )),
                        _ => None,
                    };
                    if !block(
                        m,
                        entry.domain(),
                        id.domain,
                        task,
                        wait.0,
                        control.queue(),
                        QueueOrder::from_attribute(m.load_word(config.attributes())),
                        timer,
                    ) {
                        return Ok(FlagBlocking::Retry);
                    }
                    Ok(FlagBlocking::Pending(FlagWaitPending { task, wait, out }))
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
    out: OutWord,
    timeout: Timeout,
) -> Result<ReceiveBlocking, ObjectStatus> {
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
            return Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(message)));
        }
        match timeout {
            Timeout::Poll => Ok(ReceiveBlocking::Ready(WaitOutcome::TimedOut)),
            Timeout::Forever | Timeout::Units(_) => {
                let task = m.current(entry.domain());
                let slot = entry
                    .domain()
                    .words()
                    .task_slot(task)
                    .ok_or(ObjectStatus::Param)?;
                let wait = entry.domain().words().mailbox_wait(slot);
                let timer = match timeout {
                    Timeout::Units(units) => Some((
                        TimerEntryAddress(entry.domain().words().mailbox_timer(slot)),
                        Deadline(m.tick(m.context(entry.domain())).0.wrapping_add(units)),
                    )),
                    _ => None,
                };
                if !block(
                    m,
                    entry.domain(),
                    id.domain,
                    task,
                    wait.0,
                    control.queue(),
                    QueueOrder::from_attribute(m.load_word(config.attributes())),
                    timer,
                ) {
                    return Ok(ReceiveBlocking::Retry);
                }
                Ok(ReceiveBlocking::Pending(ReceivePending { task, wait, out }))
            }
        }
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum FlagResume {
    Completed(WaitOutcome<Pattern>),
    Pending(FlagWaitPending),
    Corrupt(FlagWaitPending),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveResume {
    Completed(WaitOutcome<Message>),
    Pending(ReceivePending),
    Corrupt(ReceivePending),
}

pub fn resume_flag<M: Machine + IrqScope>(m: &M, pending: FlagWaitPending) -> FlagResume {
    if m.load_byte(pending.task.state()) & TaskState::WAIT.bits() != 0 {
        return FlagResume::Pending(pending);
    }
    match m.load_word(pending.wait.base().result()) as i32 {
        0 => {
            let pattern = Pattern(m.load_word(pending.wait.result()));
            let saved = m.mask_i();
            m.store_word(pending.out.0, pattern.0);
            m.restore_i(saved);
            FlagResume::Completed(WaitOutcome::Satisfied(pattern))
        }
        -49 => FlagResume::Completed(WaitOutcome::Released),
        -50 => FlagResume::Completed(WaitOutcome::TimedOut),
        _ => FlagResume::Corrupt(pending),
    }
}

pub fn resume_receive<M: Machine + IrqScope>(m: &M, pending: ReceivePending) -> ReceiveResume {
    if m.load_byte(pending.task.state()) & TaskState::WAIT.bits() != 0 {
        return ReceiveResume::Pending(pending);
    }
    match m.load_word(pending.wait.base().result()) as i32 {
        0 => {
            let message = Message(GuestAddress(m.load_word(pending.wait.message())));
            let saved = m.mask_i();
            m.store_word(pending.out.0, message.0 .0);
            m.restore_i(saved);
            ReceiveResume::Completed(WaitOutcome::Satisfied(message))
        }
        -49 => ReceiveResume::Completed(WaitOutcome::Released),
        -50 => ReceiveResume::Completed(WaitOutcome::TimedOut),
        _ => ReceiveResume::Corrupt(pending),
    }
}
