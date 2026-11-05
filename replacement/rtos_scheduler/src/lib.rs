//! Owned scheduler helpers of X-M1 1.50 loaders A and B behind their stock entries (#156, step S1).
//! Target entries run each helper over the CPU's [`Machine`]; host tests run it over a model.
#![cfg_attr(not(test), no_std)]

#[cfg(all(target_arch = "arm", not(test)))]
mod entries;
#[cfg(test)]
mod tests;

#[cfg(all(target_arch = "arm", not(any(feature = "domain-a", feature = "domain-b"))))]
compile_error!("select one of feature=\"domain-a\" or feature=\"domain-b\"");
#[cfg(all(feature = "domain-a", feature = "domain-b"))]
compile_error!("feature=\"domain-a\" and feature=\"domain-b\" are exclusive");

#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub use layout::{Domain, DomainId, GuestAddress, KernelContext, LockRecord, Priority, ReadyHead, Task, TaskDefinition, TaskFlags, TaskState};

#[cfg(feature = "domain-a")]
pub const DOMAIN: Domain = Domain::A;
#[cfg(feature = "domain-b")]
pub const DOMAIN: Domain = Domain::B;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Acquired(pub u32);

impl Acquired {
    pub const OK: Self = Self(0);
}

/// Result of insert, remove, requeue and rotate: [`Reschedule::NONE`], the dispatch-enabled word as
/// loaded, or in requeue's branch for the selected task 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Reschedule(pub u32);

impl Reschedule {
    pub const NONE: Self = Self(0);
}

/// Notify's argument: a context record's +0x00 word, any value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct RecordId(pub u32);

impl RecordId {
    /// A: `id <= 1`; B: `id == 0 || id == 2`.
    #[inline(always)]
    pub const fn is_local(self, domain: Domain) -> bool {
        self.0 & !domain.id().word() == 0
    }

    #[inline(always)]
    pub const fn core(self) -> IrqCore {
        IrqCore(((self.0 as i32) >> 1) as u32)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Notified {
    /// The doorbell rang.
    Remote = 0,
    Local = 1,
}

/// The stock raise-IRQ function's r0: 0 selects core 0's interrupt-controller bank, any other value core 1's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct IrqCore(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Irq(pub u32);

impl Irq {
    pub const DOORBELL: Self = Self(120);
}

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn store_byte(&self, at: GuestAddress, value: u8);
    /// `ldrex`, then `strex` of 1 when it loaded 0. True when the `strex` stored.
    fn try_claim(&self, word: GuestAddress) -> bool;
    fn dmb(&self);
    fn dsb(&self);
    /// The stock raise-IRQ function.
    fn raise_irq(&self, core: IrqCore, irq: Irq);
}

/// The low four bits of `raw`.
#[inline(always)]
pub fn priority(raw: u32) -> Priority {
    Priority::new(raw & (Priority::COUNT - 1)).unwrap_or(MOST_URGENT)
}

/// A priority byte of a T record or a priority argument.
///
/// # Safety
/// `raw` is below [`Priority::COUNT`], as in every reachable scheduler state.
#[inline(always)]
pub unsafe fn stored_priority(raw: u32) -> Priority {
    unsafe { core::hint::assert_unchecked(raw < Priority::COUNT) };
    Priority::new(raw).unwrap_or(MOST_URGENT)
}

const MOST_URGENT: Priority = match Priority::new(0) {
    Some(p) => p,
    None => panic!(),
};

#[inline(always)]
fn pointer<M: Machine>(m: &M, field: GuestAddress) -> GuestAddress {
    GuestAddress(m.load_word(field))
}

#[inline(always)]
fn task_priority<M: Machine>(m: &M, task: Task) -> Priority {
    // SAFETY: T+0x0d holds a priority in every reachable state.
    unsafe { stored_priority(m.load_byte(task.priority()) as u32) }
}

#[inline(always)]
fn ready_head<M: Machine>(m: &M, record: KernelContext, p: Priority) -> ReadyHead {
    ReadyHead(pointer(m, record.ready_heads()).offset(p.get() * ReadyHead::SIZE))
}

#[inline(always)]
fn dispatch_enabled<M: Machine>(m: &M, record: KernelContext) -> Reschedule {
    Reschedule(m.load_word(pointer(m, record.dispatch_enabled())))
}

#[inline(always)]
fn unlink<M: Machine>(m: &M, task: Task) {
    m.store_word(Task(pointer(m, task.prev())).next(), m.load_word(task.next()));
    m.store_word(Task(pointer(m, task.next())).prev(), m.load_word(task.prev()));
}

#[inline(always)]
fn link_tail<M: Machine>(m: &M, head: ReadyHead, task: Task) {
    m.store_word(task.prev(), m.load_word(head.prev()));
    m.store_word(task.next(), head.0 .0);
    m.store_word(Task(pointer(m, head.prev())).next(), task.0 .0);
    m.store_word(head.prev(), task.0 .0);
}

#[inline(always)]
fn set_bit<M: Machine>(m: &M, record: KernelContext, p: Priority) {
    let bitmap = pointer(m, record.ready_bitmap());
    m.store_word(bitmap, m.load_word(bitmap) | p.bit());
}

#[inline(always)]
fn clear_bit<M: Machine>(m: &M, record: KernelContext, p: Priority) {
    let bitmap = pointer(m, record.ready_bitmap());
    m.store_word(bitmap, m.load_word(bitmap) & !p.bit());
}

/// Lock acquire: spins until the word goes from 0 to 1, then `dmb`.
#[inline(always)]
pub fn acquire<M: Machine>(m: &M, lock: LockRecord) -> Acquired {
    let word = pointer(m, lock.word());
    while !m.try_claim(word) {}
    m.dmb();
    Acquired::OK
}

/// Lock release: `dmb`, 0 to the word, `dsb`.
#[inline(always)]
pub fn release<M: Machine>(m: &M, lock: LockRecord) {
    m.dmb();
    m.store_word(pointer(m, lock.word()), 0);
    m.dsb();
}

/// The first task of the most urgent ready list. The ready bitmap must have a bit set in 0..=15.
#[inline(always)]
pub fn find<M: Machine>(m: &M, record: KernelContext) -> Task {
    let bitmap = m.load_word(pointer(m, record.ready_bitmap()));
    Task(pointer(m, ready_head(m, record, priority(bitmap.trailing_zeros())).next()))
}

/// Marks `task` ready, appends it to its priority's list and selects it when nothing is selected
/// or it is more urgent than the selected task.
#[inline(always)]
pub fn insert<M: Machine>(m: &M, record: KernelContext, task: Task) -> Reschedule {
    let p = task_priority(m, task);
    m.store_byte(task.state(), TaskState::READY.bits());
    link_tail(m, ready_head(m, record, p), task);
    set_bit(m, record, p);
    let selected = pointer(m, record.selected());
    let current = m.load_word(selected);
    if current != 0 && p >= task_priority(m, Task(GuestAddress(current))) {
        return Reschedule::NONE;
    }
    m.store_word(selected, task.0 .0);
    dispatch_enabled(m, record)
}

/// Unlinks `task` from its ready list. A selected `task` is replaced by the next task of its list,
/// or by `find` when the list emptied, or by 0 when no list is left.
#[inline(always)]
pub fn remove<M: Machine>(m: &M, record: KernelContext, task: Task, find: impl FnOnce() -> Task) -> Reschedule {
    let p = task_priority(m, task);
    let head = ready_head(m, record, p);
    unlink(m, task);
    let first = m.load_word(head.next());
    if first == head.0 .0 {
        clear_bit(m, record, p);
        let selected = pointer(m, record.selected());
        if m.load_word(selected) != task.0 .0 {
            return Reschedule::NONE;
        }
        let next = if m.load_word(pointer(m, record.ready_bitmap())) == 0 { 0 } else { find().0 .0 };
        m.store_word(selected, next);
    } else {
        let selected = pointer(m, record.selected());
        if m.load_word(selected) != task.0 .0 {
            return Reschedule::NONE;
        }
        m.store_word(selected, first);
    }
    dispatch_enabled(m, record)
}

/// Returns `task` to its dormant state with its definition's priority.
#[inline(always)]
pub fn reset<M: Machine>(m: &M, task: Task) {
    let definition = TaskDefinition(pointer(m, task.definition()));
    m.store_byte(task.priority(), m.load_word(definition.priority()) as u8);
    m.store_byte(task.state(), TaskState::DORMANT.bits());
    let cleared = TaskFlags::QUEUED_WAKEUP.0 | TaskFlags::EXCEPTION_ENABLED.0;
    m.store_byte(task.flags(), m.load_byte(task.flags()) & !cleared);
    m.store_word(task.exception_pattern(), 0);
    m.store_word(task.priority_change_pending(), 0);
    m.store_word(task.terminate_pending(), 0);
    m.store_word(task.cancel_pending(), 0);
}

/// Sets `task`'s initial SP and the record's first-entry PC, then `dmb`, then `insert`.
#[inline(always)]
pub fn prepare<M: Machine>(m: &M, record: KernelContext, task: Task, insert: impl FnOnce() -> Reschedule) -> Reschedule {
    let [low, high] = TaskDefinition(pointer(m, task.definition())).initial_sp_terms();
    m.store_word(task.saved_sp(), m.load_word(high).wrapping_add(m.load_word(low)));
    m.store_word(task.resume_pc(), m.load_word(record.first_entry()));
    m.dmb();
    insert()
}

/// Rings IRQ 120 on the core of a remote `id` after a `dsb`.
#[inline(always)]
pub fn notify<M: Machine>(m: &M, domain: Domain, id: RecordId) -> Notified {
    if !id.is_local(domain) {
        m.dsb();
        m.raise_irq(id.core(), Irq::DOORBELL);
        Notified::Remote
    } else {
        Notified::Local
    }
}

/// Moves `task` to the tail of `to`'s list and stores `to` as its priority.
/// The selected `task` is reselected by `find` unless it became more urgent; another task is
/// replaced when `task` is now more urgent.
#[inline(always)]
pub fn requeue<M: Machine>(m: &M, record: KernelContext, task: Task, to: Priority, find: impl FnOnce() -> Task) -> Reschedule {
    let from = task_priority(m, task);
    m.store_byte(task.priority(), to.get() as u8);
    unlink(m, task);
    let old_head = ready_head(m, record, from);
    if m.load_word(old_head.next()) == old_head.0 .0 {
        clear_bit(m, record, from);
    }
    link_tail(m, ready_head(m, record, to), task);
    set_bit(m, record, to);
    let selected = pointer(m, record.selected());
    let current = m.load_word(selected);
    if current == task.0 .0 {
        if from > to {
            return Reschedule::NONE;
        }
        m.store_word(selected, find().0 .0);
        if m.load_word(selected) == current {
            return Reschedule::NONE;
        }
        return Reschedule((dispatch_enabled(m, record) != Reschedule::NONE) as u32);
    }
    if to < task_priority(m, Task(GuestAddress(current))) {
        m.store_word(selected, task.0 .0);
        return dispatch_enabled(m, record);
    }
    Reschedule::NONE
}

/// Moves the first task of `p`'s list to its tail when the list holds two or more.
#[inline(always)]
pub fn rotate<M: Machine>(m: &M, record: KernelContext, p: Priority) -> Reschedule {
    let head = ready_head(m, record, p);
    let first = m.load_word(head.next());
    if first == head.0 .0 {
        return Reschedule::NONE;
    }
    let first = Task(GuestAddress(first));
    let second = m.load_word(first.next());
    if second == head.0 .0 {
        return Reschedule::NONE;
    }
    m.store_word(head.next(), second);
    m.store_word(Task(pointer(m, first.next())).prev(), head.0 .0);
    m.store_word(first.prev(), m.load_word(head.prev()));
    m.store_word(first.next(), head.0 .0);
    m.store_word(Task(pointer(m, head.prev())).next(), first.0 .0);
    m.store_word(head.prev(), first.0 .0);
    let selected = pointer(m, record.selected());
    if m.load_word(selected) != first.0 .0 {
        return Reschedule::NONE;
    }
    m.store_word(selected, m.load_word(head.next()));
    dispatch_enabled(m, record)
}
