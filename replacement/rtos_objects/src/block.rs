use crate::create::Machine;
use crate::id::{EntryDomain, FlagId, MailboxId};
use crate::layout::{
    Domain, FlagConfig, FlagWait, GuestAddress, KernelContext, MailboxWait, Message, Task, TaskState,
    WaitBlock, WaitQueue,
};
use crate::locked::ObjectLock;
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
    fn current(&self) -> Task;
    fn remove_ready(&self, owner: KernelContext, task: Task);
    fn queue_wait(&self, owner: KernelContext, task: Task, queue: WaitQueue, order: QueueOrder);
    fn arm_timer(&self, owner: KernelContext, entry: TimerEntryAddress, deadline: Deadline);
    fn tick(&self, owner: KernelContext) -> Tick;
    fn timeout_callback(&self) -> TimeoutCallback;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pending {
    Flag { task: Task, wait: FlagWait },
    Mailbox { task: Task, wait: MailboxWait },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blocking<T> {
    Ready(WaitOutcome<T>),
    Pending(Pending),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResumeError {
    /// The wait block still carries the timer entry, so the wait has not completed.
    NotCompleted,
    WrongFamily,
}

/// The wake writes the wait block's completion word before the task becomes runnable; the
/// caller output is read by the resumed entry.
fn block<M: Block>(
    m: &M,
    entry: Domain,
    object: Domain,
    task: Task,
    wait: GuestAddress,
    queue: WaitQueue,
    order: QueueOrder,
    timer: Option<(TimerEntryAddress, Deadline)>,
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
) -> Result<Blocking<Pattern>, ObjectStatus> {
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
                Ok(Blocking::Ready(WaitOutcome::Satisfied(reported)))
            }
            None => match timeout {
                Timeout::Poll => Ok(Blocking::Ready(WaitOutcome::TimedOut)),
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
                    let timer = match timeout {
                        Timeout::Units(units) => Some((
                            TimerEntryAddress(entry.domain().words().flag_timer(slot)),
                            Deadline(m.tick(m.context(entry.domain())).0.wrapping_add(units)),
                        )),
                        _ => None,
                    };
                    block(
                        m,
                        entry.domain(),
                        id.domain,
                        task,
                        wait.0,
                        control.queue(),
                        QueueOrder::from_attribute(m.load_word(config.attributes())),
                        timer,
                    );
                    Ok(Blocking::Pending(Pending::Flag { task, wait }))
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
) -> Result<Blocking<Message>, ObjectStatus> {
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
            return Ok(Blocking::Ready(WaitOutcome::Satisfied(message)));
        }
        match timeout {
            Timeout::Poll => Ok(Blocking::Ready(WaitOutcome::TimedOut)),
            Timeout::Forever | Timeout::Units(_) => {
                let task = m.current();
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
                block(
                    m,
                    entry.domain(),
                    id.domain,
                    task,
                    wait.0,
                    control.queue(),
                    QueueOrder::from_attribute(m.load_word(config.attributes())),
                    timer,
                );
                Ok(Blocking::Pending(Pending::Mailbox { task, wait }))
            }
        }
    })
}

/// The wait block's word is 0 on a wake, -49 on a forced release and -50 on expiry.
pub fn resume_flag<M: Machine>(m: &M, pending: Pending) -> Result<WaitOutcome<Pattern>, ResumeError> {
    let Pending::Flag { wait, .. } = pending else {
        return Err(ResumeError::WrongFamily);
    };
    match m.load_word(wait.base().result()) as i32 {
        0 => Ok(WaitOutcome::Satisfied(Pattern(m.load_word(wait.result())))),
        -49 => Ok(WaitOutcome::Released),
        -50 => Ok(WaitOutcome::TimedOut),
        _ => Err(ResumeError::NotCompleted),
    }
}

/// The message pointer is the wait record's own field.
pub fn resume_receive<M: Machine>(m: &M, pending: Pending) -> Result<WaitOutcome<Message>, ResumeError> {
    let Pending::Mailbox { wait, .. } = pending else {
        return Err(ResumeError::WrongFamily);
    };
    match m.load_word(wait.base().result()) as i32 {
        0 => Ok(WaitOutcome::Satisfied(Message(GuestAddress(
            m.load_word(wait.message()),
        )))),
        -49 => Ok(WaitOutcome::Released),
        -50 => Ok(WaitOutcome::TimedOut),
        _ => Err(ResumeError::NotCompleted),
    }
}
