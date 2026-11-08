//! Owned X-M1 1.50 exception entry and exit decisions (#156). `stage/carrier.S`
//! keeps the banked save, mode and CPSR switches, frame and exception return.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub use layout::{Domain, DomainWords, GuestAddress};

/// Loader A's CP14 discriminator selects `0x11`, any other value `0x12`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum IrqTag {
    Primary = 0x11,
    Secondary = 0x12,
}

impl IrqTag {
    #[inline(always)]
    pub const fn word(self) -> u32 {
        self as u32
    }
}

/// The word the IRQ controller returns at bank `+0x200`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct VectorWord(pub u32);

impl VectorWord {
    pub const NONE: Self = Self(0);
    /// The null-vector marker that the stock acknowledge logs as code 6.
    pub const UNHANDLED: Self = Self(0x42840);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct IrqLine(u32);

impl IrqLine {
    pub const COUNT: u32 = 128;

    #[inline(always)]
    pub const fn new(line: u32) -> Option<Self> {
        if line < Self::COUNT {
            Some(Self(line))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub const fn group(self) -> u32 {
        self.0 / 32
    }

    #[inline(always)]
    pub const fn bit(self) -> u32 {
        1 << (self.0 % 32)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
}

/// The common dispatcher's fault kinds; code 6 is the acknowledge's log code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum FaultKind {
    Undefined = 1,
    Svc = 2,
    PrefetchAbort = 3,
    DataAbort = 4,
    Reserved = 5,
}

/// `spsr`, `r0`..`r3`, `r12`, `lr`, `pc`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct IrqFrame(pub GuestAddress);

impl IrqFrame {
    pub const WORDS: u32 = 8;

    #[inline(always)]
    pub const fn spsr(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn pc(self) -> GuestAddress {
        self.0.offset(0x1c)
    }
}

/// `r0`..`r12`, `lr`, `spsr`, then the two words the vector pushed first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FaultFrame(pub GuestAddress);

impl FaultFrame {
    pub const WORDS: u32 = 16;

    #[inline(always)]
    pub const fn pc(self) -> GuestAddress {
        self.0.offset(0x3c)
    }
}

/// Selects the carrier's dispatch branch, task-stack restore or nested return.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ExitClass {
    Nested = 0,
    Dispatch = 1,
    OuterReturn = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryClass {
    Outer,
    Nested,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Bank(GuestAddress);

impl Bank {
    pub const PRIMARY: Self = Self(GuestAddress(0xfffb_3000));
    pub const SECONDARY: Self = Self(GuestAddress(0xfffb_4000));

    #[inline(always)]
    pub const fn of(primary: bool) -> Self {
        if primary {
            Self::PRIMARY
        } else {
            Self::SECONDARY
        }
    }

    #[inline(always)]
    pub const fn address(self) -> GuestAddress {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExceptionDomain {
    pub words: DomainWords,
    /// The one word both cores serialize on.
    pub lock_word: GuestAddress,
    /// Sixteen `{word, line}` slots indexed by priority.
    pub active_irqs: GuestAddress,
    /// Incremented when `+0x200` reads zero or the null marker.
    pub unhandled: GuestAddress,
    /// 128 24-byte handler records.
    pub handler_table: GuestAddress,
    /// The 17-word record the common dispatcher publishes.
    pub fault_record: GuestAddress,
    /// Called as `hook(kind, pc)` when nonzero.
    pub exception_hook: GuestAddress,
    /// Nonzero selects the dispatcher's resume branch.
    pub resume_gate: GuestAddress,
    /// Called as `hook(pc)` by the FIQ vector when nonzero.
    pub fiq_hook: GuestAddress,
    /// The controller's null-vector marker for this loader.
    pub unhandled_vector: VectorWord,
    /// The outermost entry saves the frame pointer at `irq_stack_top - 4`.
    pub irq_stack_top: u32,
}

impl ExceptionDomain {
    pub const fn of(domain: Domain) -> Self {
        match domain {
            Domain::A => Self {
                words: Domain::A.words(),
                lock_word: GuestAddress(0x400c_0804),
                active_irqs: GuestAddress(0x4005_4500),
                unhandled: GuestAddress(0x4005_44fc),
                handler_table: GuestAddress(0x4005_38fc),
                fault_record: GuestAddress(0x4004_f118),
                exception_hook: GuestAddress(0x4004_f114),
                resume_gate: GuestAddress(0x4004_f10c),
                fiq_hook: GuestAddress(0x4004_f110),
                unhandled_vector: VectorWord(0x42840),
                irq_stack_top: 0x7000_2000,
            },
            Domain::B => Self {
                words: Domain::B.words(),
                lock_word: GuestAddress(0x400c_0804),
                active_irqs: GuestAddress(0x400e_44c0),
                unhandled: GuestAddress(0x400e_44bc),
                handler_table: GuestAddress(0x400e_38bc),
                fault_record: GuestAddress(0x400d_f0d8),
                exception_hook: GuestAddress(0x400d_f0d4),
                resume_gate: GuestAddress(0x400d_f0cc),
                fiq_hook: GuestAddress(0x400d_f0d0),
                unhandled_vector: VectorWord(0xd2840),
                irq_stack_top: 0x7000_2000,
            },
        }
    }
}

/// The target maps these to retained helpers, CP15 and memory; the host model
/// uses a local monitor.
pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn try_acquire(&self, word: GuestAddress, stamp: IrqTag) -> bool;
    fn release(&self, word: GuestAddress);
    /// `0x427b8`: true when the CP14 discriminator is `0x4003`.
    fn primary_identity(&self) -> bool;
    fn set_tpidrurw_bit31(&self);
    fn clear_tpidrurw_bit31(&self);
    fn log(&self, code: u32, value: u32);
    fn call(&self, address: u32, first: u32, second: u32);
}

/// Stock `0x40234..0x40254`. The carrier performs the IRQ-stack switch.
#[inline(always)]
pub fn enter<M: Machine>(m: &M, domain: &ExceptionDomain) -> EntryClass {
    let old = m.load_word(domain.words.handler_context);
    m.store_word(domain.words.handler_context, old.wrapping_add(1));
    if old == 0 {
        EntryClass::Outer
    } else {
        EntryClass::Nested
    }
}

/// Stock `0x40258..0x402bc`. A free word retries the exclusive pair without
/// re-observing; a foreign tag re-observes.
#[inline(always)]
pub fn lock_acquire<M: Machine>(m: &M, domain: &ExceptionDomain) {
    let stamp = if m.primary_identity() {
        IrqTag::Primary
    } else {
        IrqTag::Secondary
    };
    loop {
        let observed = m.load_word(domain.lock_word);
        if observed == 0 {
            loop {
                if m.try_acquire(domain.lock_word, stamp) {
                    return;
                }
            }
        } else if observed == stamp.word() {
            return;
        }
    }
}

#[inline(always)]
fn bank<M: Machine>(m: &M) -> Bank {
    Bank::of(m.primary_identity())
}

/// The highest set bit of the first nonzero `+0x230` group.
#[inline(always)]
fn first_line<M: Machine>(m: &M, bank: Bank) -> Option<IrqLine> {
    let base = bank.address();
    let mut group = 0;
    while group < 4 {
        let bits = m.load_word(base.offset(0x230 + 4 * group));
        if bits != 0 {
            return IrqLine::new(group * 32 + (31 - bits.leading_zeros()));
        }
        group += 1;
    }
    None
}

#[inline(always)]
fn priority<M: Machine>(m: &M, domain: &ExceptionDomain, line: IrqLine) -> Priority {
    let raw = m.load_word(domain.handler_table.offset(24 * line.get() + 8));
    Priority::new(raw & (Priority::COUNT - 1)).unwrap_or(Priority(15))
}

/// Stock `0x430a8`. No pending bit is cleared here.
#[inline(always)]
pub fn acknowledge<M: Machine>(m: &M, domain: &ExceptionDomain) -> VectorWord {
    let bank = bank(m);
    let base = bank.address();
    let vector = VectorWord(m.load_word(base.offset(0x200)));
    if vector == VectorWord::NONE || vector == domain.unhandled_vector {
        let count = m.load_word(domain.unhandled).wrapping_add(1);
        m.store_word(domain.unhandled, count);
        m.log(6, count);
        return VectorWord::NONE;
    }
    if let Some(line) = first_line(m, bank) {
        m.set_tpidrurw_bit31();
        let slot = domain.active_irqs.offset(8 * priority(m, domain, line).get());
        m.store_word(slot, m.load_word(base.offset(0x1c0)));
        m.store_word(slot.offset(4), line.get());
    }
    vector
}

/// Stock `0x431d8`. The line is re-derived from the active bitmap, not from the
/// acknowledged vector.
#[inline(always)]
pub fn end_of_interrupt<M: Machine>(m: &M, domain: &ExceptionDomain) {
    let bank = bank(m);
    let base = bank.address();
    if let Some(line) = first_line(m, bank) {
        m.store_word(base.offset(0x120 + 4 * line.group()), line.bit());
        let slot = domain.active_irqs.offset(8 * priority(m, domain, line).get());
        if m.load_word(domain.words.handler_context) == 1 {
            m.clear_tpidrurw_bit31();
        }
        m.store_word(slot, 0);
        m.store_word(slot.offset(4), u32::MAX);
    }
    m.store_word(base.offset(0x200), 0);
}

/// Stock `0x402f8..0x40348`. The release precedes the decrement; only a zero
/// counter consumes the dispatch flag.
#[inline(always)]
pub fn exit<M: Machine>(m: &M, domain: &ExceptionDomain) -> ExitClass {
    let remaining = m.load_word(domain.words.handler_context).wrapping_sub(1);
    if remaining == 0 {
        m.release(domain.lock_word);
    }
    m.store_word(domain.words.handler_context, remaining);
    if remaining != 0 {
        return ExitClass::Nested;
    }
    if m.load_word(domain.words.reschedule) != 0 {
        m.store_word(domain.words.reschedule, 0);
        return ExitClass::Dispatch;
    }
    ExitClass::OuterReturn
}

/// Stock `0x40cac`'s record, log and hook. The carrier owns the resume gate and
/// halt.
#[inline(always)]
pub fn record_fault<M: Machine>(m: &M, domain: &ExceptionDomain, kind: FaultKind, frame: FaultFrame) {
    let hook = record_fault_raw(m, domain, kind as u32, frame);
    if hook != 0 {
        m.call(hook, kind as u32, m.load_word(frame.pc()));
    }
}

#[inline(always)]
pub fn record_fault_raw<M: Machine>(m: &M, domain: &ExceptionDomain, kind: u32, frame: FaultFrame) -> u32 {
    m.store_word(domain.fault_record, kind);
    let mut index = 0;
    while index < FaultFrame::WORDS {
        m.store_word(domain.fault_record.offset(4 + 4 * index), m.load_word(frame.0.offset(4 * index)));
        index += 1;
    }
    m.log(kind, m.load_word(frame.pc()));
    m.load_word(domain.exception_hook)
}

/// Stock `0x40d70`: the published hook receives the saved PC only.
#[inline(always)]
pub fn fiq_hook<M: Machine>(m: &M, domain: &ExceptionDomain, frame: FaultFrame) {
    let hook = m.load_word(domain.fiq_hook);
    if hook != 0 {
        m.call(hook, m.load_word(frame.pc()), 0);
    }
}

#[cfg(test)]
mod tests;
