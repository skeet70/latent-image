//! Allocation-free task-record creation and deletion for root slots `+0x0` and `+0x4` of the
//! X-M1 1.50 loaders. The raw `C`/`T` records keep their stock layout.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

#[cfg(test)]
mod tests;

use layout::{Domain, GuestAddress, Task, TaskDefinition, TaskSlot};

/// The I/F bits the stock mask pair saved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedInterrupts(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TaskAttributes(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TaskArgument(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EntryPoint(pub GuestAddress);

/// The request's external priority, 1..=16; the record stores one less.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct InitialPriority(u8);

impl InitialPriority {
    pub const COUNT: u32 = 16;

    #[inline(always)]
    pub const fn new(raw: u32) -> Option<Self> {
        if raw >= 1 && raw <= Self::COUNT {
            Some(Self(raw as u8))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }

    /// The record byte at `C+0xc` and `T+0xd`.
    #[inline(always)]
    pub const fn stored(self) -> u8 {
        self.0 - 1
    }
}

/// Stack bytes, rounded up to four the way the stock body does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct StackBytes(u32);

impl StackBytes {
    #[inline(always)]
    pub const fn new(raw: u32) -> Option<Self> {
        if raw <= u32::MAX - 3 {
            Some(Self(raw))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn rounded(self) -> u32 {
        (self.0 + 3) & !3
    }
}

/// The six request words, validated at the typed boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskRequest {
    attributes: TaskAttributes,
    argument: TaskArgument,
    entry: EntryPoint,
    priority: InitialPriority,
    stack_bytes: StackBytes,
    stack_base: GuestAddress,
}

impl TaskRequest {
    #[inline(always)]
    pub fn new(
        attributes: u32,
        argument: u32,
        entry: GuestAddress,
        priority: u32,
        stack_bytes: u32,
        stack_base: GuestAddress,
    ) -> Result<Self, TaskRecordError> {
        Ok(Self {
            attributes: TaskAttributes(attributes),
            argument: TaskArgument(argument),
            entry: EntryPoint(entry),
            priority: InitialPriority::new(priority).ok_or(TaskRecordError::InvalidPriority)?,
            stack_bytes: StackBytes::new(stack_bytes).ok_or(TaskRecordError::InvalidStackSize)?,
            stack_base,
        })
    }

    #[inline(always)]
    pub const fn attributes(self) -> u32 {
        self.attributes.0
    }

    #[inline(always)]
    pub const fn argument(self) -> u32 {
        self.argument.0
    }

    #[inline(always)]
    pub const fn entry(self) -> GuestAddress {
        self.entry.0
    }

    #[inline(always)]
    pub const fn priority(self) -> u32 {
        self.priority.get()
    }

    #[inline(always)]
    pub const fn stack_bytes(self) -> u32 {
        self.stack_bytes.0
    }

    #[inline(always)]
    pub const fn stack_base(self) -> GuestAddress {
        self.stack_base
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeleteMode {
    /// Clear only `C+0x24`; the record and its task stay in place.
    Clear,
    /// Restore the definition and task records to their stock defaults.
    Reset,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskRecordError {
    InvalidSlot,
    InvalidPriority,
    InvalidStackSize,
    AlreadyExists,
    NotCreated,
    /// `T+0xc` is not dormant.
    TargetLive,
    /// `T+0x8` does not point at this slot's definition.
    UnboundRecord,
}

impl TaskRecordError {
    /// The stock-ABI status the adapter returns.
    #[inline(always)]
    pub const fn status(self) -> i32 {
        match self {
            Self::AlreadyExists | Self::NotCreated | Self::TargetLive => -41,
            Self::InvalidSlot | Self::InvalidPriority | Self::InvalidStackSize | Self::UnboundRecord => -18,
        }
    }
}

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_byte(&self, at: GuestAddress, value: u8);
    fn mask_interrupts(&self) -> SavedInterrupts;
    fn restore_interrupts(&self, saved: SavedInterrupts);
    fn dmb(&self);
}

/// `+0x0`, with the stock mask pair around [`create_masked`].
#[inline(always)]
pub fn create<M: Machine>(m: &M, domain: Domain, slot: TaskSlot, request: TaskRequest) -> Result<TaskSlot, TaskRecordError> {
    let saved = m.mask_interrupts();
    let result = create_masked(
        m,
        domain,
        slot,
        request.attributes(),
        request.argument(),
        request.entry(),
        request.priority.stored(),
        request.stack_bytes.rounded(),
        request.stack_base(),
    );
    m.restore_interrupts(saved);
    result
}

/// `+0x0` with interrupts already masked. No word is written when a check or the request
/// validation fails.
#[inline(always)]
pub fn create_masked<M: Machine>(
    m: &M,
    domain: Domain,
    slot: TaskSlot,
    attributes: u32,
    argument: u32,
    entry: GuestAddress,
    priority: u8,
    stack_size: u32,
    stack_base: GuestAddress,
) -> Result<TaskSlot, TaskRecordError> {
    let words = domain.words();
    let definition = words.task_definition(slot);
    let task = words.task(slot);
    if m.load_word(definition.exists()) != 0 {
        return Err(TaskRecordError::AlreadyExists);
    }
    if m.load_byte(task.state()) != 0 {
        return Err(TaskRecordError::TargetLive);
    }
    if m.load_word(task.definition()) != definition.0 .0 {
        return Err(TaskRecordError::UnboundRecord);
    }
    write_definition(m, &words, definition, slot, attributes, argument, entry, priority, stack_size, stack_base);
    reset_task(m, task, domain, priority, definition.exists(), 1);
    Ok(slot)
}

/// `+0x0` from the six request words at `request_at`, read only after the checks.
#[inline(always)]
pub fn create_masked_at<M: Machine>(
    m: &M,
    domain: Domain,
    slot: TaskSlot,
    request_at: GuestAddress,
) -> Result<TaskSlot, TaskRecordError> {
    let words = domain.words();
    let definition = words.task_definition(slot);
    let task = words.task(slot);
    if m.load_word(definition.exists()) != 0 {
        return Err(TaskRecordError::AlreadyExists);
    }
    if m.load_byte(task.state()) != 0 {
        return Err(TaskRecordError::TargetLive);
    }
    if m.load_word(task.definition()) != definition.0 .0 {
        return Err(TaskRecordError::UnboundRecord);
    }
    let priority = m.load_word(request_at.offset(0x0c));
    if priority < 1 || priority > InitialPriority::COUNT {
        return Err(TaskRecordError::InvalidPriority);
    }
    let stack_bytes = m.load_word(request_at.offset(0x10));
    if stack_bytes > u32::MAX - 3 {
        return Err(TaskRecordError::InvalidStackSize);
    }
    write_definition(
        m,
        &words,
        definition,
        slot,
        m.load_word(request_at),
        m.load_word(request_at.offset(0x04)),
        GuestAddress(m.load_word(request_at.offset(0x08))),
        priority.wrapping_sub(1) as u8,
        (stack_bytes + 3) & !3,
        GuestAddress(m.load_word(request_at.offset(0x14))),
    );
    reset_task(m, task, domain, priority.wrapping_sub(1) as u8, definition.exists(), 1);
    Ok(slot)
}

/// `+0x0` for the stock ABI: the status word directly, with no `Result` state machine.
#[inline(always)]
pub fn create_at_status<M: Machine>(m: &M, domain: Domain, id: u32, request_at: GuestAddress) -> i32 {
    let slot = match TaskSlot::new(id & 0xffff) {
        Some(slot) => slot,
        None => return TaskRecordError::InvalidSlot.status(),
    };
    let words = domain.words();
    let definition = words.task_definition(slot);
    let task = words.task(slot);
    if m.load_word(definition.exists()) != 0 {
        return TaskRecordError::AlreadyExists.status();
    }
    if m.load_byte(task.state()) != 0 {
        return TaskRecordError::TargetLive.status();
    }
    if m.load_word(task.definition()) != definition.0 .0 {
        return TaskRecordError::UnboundRecord.status();
    }
    let priority = m.load_word(request_at.offset(0x0c));
    if priority < 1 || priority > InitialPriority::COUNT {
        return TaskRecordError::InvalidPriority.status();
    }
    let stack_bytes = m.load_word(request_at.offset(0x10));
    if stack_bytes > u32::MAX - 3 {
        return TaskRecordError::InvalidStackSize.status();
    }
    write_definition(
        m,
        &words,
        definition,
        slot,
        m.load_word(request_at),
        m.load_word(request_at.offset(0x04)),
        GuestAddress(m.load_word(request_at.offset(0x08))),
        priority.wrapping_sub(1) as u8,
        (stack_bytes + 3) & !3,
        GuestAddress(m.load_word(request_at.offset(0x14))),
    );
    reset_task(m, task, domain, priority.wrapping_sub(1) as u8, definition.exists(), 1);
    0
}

#[inline(always)]
fn write_definition<M: Machine>(
    m: &M,
    words: &layout::DomainWords,
    definition: TaskDefinition,
    slot: TaskSlot,
    attributes: u32,
    argument: u32,
    entry: GuestAddress,
    priority: u8,
    stack_size: u32,
    stack_base: GuestAddress,
) {
    m.store_word(definition.attributes(), attributes);
    m.store_word(definition.argument(), argument);
    m.store_word(definition.entry(), entry.0);
    m.store_word(definition.priority(), priority as u32);
    m.store_word(definition.stack_size(), stack_size);
    m.store_word(definition.stack(), stack_base.0);
    m.store_word(definition.exception_attributes(), 0);
    m.store_word(definition.exception_handler(), 0);
    m.store_word(definition.slot(), slot.get());
    m.store_word(words.task_creation_id(slot).word(), slot.get());
}

/// `+0x4`, with the stock mask pair around [`delete_masked`].
#[inline(always)]
pub fn delete<M: Machine>(m: &M, domain: Domain, slot: TaskSlot, mode: DeleteMode) -> Result<(), TaskRecordError> {
    let saved = m.mask_interrupts();
    let result = delete_masked(m, domain, slot, mode);
    m.restore_interrupts(saved);
    result
}

/// `+0x4` with interrupts already masked.
#[inline(always)]
pub fn delete_masked<M: Machine>(m: &M, domain: Domain, slot: TaskSlot, mode: DeleteMode) -> Result<(), TaskRecordError> {
    let words = domain.words();
    let definition = words.task_definition(slot);
    if m.load_word(definition.exists()) == 0 {
        return Err(TaskRecordError::NotCreated);
    }
    if mode == DeleteMode::Clear {
        m.store_word(definition.exists(), 0);
        return Ok(());
    }
    let task = words.task(slot);
    if m.load_byte(task.state()) != 0 {
        return Err(TaskRecordError::TargetLive);
    }
    if m.load_word(task.definition()) != definition.0 .0 {
        return Err(TaskRecordError::UnboundRecord);
    }
    m.store_word(definition.attributes(), 0);
    m.store_word(definition.argument(), slot.get() - 1);
    m.store_word(definition.entry(), 0);
    m.store_word(definition.priority(), InitialPriority::COUNT - 1);
    m.store_word(definition.stack_size(), 4);
    m.store_word(definition.stack(), words.default_stack_word.address().0);
    m.store_word(definition.exception_attributes(), 0);
    m.store_word(definition.exception_handler(), 0);
    m.store_word(definition.slot(), slot.get());
    m.store_word(words.task_creation_id(slot).word(), slot.get());
    reset_task(m, task, domain, (InitialPriority::COUNT - 1) as u8, definition.exists(), 0);
    Ok(())
}

/// `+0x4` for the stock ABI: the status word directly.
#[inline(always)]
pub fn delete_at_status<M: Machine>(m: &M, domain: Domain, id: u32, mode: u32) -> i32 {
    let slot = match TaskSlot::new(id & 0xffff) {
        Some(slot) => slot,
        None => return TaskRecordError::InvalidSlot.status(),
    };
    let words = domain.words();
    let definition = words.task_definition(slot);
    if m.load_word(definition.exists()) == 0 {
        return TaskRecordError::NotCreated.status();
    }
    if mode != 1 {
        m.store_word(definition.exists(), 0);
        return 0;
    }
    let task = words.task(slot);
    if m.load_byte(task.state()) != 0 {
        return TaskRecordError::TargetLive.status();
    }
    if m.load_word(task.definition()) != definition.0 .0 {
        return TaskRecordError::UnboundRecord.status();
    }
    m.store_word(definition.attributes(), 0);
    m.store_word(definition.argument(), slot.get() - 1);
    m.store_word(definition.entry(), 0);
    m.store_word(definition.priority(), InitialPriority::COUNT - 1);
    m.store_word(definition.stack_size(), 4);
    m.store_word(definition.stack(), words.default_stack_word.address().0);
    m.store_word(definition.exception_attributes(), 0);
    m.store_word(definition.exception_handler(), 0);
    m.store_word(definition.slot(), slot.get());
    m.store_word(words.task_creation_id(slot).word(), slot.get());
    reset_task(m, task, domain, (InitialPriority::COUNT - 1) as u8, definition.exists(), 0);
    0
}

/// The `T` reset and publication both root operations share.
#[cfg_attr(feature = "stage", link_section = ".text.task_reset_fields")]
#[inline(never)]
pub fn reset_task<M: Machine>(
    m: &M,
    task: Task,
    domain: Domain,
    priority: u8,
    exists: GuestAddress,
    exists_value: u32,
) {
    m.store_byte(task.flags(), m.load_byte(task.flags()) & !0x07);
    m.store_word(task.domain(), domain.id().word());
    m.store_word(task.terminate_pending(), 0);
    m.store_word(task.priority_change_pending(), 0);
    m.store_word(task.cancel_pending(), 0);
    m.store_word(task.context_discarded(), 0);
    m.store_byte(task.priority(), priority);
    m.store_byte(task.state(), 0);
    m.store_word(task.exception_pattern(), 0);
    m.dmb();
    m.store_word(exists, exists_value);
}
