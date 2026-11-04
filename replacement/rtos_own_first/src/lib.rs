//! Owned bodies for the 17 own-first RTOS root slots of X-M1 1.50 loaders A and B (#156).
//! Target entries run each slot over the CPU's [`Machine`]; host tests run it over a model.
#![cfg_attr(not(test), no_std)]

#[cfg(all(target_arch = "arm", not(test)))]
mod entries;
#[cfg(test)]
mod tests;

#[cfg(all(target_arch = "arm", not(any(feature = "domain-a", feature = "domain-b"))))]
compile_error!("select one of feature=\"domain-a\" or feature=\"domain-b\"");
#[cfg(all(feature = "domain-a", feature = "domain-b"))]
compile_error!("feature=\"domain-a\" and feature=\"domain-b\" are exclusive");

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct GuestAddress(pub u32);

impl GuestAddress {
    #[inline(always)]
    pub const fn offset(self, bytes: u32) -> Self {
        Self(self.0.wrapping_add(bytes))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Cpsr(pub u32);

impl Cpsr {
    pub const IRQ_MASK: u32 = 0x80;

    #[inline(always)]
    pub const fn irq_masked(self) -> bool {
        self.0 & Self::IRQ_MASK != 0
    }

    #[inline(always)]
    pub const fn with_irq_masked(self, masked: bool) -> Self {
        if masked {
            Self(self.0 | Self::IRQ_MASK)
        } else {
            Self(self.0 & !Self::IRQ_MASK)
        }
    }
}

/// `MRC p14, 0, Rt, c2, c0, 0`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct DebugIdentity(pub u32);

impl DebugIdentity {
    pub const PRIMARY: Self = Self(0x4003);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct ServiceStatus(pub i32);

impl ServiceStatus {
    pub const OK: Self = Self(0);
    pub const CONTEXT_ERROR: Self = Self(-25);
}

/// 0 or 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Flag(pub u32);

impl Flag {
    #[inline(always)]
    pub const fn from_bool(value: bool) -> Self {
        Self(value as u32)
    }
}

/// CPSR bit 7 alone: 0 or 0x80.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedIrqMask(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TaskId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SystemTime(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    A,
    B,
}

/// Stock data words of one loader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainWords {
    /// Nonzero while a handler runs.
    pub handler_context: GuestAddress,
    pub current_task: GuestAddress,
    pub dispatch_enabled: GuestAddress,
    pub timer_base: GuestAddress,
    pub time_offset: GuestAddress,
    /// Task record 0; records are 56 bytes.
    pub task_table: GuestAddress,
}

impl Domain {
    pub const fn words(self) -> DomainWords {
        match self {
            Domain::A => DomainWords {
                handler_context: GuestAddress(0x4005_45a4),
                current_task: GuestAddress(0x4005_4760),
                dispatch_enabled: GuestAddress(0x4005_4764),
                timer_base: GuestAddress(0x4005_476c),
                time_offset: GuestAddress(0x4005_4774),
                task_table: GuestAddress(0x4005_ff0c),
            },
            Domain::B => DomainWords {
                handler_context: GuestAddress(0x400e_4564),
                current_task: GuestAddress(0x400e_4720),
                dispatch_enabled: GuestAddress(0x400e_4724),
                timer_base: GuestAddress(0x400e_472c),
                time_offset: GuestAddress(0x400e_4734),
                task_table: GuestAddress(0x400e_fecc),
            },
        }
    }
}

#[cfg(feature = "domain-a")]
pub const DOMAIN: Domain = Domain::A;
#[cfg(feature = "domain-b")]
pub const DOMAIN: Domain = Domain::B;

const TASK_FLAGS_BYTE: u32 = 0xe;
const TASK_FLAG_BIT2: u8 = 0x4;
const INVERSE_OF_7: u32 = 0xb6db_6db7;

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn cpsr(&self) -> Cpsr;
    /// `MSR CPSR_c`: replaces bits 7:0 only.
    fn write_cpsr_control(&self, value: Cpsr);
    fn debug_identity(&self) -> DebugIdentity;
}

/// The handler-context word value a service admits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Context {
    Task,
    Handler,
}

impl Context {
    #[inline(always)]
    fn admits(self, handler_context: u32) -> bool {
        match self {
            Context::Task => handler_context == 0,
            Context::Handler => handler_context != 0,
        }
    }
}

/// Runs `body` with IRQ masked when the context word matches and IRQ is unmasked at entry.
#[inline(always)]
fn masked_service<M: Machine>(m: &M, w: &DomainWords, context: Context, body: impl FnOnce()) -> ServiceStatus {
    if !context.admits(m.load_word(w.handler_context)) {
        return ServiceStatus::CONTEXT_ERROR;
    }
    let entry = m.cpsr();
    if entry.irq_masked() {
        return ServiceStatus::CONTEXT_ERROR;
    }
    m.write_cpsr_control(entry.with_irq_masked(true));
    body();
    m.write_cpsr_control(entry);
    ServiceStatus::OK
}

#[inline(always)]
fn set_cpu_lock<M: Machine>(m: &M, w: &DomainWords, context: Context, locked: bool) -> ServiceStatus {
    if !context.admits(m.load_word(w.handler_context)) {
        return ServiceStatus::CONTEXT_ERROR;
    }
    let entry = m.cpsr();
    if entry.irq_masked() != locked {
        m.write_cpsr_control(entry.with_irq_masked(locked));
    }
    ServiceStatus::OK
}

/// `((((task - table) as i32) >> 3) as u32) * 0xb6db6db7 + 1`, wrapping.
#[inline(always)]
pub fn task_id(table: GuestAddress, task: GuestAddress) -> TaskId {
    let scaled = ((task.0.wrapping_sub(table.0) as i32) >> 3) as u32;
    TaskId(scaled.wrapping_mul(INVERSE_OF_7).wrapping_add(1))
}

/// +0x60
#[inline(always)]
pub fn slot_60<M: Machine>(m: &M, w: &DomainWords) -> Flag {
    let task = m.load_word(w.current_task);
    if task == 0 {
        return Flag(1);
    }
    Flag::from_bool(m.load_byte(GuestAddress(task).offset(TASK_FLAGS_BYTE)) & TASK_FLAG_BIT2 == 0)
}

/// +0xfc
#[inline(always)]
pub fn slot_fc<M: Machine>(m: &M, w: &DomainWords, now: GuestAddress) -> ServiceStatus {
    masked_service(m, w, Context::Task, || {
        let offset = m.load_word(now).wrapping_sub(m.load_word(w.timer_base));
        m.store_word(w.time_offset, offset);
    })
}

/// +0x100
#[inline(always)]
pub fn slot_100<M: Machine>(m: &M, w: &DomainWords, now: GuestAddress) -> ServiceStatus {
    masked_service(m, w, Context::Task, || {
        let time = m.load_word(w.timer_base).wrapping_add(m.load_word(w.time_offset));
        m.store_word(now, time);
    })
}

/// +0x128
#[inline(always)]
pub fn slot_128<M: Machine>(m: &M, w: &DomainWords, id: GuestAddress) -> ServiceStatus {
    masked_service(m, w, Context::Task, || {
        let task = GuestAddress(m.load_word(w.current_task));
        m.store_word(id, task_id(w.task_table, task).0);
    })
}

/// +0x12c
#[inline(always)]
pub fn slot_12c<M: Machine>(m: &M, w: &DomainWords, id: GuestAddress) -> ServiceStatus {
    masked_service(m, w, Context::Handler, || {
        let task = m.load_word(w.current_task);
        let value = if task == 0 { TaskId(0) } else { task_id(w.task_table, GuestAddress(task)) };
        m.store_word(id, value.0);
    })
}

/// +0x130
#[inline(always)]
pub fn slot_130<M: Machine>(m: &M, w: &DomainWords) -> ServiceStatus {
    set_cpu_lock(m, w, Context::Task, true)
}

/// +0x134
#[inline(always)]
pub fn slot_134<M: Machine>(m: &M, w: &DomainWords) -> ServiceStatus {
    set_cpu_lock(m, w, Context::Handler, true)
}

/// +0x138
#[inline(always)]
pub fn slot_138<M: Machine>(m: &M, w: &DomainWords) -> ServiceStatus {
    set_cpu_lock(m, w, Context::Task, false)
}

/// +0x13c
#[inline(always)]
pub fn slot_13c<M: Machine>(m: &M, w: &DomainWords) -> ServiceStatus {
    set_cpu_lock(m, w, Context::Handler, false)
}

/// +0x148
#[inline(always)]
pub fn slot_148<M: Machine>(m: &M, w: &DomainWords) -> Flag {
    Flag::from_bool(m.load_word(w.handler_context) != 0)
}

/// +0x14c
#[inline(always)]
pub fn slot_14c<M: Machine>(m: &M) -> Flag {
    Flag::from_bool(m.cpsr().irq_masked())
}

/// +0x150
#[inline(always)]
pub fn slot_150<M: Machine>(m: &M, w: &DomainWords) -> Flag {
    Flag::from_bool(m.load_word(w.dispatch_enabled) == 0)
}

/// +0x154
#[inline(always)]
pub fn slot_154<M: Machine>(m: &M, w: &DomainWords) -> Flag {
    Flag::from_bool(
        m.load_word(w.handler_context) != 0 || m.cpsr().irq_masked() || m.load_word(w.dispatch_enabled) == 0,
    )
}

/// +0x164
#[inline(always)]
pub fn slot_164<M: Machine>(m: &M) -> SavedIrqMask {
    let entry = m.cpsr();
    m.write_cpsr_control(entry.with_irq_masked(true));
    SavedIrqMask(entry.0 & Cpsr::IRQ_MASK)
}

/// +0x168. The result keeps the entry flags.
#[inline(always)]
pub fn slot_168<M: Machine>(m: &M, saved: SavedIrqMask) -> Cpsr {
    let entry = m.cpsr();
    let restored = Cpsr((saved.0 & Cpsr::IRQ_MASK) | (entry.0 & !Cpsr::IRQ_MASK));
    m.write_cpsr_control(restored);
    restored
}

/// +0x16c over the +0x148 service and the +0x128 service, the latter yielding `None` on error.
#[inline(always)]
pub fn slot_16c(handler_context: impl FnOnce() -> Flag, own_task_id: impl FnOnce() -> Option<TaskId>) -> TaskId {
    if handler_context() == Flag(1) {
        return TaskId(0);
    }
    own_task_id().unwrap_or(TaskId(0))
}

/// +0x1618
#[inline(always)]
pub fn slot_1618<M: Machine>(m: &M) -> Flag {
    Flag::from_bool(m.debug_identity() != DebugIdentity::PRIMARY)
}
