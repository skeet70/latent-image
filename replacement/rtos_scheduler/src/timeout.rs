//! Wait-timeout and delay completion callbacks (#156): `kernel_timeout_task` A `0x44318` /
//! B `0xd42d8` and the delay callback A `0x44634` / B `0xd45f4`.

use super::layout::{
    Domain, DomainId, GuestAddress, KernelContext, Task, TaskFlags, TaskState, WaitBlock,
};
use super::Machine;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Completion {
    TimedOut,
    Elapsed,
}

impl Completion {
    pub const fn result(self) -> i32 {
        match self {
            Self::TimedOut => -50,
            Self::Elapsed => 0,
        }
    }
}

#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
fn current_context<M: Machine>(m: &M, domain: Domain) -> KernelContext {
    KernelContext(GuestAddress(m.load_word(domain.words().current_context)))
}

/// The arming object stores its one-based domain here.
#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
fn owner_domain(owner: u32) -> Option<Domain> {
    match DomainId::from_word(owner) {
        Some(DomainId::A) => Some(Domain::A),
        Some(DomainId::B) => Some(Domain::B),
        _ => None,
    }
}

#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
fn unlink<M: Machine>(m: &M, task: Task) {
    let next = m.load_word(task.next());
    let previous = m.load_word(task.prev());
    m.store_word(GuestAddress(previous), next);
    m.store_word(GuestAddress(next).offset(0x04), previous);
}

#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
fn request_dispatch<M: Machine>(m: &M, domain: Domain) {
    m.store_word(domain.words().reschedule, 1);
}

/// The object-queue handshake. `false` when a competing release dequeued the task while the
/// locks were dropped.
#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
fn detach_object<M: Machine>(m: &M, domain: Domain, task: Task) -> bool {
    let wait = WaitBlock(GuestAddress(m.load_word(task.wait())));
    let owner = match owner_domain(m.load_word(wait.owner())) {
        Some(owner) => domain.words().context(owner),
        None => return true,
    };
    m.store_word(task.cancel_pending(), 1);
    m.lock_release(current_context(m, domain).scheduler_lock());
    m.irq_enable();
    m.handshake_window(task);
    loop {
        m.irq_disable();
        while !m.lock_acquire(owner.object_lock()) {}
        if m.lock_acquire(current_context(m, domain).scheduler_lock()) {
            break;
        }
        m.lock_release(owner.object_lock());
        m.irq_enable();
    }
    if m.load_word(task.cancel_pending()) == 0 {
        m.lock_release(owner.object_lock());
        return false;
    }
    m.store_word(task.cancel_pending(), 0);
    unlink(m, task);
    m.lock_release(owner.object_lock());
    true
}

#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
fn complete<M: Machine>(m: &M, domain: Domain, task: Task) -> i32 {
    let context = current_context(m, domain);
    if m.load_word(task.terminate_pending()) != 0 {
        m.reset_task(task);
        m.store_word(task.context_discarded(), 1);
        let flags = m.load_byte(task.flags());
        if flags & TaskFlags::QUEUED_ACTIVATION.0 == 0 {
            return 0;
        }
        m.store_byte(task.flags(), flags & !TaskFlags::QUEUED_ACTIVATION.0);
        if !m.restart_task(context, task) {
            return 0;
        }
        request_dispatch(m, domain);
        return 1;
    }
    if m.load_word(task.priority_change_pending()) != 0 {
        m.store_byte(task.priority(), m.load_word(task.pending_priority()) as u8);
        m.store_word(task.priority_change_pending(), 0);
    }
    m.store_word(task.cancel_pending(), 0);
    if m.load_byte(task.state()) & TaskState::SUSPENDED.bits() != 0 {
        m.store_byte(task.state(), TaskState::SUSPENDED.bits());
        return 0;
    }
    if m.insert_ready(context, task) {
        request_dispatch(m, domain);
        return 1;
    }
    0
}

/// `kernel_timeout_task`. `r0` reports whether delayed dispatch was requested.
#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
pub fn timeout<M: Machine>(m: &M, domain: Domain, task: Task) -> i32 {
    if m.load_byte(task.state()) & TaskState::OBJECT_QUEUED.bits() != 0
        && !detach_object(m, domain, task)
    {
        return 0;
    }
    let wait = WaitBlock(GuestAddress(m.load_word(task.wait())));
    m.store_word(wait.result(), Completion::TimedOut.result() as u32);
    complete(m, domain, task)
}

/// `r0` reports whether delayed dispatch was requested.
#[cfg_attr(feature = "stage", link_section = ".text.scheduler_timeout_core")]
pub fn delay<M: Machine>(m: &M, domain: Domain, task: Task) -> i32 {
    let wait = WaitBlock(GuestAddress(m.load_word(task.wait())));
    m.store_word(wait.result(), Completion::Elapsed.result() as u32);
    complete(m, domain, task)
}
