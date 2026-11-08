use crate::create::Machine;
use crate::layout::{GuestAddress, KernelContext, LockRecord, Task, TaskState};

pub trait IrqScope {
    fn disable_irq(&self);
    fn enable_irq(&self);
}

/// The always-succeeding acquire and release, and the primitives the blocking services read.
pub trait Locks: Machine + IrqScope {
    fn lock(&self, lock: LockRecord);
    fn unlock(&self, lock: LockRecord);
}

#[inline(always)]
fn running<M: Machine>(m: &M, scheduler: KernelContext) -> Task {
    let cell = GuestAddress(m.load_word(scheduler.running()));
    Task(GuestAddress(m.load_word(cell)))
}

#[inline(always)]
pub fn running_ready<M: Machine>(m: &M, scheduler: KernelContext) -> bool {
    m.load_byte(running(m, scheduler).state()) & TaskState::READY.bits() != 0
}

#[inline(always)]
pub fn current<M: Machine>(m: &M, scheduler: KernelContext) -> Task {
    running(m, scheduler)
}
