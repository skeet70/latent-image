use crate::id::{EntryDomain, PoolId};
use crate::layout::{GuestAddress, Task, TaskSlot, TaskState};
use crate::machine::{Block, Enqueue, Irq, Locks, Machine, SaveIrq};
use crate::status::{PoolStatus, WaitOutcome};
use crate::take::{take, BlockAddress};
use crate::timeout::Timeout;

/// A caller-supplied word the body writes on success.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct OutWord(pub GuestAddress);

#[derive(Debug, PartialEq, Eq)]
pub struct Pending {
    pub task: Task,
    pub wait: crate::layout::FixedPoolWait,
    pub out: OutWord,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PoolBlocking {
    Ready(WaitOutcome<BlockAddress>),
    Pending(Pending),
}

#[derive(Debug, PartialEq, Eq)]
pub enum PoolResume {
    Completed(WaitOutcome<BlockAddress>),
    Pending(Pending),
    Corrupt(Pending),
}

/// Slot `+0xf0`.
#[inline(never)]
#[cfg_attr(target_os = "none", link_section = ".text.fp_body_poll")]
pub fn poll<M: Machine + Locks + Irq>(
    m: &M,
    id: PoolId,
    out: OutWord,
) -> Result<WaitOutcome<BlockAddress>, PoolStatus> {
    let words = id.domain.words();
    let lock = words.context(id.domain).object_lock();
    m.disable_irq();
    m.lock(lock);
    let result = match take(m, id, out.0) {
        Some(block) => WaitOutcome::Satisfied(block),
        None => WaitOutcome::TimedOut,
    };
    m.unlock(lock);
    m.enable_irq();
    Ok(result)
}

/// Slots `+0xec` and `+0xf4`. The output word is written only on success.
#[inline(never)]
#[cfg_attr(target_os = "none", link_section = ".text.fp_body_get")]
pub fn get<M: Block>(
    m: &M,
    entry: EntryDomain,
    id: PoolId,
    out: OutWord,
    timeout: Timeout,
) -> Result<PoolBlocking, PoolStatus> {
    let words = id.domain.words();
    let control = words.fixed_pool_control(id.slot);
    let object_lock = words.context(id.domain).object_lock();
    let entry_words = entry.domain().words();
    let scheduler = entry_words.context(entry.domain());
    let scheduler_lock = scheduler.scheduler_lock();
    let slot = running_slot(m, entry.domain(), scheduler)?;
    m.disable_irq();
    loop {
        m.lock(object_lock);
        if let Some(block) = take(m, id, out.0) {
            m.unlock(object_lock);
            m.enable_irq();
            return Ok(PoolBlocking::Ready(WaitOutcome::Satisfied(block)));
        }
        if timeout == Timeout::Poll {
            m.unlock(object_lock);
            m.enable_irq();
            return Ok(PoolBlocking::Ready(WaitOutcome::TimedOut));
        }
        m.lock(scheduler_lock);
        if m.load_byte(current(m, scheduler).state()) & TaskState::READY.bits() != 0 {
            break;
        }
        m.unlock(scheduler_lock);
        m.unlock(object_lock);
        m.enable_irq();
        m.disable_irq();
    }
    let task = current(m, scheduler);
    let wait = entry_words.fixed_pool_wait(slot);
    let how = match timeout {
        Timeout::Units(units) => Enqueue::Timed {
            timer: entry_words.fixed_pool_timer(slot),
            timeout: units as i32,
        },
        _ => Enqueue::Plain,
    };
    m.enqueue(
        entry.domain(),
        id.domain.id().word(),
        control.queue(),
        wait,
        how,
    );
    m.unlock(scheduler_lock);
    m.unlock(object_lock);
    m.dispatch(dispatch_code(timeout));
    Ok(PoolBlocking::Pending(Pending { task, wait, out }))
}

#[inline(always)]
fn dispatch_code(timeout: Timeout) -> i32 {
    match timeout {
        Timeout::Units(_) => -75,
        _ => -73,
    }
}

/// Slot `+0x170`. The owned body reads the exists word, which stock omits.
#[inline(never)]
#[cfg_attr(target_os = "none", link_section = ".text.fp_body_poll_unchecked")]
pub fn poll_unchecked<M: Machine + Locks + SaveIrq>(
    m: &M,
    id: PoolId,
    out: OutWord,
) -> Result<WaitOutcome<BlockAddress>, PoolStatus> {
    let words = id.domain.words();
    let config = words.fixed_pool_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    let saved = m.save_irq();
    m.lock(lock);
    let result = if m.load_word(config.exists()) == 0 {
        Err(PoolStatus::NotCreated)
    } else {
        Ok(match take(m, id, out.0) {
            Some(block) => WaitOutcome::Satisfied(block),
            None => WaitOutcome::TimedOut,
        })
    };
    m.unlock(lock);
    m.restore_irq(saved);
    result
}

/// The resumed caller stores the delivered block, then restores the I bit the dispatcher masked.
#[inline(never)]
#[cfg_attr(target_os = "none", link_section = ".text.fp_body_resume")]
pub fn resume<M: Machine + Irq>(m: &M, pending: Pending) -> PoolResume {
    if m.load_byte(pending.task.state()) & TaskState::WAIT.bits() != 0 {
        return PoolResume::Pending(pending);
    }
    let result = m.load_word(pending.wait.base().result()) as i32;
    if result == 0 {
        let block = BlockAddress(GuestAddress(m.load_word(pending.wait.block())));
        m.store_word(pending.out.0, block.0 .0);
    }
    m.enable_irq();
    match result {
        0 => PoolResume::Completed(WaitOutcome::Satisfied(BlockAddress(GuestAddress(
            m.load_word(pending.wait.block()),
        )))),
        -49 => PoolResume::Completed(WaitOutcome::Released),
        -50 => PoolResume::Completed(WaitOutcome::TimedOut),
        _ => PoolResume::Corrupt(pending),
    }
}

#[inline(always)]
fn current<M: Machine>(m: &M, scheduler: crate::layout::KernelContext) -> Task {
    Task(GuestAddress(m.load_word(GuestAddress(m.load_word(scheduler.running())))))
}

#[inline(always)]
fn running_slot<M: Machine>(
    m: &M,
    _entry: crate::layout::Domain,
    scheduler: crate::layout::KernelContext,
) -> Result<TaskSlot, PoolStatus> {
    let task = current(m, scheduler);
    let definition = GuestAddress(m.load_word(task.definition()));
    TaskSlot::new(m.load_word(definition.offset(0x20))).ok_or(PoolStatus::InvalidIndex)
}
