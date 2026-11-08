use crate::layout::{
    Domain, FixedPoolWait, GuestAddress, KernelContext, LockRecord, Task, WaitQueue,
};

/// The I/F bits the stock mask pair returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedInterrupts(pub u32);

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_byte(&self, at: GuestAddress, value: u8);
    fn mask_interrupts(&self) -> SavedInterrupts;
    fn restore_interrupts(&self, saved: SavedInterrupts);
    fn dmb(&self);
}

pub trait Locks {
    fn lock(&self, record: LockRecord);
    fn unlock(&self, record: LockRecord);
}

pub trait Irq {
    fn disable_irq(&self);
    fn enable_irq(&self);
}

/// `+0x170`'s I-bit pair.
pub trait SaveIrq {
    fn save_irq(&self) -> u32;
    fn restore_irq(&self, saved: u32);
}

/// The retained `__aeabi_uidivmod` pair.
pub trait DivMod {
    fn divmod(&self, numerator: u32, denominator: u32) -> (u32, u32);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    Unchanged,
    Changed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispatchTarget {
    Local,
    Remote,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Enqueue {
    Plain,
    Timed { timer: GuestAddress, timeout: i32 },
}

pub trait Wake: Machine + Locks + Irq {
    fn context(&self, domain: Domain) -> KernelContext;
    /// T+0x20.
    fn waiter_domain(&self, task: Task) -> Domain;
    /// The owner record's `+0x00` word.
    fn record_id(&self, owner: KernelContext) -> u32;
    /// A `0x44018` / B `0xd3fd4`.
    fn notify(&self, id: u32) -> DispatchTarget;
    /// A `0x43640` / B `0xd3640`; the entry overwrites r0 before selecting a task.
    fn dispatch(&self, code: i32);
    /// The entry domain's reschedule word, consumed at the outermost IRQ exit.
    fn request_delayed_dispatch(&self);
    /// A `0x44568` / B `0xd4528`.
    fn release(&self, owner: KernelContext, task: Task) -> Selection;
    /// A `0x44744` / B `0xd4704`.
    fn cancel(&self, owner: KernelContext, task: Task) -> Selection;
}

pub trait Block: Wake {
    /// A `0x44800`/`0x448e4` or B `0xd47c0`/`0xd48a4`; `owner` is the wait block's domain id.
    fn enqueue(
        &self,
        entry: Domain,
        owner: u32,
        queue: WaitQueue,
        wait: FixedPoolWait,
        how: Enqueue,
    );
}
