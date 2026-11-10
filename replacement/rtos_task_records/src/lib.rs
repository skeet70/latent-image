//! Allocation-free task-record creation and deletion for root slots `+0x0` and `+0x4` of the
//! X-M1 1.50 loaders. The raw `C`/`T` records keep their stock layout.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

#[cfg(test)]
mod tests;

use layout::{Domain, GuestAddress, Priority, Task, TaskDefinition, TaskSlot};

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

#[inline(always)]
fn parse_initial_priority(raw: u32) -> Result<Priority, TaskRecordError> {
    Priority::new(raw.wrapping_sub(1)).ok_or(TaskRecordError::InvalidPriority)
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
    priority: Priority,
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
            priority: parse_initial_priority(priority)?,
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
        self.priority.get() + 1
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
    let result = create_masked(m, domain, slot, request);
    m.restore_interrupts(saved);
    result
}

/// `+0x0` with interrupts already masked.
#[inline(always)]
pub fn create_masked<M: Machine>(
    m: &M,
    domain: Domain,
    slot: TaskSlot,
    request: TaskRequest,
) -> Result<TaskSlot, TaskRecordError> {
    create_with_request(m, domain, slot, || Ok(request))
}

/// `+0x0` from the six request words at `request_at`.
#[inline(always)]
pub fn create_masked_at<M: Machine>(
    m: &M,
    domain: Domain,
    slot: TaskSlot,
    request_at: GuestAddress,
) -> Result<TaskSlot, TaskRecordError> {
    create_with_request(m, domain, slot, || {
        let priority = parse_initial_priority(m.load_word(request_at.offset(0x0c)))?;
        let stack_bytes = StackBytes::new(m.load_word(request_at.offset(0x10)))
            .ok_or(TaskRecordError::InvalidStackSize)?;
        Ok(TaskRequest {
            attributes: TaskAttributes(m.load_word(request_at)),
            argument: TaskArgument(m.load_word(request_at.offset(0x04))),
            entry: EntryPoint(GuestAddress(m.load_word(request_at.offset(0x08)))),
            priority,
            stack_bytes,
            stack_base: GuestAddress(m.load_word(request_at.offset(0x14))),
        })
    })
}

#[inline(always)]
fn create_with_request<M: Machine>(
    m: &M,
    domain: Domain,
    slot: TaskSlot,
    request: impl FnOnce() -> Result<TaskRequest, TaskRecordError>,
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
    let request = request()?;
    write_definition(m, &words, definition, slot, request);
    reset_task(m, task, domain, request.priority, definition.exists(), DefinitionState::Published);
    Ok(slot)
}

#[inline(always)]
fn write_definition<M: Machine>(
    m: &M,
    words: &layout::DomainWords,
    definition: TaskDefinition,
    slot: TaskSlot,
    request: TaskRequest,
) {
    m.store_word(definition.attributes(), request.attributes());
    m.store_word(definition.argument(), request.argument());
    m.store_word(definition.entry(), request.entry().0);
    m.store_word(definition.priority(), request.priority.get());
    m.store_word(definition.stack_size(), request.stack_bytes.rounded());
    m.store_word(definition.stack(), request.stack_base().0);
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
    if mode == DeleteMode::Clear {
        if m.load_word(definition.exists()) == 0 {
            return Err(TaskRecordError::NotCreated);
        }
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
    let request = TaskRequest {
        attributes: TaskAttributes(0),
        argument: TaskArgument(slot.get() - 1),
        entry: EntryPoint(GuestAddress(0)),
        priority: Priority::new(Priority::COUNT - 1).unwrap(),
        stack_bytes: StackBytes(4),
        stack_base: words.default_stack_word.address(),
    };
    write_definition(m, &words, definition, slot, request);
    reset_task(m, task, domain, request.priority, definition.exists(), DefinitionState::Withdrawn);
    Ok(())
}

#[repr(u32)]
enum DefinitionState {
    Withdrawn = 0,
    Published = 1,
}

/// The `T` reset and publication both root operations share.
#[cfg_attr(feature = "stage", link_section = ".text.task_reset_fields")]
#[inline(never)]
fn reset_task<M: Machine>(
    m: &M,
    task: Task,
    domain: Domain,
    priority: Priority,
    exists: GuestAddress,
    state: DefinitionState,
) {
    m.store_byte(task.flags(), m.load_byte(task.flags()) & !0x07);
    m.store_word(task.domain(), domain.id().word());
    m.store_word(task.terminate_pending(), 0);
    m.store_word(task.priority_change_pending(), 0);
    m.store_word(task.cancel_pending(), 0);
    m.store_word(task.context_discarded(), 0);
    m.store_byte(task.priority(), priority.get() as u8);
    m.store_byte(task.state(), 0);
    m.store_word(task.exception_pattern(), 0);
    m.dmb();
    m.store_word(exists, state as u32);
}
