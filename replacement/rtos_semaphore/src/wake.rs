use crate::create::Machine;
use crate::layout::{
    Domain, GuestAddress, KernelContext, Task, TaskFlags, TaskState,
};
use crate::locks::Locks;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum WakeMode {
    Task,
    Handler,
}

/// The wait-block completion the release stores.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitCompletion {
    Satisfied,
    /// A delete or a terminate; the raw ABI code is -49.
    Cancelled,
}

impl WaitCompletion {
    #[inline(always)]
    pub const fn code(self) -> i32 {
        match self {
            Self::Satisfied => 0,
            Self::Cancelled => -49,
        }
    }
}

/// The S1 helpers the stock wait release A `0x44568` / B `0xd4528` composes.
pub trait Primitives: Machine {
    fn reset_task(&self, task: Task);
    fn prepare(&self, owner: KernelContext, task: Task) -> Selection;
    fn insert(&self, owner: KernelContext, task: Task) -> Selection;
    fn remove_timer(&self, owner: KernelContext, entry: GuestAddress);
}

/// The stock wait release: timer removal, the completion word, terminate and priority handshakes,
/// the suspended test, then a ready insert.
#[inline(always)]
pub fn release_with<P: Primitives>(
    m: &P,
    owner: KernelContext,
    task: Task,
    completion: WaitCompletion,
) -> Selection {
    let wait = GuestAddress(m.load_word(task.wait()));
    let entry = m.load_word(wait);
    if entry != 0 {
        m.remove_timer(owner, GuestAddress(entry));
    }
    if completion == WaitCompletion::Cancelled
        && m.load_byte(task.state()) & TaskState::OBJECT_QUEUED.bits() != 0
    {
        unlink(m, task);
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

/// The retained wait-release entries: `0x44568` (result 0) and `0x44744` (result -49).
pub trait Release: Machine {
    fn context(&self, domain: Domain) -> KernelContext;
    fn release(&self, owner: KernelContext, task: Task) -> Selection;
    fn cancel(&self, owner: KernelContext, task: Task) -> Selection;
}

pub trait Wake: Release + Locks {
    /// The entry domain's handler-context word.
    fn in_handler(&self) -> bool;
    /// T+0x20.
    fn waiter_domain(&self, task: Task) -> Domain;
    /// The owner record's `+0x00` word.
    fn record_id(&self, owner: KernelContext) -> u32;
    /// A `0x44018` / B `0xd3fd4`.
    fn notify(&self, id: u32) -> DispatchTarget;
    /// A `0x43640` / B `0xd3640`; entry overwrites r0 before selecting a task.
    fn dispatch(&self, code: i32);
    /// The entry domain's reschedule word, consumed at the outermost IRQ exit.
    fn request_delayed_dispatch(&self);
    /// Dispatch from task context, or defer through the reschedule word in a handler.
    #[inline(always)]
    fn finish_dispatch(&self, code: i32) {
        if self.in_handler() {
            self.request_delayed_dispatch();
        } else {
            self.dispatch(code);
        }
    }
}

#[inline(always)]
pub fn unlink<M: Machine>(m: &M, task: Task) {
    let prev = m.load_word(task.prev());
    let next = m.load_word(task.next());
    m.store_word(GuestAddress(prev), next);
    m.store_word(GuestAddress(next).offset(0x04), prev);
}
