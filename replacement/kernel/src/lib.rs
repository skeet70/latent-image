//! The kernel services owned crates call: typed handles over the stock interface slots.
//!
//! A handle names an object stock created and owns nothing. Every call enters a word of the running
//! image's kernel through that image's [`Backend`]. A token proves the caller's context once, at
//! entry, and carries it to the calls that require it.

#![cfg_attr(target_arch = "arm", no_std)]

use core::marker::PhantomData;
use core::num::{NonZeroU32, NonZeroU8};

mod sealed {
    pub trait Sealed {}
}

#[cfg(not(target_arch = "arm"))]
pub mod fake;
pub mod slot_table;

#[cfg(test)]
mod host_tests;
#[cfg(test)]
mod slot_table_tests;

pub use slot_table::SlotTable;

/// A task number, 1..=120.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(NonZeroU8);
impl TaskId {
    pub const MAX: u8 = 120;
    pub const fn new(number: u8) -> Option<Self> {
        if number >= 1 && number <= Self::MAX { Some(Self(NonZeroU8::new(number).unwrap())) } else { None }
    }
    pub const fn get(self) -> u8 { self.0.get() }
}

/// A semaphore number, 1..=250.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemaphoreId(NonZeroU8);
impl SemaphoreId {
    pub const MAX: u8 = 250;
    pub const fn new(number: u8) -> Option<Self> {
        if number >= 1 && number <= Self::MAX { Some(Self(NonZeroU8::new(number).unwrap())) } else { None }
    }
    pub const fn get(self) -> u8 { self.0.get() }
}

/// Kernel clock units: a timeout is added unscaled to the domain clock word, which advances
/// [`ClockUnits::PER_TICK`] per tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockUnits(NonZeroU32);
impl ClockUnits {
    pub const PER_TICK: u32 = 10;
    pub const fn new(units: u32) -> Option<Self> {
        if units >= 1 && units <= i32::MAX as u32 { Some(Self(NonZeroU32::new(units).unwrap())) } else { None }
    }
    pub const fn from_ticks(ticks: u32) -> Option<Self> {
        match ticks.checked_mul(Self::PER_TICK) {
            Some(units) => Self::new(units),
            None => None,
        }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timeout {
    /// The slot returns `TimedOut` instead of blocking.
    Poll,
    /// No timer is armed.
    Forever,
    Units(ClockUnits),
}
impl Timeout {
    pub(crate) const fn word(self) -> u32 {
        match self {
            Timeout::Poll => 0,
            Timeout::Forever => u32::MAX,
            Timeout::Units(units) => units.get(),
        }
    }
}

/// The word a slot returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status(pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Parameter,
    InvalidId,
    Context,
    Busy,
    State,
    NotCreated,
    Overflow,
    Released,
    TimedOut,
    Other(Status),
}

impl Status {
    /// The typed error of this word.
    pub const fn check(self) -> Result<(), Error> {
        match self.0 {
            0 => Ok(()),
            -17 => Err(Error::Parameter),
            -18 => Err(Error::InvalidId),
            -25 => Err(Error::Context),
            -28 => Err(Error::Busy),
            -41 => Err(Error::State),
            -42 => Err(Error::NotCreated),
            -43 => Err(Error::Overflow),
            -49 => Err(Error::Released),
            -50 => Err(Error::TimedOut),
            _ => Err(Error::Other(self)),
        }
    }
}

impl Error {
    /// The word this error was parsed from.
    pub const fn status(self) -> Status {
        Status(match self {
            Error::Parameter => -17,
            Error::InvalidId => -18,
            Error::Context => -25,
            Error::Busy => -28,
            Error::State => -41,
            Error::NotCreated => -42,
            Error::Overflow => -43,
            Error::Released => -49,
            Error::TimedOut => -50,
            Error::Other(status) => status.0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Task,
    Handler,
}

/// The prior interrupt state a mask or lock returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrqState(u32);
impl IrqState {
    pub(crate) const fn new(word: u32) -> Self { Self(word) }
    pub const fn word(self) -> u32 { self.0 }
}

/// The kernel behind one image.
pub trait Backend: sealed::Sealed {
    fn current_task(&self) -> Option<TaskId>;
    fn in_handler(&self) -> bool;
    fn irq_mask(&self) -> IrqState;
    fn irq_restore(&self, saved: IrqState);
    fn cores_lock(&self) -> IrqState;
    fn cores_unlock(&self, saved: IrqState);
    fn semaphore_wait(&self, id: SemaphoreId, timeout: Timeout) -> Result<(), Error>;
    fn semaphore_signal(&self, id: SemaphoreId, form: Form) -> Result<(), Error>;
}

pub struct Kernel<B: Backend> {
    backend: B,
}
impl<B: Backend> Kernel<B> {
    pub const fn new(backend: B) -> Self { Self { backend } }
    pub fn backend(&self) -> &B { &self.backend }

    pub fn task(&self) -> Option<TaskContext<'_, B>> {
        self.backend.current_task().map(|task| TaskContext { kernel: self, task, _not_send: PhantomData })
    }

    pub fn context(&self) -> Context<'_, B> {
        if self.backend.in_handler() {
            Context::Interrupt(InterruptContext { kernel: self, _not_send: PhantomData })
        } else {
            match self.backend.current_task() {
                Some(task) => Context::Task(TaskContext { kernel: self, task, _not_send: PhantomData }),
                None => Context::Masked(MaskedContext { kernel: self, _not_send: PhantomData }),
            }
        }
    }
}

/// Proof that a nonzero task number was read at entry: the context word was 0 and IRQ was clear.
pub struct TaskContext<'k, B: Backend> {
    kernel: &'k Kernel<B>,
    task: TaskId,
    _not_send: PhantomData<*mut ()>,
}
impl<B: Backend> TaskContext<'_, B> {
    pub fn id(&self) -> TaskId { self.task }
}

/// Proof that the handler word was 1 at entry.
pub struct InterruptContext<'k, B: Backend> {
    kernel: &'k Kernel<B>,
    _not_send: PhantomData<*mut ()>,
}

/// Proof that the handler word and the task word were both 0 at entry.
pub struct MaskedContext<'k, B: Backend> {
    kernel: &'k Kernel<B>,
    _not_send: PhantomData<*mut ()>,
}

pub enum Context<'k, B: Backend> {
    Task(TaskContext<'k, B>),
    Interrupt(InterruptContext<'k, B>),
    Masked(MaskedContext<'k, B>),
}

impl<B: Backend> sealed::Sealed for TaskContext<'_, B> {}
impl<B: Backend> sealed::Sealed for InterruptContext<'_, B> {}
impl<B: Backend> sealed::Sealed for MaskedContext<'_, B> {}

pub trait Caller<B: Backend>: sealed::Sealed {
    fn form(&self) -> Form;
    fn kernel(&self) -> &Kernel<B>;
}
impl<B: Backend> Caller<B> for TaskContext<'_, B> {
    fn form(&self) -> Form { Form::Task }
    fn kernel(&self) -> &Kernel<B> { self.kernel }
}
impl<B: Backend> Caller<B> for InterruptContext<'_, B> {
    fn form(&self) -> Form { Form::Handler }
    fn kernel(&self) -> &Kernel<B> { self.kernel }
}

pub trait Lockable<B: Backend>: sealed::Sealed {
    fn mask_irq(&mut self) -> IrqMask<'_, B>;
    fn lock_cores(&mut self) -> CoreLock<'_, B>;
}
macro_rules! lockable {
    ($token:ident) => {
        impl<B: Backend> Lockable<B> for $token<'_, B> {
            fn mask_irq(&mut self) -> IrqMask<'_, B> {
                IrqMask { kernel: self.kernel, saved: self.kernel.backend.irq_mask() }
            }
            fn lock_cores(&mut self) -> CoreLock<'_, B> {
                CoreLock { kernel: self.kernel, saved: self.kernel.backend.cores_lock() }
            }
        }
    };
}
lockable!(TaskContext);
lockable!(InterruptContext);
lockable!(MaskedContext);

/// CPSR.I set on this core.
pub struct IrqMask<'c, B: Backend> {
    kernel: &'c Kernel<B>,
    saved: IrqState,
}
impl<B: Backend> IrqMask<'_, B> {
    pub fn lock_cores(&mut self) -> CoreLock<'_, B> {
        CoreLock { kernel: self.kernel, saved: self.kernel.backend.cores_lock() }
    }
}
impl<B: Backend> Drop for IrqMask<'_, B> {
    fn drop(&mut self) { self.kernel.backend.irq_restore(self.saved); }
}

/// The image's cross-core critical section.
pub struct CoreLock<'c, B: Backend> {
    kernel: &'c Kernel<B>,
    saved: IrqState,
}
impl<B: Backend> Drop for CoreLock<'_, B> {
    fn drop(&mut self) { self.kernel.backend.cores_unlock(self.saved); }
}

/// A name for a semaphore other code created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Semaphore(SemaphoreId);
impl Semaphore {
    pub const fn stock(id: SemaphoreId) -> Self { Self(id) }
    /// Blocks until the count is taken or a signal readies the caller.
    pub fn wait<B: Backend>(self, cx: &TaskContext<'_, B>, timeout: Timeout) -> Result<(), Error> {
        cx.kernel.backend.semaphore_wait(self.0, timeout)
    }
    /// Readies the first waiter, else increments the count.
    pub fn signal<B: Backend>(self, cx: &impl Caller<B>) -> Result<(), Error> {
        cx.kernel().backend.semaphore_signal(self.0, cx.form())
    }
}

/// A name for a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Task(TaskId);
impl Task {
    pub const fn stock(id: TaskId) -> Self { Self(id) }
    pub fn id(self) -> TaskId { self.0 }
}
