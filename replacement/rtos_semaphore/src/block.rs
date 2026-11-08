use crate::count::Count;
use crate::id::{EntryDomain, SemaphoreId};
use crate::layout::{
    Domain, GuestAddress, KernelContext, Task, TaskDefinition, TaskSlot, TaskState, WaitBlock,
    WaitQueue,
};
use crate::locks::{current, running_ready, Locks};
use crate::status::{SemaphoreStatus, WaitOutcome};
use crate::timeout::Timeout;
use crate::wake::Wake;

/// A per-task timer entry address (`g_wait_timers`, `struct timer_entry`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerEntryAddress(pub GuestAddress);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Enqueue {
    Plain,
    Timed {
        timer: TimerEntryAddress,
        timeout: i32,
    },
}

pub trait Block: Locks + Wake {
    /// The stock wait-queue enqueue, A `0x44800`/`0x448e4` and B `0xd47c0`/`0xd48a4`.
    fn enqueue(
        &self,
        entry: Domain,
        owner: KernelContext,
        queue: WaitQueue,
        wait: WaitBlock,
        how: Enqueue,
    );
}

#[derive(Debug, PartialEq, Eq)]
pub struct SemaphorePending {
    pub task: Task,
    pub wait: WaitBlock,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SemaphoreBlocking {
    Ready(WaitOutcome),
    Pending(SemaphorePending),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SemaphoreResume {
    Completed(WaitOutcome),
    Pending(SemaphorePending),
    Corrupt(SemaphorePending),
}

/// Slot `+0x78`.
#[inline(always)]
pub fn poll<M: Locks>(m: &M, id: SemaphoreId) -> Result<WaitOutcome, SemaphoreStatus> {
    let words = id.domain.words();
    let control = words.semaphore_control(id.slot);
    let object_lock = words.context(id.domain).object_lock();
    m.disable_irq();
    m.lock(object_lock);
    let count = Count::from_word(m.load_word(control.count()));
    let outcome = if count.get() == 0 {
        WaitOutcome::TimedOut
    } else {
        m.store_word(control.count(), count.decrement().get());
        WaitOutcome::Acquired
    };
    m.unlock(object_lock);
    m.enable_irq();
    Ok(outcome)
}

/// Slot `+0x74`.
#[inline(always)]
pub fn wait<M: Block>(
    m: &M,
    entry: EntryDomain,
    id: SemaphoreId,
) -> Result<SemaphoreBlocking, SemaphoreStatus> {
    block(m, entry, id, 0, false)
}

/// Slot `+0x7c`.
#[inline(always)]
pub fn timed_wait<M: Block>(
    m: &M,
    entry: EntryDomain,
    id: SemaphoreId,
    timeout: Timeout,
) -> Result<SemaphoreBlocking, SemaphoreStatus> {
    block(m, entry, id, timeout.raw(), true)
}

/// The shared body of `+0x74` and `+0x7c`.
#[inline(always)]
pub fn block<M: Block>(
    m: &M,
    entry: EntryDomain,
    id: SemaphoreId,
    timeout: i32,
    timed: bool,
) -> Result<SemaphoreBlocking, SemaphoreStatus> {
    let words = id.domain.words();
    let control = words.semaphore_control(id.slot);
    let object_lock = words.context(id.domain).object_lock();
    let entry_words = entry.domain().words();
    let scheduler = entry_words.context(entry.domain());
    let scheduler_lock = scheduler.scheduler_lock();
    let slot = running_slot(m, scheduler)?;
    m.disable_irq();
    loop {
        m.lock(object_lock);
        let count = Count::from_word(m.load_word(control.count()));
        if count.get() != 0 {
            m.store_word(control.count(), count.decrement().get());
            m.unlock(object_lock);
            m.enable_irq();
            return Ok(SemaphoreBlocking::Ready(WaitOutcome::Acquired));
        }
        if timed && timeout == 0 {
            m.unlock(object_lock);
            m.enable_irq();
            return Ok(SemaphoreBlocking::Ready(WaitOutcome::TimedOut));
        }
        m.lock(scheduler_lock);
        if running_ready(m, scheduler) {
            break;
        }
        m.unlock(scheduler_lock);
        m.unlock(object_lock);
        m.enable_irq();
        m.disable_irq();
    }
    let task = current(m, scheduler);
    let wait = entry_words.semaphore_wait(slot);
    let how = if timed {
        Enqueue::Timed {
            timer: TimerEntryAddress(entry_words.semaphore_timer(slot)),
            timeout,
        }
    } else {
        Enqueue::Plain
    };
    m.enqueue(entry.domain(), m.context(id.domain), control.queue(), wait, how);
    m.unlock(scheduler_lock);
    m.unlock(object_lock);
    m.dispatch(if timed { -39 } else { -37 });
    m.enable_irq();
    Ok(SemaphoreBlocking::Pending(SemaphorePending { task, wait }))
}

#[inline(always)]
pub fn resume<M: crate::create::Machine>(m: &M, pending: SemaphorePending) -> SemaphoreResume {
    if m.load_byte(pending.task.state()) & TaskState::WAIT.bits() != 0 {
        return SemaphoreResume::Pending(pending);
    }
    match m.load_word(pending.wait.result()) as i32 {
        0 => SemaphoreResume::Completed(WaitOutcome::Acquired),
        -49 => SemaphoreResume::Completed(WaitOutcome::Released),
        -50 => SemaphoreResume::Completed(WaitOutcome::TimedOut),
        _ => SemaphoreResume::Corrupt(pending),
    }
}

#[inline(always)]
fn running_slot<M: crate::create::Machine>(
    m: &M,
    scheduler: KernelContext,
) -> Result<TaskSlot, SemaphoreStatus> {
    let task = current(m, scheduler);
    let definition = TaskDefinition(GuestAddress(m.load_word(task.definition())));
    TaskSlot::new(m.load_word(definition.slot())).ok_or(SemaphoreStatus::InvalidId)
}
