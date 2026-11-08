//! Owned task lifecycle root services `+0x18`, `+0x1c`, `+0x28`, `+0x38`, `+0x3c` and
//! `+0x4c` of both X-M1 1.50 kernels (#156).
//!
//! One unit: the six services share the task record, ready lists, cross-object
//! `T+0x24`/`T+0x28`/`T+0x2c` handshakes and the borrowed timer seam.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

use layout::{
    Domain, DomainId, GuestAddress, KernelContext, LockRecord, Priority, ReadyHead, Task,
    TaskDefinition, TaskFlags, TaskState, WaitBlock, WaitQueue,
};

#[cfg(test)]
#[path = "s3d_tests.rs"]
mod tests;

pub const TASK_STRIDE: u32 = Task::SIZE;
pub const TIMER_STRIDE: u32 = 0x0c;
pub const SLEEP_RESULT_STRIDE: u32 = 0x04;
pub const TASK_SLOT_WORD: u32 = 0x20;
pub const TASK_COUNT: u32 = 120;

/// The stock `CPSR.I` bit a service saves and restores.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct InterruptState(pub u32);

impl InterruptState {
    pub const IRQ_MASKED: u32 = 0x80;
    pub const FIQ_MASKED: u32 = 0x40;

    pub const fn masked(self) -> bool {
        self.0 & Self::IRQ_MASKED != 0
    }
}

/// `r0` of every id-taking service: `(class << 16) | number`, signed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EncodedId(pub i32);

impl EncodedId {
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// `r0 asr 16`: the record selector, not the executing domain.
    pub const fn class(self) -> i32 {
        self.0 >> 16
    }

    /// `sxth(r0)`: one-based task number, zero for the running task or negative.
    pub const fn number(self) -> i32 {
        self.0 as i16 as i32
    }
}

/// `r1` of priority change, checked before the creation record is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct PriorityArgument(pub u32);

impl PriorityArgument {
    pub const fn effective(self, creation_priority: u32) -> u8 {
        if self.0 == 0 {
            creation_priority as u8
        } else {
            (self.0 - 1) as u8
        }
    }
}

/// A timeout or delay in kernel clock units, signed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Ticks(pub i32);

/// The switch codes the stock bodies pass to the dispatcher entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum SwitchReason {
    Terminate = -12,
    Priority = -13,
    Sleep = -18,
    Release = -21,
    Delay = -25,
}

impl SwitchReason {
    pub const fn code(self) -> i32 {
        self as i32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServiceError {
    /// Context word nonzero or `CPSR.I` set at entry.
    NotTaskContext,
    /// Class outside 0..2 or a task number outside the record's count.
    InvalidId,
    /// `C+0x24` is clear.
    Nonexistent,
    /// The executing core's own running task.
    CurrentTask,
    /// `T+0xc` is dormant, or the handshake partner already finished.
    Dormant,
    /// `T+0xc` is not a wait state.
    NotWaiting,
    /// `r0 < -1` for timed sleep or `r0 < 0` for delay.
    BadTimeout,
}

impl ServiceError {
    #[inline(always)]
    pub const fn status(self) -> i32 {
        match self {
            Self::NotTaskContext => -25,
            Self::InvalidId => -18,
            Self::Nonexistent => -42,
            Self::CurrentTask => -28,
            Self::Dormant | Self::NotWaiting => -41,
            Self::BadTimeout => -17,
        }
    }
}

/// The timer entry a sleep or delay arms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TimerEntry(pub GuestAddress);

impl TimerEntry {
    pub const fn index(self) -> GuestAddress {
        self.0
    }

    pub const fn callback(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    pub const fn argument(self) -> GuestAddress {
        self.0.offset(0x08)
    }
}

/// The guest operations the six services use. The adapter maps them to stock calls and
/// A32 instructions; the host model maps them to a scheduler and timer model.
pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_byte(&self, at: GuestAddress, value: u8);

    /// Read `CPSR.I`, then mask IRQ.
    fn save_and_mask(&self) -> InterruptState;
    fn restore(&self, state: InterruptState);
    fn masked(&self) -> bool;

    /// The record's acquire function. Stock spins and always reports success.
    fn lock_acquire(&self, lock: LockRecord) -> bool;
    fn lock_release(&self, lock: LockRecord);

    /// `kernel_remove_ready`: unlink, update bitmap and selection; nonzero means dispatch.
    fn remove_ready(&self, context: KernelContext, task: Task) -> bool;
    /// `FUN_00043db8`: mark ready, link at tail, update bitmap and selection.
    fn insert_ready(&self, context: KernelContext, task: Task) -> bool;
    fn reset_task(&self, task: Task);
    /// `FUN_00044058`: set the initial SP and resume PC, then insert.
    fn restart_task(&self, context: KernelContext, task: Task) -> bool;
    /// `kernel_notify_dispatch`: nonzero for a local record, zero after a remote doorbell.
    fn notify_dispatch(&self, context: KernelContext) -> bool;
    /// `FUN_00043760`: save the caller and dispatch; returns when the task is resumed.
    fn switch(&self, reason: SwitchReason);

    /// The borrowed timer heap seam. Insertion requires the entry to be inactive.
    fn timer_insert(&self, context: KernelContext, entry: TimerEntry, expiry: u32);
    /// Removal requires an active entry; callers check [`Machine::timer_active`] first.
    fn timer_remove(&self, context: KernelContext, entry: TimerEntry);
    /// Membership by heap slot.
    fn timer_active(&self, context: KernelContext, entry: TimerEntry) -> bool;

    /// A test seam after a scheduler lock is dropped for a cross-object handshake.
    fn handshake_window(&self, _task: Task) {}
    /// A test seam after the scheduler lock is dropped before a dispatcher entry.
    fn switch_window(&self, _task: Task) {}
}

#[inline(always)]
fn word<M: Machine>(m: &M, at: GuestAddress) -> u32 {
    m.load_word(at)
}

#[inline(always)]
fn put<M: Machine>(m: &M, at: GuestAddress, value: u32) {
    m.store_word(at, value);
}

#[inline(always)]
fn byte<M: Machine>(m: &M, at: GuestAddress) -> u8 {
    m.load_byte(at)
}

#[inline(always)]
fn put_byte<M: Machine>(m: &M, at: GuestAddress, value: u8) {
    m.store_byte(at, value);
}

#[inline(always)]
fn current_context<M: Machine>(m: &M, words: &layout::DomainWords) -> KernelContext {
    KernelContext(GuestAddress(word(m, words.current_context)))
}

#[inline(always)]
fn running<M: Machine>(m: &M, words: &layout::DomainWords) -> Task {
    Task(GuestAddress(word(m, words.running)))
}

#[inline(always)]
fn definition<M: Machine>(m: &M, task: Task) -> TaskDefinition {
    TaskDefinition(GuestAddress(word(m, task.definition())))
}

#[inline(always)]
fn task_at<M: Machine>(m: &M, record: KernelContext, number: u32) -> Task {
    Task(GuestAddress(
        word(m, record.tasks()).wrapping_add(number.wrapping_sub(1) * TASK_STRIDE),
    ))
}

#[inline(always)]
fn record_for<M: Machine>(
    m: &M,
    words: &layout::DomainWords,
    class: i32,
) -> Option<KernelContext> {
    match class {
        0 => Some(current_context(m, words)),
        1 => Some(words.context(Domain::A)),
        2 => Some(words.context(Domain::B)),
        _ => None,
    }
}

#[inline(always)]
fn task_context<M: Machine>(m: &M, words: &layout::DomainWords) -> bool {
    word(m, words.handler_context) == 0 && !m.masked()
}

#[inline(always)]
fn handler_context<M: Machine>(m: &M, words: &layout::DomainWords) -> bool {
    word(m, words.handler_context) != 0 && !m.masked()
}

#[inline(always)]
fn exists<M: Machine>(m: &M, task: Task) -> bool {
    word(m, definition(m, task).exists()) != 0
}

/// Masks IRQ and spins until `lock` is held. A failed attempt restores the entry state
/// before retrying, as the stock loops do.
#[inline(always)]
fn lock<M: Machine>(m: &M, lock_record: LockRecord) -> InterruptState {
    let saved = m.save_and_mask();
    while !m.lock_acquire(lock_record) {
        m.restore(saved);
        m.save_and_mask();
    }
    saved
}

/// Takes `lock` while IRQ is already masked and one other lock is held.
#[inline(always)]
fn lock_masked<M: Machine>(m: &M, lock_record: LockRecord) {
    while !m.lock_acquire(lock_record) {}
}

#[inline(always)]
fn unlock<M: Machine>(m: &M, lock_record: LockRecord, saved: InterruptState) {
    m.lock_release(lock_record);
    m.restore(saved);
}

#[inline(always)]
fn owner_context<M: Machine>(
    m: &M,
    words: &layout::DomainWords,
    task: Task,
) -> Option<KernelContext> {
    let wait = WaitBlock(GuestAddress(word(m, task.wait())));
    match word(m, wait.owner()) {
        1 => Some(words.context(Domain::A)),
        2 => Some(words.context(Domain::B)),
        _ => None,
    }
}

/// The `wait.result` receipt can name a node a timeout already popped.
#[inline(always)]
fn detach_waiter<M: Machine>(m: &M, record: KernelContext, task: Task) {
    let wait = WaitBlock(GuestAddress(word(m, task.wait())));
    let receipt = word(m, wait.result());
    if receipt != 0 {
        let entry = TimerEntry(GuestAddress(receipt));
        if m.timer_active(record, entry) {
            m.timer_remove(record, entry);
        }
    }
    let state = byte(m, task.state());
    if state & TaskState::OBJECT_QUEUED.bits() != 0 {
        let next = word(m, task.next());
        let previous = word(m, task.prev());
        put(m, Task(GuestAddress(previous)).next(), next);
        put(m, Task(GuestAddress(next)).prev(), previous);
    }
}

/// The `FUN_0004482c` release state machine over [`detach_waiter`].
#[inline(always)]
fn release_locked<M: Machine>(m: &M, record: KernelContext, task: Task) -> bool {
    detach_waiter(m, record, task);
    let wait = WaitBlock(GuestAddress(word(m, task.wait())));
    put(m, wait.result(), (-49i32) as u32);
    if word(m, task.terminate_pending()) != 0 {
        m.reset_task(task);
        put(m, task.context_discarded(), 1);
        let flags = byte(m, task.flags());
        if flags & TaskFlags::QUEUED_ACTIVATION.0 == 0 {
            return false;
        }
        put_byte(m, task.flags(), flags & !TaskFlags::QUEUED_ACTIVATION.0);
        return m.restart_task(record, task);
    }
    if word(m, task.priority_change_pending()) != 0 {
        put_byte(m, task.priority(), word(m, task.pending_priority()) as u8);
        put(m, task.priority_change_pending(), 0);
    }
    put(m, task.cancel_pending(), 0);
    if byte(m, task.state()) & TaskState::SUSPENDED.bits() != 0 {
        put_byte(m, task.state(), TaskState::SUSPENDED.bits());
        return false;
    }
    m.insert_ready(record, task)
}

#[inline(always)]
fn notify_then_switch<M: Machine>(
    m: &M,
    record: KernelContext,
    lock_record: LockRecord,
    saved: InterruptState,
    reason: SwitchReason,
) {
    let local = m.notify_dispatch(record);
    m.lock_release(lock_record);
    if local {
        m.switch(reason);
    }
    m.restore(saved);
}

/// `kernel_remove_ready`'s `find` helper: the first task of the most urgent ready list.
#[inline(always)]
fn find_ready<M: Machine>(m: &M, record: KernelContext) -> Task {
    let bitmap = word(m, GuestAddress(word(m, record.ready_bitmap())));
    let priority = Priority::new(bitmap.trailing_zeros()).unwrap_or_else(|| Priority::new(0).unwrap());
    let head = ReadyHead(GuestAddress(
        word(m, record.ready_heads()) + priority.get() * ReadyHead::SIZE,
    ));
    Task(GuestAddress(word(m, head.next())))
}

/// `kernel_requeue_priority`: move a ready task to `to`, then repair selection.
#[inline(always)]
fn requeue_priority<M: Machine>(
    m: &M,
    record: KernelContext,
    task: Task,
    to: u8,
) -> bool {
    let from = byte(m, task.priority());
    put_byte(m, task.priority(), to);
    let next = word(m, task.next());
    let previous = word(m, task.prev());
    put(m, Task(GuestAddress(previous)).next(), next);
    put(m, Task(GuestAddress(next)).prev(), previous);
    let old_head = ReadyHead(GuestAddress(
        word(m, record.ready_heads()) + (from as u32) * ReadyHead::SIZE,
    ));
    if word(m, old_head.next()) == old_head.0 .0 {
        let bitmap_at = GuestAddress(word(m, record.ready_bitmap()));
        let bitmap = word(m, bitmap_at);
        put(m, bitmap_at, bitmap & !(1u32 << from));
    }
    let new_head = ReadyHead(GuestAddress(
        word(m, record.ready_heads()) + (to as u32) * ReadyHead::SIZE,
    ));
    put(m, task.next(), new_head.0 .0);
    put(m, task.prev(), word(m, new_head.prev()));
    put(m, Task(GuestAddress(word(m, new_head.prev()))).next(), task.0 .0);
    put(m, new_head.prev(), task.0 .0);
    let bitmap_at = GuestAddress(word(m, record.ready_bitmap()));
    let bitmap = word(m, bitmap_at);
    put(m, bitmap_at, bitmap | (1u32 << to));
    let selected_at = GuestAddress(word(m, record.selected()));
    let current = word(m, selected_at);
    if current == task.0 .0 {
        if from > to {
            return false;
        }
        put(m, selected_at, find_ready(m, record).0 .0);
        if word(m, selected_at) == current {
            return false;
        }
        return word(m, GuestAddress(word(m, record.dispatch_enabled()))) != 0;
    }
    if (to as u32) < byte(m, Task(GuestAddress(current)).priority()) as u32 {
        put(m, selected_at, task.0 .0);
        return word(m, GuestAddress(word(m, record.dispatch_enabled()))) != 0;
    }
    false
}

/// `FUN_00044aa8`: reinsert a waiter into its priority-ordered object queue.
#[inline(always)]
fn reorder_wait_queue<M: Machine>(m: &M, queue: WaitQueue, task: Task) {
    let config = word(m, queue.config());
    if word(m, GuestAddress(config)) & 1 == 0 {
        return;
    }
    let next = word(m, task.next());
    let previous = word(m, task.prev());
    put(m, Task(GuestAddress(previous)).next(), next);
    put(m, Task(GuestAddress(next)).prev(), previous);
    let priority = byte(m, task.priority());
    let mut position = word(m, queue.next());
    while position != queue.0 .0 {
        if byte(m, Task(GuestAddress(position)).priority()) > priority {
            break;
        }
        position = word(m, Task(GuestAddress(position)).next());
    }
    let before = word(m, Task(GuestAddress(position)).prev());
    put(m, task.prev(), before);
    put(m, task.next(), position);
    put(m, Task(GuestAddress(before)).next(), task.0 .0);
    put(m, Task(GuestAddress(position)).prev(), task.0 .0);
}

/// `kernel_timer_insert` membership test by heap slot.
#[inline(always)]
pub fn timer_heap_active<M: Machine>(
    m: &M,
    record: KernelContext,
    entry: TimerEntry,
) -> bool {
    let index = word(m, entry.index());
    if index == 0 {
        return false;
    }
    let count = word(m, GuestAddress(word(m, record.timer_count())));
    if index > count {
        return false;
    }
    let nodes = GuestAddress(word(m, record.timer_heap()));
    let slot = nodes.offset(index.wrapping_sub(1) * TIMER_STRIDE);
    word(m, slot.offset(0x08)) == entry.0 .0
}

#[inline(always)]
fn arm_timer<M: Machine>(
    m: &M,
    record: KernelContext,
    entry: TimerEntry,
    expiry: u32,
) {
    if m.timer_active(record, entry) {
        m.timer_remove(record, entry);
    }
    m.timer_insert(record, entry, expiry);
}

#[inline(always)]
fn clock<M: Machine>(m: &M, record: KernelContext) -> u32 {
    word(m, GuestAddress(word(m, record.tick_now())))
}

#[inline(always)]
fn sleep_result(domain: Domain, slot: u32) -> WaitBlock {
    let base = match domain {
        Domain::A => GuestAddress(0x4005_477c),
        Domain::B => GuestAddress(0x400e_473c),
    };
    WaitBlock(base.offset(slot.wrapping_sub(1) * SLEEP_RESULT_STRIDE))
}

#[inline(always)]
fn sleep_timer(domain: Domain, slot: u32) -> TimerEntry {
    let base = match domain {
        Domain::A => GuestAddress(0x4005_495c),
        Domain::B => GuestAddress(0x400e_491c),
    };
    TimerEntry(base.offset(slot.wrapping_sub(1) * TIMER_STRIDE))
}

#[inline(always)]
fn timeout_callback(domain: Domain) -> u32 {
    match domain {
        Domain::A => 0x0004_4318,
        Domain::B => 0x000d_42d8,
    }
}

#[inline(always)]
fn delay_callback(domain: Domain) -> u32 {
    match domain {
        Domain::A => 0x0004_4634,
        Domain::B => 0x000d_45f4,
    }
}

/// `kernel_terminate_task`, `+0x18`.
#[inline(always)]
pub fn terminate<M: Machine>(m: &M, domain: Domain, id: EncodedId) -> Result<(), ServiceError> {
    let words = domain.words();
    if !task_context(m, &words) {
        return Err(ServiceError::NotTaskContext);
    }
    let record = record_for(m, &words, id.class()).ok_or(ServiceError::InvalidId)?;
    let number = id.number();
    if number <= 0 || number as u32 > word(m, record.task_count()) {
        return Err(ServiceError::InvalidId);
    }
    let task = task_at(m, record, number as u32);
    if !exists(m, task) {
        return Err(ServiceError::Nonexistent);
    }
    if task == running(m, &words) {
        return Err(ServiceError::CurrentTask);
    }
    let saved = lock(m, record.scheduler_lock());
    let state = byte(m, task.state());
    if state == 0 {
        unlock(m, record.scheduler_lock(), saved);
        return Err(ServiceError::Dormant);
    }
    let mut dispatch;
    if state & TaskState::READY.bits() != 0 {
        dispatch = m.remove_ready(record, task);
    } else if state & TaskState::WAIT.bits() == 0 {
        dispatch = false;
    } else if state & TaskState::OWNER_LOCK_QUEUED.bits() == 0 {
        detach_waiter(m, record, task);
        dispatch = false;
    } else {
        if word(m, task.terminate_pending()) != 0 {
            let flags = byte(m, task.flags());
            let result = if flags & TaskFlags::QUEUED_ACTIVATION.0 != 0 {
                put_byte(m, task.flags(), flags & !TaskFlags::QUEUED_ACTIVATION.0);
                Ok(())
            } else {
                Err(ServiceError::Dormant)
            };
            // Stock returns here with IRQ still masked; every other path restores it.
            unlock(m, record.scheduler_lock(), saved);
            return result;
        }
        let owner = owner_context(m, &words, task).ok_or(ServiceError::InvalidId)?;
        put(m, task.terminate_pending(), 1);
        m.lock_release(record.scheduler_lock());
        m.handshake_window(task);
        lock_masked(m, owner.object_lock());
        lock_masked(m, record.scheduler_lock());
        if word(m, task.terminate_pending()) == 0 {
            m.lock_release(record.scheduler_lock());
            m.lock_release(owner.object_lock());
            m.restore(saved);
            return Ok(());
        }
        detach_waiter(m, record, task);
        m.lock_release(owner.object_lock());
        dispatch = false;
    }
    m.reset_task(task);
    put(m, task.context_discarded(), 1);
    let flags = byte(m, task.flags());
    if flags & TaskFlags::QUEUED_ACTIVATION.0 != 0 {
        put_byte(m, task.flags(), flags & !TaskFlags::QUEUED_ACTIVATION.0);
        if m.restart_task(record, task) {
            dispatch = true;
        }
    }
    if dispatch {
        notify_then_switch(m, record, record.scheduler_lock(), saved, SwitchReason::Terminate);
    } else {
        unlock(m, record.scheduler_lock(), saved);
    }
    Ok(())
}

/// `kernel_change_task_priority`, `+0x1c`.
#[inline(always)]
pub fn change_priority<M: Machine>(
    m: &M,
    domain: Domain,
    id: EncodedId,
    priority: PriorityArgument,
) -> Result<(), ServiceError> {
    let words = domain.words();
    if !task_context(m, &words) {
        return Err(ServiceError::NotTaskContext);
    }
    let record = record_for(m, &words, id.class()).ok_or(ServiceError::InvalidId)?;
    let number = id.number();
    if number > 0 {
        if number as u32 > word(m, record.task_count()) {
            return Err(ServiceError::InvalidId);
        }
    } else if number != 0 {
        return Err(ServiceError::InvalidId);
    }
    if priority.0 > Priority::COUNT {
        return Err(ServiceError::BadTimeout);
    }
    let task = if number != 0 {
        task_at(m, record, number as u32)
    } else {
        running(m, &words)
    };
    if !exists(m, task) {
        return Err(ServiceError::Nonexistent);
    }
    let effective = priority.effective(word(m, definition(m, task).priority()));
    let saved = lock(m, record.scheduler_lock());
    let state = byte(m, task.state());
    if state == 0 || (task == running(m, &words) && word(m, task.context_discarded()) != 0) {
        unlock(m, record.scheduler_lock(), saved);
        return Err(ServiceError::Dormant);
    }
    if state & TaskState::READY.bits() != 0 {
        if !requeue_priority(m, record, task, effective) {
            unlock(m, record.scheduler_lock(), saved);
        } else {
            notify_then_switch(m, record, record.scheduler_lock(), saved, SwitchReason::Priority);
        }
        return Ok(());
    }
    if state & TaskState::OWNER_LOCK_QUEUED.bits() == 0 {
        put_byte(m, task.priority(), effective);
        unlock(m, record.scheduler_lock(), saved);
        return Ok(());
    }
    let owner = owner_context(m, &words, task).ok_or(ServiceError::InvalidId)?;
    put(m, task.priority_change_pending(), 1);
    put(m, task.pending_priority(), effective as u32);
    m.lock_release(record.scheduler_lock());
    m.handshake_window(task);
    lock_masked(m, owner.object_lock());
    lock_masked(m, record.scheduler_lock());
    if word(m, task.priority_change_pending()) == 0 {
        m.lock_release(record.scheduler_lock());
        m.lock_release(owner.object_lock());
        m.restore(saved);
        return Ok(());
    }
    put_byte(m, task.priority(), word(m, task.pending_priority()) as u8);
    put(m, task.priority_change_pending(), 0);
    let wait = WaitBlock(GuestAddress(word(m, task.wait())));
    reorder_wait_queue(m, WaitQueue(GuestAddress(word(m, wait.queue()))), task);
    m.lock_release(owner.object_lock());
    unlock(m, record.scheduler_lock(), saved);
    Ok(())
}

/// `kernel_timed_sleep_task`, `+0x28`.
#[inline(always)]
pub fn timed_sleep<M: Machine>(m: &M, domain: Domain, timeout: Ticks) -> i32 {
    let words = domain.words();
    if !task_context(m, &words) || word(m, words.dispatch_enabled) == 0 {
        return -25;
    }
    if timeout.0 < -1 {
        return -17;
    }
    let slot = word(
        m,
        definition(m, running(m, &words)).0.offset(TASK_SLOT_WORD),
    );
    let saved = loop {
        let saved = m.save_and_mask();
        let record = current_context(m, &words);
        if m.lock_acquire(record.scheduler_lock()) {
            if byte(m, running(m, &words).state()) & TaskState::READY.bits() != 0 {
                break saved;
            }
            m.lock_release(record.scheduler_lock());
        }
        m.restore(saved);
    };
    let record = current_context(m, &words);
    let task = running(m, &words);
    let flags = byte(m, task.flags());
    if flags & TaskFlags::QUEUED_WAKEUP.0 != 0 {
        put_byte(m, task.flags(), flags & !TaskFlags::QUEUED_WAKEUP.0);
        unlock(m, record.scheduler_lock(), saved);
        return 0;
    }
    if timeout.0 == 0 {
        unlock(m, record.scheduler_lock(), saved);
        return -50;
    }
    let wait = sleep_result(domain, slot);
    let timer = sleep_timer(domain, slot);
    put_byte(m, task.state(), TaskState::WAIT.bits() | TaskState::SLEEPING.bits());
    m.remove_ready(record, task);
    put(m, task.wait(), wait.0 .0);
    if timeout.0 > 0 {
        put(m, wait.result(), timer.0 .0);
        put(m, timer.callback(), timeout_callback(domain));
        put(m, timer.argument(), task.0 .0);
        arm_timer(m, record, timer, clock(m, record).wrapping_add(timeout.0 as u32));
    } else {
        put(m, wait.result(), 0);
    }
    m.lock_release(record.scheduler_lock());
    m.switch_window(task);
    m.switch(SwitchReason::Sleep);
    let result = word(m, wait.result()) as i32;
    m.restore(saved);
    result
}

/// The shared `+0x38` body; `handler` selects the `+0x3c` context rule and the
/// delayed-dispatch word instead of a dispatcher entry.
#[inline(always)]
fn release<M: Machine>(m: &M, domain: Domain, id: EncodedId, handler: bool) -> Result<(), ServiceError> {
    let words = domain.words();
    if if handler { !handler_context(m, &words) } else { !task_context(m, &words) } {
        return Err(ServiceError::NotTaskContext);
    }
    let record = record_for(m, &words, id.class()).ok_or(ServiceError::InvalidId)?;
    let number = id.number();
    if number <= 0 || number as u32 > word(m, record.task_count()) {
        return Err(ServiceError::InvalidId);
    }
    let task = task_at(m, record, number as u32);
    if !exists(m, task) {
        return Err(ServiceError::Nonexistent);
    }
    let saved = lock(m, record.scheduler_lock());
    let state = byte(m, task.state());
    if state & TaskState::WAIT.bits() == 0 {
        unlock(m, record.scheduler_lock(), saved);
        return Err(ServiceError::NotWaiting);
    }
    if state & TaskState::OBJECT_QUEUED.bits() == 0 {
        let dispatch = release_locked(m, record, task);
        if dispatch {
            if handler {
                put(m, words.reschedule, 1);
                unlock(m, record.scheduler_lock(), saved);
            } else {
                notify_then_switch(m, record, record.scheduler_lock(), saved, SwitchReason::Release);
            }
        } else {
            unlock(m, record.scheduler_lock(), saved);
        }
        return Ok(());
    }
    let owner = owner_context(m, &words, task).ok_or(ServiceError::InvalidId)?;
    put(m, task.cancel_pending(), 1);
    m.lock_release(record.scheduler_lock());
    m.handshake_window(task);
    lock_masked(m, owner.object_lock());
    lock_masked(m, record.scheduler_lock());
    if word(m, task.cancel_pending()) == 0 {
        m.lock_release(record.scheduler_lock());
        m.lock_release(owner.object_lock());
        m.restore(saved);
        return Err(ServiceError::Dormant);
    }
    put(m, task.cancel_pending(), 0);
    let dispatch = release_locked(m, record, task);
    m.lock_release(owner.object_lock());
    if dispatch {
        if handler {
            put(m, words.reschedule, 1);
            unlock(m, record.scheduler_lock(), saved);
        } else {
            notify_then_switch(m, record, record.scheduler_lock(), saved, SwitchReason::Release);
        }
    } else {
        unlock(m, record.scheduler_lock(), saved);
    }
    Ok(())
}

/// `kernel_release_task_wait`, `+0x38`.
#[inline(always)]
pub fn release_wait<M: Machine>(m: &M, domain: Domain, id: EncodedId) -> Result<(), ServiceError> {
    release(m, domain, id, false)
}

/// `kernel_interrupt_release_task_wait`, `+0x3c`.
#[inline(always)]
pub fn interrupt_release_wait<M: Machine>(
    m: &M,
    domain: Domain,
    id: EncodedId,
) -> Result<(), ServiceError> {
    release(m, domain, id, true)
}

/// `kernel_delay_task`, `+0x4c`.
#[inline(always)]
pub fn delay<M: Machine>(m: &M, domain: Domain, delay_ticks: Ticks) -> i32 {
    let words = domain.words();
    if !task_context(m, &words) || word(m, words.dispatch_enabled) == 0 {
        return -25;
    }
    if delay_ticks.0 < 0 {
        return -17;
    }
    let slot = word(
        m,
        definition(m, running(m, &words)).0.offset(TASK_SLOT_WORD),
    );
    let saved = loop {
        let saved = m.save_and_mask();
        let record = current_context(m, &words);
        if m.lock_acquire(record.scheduler_lock()) {
            if byte(m, running(m, &words).state()) & TaskState::READY.bits() != 0 {
                break saved;
            }
            m.lock_release(record.scheduler_lock());
        }
        m.restore(saved);
    };
    let record = current_context(m, &words);
    let task = running(m, &words);
    let wait = sleep_result(domain, slot);
    let timer = sleep_timer(domain, slot);
    put_byte(m, task.state(), TaskState::WAIT.bits());
    m.remove_ready(record, task);
    put(m, task.wait(), wait.0 .0);
    put(m, wait.result(), timer.0 .0);
    put(m, timer.callback(), delay_callback(domain));
    put(m, timer.argument(), task.0 .0);
    arm_timer(m, record, timer, clock(m, record).wrapping_add(delay_ticks.0 as u32));
    m.lock_release(record.scheduler_lock());
    m.switch_window(task);
    m.switch(SwitchReason::Delay);
    let result = word(m, wait.result()) as i32;
    m.restore(saved);
    result
}

/// The stock task slot word `C+0x20` of a running task; a valid slot is 1..=120.
pub fn running_slot<M: Machine>(m: &M, domain: Domain) -> u32 {
    word(
        m,
        definition(m, running(m, &domain.words())).0.offset(TASK_SLOT_WORD),
    )
}

/// `DomainId` of a context record, for the adapter's local/remote notification.
pub fn context_domain<M: Machine>(m: &M, record: KernelContext) -> Option<DomainId> {
    DomainId::from_word(word(m, record.id()))
}
