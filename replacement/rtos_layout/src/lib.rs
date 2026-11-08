//! Stock data addresses and record views of the X-M1 1.50 RTOS kernels in loaders A and B (#156).
//! A view wraps a record's guest address and yields the guest addresses of its fields.
#![cfg_attr(not(test), no_std)]

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct GuestAddress(pub u32);

impl GuestAddress {
    #[inline(always)]
    pub const fn offset(self, bytes: u32) -> Self {
        Self(self.0.wrapping_add(bytes))
    }

    /// The physical spelling of a `0x40000000`-aliased guest address.
    #[inline(always)]
    pub const fn physical(self) -> u32 {
        if self.0 >= 0x4000_0000 && self.0 < 0x6000_0000 {
            self.0 - 0x4000_0000
        } else {
            self.0
        }
    }
}

#[path = "../../rtos_reservation/src/lib.rs"]
pub mod reservation;

pub use reservation::{
    Span, TaskPool, TimerArena, TimerArenas, TASK_POOL, TIMER_ARENA_CAPACITY, TIMER_ARENAS, TIMER_NODE_BYTES,
};

/// The loader whose kernel owns the data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    A,
    B,
}

/// Context record +0x00: `Own` in the executing kernel's record of its own domain.
/// Context record +0x04 and T+0x20 hold `A` or `B`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum DomainId {
    Own = 0,
    A = 1,
    B = 2,
}

impl DomainId {
    #[inline(always)]
    pub const fn from_word(word: u32) -> Option<Self> {
        match word {
            0 => Some(Self::Own),
            1 => Some(Self::A),
            2 => Some(Self::B),
            _ => None,
        }
    }

    #[inline(always)]
    pub const fn word(self) -> u32 {
        self as u32
    }
}

/// 1..=120.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct TaskSlot(u8);

impl TaskSlot {
    pub const COUNT: u32 = 120;

    #[inline(always)]
    pub const fn new(slot: u32) -> Option<Self> {
        if slot >= 1 && slot <= Self::COUNT {
            Some(Self(slot as u8))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

/// 1..=the semaphore count word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct SemaphoreSlot(u16);

impl SemaphoreSlot {
    pub const COUNT: u32 = 250;

    #[inline(always)]
    pub const fn new(slot: u32, count: u32) -> Option<Self> {
        if slot >= 1 && slot <= count && slot <= Self::COUNT {
            Some(Self(slot as u16))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

/// 0..=15; 0 is the most urgent.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct Priority(u8);

impl Priority {
    pub const COUNT: u32 = 16;

    #[inline(always)]
    pub const fn new(priority: u32) -> Option<Self> {
        if priority < Self::COUNT {
            Some(Self(priority as u8))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }

    /// The priority's bit in the ready bitmap.
    #[inline(always)]
    pub const fn bit(self) -> u32 {
        1 << self.0
    }
}

/// 1..=the eventflag count word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct FlagSlot(u16);

impl FlagSlot {
    pub const COUNT: u32 = 120;

    #[inline(always)]
    pub const fn new(slot: u32, count: u32) -> Option<Self> {
        if slot >= 1 && slot <= count && slot <= Self::COUNT {
            Some(Self(slot as u16))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

/// 1..=the mailbox count word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct MailboxSlot(u16);

impl MailboxSlot {
    pub const COUNT: u32 = 1;

    #[inline(always)]
    pub const fn new(slot: u32, count: u32) -> Option<Self> {
        if slot >= 1 && slot <= count && slot <= Self::COUNT {
            Some(Self(slot as u16))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

/// 1..=the cyclic count word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct CyclicSlot(u16);

impl CyclicSlot {
    pub const COUNT: u32 = 64;

    #[inline(always)]
    pub const fn new(slot: u32, count: u32) -> Option<Self> {
        if slot >= 1 && slot <= count && slot <= Self::COUNT {
            Some(Self(slot as u16))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

/// Head of an object's waiter queue, linked through T+0x00/+0x04.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct WaitQueue(pub GuestAddress);

impl WaitQueue {
    pub const SIZE: u32 = 0x0c;

    #[inline(always)]
    pub const fn next(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn prev(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    /// Pointer to the creation record; its attribute bit 0 selects priority order.
    #[inline(always)]
    pub const fn config(self) -> GuestAddress {
        self.0.offset(0x08)
    }
}

/// The per-wait record T+0x14 points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct WaitBlock(pub GuestAddress);

impl WaitBlock {
    pub const SIZE: u32 = 0x0c;

    /// Timer entry while waiting, then the result 0, -49 or -50.
    #[inline(always)]
    pub const fn result(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn queue(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    /// One-based owner domain of the object.
    #[inline(always)]
    pub const fn owner(self) -> GuestAddress {
        self.0.offset(0x08)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FlagConfig(pub GuestAddress);

impl FlagConfig {
    pub const SIZE: u32 = 0x0c;
    /// Attribute bit 0.
    pub const PRIORITY_ORDER: u32 = 0x01;
    /// Attribute bit 2; clears the whole pattern, not only the matched bits.
    pub const CLEAR_ON_MATCH: u32 = 0x04;

    #[inline(always)]
    pub const fn attributes(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn initial(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn exists(self) -> GuestAddress {
        self.0.offset(0x08)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FlagControl(pub GuestAddress);

impl FlagControl {
    pub const SIZE: u32 = 0x10;

    #[inline(always)]
    pub const fn queue(self) -> WaitQueue {
        WaitQueue(self.0)
    }

    #[inline(always)]
    pub const fn pattern(self) -> GuestAddress {
        self.0.offset(0x0c)
    }
}

/// A task's eventflag wait record, in a table indexed by the task slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FlagWait(pub GuestAddress);

impl FlagWait {
    pub const SIZE: u32 = 0x18;

    #[inline(always)]
    pub const fn base(self) -> WaitBlock {
        WaitBlock(self.0)
    }

    #[inline(always)]
    pub const fn pattern(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    #[inline(always)]
    pub const fn mode(self) -> GuestAddress {
        self.0.offset(0x10)
    }

    /// The pattern reported to the waiter when the wait is satisfied.
    #[inline(always)]
    pub const fn result(self) -> GuestAddress {
        self.0.offset(0x14)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct MailboxConfig(pub GuestAddress);

impl MailboxConfig {
    pub const SIZE: u32 = 0x0c;
    /// Attribute bit 0.
    pub const PRIORITY_ORDER: u32 = 0x01;
    /// Attribute bit 1; orders queued messages and enables `priority_limit`.
    pub const MESSAGE_PRIORITY: u32 = 0x02;

    #[inline(always)]
    pub const fn attributes(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn priority_limit(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn exists(self) -> GuestAddress {
        self.0.offset(0x08)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct MailboxControl(pub GuestAddress);

impl MailboxControl {
    pub const SIZE: u32 = 0x14;

    #[inline(always)]
    pub const fn queue(self) -> WaitQueue {
        WaitQueue(self.0)
    }

    #[inline(always)]
    pub const fn head(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    /// Written only by the FIFO append.
    #[inline(always)]
    pub const fn tail(self) -> GuestAddress {
        self.0.offset(0x10)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SemaphoreConfig(pub GuestAddress);

impl SemaphoreConfig {
    pub const SIZE: u32 = 0x10;
    /// Attribute bit 0; orders the waiter queue by priority.
    pub const PRIORITY_ORDER: u32 = 0x01;

    #[inline(always)]
    pub const fn attributes(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn initial(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn maximum(self) -> GuestAddress {
        self.0.offset(0x08)
    }

    #[inline(always)]
    pub const fn exists(self) -> GuestAddress {
        self.0.offset(0x0c)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SemaphoreControl(pub GuestAddress);

impl SemaphoreControl {
    pub const SIZE: u32 = 0x10;

    #[inline(always)]
    pub const fn queue(self) -> WaitQueue {
        WaitQueue(self.0)
    }

    #[inline(always)]
    pub const fn count(self) -> GuestAddress {
        self.0.offset(0x0c)
    }
}

/// A task's mailbox wait record, in a table indexed by the task slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct MailboxWait(pub GuestAddress);

impl MailboxWait {
    pub const SIZE: u32 = 0x10;

    #[inline(always)]
    pub const fn base(self) -> WaitBlock {
        WaitBlock(self.0)
    }

    /// Receives the message pointer when the wait is satisfied.
    #[inline(always)]
    pub const fn message(self) -> GuestAddress {
        self.0.offset(0x0c)
    }
}

/// A caller-owned message; the kernel overwrites `next` when it queues it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Message(pub GuestAddress);

impl Message {
    pub const SIZE: u32 = 0x08;

    #[inline(always)]
    pub const fn next(self) -> GuestAddress {
        self.0
    }

    /// Read only under mailbox attribute bit 1, as a signed word.
    #[inline(always)]
    pub const fn priority(self) -> GuestAddress {
        self.0.offset(0x04)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CyclicConfig(pub GuestAddress);

impl CyclicConfig {
    pub const SIZE: u32 = 0x18;
    /// Attribute bit 1; boot-time activation.
    pub const START: u32 = 0x02;

    #[inline(always)]
    pub const fn attributes(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn argument(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn handler(self) -> GuestAddress {
        self.0.offset(0x08)
    }

    #[inline(always)]
    pub const fn period(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    #[inline(always)]
    pub const fn phase(self) -> GuestAddress {
        self.0.offset(0x10)
    }

    #[inline(always)]
    pub const fn exists(self) -> GuestAddress {
        self.0.offset(0x14)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CyclicControl(pub GuestAddress);

impl CyclicControl {
    pub const SIZE: u32 = 0x18;

    #[inline(always)]
    pub const fn config(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn active(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn expiry(self) -> GuestAddress {
        self.0.offset(0x08)
    }

    #[inline(always)]
    pub const fn timer(self) -> GuestAddress {
        self.0.offset(0x0c)
    }
}

/// T+0x0c. `READY`, `WAIT` and `SUSPENDED` combine with each other and with the wait kinds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(transparent)]
pub struct TaskState(u8);

impl TaskState {
    pub const DORMANT: Self = Self(0);
    pub const READY: Self = Self(0x01);
    pub const WAIT: Self = Self(0x02);
    pub const SUSPENDED: Self = Self(0x04);
    /// Wait kind: sleep or delay.
    pub const SLEEPING: Self = Self(0x08);
    /// Wait kind: linked into an object's queue.
    pub const OBJECT_QUEUED: Self = Self(0x10);
    /// Wait kind: linked into a wait queue guarded by the object domain's object lock.
    pub const OWNER_LOCK_QUEUED: Self = Self(0x20);
    const DEFINED: u8 = 0x3f;

    #[inline(always)]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        if byte & !Self::DEFINED == 0 {
            Some(Self(byte))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[inline(always)]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[inline(always)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline(always)]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// T+0x0e. Bits 3-7 have no known reader or writer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(transparent)]
pub struct TaskFlags(pub u8);

impl TaskFlags {
    /// One queued activation request.
    pub const QUEUED_ACTIVATION: Self = Self(0x01);
    /// One queued wakeup request.
    pub const QUEUED_WAKEUP: Self = Self(0x02);
    pub const EXCEPTION_ENABLED: Self = Self(0x04);

    #[inline(always)]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The T record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Task(pub GuestAddress);

impl Task {
    pub const SIZE: u32 = 0x38;

    /// Ready list or wait queue link.
    #[inline(always)]
    pub const fn next(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn prev(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    /// Pointer to the task definition.
    #[inline(always)]
    pub const fn definition(self) -> GuestAddress {
        self.0.offset(0x08)
    }

    /// [`TaskState`] byte.
    #[inline(always)]
    pub const fn state(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    /// Current [`Priority`] byte.
    #[inline(always)]
    pub const fn priority(self) -> GuestAddress {
        self.0.offset(0x0d)
    }

    /// [`TaskFlags`] byte.
    #[inline(always)]
    pub const fn flags(self) -> GuestAddress {
        self.0.offset(0x0e)
    }

    /// Pending task exception bits.
    #[inline(always)]
    pub const fn exception_pattern(self) -> GuestAddress {
        self.0.offset(0x10)
    }

    /// Pointer to the wait record.
    #[inline(always)]
    pub const fn wait(self) -> GuestAddress {
        self.0.offset(0x14)
    }

    #[inline(always)]
    pub const fn saved_sp(self) -> GuestAddress {
        self.0.offset(0x18)
    }

    #[inline(always)]
    pub const fn resume_pc(self) -> GuestAddress {
        self.0.offset(0x1c)
    }

    /// [`DomainId`] `A` or `B`.
    #[inline(always)]
    pub const fn domain(self) -> GuestAddress {
        self.0.offset(0x20)
    }

    #[inline(always)]
    pub const fn terminate_pending(self) -> GuestAddress {
        self.0.offset(0x24)
    }

    /// Set while a priority change waits; the value is at [`Task::pending_priority`].
    #[inline(always)]
    pub const fn priority_change_pending(self) -> GuestAddress {
        self.0.offset(0x28)
    }

    #[inline(always)]
    pub const fn cancel_pending(self) -> GuestAddress {
        self.0.offset(0x2c)
    }

    /// Nonzero: the next switch away from the task does not save its registers.
    #[inline(always)]
    pub const fn context_discarded(self) -> GuestAddress {
        self.0.offset(0x30)
    }

    #[inline(always)]
    pub const fn pending_priority(self) -> GuestAddress {
        self.0.offset(0x34)
    }
}

/// The record [`Task::definition`] points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TaskDefinition(pub GuestAddress);

impl TaskDefinition {
    pub const SIZE: u32 = 0x28;

    #[inline(always)]
    pub const fn attributes(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn argument(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn entry(self) -> GuestAddress {
        self.0.offset(0x08)
    }

    /// Word whose low byte is the initial [`Priority`].
    #[inline(always)]
    pub const fn priority(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    /// Rounded stack bytes.
    #[inline(always)]
    pub const fn stack_size(self) -> GuestAddress {
        self.0.offset(0x10)
    }

    #[inline(always)]
    pub const fn stack(self) -> GuestAddress {
        self.0.offset(0x14)
    }

    #[inline(always)]
    pub const fn exception_attributes(self) -> GuestAddress {
        self.0.offset(0x18)
    }

    #[inline(always)]
    pub const fn exception_handler(self) -> GuestAddress {
        self.0.offset(0x1c)
    }

    /// One-based creation id.
    #[inline(always)]
    pub const fn slot(self) -> GuestAddress {
        self.0.offset(0x20)
    }

    #[inline(always)]
    pub const fn exists(self) -> GuestAddress {
        self.0.offset(0x24)
    }

    /// Two words whose wrapping sum is the initial saved SP.
    #[inline(always)]
    pub const fn initial_sp_terms(self) -> [GuestAddress; 2] {
        [self.0.offset(0x10), self.0.offset(0x14)]
    }
}

/// 1..=the fixed-pool count word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct FixedPoolSlot(u16);

impl FixedPoolSlot {
    pub const COUNT: u32 = 122;

    #[inline(always)]
    pub const fn new(slot: u32, count: u32) -> Option<Self> {
        if slot >= 1 && slot <= count && slot <= Self::COUNT {
            Some(Self(slot as u16))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FixedPoolConfig(pub GuestAddress);

impl FixedPoolConfig {
    pub const SIZE: u32 = 0x14;
    pub const PRIORITY_ORDER: u32 = 0x01;

    #[inline(always)]
    pub const fn attributes(self) -> GuestAddress {
        self.0
    }

    /// The rounded stride, `(size + 3) & !3`.
    #[inline(always)]
    pub const fn block_size(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn begin(self) -> GuestAddress {
        self.0.offset(0x08)
    }

    #[inline(always)]
    pub const fn end(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    #[inline(always)]
    pub const fn exists(self) -> GuestAddress {
        self.0.offset(0x10)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FixedPoolControl(pub GuestAddress);

impl FixedPoolControl {
    pub const SIZE: u32 = 0x14;

    #[inline(always)]
    pub const fn queue(self) -> WaitQueue {
        WaitQueue(self.0)
    }

    /// Next block the bump allocator has never handed out.
    #[inline(always)]
    pub const fn next_free(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    /// Head of the released-block list; each block's first word links the next.
    #[inline(always)]
    pub const fn recycled(self) -> GuestAddress {
        self.0.offset(0x10)
    }
}

/// A task's fixed-pool wait record, in a table indexed by the task slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FixedPoolWait(pub GuestAddress);

impl FixedPoolWait {
    pub const SIZE: u32 = 0x10;

    #[inline(always)]
    pub const fn base(self) -> WaitBlock {
        WaitBlock(self.0)
    }

    /// Receives the block address when the wait is satisfied.
    #[inline(always)]
    pub const fn block(self) -> GuestAddress {
        self.0.offset(0x0c)
    }
}

/// The creation-id map entry for one task slot; the stored word is the id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TaskCreationId(pub GuestAddress);

impl TaskCreationId {
    #[inline(always)]
    pub const fn word(self) -> GuestAddress {
        self.0
    }
}

/// The stock word `&g_default_stack_word` a reset definition points its stack at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct DefaultStackWord(pub GuestAddress);

impl DefaultStackWord {
    #[inline(always)]
    pub const fn address(self) -> GuestAddress {
        self.0
    }
}

/// Head of one circular ready list, linked through T+0x00/+0x04; an empty head points at itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct ReadyHead(pub GuestAddress);

impl ReadyHead {
    pub const SIZE: u32 = 8;

    #[inline(always)]
    pub const fn next(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn prev(self) -> GuestAddress {
        self.0.offset(0x04)
    }
}

/// `{word, acquire, release}` in a context record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct LockRecord(pub GuestAddress);

impl LockRecord {
    pub const SIZE: u32 = 0x0c;

    /// Pointer to the lock word.
    #[inline(always)]
    pub const fn word(self) -> GuestAddress {
        self.0
    }

    /// Pointer to the acquire function.
    #[inline(always)]
    pub const fn acquire(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    /// Pointer to the release function.
    #[inline(always)]
    pub const fn release(self) -> GuestAddress {
        self.0.offset(0x08)
    }
}

/// The context record describing one domain. Pointer fields hold addresses in the described
/// domain's kernel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct KernelContext(pub GuestAddress);

impl KernelContext {
    pub const SIZE: u32 = 0x84;

    /// [`DomainId`].
    #[inline(always)]
    pub const fn id(self) -> GuestAddress {
        self.0
    }

    /// [`DomainId`] `A` or `B` of the described domain.
    #[inline(always)]
    pub const fn owner(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn scheduler_lock(self) -> LockRecord {
        LockRecord(self.0.offset(0x08))
    }

    #[inline(always)]
    pub const fn object_lock(self) -> LockRecord {
        LockRecord(self.0.offset(0x14))
    }

    #[inline(always)]
    pub const fn ready_heads(self) -> GuestAddress {
        self.0.offset(0x20)
    }

    #[inline(always)]
    pub const fn ready_bitmap(self) -> GuestAddress {
        self.0.offset(0x24)
    }

    #[inline(always)]
    pub const fn running(self) -> GuestAddress {
        self.0.offset(0x28)
    }

    #[inline(always)]
    pub const fn selected(self) -> GuestAddress {
        self.0.offset(0x2c)
    }

    #[inline(always)]
    pub const fn dispatch_enabled(self) -> GuestAddress {
        self.0.offset(0x30)
    }

    #[inline(always)]
    pub const fn timer_heap(self) -> GuestAddress {
        self.0.offset(0x34)
    }

    #[inline(always)]
    pub const fn timer_count(self) -> GuestAddress {
        self.0.offset(0x38)
    }

    #[inline(always)]
    pub const fn timer_base(self) -> GuestAddress {
        self.0.offset(0x3c)
    }

    #[inline(always)]
    pub const fn tick_now(self) -> GuestAddress {
        self.0.offset(0x40)
    }

    #[inline(always)]
    pub const fn tasks(self) -> GuestAddress {
        self.0.offset(0x44)
    }

    #[inline(always)]
    pub const fn task_count(self) -> GuestAddress {
        self.0.offset(0x48)
    }

    #[inline(always)]
    pub const fn semaphores(self) -> GuestAddress {
        self.0.offset(0x4c)
    }

    #[inline(always)]
    pub const fn semaphore_count(self) -> GuestAddress {
        self.0.offset(0x50)
    }

    #[inline(always)]
    pub const fn event_flags(self) -> GuestAddress {
        self.0.offset(0x54)
    }

    #[inline(always)]
    pub const fn event_flag_count(self) -> GuestAddress {
        self.0.offset(0x58)
    }

    #[inline(always)]
    pub const fn data_queues(self) -> GuestAddress {
        self.0.offset(0x5c)
    }

    #[inline(always)]
    pub const fn data_queue_count(self) -> GuestAddress {
        self.0.offset(0x60)
    }

    #[inline(always)]
    pub const fn mailboxes(self) -> GuestAddress {
        self.0.offset(0x64)
    }

    #[inline(always)]
    pub const fn mailbox_count(self) -> GuestAddress {
        self.0.offset(0x68)
    }

    #[inline(always)]
    pub const fn fixed_pools(self) -> GuestAddress {
        self.0.offset(0x6c)
    }

    #[inline(always)]
    pub const fn fixed_pool_count(self) -> GuestAddress {
        self.0.offset(0x70)
    }

    #[inline(always)]
    pub const fn cyclics(self) -> GuestAddress {
        self.0.offset(0x74)
    }

    #[inline(always)]
    pub const fn cyclic_count(self) -> GuestAddress {
        self.0.offset(0x78)
    }

    /// Pointer to the task first-entry trampoline.
    #[inline(always)]
    pub const fn first_entry(self) -> GuestAddress {
        self.0.offset(0x7c)
    }

    #[inline(always)]
    pub const fn cyclic_callback(self) -> GuestAddress {
        self.0.offset(0x80)
    }
}

/// Stock data words of one loader's kernel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainWords {
    /// Nonzero while a handler runs.
    pub handler_context: GuestAddress,
    pub scheduler_lock: GuestAddress,
    pub object_lock: GuestAddress,
    /// Pointer to the executing kernel's [`KernelContext`] of its own domain.
    pub current_context: GuestAddress,
    /// Two [`KernelContext`] records, A's then B's.
    pub contexts: GuestAddress,
    /// Set by the IRQ 120 handler, consumed at the outermost IRQ exit.
    pub reschedule: GuestAddress,
    /// [`Priority::COUNT`] [`ReadyHead`]s.
    pub ready_heads: GuestAddress,
    pub ready_bitmap: GuestAddress,
    pub running: GuestAddress,
    pub dispatch_enabled: GuestAddress,
    pub selected: GuestAddress,
    pub timer_base: GuestAddress,
    pub tick_now: GuestAddress,
    pub time_offset: GuestAddress,
    pub timer_count: GuestAddress,
    pub timer_heap: GuestAddress,
    /// [`FixedPoolSlot::COUNT`] creation records.
    pub fixed_pool_configs: GuestAddress,
    pub fixed_pool_controls: GuestAddress,
    pub fixed_pool_waits: GuestAddress,
    pub fixed_pool_timers: GuestAddress,
    /// The one-word block a deleted pool's creation record spans.
    pub default_pool_storage: GuestAddress,
    /// [`TaskSlot::COUNT`] [`Task`] records.
    pub task_table: GuestAddress,
    /// [`SemaphoreSlot::COUNT`] creation records.
    pub semaphore_configs: GuestAddress,
    pub semaphore_controls: GuestAddress,
    /// [`TaskSlot::COUNT`] [`WaitBlock`]s, indexed by the waiting task.
    pub semaphore_waits: GuestAddress,
    pub semaphore_timers: GuestAddress,
    /// [`TaskSlot::COUNT`] [`TaskDefinition`] records.
    pub task_definitions: GuestAddress,
    /// [`TaskSlot::COUNT`] [`TaskCreationId`] words.
    pub task_slot_ids: GuestAddress,
    /// `&g_default_stack_word`.
    pub default_stack_word: DefaultStackWord,
    /// [`FlagSlot::COUNT`] creation records.
    pub flag_configs: GuestAddress,
    pub flag_controls: GuestAddress,
    /// [`TaskSlot::COUNT`] wait records, indexed by the waiting task.
    pub flag_waits: GuestAddress,
    pub flag_timers: GuestAddress,
    /// [`MailboxSlot::COUNT`] creation records.
    pub mailbox_configs: GuestAddress,
    pub mailbox_controls: GuestAddress,
    pub mailbox_waits: GuestAddress,
    pub mailbox_timers: GuestAddress,
    /// [`CyclicSlot::COUNT`] creation records.
    pub cyclic_configs: GuestAddress,
    /// [`CyclicSlot::COUNT`] control records.
    pub cyclic_controls: GuestAddress,
}

impl DomainWords {
    #[inline(always)]
    pub const fn context(&self, of: Domain) -> KernelContext {
        let index = match of {
            Domain::A => 0,
            Domain::B => 1,
        };
        KernelContext(self.contexts.offset(index * KernelContext::SIZE))
    }

    #[inline(always)]
    pub const fn task(&self, slot: TaskSlot) -> Task {
        Task(self.task_table.offset((slot.get() - 1) * Task::SIZE))
    }

    #[inline(always)]
    pub const fn task_definition(&self, slot: TaskSlot) -> TaskDefinition {
        TaskDefinition(self.task_definitions.offset((slot.get() - 1) * TaskDefinition::SIZE))
    }

    #[inline(always)]
    pub const fn task_creation_id(&self, slot: TaskSlot) -> TaskCreationId {
        TaskCreationId(self.task_slot_ids.offset((slot.get() - 1) * 4))
    }

    /// `None` unless `task` is a record start in this table.
    #[inline(always)]
    pub const fn task_slot(&self, task: Task) -> Option<TaskSlot> {
        let bytes = task.0 .0.wrapping_sub(self.task_table.0);
        if bytes % Task::SIZE != 0 {
            return None;
        }
        TaskSlot::new(bytes / Task::SIZE + 1)
    }

    #[inline(always)]
    pub const fn ready_head(&self, priority: Priority) -> ReadyHead {
        ReadyHead(self.ready_heads.offset(priority.get() * ReadyHead::SIZE))
    }

    #[inline(always)]
    pub const fn semaphore_config(&self, slot: SemaphoreSlot) -> SemaphoreConfig {
        SemaphoreConfig(self.semaphore_configs.offset((slot.get() - 1) * SemaphoreConfig::SIZE))
    }

    #[inline(always)]
    pub const fn semaphore_control(&self, slot: SemaphoreSlot) -> SemaphoreControl {
        SemaphoreControl(self.semaphore_controls.offset((slot.get() - 1) * SemaphoreControl::SIZE))
    }

    #[inline(always)]
    pub const fn semaphore_wait(&self, task: TaskSlot) -> WaitBlock {
        WaitBlock(self.semaphore_waits.offset((task.get() - 1) * WaitBlock::SIZE))
    }

    #[inline(always)]
    pub const fn semaphore_timer(&self, task: TaskSlot) -> GuestAddress {
        self.semaphore_timers.offset((task.get() - 1) * TIMER_ENTRY_SIZE)
    }

    #[inline(always)]
    pub const fn fixed_pool_config(&self, slot: FixedPoolSlot) -> FixedPoolConfig {
        FixedPoolConfig(self.fixed_pool_configs.offset((slot.get() - 1) * FixedPoolConfig::SIZE))
    }

    #[inline(always)]
    pub const fn fixed_pool_control(&self, slot: FixedPoolSlot) -> FixedPoolControl {
        FixedPoolControl(self.fixed_pool_controls.offset((slot.get() - 1) * FixedPoolControl::SIZE))
    }

    #[inline(always)]
    pub const fn fixed_pool_wait(&self, task: TaskSlot) -> FixedPoolWait {
        FixedPoolWait(self.fixed_pool_waits.offset((task.get() - 1) * FixedPoolWait::SIZE))
    }

    #[inline(always)]
    pub const fn fixed_pool_timer(&self, task: TaskSlot) -> GuestAddress {
        self.fixed_pool_timers.offset((task.get() - 1) * TIMER_ENTRY_SIZE)
    }

    #[inline(always)]
    pub const fn flag_config(&self, slot: FlagSlot) -> FlagConfig {
        FlagConfig(self.flag_configs.offset((slot.get() - 1) * FlagConfig::SIZE))
    }

    #[inline(always)]
    pub const fn flag_control(&self, slot: FlagSlot) -> FlagControl {
        FlagControl(self.flag_controls.offset((slot.get() - 1) * FlagControl::SIZE))
    }

    #[inline(always)]
    pub const fn flag_wait(&self, task: TaskSlot) -> FlagWait {
        FlagWait(self.flag_waits.offset((task.get() - 1) * FlagWait::SIZE))
    }

    #[inline(always)]
    pub const fn flag_timer(&self, task: TaskSlot) -> GuestAddress {
        self.flag_timers.offset((task.get() - 1) * TIMER_ENTRY_SIZE)
    }

    #[inline(always)]
    pub const fn mailbox_config(&self, slot: MailboxSlot) -> MailboxConfig {
        MailboxConfig(self.mailbox_configs.offset((slot.get() - 1) * MailboxConfig::SIZE))
    }

    #[inline(always)]
    pub const fn mailbox_control(&self, slot: MailboxSlot) -> MailboxControl {
        MailboxControl(self.mailbox_controls.offset((slot.get() - 1) * MailboxControl::SIZE))
    }

    #[inline(always)]
    pub const fn mailbox_wait(&self, task: TaskSlot) -> MailboxWait {
        MailboxWait(self.mailbox_waits.offset((task.get() - 1) * MailboxWait::SIZE))
    }

    #[inline(always)]
    pub const fn mailbox_timer(&self, task: TaskSlot) -> GuestAddress {
        self.mailbox_timers.offset((task.get() - 1) * TIMER_ENTRY_SIZE)
    }

    #[inline(always)]
    pub const fn cyclic_config(&self, slot: CyclicSlot) -> CyclicConfig {
        CyclicConfig(self.cyclic_configs.offset((slot.get() - 1) * CyclicConfig::SIZE))
    }

    #[inline(always)]
    pub const fn cyclic_control(&self, slot: CyclicSlot) -> CyclicControl {
        CyclicControl(self.cyclic_controls.offset((slot.get() - 1) * CyclicControl::SIZE))
    }
}

/// A heap entry: one-based slot, callback and argument.
pub const TIMER_ENTRY_SIZE: u32 = 0x0c;

impl Domain {
    #[inline(always)]
    pub const fn id(self) -> DomainId {
        match self {
            Domain::A => DomainId::A,
            Domain::B => DomainId::B,
        }
    }

    #[inline(always)]
    pub const fn timer_arena(self) -> TimerArena {
        match self {
            Domain::A => TIMER_ARENAS.a,
            Domain::B => TIMER_ARENAS.b,
        }
    }

    #[inline(always)]
    pub const fn words(self) -> DomainWords {
        match self {
            Domain::A => DomainWords {
                handler_context: GuestAddress(0x4005_45a4),
                scheduler_lock: GuestAddress(0x4005_45c0),
                object_lock: GuestAddress(0x4005_45c4),
                current_context: GuestAddress(0x4005_45c8),
                contexts: GuestAddress(0x4005_45cc),
                reschedule: GuestAddress(0x4005_46d8),
                ready_heads: GuestAddress(0x4005_46dc),
                ready_bitmap: GuestAddress(0x4005_475c),
                running: GuestAddress(0x4005_4760),
                dispatch_enabled: GuestAddress(0x4005_4764),
                selected: GuestAddress(0x4005_4768),
                timer_base: GuestAddress(0x4005_476c),
                tick_now: GuestAddress(0x4005_4770),
                time_offset: GuestAddress(0x4005_4774),
                timer_count: GuestAddress(0x4005_4778),
                timer_heap: GuestAddress(0x4005_d718),
                fixed_pool_configs: GuestAddress(0x4006_194c),
                fixed_pool_controls: GuestAddress(0x4005_f584),
                fixed_pool_waits: GuestAddress(0x4005_855c),
                fixed_pool_timers: GuestAddress(0x4005_8cdc),
                default_pool_storage: GuestAddress(0x4006_2c4c),
                task_table: GuestAddress(0x4005_ff0c),
                semaphore_configs: GuestAddress(0x4005_dfd8),
                semaphore_controls: GuestAddress(0x4005_af0c),
                semaphore_waits: GuestAddress(0x4005_4efc),
                semaphore_timers: GuestAddress(0x4005_549c),
                task_definitions: GuestAddress(0x4005_beac),
                task_slot_ids: GuestAddress(0x4006_22d4),
                default_stack_word: DefaultStackWord(GuestAddress(0x4006_2c50)),
                flag_configs: GuestAddress(0x4005_d178),
                flag_controls: GuestAddress(0x4006_24b4),
                flag_waits: GuestAddress(0x4005_5fdc),
                flag_timers: GuestAddress(0x4005_5a3c),
                mailbox_configs: GuestAddress(0x4005_d16c),
                mailbox_controls: GuestAddress(0x4006_2c34),
                mailbox_waits: GuestAddress(0x4005_783c),
                mailbox_timers: GuestAddress(0x4005_7fbc),
                cyclic_configs: GuestAddress(0x4005_ef78),
                cyclic_controls: GuestAddress(0x4005_928c),
            },
            Domain::B => DomainWords {
                handler_context: GuestAddress(0x400e_4564),
                scheduler_lock: GuestAddress(0x400e_4580),
                object_lock: GuestAddress(0x400e_4584),
                current_context: GuestAddress(0x400e_4588),
                contexts: GuestAddress(0x400e_458c),
                reschedule: GuestAddress(0x400e_4698),
                ready_heads: GuestAddress(0x400e_469c),
                ready_bitmap: GuestAddress(0x400e_471c),
                running: GuestAddress(0x400e_4720),
                dispatch_enabled: GuestAddress(0x400e_4724),
                selected: GuestAddress(0x400e_4728),
                timer_base: GuestAddress(0x400e_472c),
                tick_now: GuestAddress(0x400e_4730),
                time_offset: GuestAddress(0x400e_4734),
                timer_count: GuestAddress(0x400e_4738),
                timer_heap: GuestAddress(0x400e_d6d8),
                fixed_pool_configs: GuestAddress(0x400f_190c),
                fixed_pool_controls: GuestAddress(0x400e_f544),
                fixed_pool_waits: GuestAddress(0x400e_851c),
                fixed_pool_timers: GuestAddress(0x400e_8c9c),
                default_pool_storage: GuestAddress(0x400f_2c0c),
                task_table: GuestAddress(0x400e_fecc),
                semaphore_configs: GuestAddress(0x400e_df98),
                semaphore_controls: GuestAddress(0x400e_aecc),
                semaphore_waits: GuestAddress(0x400e_4ebc),
                semaphore_timers: GuestAddress(0x400e_545c),
                task_definitions: GuestAddress(0x400e_be6c),
                task_slot_ids: GuestAddress(0x400f_2294),
                default_stack_word: DefaultStackWord(GuestAddress(0x400f_2c10)),
                flag_configs: GuestAddress(0x400e_d138),
                flag_controls: GuestAddress(0x400f_2474),
                flag_waits: GuestAddress(0x400e_5f9c),
                flag_timers: GuestAddress(0x400e_59fc),
                mailbox_configs: GuestAddress(0x400e_d12c),
                mailbox_controls: GuestAddress(0x400f_2bf4),
                mailbox_waits: GuestAddress(0x400e_77fc),
                mailbox_timers: GuestAddress(0x400e_7f7c),
                cyclic_configs: GuestAddress(0x400e_ef38),
                cyclic_controls: GuestAddress(0x400e_924c),
            },
        }
    }
}
