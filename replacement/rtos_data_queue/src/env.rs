use crate::layout::{
    Domain, GuestAddress, KernelContext, LockRecord, Task, TaskFlags, TaskState, WaitQueue,
};

/// The I/F bits the stock mask pair returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedInterrupts(pub u32);

/// The I-only mask the resume tail saves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedMask(pub u32);

/// An absolute expiry in kernel clock units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Deadline(pub u32);

/// The executing context's clock, `*context->tick`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Tick(pub u32);

/// The stock timeout callback stored into the entry, `kernel_timeout_task`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TimeoutCallback(pub u32);

/// The per-task timer entry (`g_dtq_receive_timers`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerEntryAddress(pub GuestAddress);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    Unchanged,
    Changed,
}

/// `Remote` rang the peer's IRQ 120.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispatchTarget {
    Local,
    Remote,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueOrder {
    Fifo,
    Priority,
}

impl QueueOrder {
    pub const fn from_attribute(attributes: u32) -> Self {
        if attributes & 1 != 0 {
            Self::Priority
        } else {
            Self::Fifo
        }
    }
}

/// The wait-block completion the release stores.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitCompletion {
    Satisfied,
    /// A forced release or delete; the raw ABI code is -49 and carries no further cause.
    Released,
}

impl WaitCompletion {
    pub const fn code(self) -> i32 {
        match self {
            Self::Satisfied => 0,
            Self::Released => -49,
        }
    }
}

pub trait Env {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_byte(&self, at: GuestAddress, value: u8);
    fn dmb(&self);
    fn dsb(&self);

    fn mask_if(&self) -> SavedInterrupts;
    fn restore_if(&self, saved: SavedInterrupts);
    fn mask_i(&self) -> SavedMask;
    fn restore_i(&self, saved: SavedMask);

    fn with_object_lock<R>(&self, lock: LockRecord, body: impl FnOnce() -> R) -> R;
    fn with_object_lock_after<R>(
        &self,
        lock: LockRecord,
        body: impl FnOnce() -> R,
        after: impl FnOnce(&R),
    ) -> R;
    fn with_scheduler_lock<R>(&self, domain: Domain, body: impl FnOnce() -> R) -> R;

    fn context(&self, domain: Domain) -> KernelContext;
    fn current(&self, entry: Domain) -> Task;
    fn remove_ready(&self, owner: KernelContext, task: Task);
    fn queue_wait(&self, owner: KernelContext, task: Task, queue: WaitQueue, order: QueueOrder);
    fn arm_timer(&self, owner: KernelContext, entry: TimerEntryAddress, deadline: Deadline);
    fn tick(&self, owner: KernelContext) -> Tick;
    fn timeout_callback(&self) -> TimeoutCallback;
    fn reset_task(&self, task: Task);
    fn prepare(&self, owner: KernelContext, task: Task) -> Selection;
    fn insert(&self, owner: KernelContext, task: Task) -> Selection;
    fn remove_timer(&self, owner: KernelContext, entry: GuestAddress);
    fn waiter_domain(&self, task: Task) -> Domain;
    fn record_id(&self, owner: KernelContext) -> u32;
    fn notify(&self, id: u32) -> DispatchTarget;
    fn dispatch(&self, code: i32);
    fn request_delayed_dispatch(&self);
}

pub(crate) fn unlink<M: Env>(m: &M, task: Task) {
    let prev = m.load_word(task.prev());
    let next = m.load_word(task.next());
    m.store_word(GuestAddress(prev), next);
    m.store_word(GuestAddress(next).offset(0x04), prev);
}

/// The stock wait release, A `0x44568` / B `0xd4528`.
pub fn release<M: Env>(m: &M, owner: KernelContext, task: Task, completion: WaitCompletion) -> Selection {
    let wait = GuestAddress(m.load_word(task.wait()));
    let entry = m.load_word(wait);
    if entry != 0 {
        m.remove_timer(owner, GuestAddress(entry));
    }
    m.store_word(wait, completion.code() as u32);
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

pub(crate) fn dispatch_after<M: Env>(m: &M, owner: KernelContext, released: Selection) -> bool {
    match released {
        Selection::Unchanged => false,
        Selection::Changed => m.notify(m.record_id(owner)) == DispatchTarget::Local,
    }
}
