#![forbid(unsafe_code)]

use core::cell::Cell;
use core::num::NonZeroU32;


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds(pub u32);
impl Milliseconds {
    fn ticks(self) -> u32 {
        let scaled = u64::from(self.0) * 2;
        ((scaled + 4) / 5).max(1) as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerHandle(NonZeroU32);
impl TimerHandle {
    pub const fn new(word: u32) -> Option<Self> {
        match NonZeroU32::new(word) { Some(word) => Some(Self(word)), None => None }
    }
    pub const fn word(self) -> u32 { self.0.get() }
}
impl From<TimerHandle> for HandleWord {
    fn from(handle: TimerHandle) -> Self { Self(handle.word()) }
}

/// A handle word as a retained caller passed it, possibly zero or stale; the retained resolver decides what it names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandleWord(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotIndex(u8);
impl SlotIndex {
    pub const fn new(index: u8) -> Option<Self> {
        if index < 32 { Some(Self(index)) } else { None }
    }
    pub const fn get(self) -> u8 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Repeat { Once, Periodic }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Uninitialized, Exhausted, Backend(i32) }
impl Error {
    pub const fn status(self) -> i32 {
        match self { Self::Uninitialized => -255, Self::Exhausted => -20, Self::Backend(word) => word }
    }
}

#[derive(Clone, Copy)]
enum Slot<C: Copy> {
    Free,
    Claimed,
    Active { next: u32, reload: u32, repeat: Repeat, callback: Option<C> },
}

pub trait TimerPorts<C: Copy> {
    fn allocate(&mut self, slot: SlotIndex) -> Result<TimerHandle, Error>;
    fn resolve(&mut self, timer: HandleWord) -> Result<SlotIndex, Error>;
    fn release(&mut self, slot: SlotIndex);
    fn invoke(&mut self, callback: C);
}

pub struct Timers<C: Copy> {
    initialized: Cell<bool>,
    slots: [Cell<Slot<C>>; 32],
}
impl<C: Copy> Default for Timers<C> {
    fn default() -> Self { Self::new() }
}
impl<C: Copy> Timers<C> {
    pub const fn new() -> Self {
        Self { initialized: Cell::new(false), slots: [const { Cell::new(Slot::Free) }; 32] }
    }

    pub fn initialize(&self) -> Result<(), Error> {
        if self.initialized.replace(true) { Err(Error::Uninitialized) } else { Ok(()) }
    }

    pub fn start(&self, period: Milliseconds, repeat: Repeat, callback: Option<C>, ports: &mut impl TimerPorts<C>) -> Result<TimerHandle, Error> {
        if !self.initialized.get() { return Err(Error::Uninitialized); }
        let index = self.slots.iter().position(|slot| matches!(slot.get(), Slot::Free)).ok_or(Error::Exhausted)?;
        self.slots[index].set(Slot::Claimed);
        let handle = ports.allocate(SlotIndex(index as u8))?;
        let reload = period.ticks();
        let next = if period.0 == 0 { 1 } else { reload + 1 };
        self.slots[index].set(Slot::Active { next, reload, repeat, callback });
        Ok(handle)
    }

    pub fn cancel(&self, handle: HandleWord, ports: &mut impl TimerPorts<C>) {
        if let Ok(index) = ports.resolve(handle) {
            self.slots[usize::from(index.0)].set(Slot::Free);
            ports.release(index);
        }
    }

    pub fn set_reload(&self, handle: HandleWord, period: Milliseconds, ports: &mut impl TimerPorts<C>) -> Result<(), Error> {
        let index = ports.resolve(handle)?;
        let slot = &self.slots[usize::from(index.0)];
        if let Slot::Active { next, repeat, callback, .. } = slot.get() {
            slot.set(Slot::Active { next, reload: period.ticks(), repeat, callback });
        }
        Ok(())
    }

    pub fn tick(&self, ports: &mut impl TimerPorts<C>) {
        if !self.initialized.get() { return; }
        for (index, slot) in self.slots.iter().enumerate() {
            if let Slot::Active { next, reload, repeat, callback } = slot.get() {
                let remaining = next.wrapping_sub(1);
                slot.set(Slot::Active { next: remaining, reload, repeat, callback });
                if remaining != 0 { continue; }
                if let Some(callback) = callback { ports.invoke(callback); }
                // A callback can cancel or replace the timer before this continuation.
                match slot.get() {
                    Slot::Active { reload, repeat: Repeat::Periodic, callback, .. } => {
                        slot.set(Slot::Active { next: reload, reload, repeat: Repeat::Periodic, callback });
                    }
                    Slot::Free if repeat == Repeat::Periodic => {}
                    _ => {
                        ports.release(SlotIndex(index as u8));
                        slot.set(Slot::Free);
                    }
                }
            }
        }
    }

    /// Releases the slots that hold a handle. A slot whose handle allocation failed stays claimed through
    /// shutdown and the next initialize, as in stock.
    pub fn shutdown(&self, ports: &mut impl TimerPorts<C>) {
        for (index, slot) in self.slots.iter().enumerate() {
            if let Slot::Active { .. } = slot.get() {
                slot.set(Slot::Free);
                ports.release(SlotIndex(index as u8));
            }
        }
        self.initialized.set(false);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DelayTask(u8);
impl DelayTask {
    pub const fn new(task: u8) -> Option<Self> {
        if task >= 1 && task <= 104 { Some(Self(task)) } else { None }
    }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DelayProgress { Idle, Waiting(u32), Complete }

pub struct Delays {
    tasks: [Cell<DelayProgress>; 104],
}
impl Default for Delays { fn default() -> Self { Self::new() } }
impl Delays {
    pub const fn new() -> Self { Self { tasks: [const { Cell::new(DelayProgress::Idle) }; 104] } }
    pub fn begin(&self, task: DelayTask, period: Milliseconds, timer_running: bool) {
        let extra = u32::from(period.0 != 0 && timer_running);
        self.tasks[usize::from(task.0 - 1)].set(DelayProgress::Waiting(period.ticks() + extra));
    }
    pub fn tick(&self, mut complete: impl FnMut(DelayTask)) -> bool {
        let mut had_waiter = false;
        for (index, task) in self.tasks.iter().enumerate() {
            if let DelayProgress::Waiting(remaining) = task.get() {
                had_waiter = true;
                let remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    complete(DelayTask(index as u8 + 1));
                    task.set(DelayProgress::Complete);
                } else { task.set(DelayProgress::Waiting(remaining)); }
            }
        }
        had_waiter
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DelayMessage { Tick, Unsupported }
pub trait TaskPorts<C: Copy> {
    fn invoke(&mut self, callback: C);
    fn fault(&mut self);
}
pub fn delay_message<C: Copy>(message: DelayMessage, callback: Option<C>, receive_failed: bool, ports: &mut impl TaskPorts<C>) {
    if receive_failed { ports.fault(); }
    match message {
        DelayMessage::Tick => { if let Some(callback) = callback { ports.invoke(callback); } }
        DelayMessage::Unsupported => ports.fault(),
    }
}
pub fn timer_message<C: Copy>(callback: Option<C>, ports: &mut impl TaskPorts<C>) {
    if let Some(callback) = callback { ports.invoke(callback); }
}

