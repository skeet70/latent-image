//! X-M1 1.50 task execution at the loader's zero-context, IRQ-enabled boundary.
//! Admission depends on source-bound loader code and continuous context/current observation.

use crate::{modal_permit::{CoreId, Cpsr}, provenance::Guest};
use std::num::NonZeroU16;

const TASK_COUNT: u32 = 120;
const NODE_BYTES: u32 = 56;
const CREATION_BYTES: u32 = 40;
const DRAM_END: u32 = 0x2000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelId { Primary, Secondary }

impl KernelId {
    pub const fn current_cell(self) -> Guest {
        match self { Self::Primary => Guest(0x4005_4760), Self::Secondary => Guest(0x400e_4720) }
    }
    pub const fn context_cell(self) -> Guest {
        match self { Self::Primary => Guest(0x4005_45a4), Self::Secondary => Guest(0x400e_4564) }
    }
    const fn nodes(self) -> Guest {
        match self { Self::Primary => Guest(0x4005_ff0c), Self::Secondary => Guest(0x400e_fecc) }
    }
    const fn creations(self) -> Guest {
        match self { Self::Primary => Guest(0x4005_beac), Self::Secondary => Guest(0x400e_be6c) }
    }
    /// Lazy-VFP owner word, stored with TPIDRURW by the undefined-instruction handler at 0x42440/0xd2440.
    pub const fn vfp_owner_cell(self) -> Guest {
        match self { Self::Primary => Guest(0x0004_dad4), Self::Secondary => Guest(0x000d_da98) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskId(NonZeroU16);
impl TaskId { pub const fn get(self) -> u16 { self.0.get() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfpOwner { Unowned, Task(TaskId) }
impl VfpOwner {
    /// Loader image value before any task traps on VFP.
    pub const UNOWNED: u32 = 0xffff_ffff;
    pub const fn word(self) -> u32 {
        match self { Self::Unowned => Self::UNOWNED, Self::Task(id) => id.get() as u32 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskNode(Guest);
impl TaskNode { pub const fn address(self) -> Guest { self.0 } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreationRecord(Guest);
impl CreationRecord { pub const fn address(self) -> Guest { self.0 } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskIdentity {
    pub kernel: KernelId,
    pub id: TaskId,
    pub node: TaskNode,
    pub creation: CreationRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalExtent { start: Guest, end: Guest }
impl PhysicalExtent {
    pub fn new(address: Guest, bytes: u32) -> Result<Self, TaskError> {
        let start = physical(address)?;
        let end = start.0.checked_add(bytes).filter(|end| bytes != 0 && *end <= DRAM_END)
            .ok_or(TaskError::InvalidExtent { address, bytes })?;
        Ok(Self { start, end: Guest(end) })
    }
    pub const fn start(self) -> Guest { self.start }
    pub const fn end(self) -> Guest { self.end }
    pub const fn bytes(self) -> u32 { self.end.0 - self.start.0 }
    pub fn overlaps(self, other: Self) -> bool { self.start < other.end && other.start < self.end }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackExtent(PhysicalExtent);
impl StackExtent {
    pub const fn physical(self) -> PhysicalExtent { self.0 }

    pub fn validate_sp(self, sp: Guest) -> Result<(), TaskError> {
        let observed = physical(sp)?;
        // An empty downward-growing stack may have SP at its exclusive upper bound.
        if sp.0 % 4 != 0 || observed < self.0.start || observed > self.0.end {
            return Err(TaskError::StackPointerOutside { sp, stack: self });
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLayout {
    pub kernel: KernelId,
    pub context: PhysicalExtent,
    pub current: PhysicalExtent,
    pub nodes: PhysicalExtent,
    pub creations: PhysicalExtent,
    pub vfp_owner: PhysicalExtent,
}

pub fn source_layouts() -> [SourceLayout; 2] {
    [KernelId::Primary, KernelId::Secondary].map(|kernel| {
        let extent = |address: Guest, bytes| {
            let start = Guest(address.0 & !0x4000_0000);
            PhysicalExtent { start, end: Guest(start.0 + bytes) }
        };
        SourceLayout {
            kernel, current: extent(kernel.current_cell(), 4),
            context: extent(kernel.context_cell(), 4),
            nodes: extent(kernel.nodes(), NODE_BYTES * TASK_COUNT),
            creations: extent(kernel.creations(), CREATION_BYTES * TASK_COUNT),
            vfp_owner: extent(kernel.vfp_owner_cell(), 4),
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryError { Unmapped, Translation, ActiveTcm, AliasMismatch, PhysicalMismatch, Read }

/// Reads agree across both cores, both DRAM aliases and committed physical memory.
pub trait ValidatedTaskMemory {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError>;
    fn validate_stack(&self, extent: StackExtent) -> Result<(), MemoryError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskError {
    Unavailable,
    InvalidCore(CoreId),
    InvalidExecutionCpsr(Cpsr),
    ContextNonzero { kernel: KernelId, depth: u32 },
    WrongKernel { core: CoreId, observed: KernelId },
    SampledCoreMismatch { sampled: CoreId, observed: CoreId },
    Memory(MemoryError),
    NullCurrent,
    MisalignedNode(Guest),
    OutsideNodeArrays(Guest),
    WrongBacklink { expected: Guest, observed: Guest },
    WrongCreationId { expected: TaskId, observed: u32 },
    NotCreated(u32),
    InvalidAddress(Guest),
    InvalidExtent { address: Guest, bytes: u32 },
    StackPointerOutside { sp: Guest, stack: StackExtent },
    InvalidVfpOwner(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskExecution {
    identity: TaskIdentity,
    stack: StackExtent,
    creation_words: [u32; 10],
    execution_core: CoreId,
}
impl TaskExecution {
    pub const fn identity(&self) -> TaskIdentity { self.identity }
    pub const fn stack(&self) -> StackExtent { self.stack }
    pub const fn execution_core(&self) -> CoreId { self.execution_core }
    pub fn validate_execution(&self, core: CoreId, sp: Guest, cpsr: Cpsr) -> Result<(), TaskError> {
        if core != self.execution_core {
            return Err(TaskError::SampledCoreMismatch { sampled: self.execution_core, observed: core });
        }
        validate_cpsr(cpsr)?;
        self.stack.validate_sp(sp)
    }
    pub fn identity_extents(&self) -> [PhysicalExtent; 2] {
        let creation = self.identity.creation.0.0 & !0x4000_0000;
        let backlink = (self.identity.node.0.0 & !0x4000_0000) + 8;
        [PhysicalExtent { start: Guest(creation), end: Guest(creation + CREATION_BYTES) },
         PhysicalExtent { start: Guest(backlink), end: Guest(backlink + 4) }]
    }
}

fn physical(address: Guest) -> Result<Guest, TaskError> {
    let value = address.0 & !0x4000_0000;
    if value >= DRAM_END { return Err(TaskError::InvalidAddress(address)); }
    Ok(Guest(value))
}

fn word(memory: &impl ValidatedTaskMemory, address: Guest) -> Result<u32, TaskError> {
    let mut bytes = [0; 4];
    memory.read(address, &mut bytes).map_err(TaskError::Memory)?;
    Ok(u32::from_le_bytes(bytes))
}

fn validate_cpsr(cpsr: Cpsr) -> Result<(), TaskError> {
    const EXECUTION_MASK: u32 = (1 << 24) | (1 << 9) | (1 << 7) | (1 << 5) | 0x1f;
    if cpsr.0 & EXECUTION_MASK != 0x13 { return Err(TaskError::InvalidExecutionCpsr(cpsr)); }
    Ok(())
}

pub fn sample(
    memory: &impl ValidatedTaskMemory, execution_core: CoreId, sp: Guest, cpsr: Cpsr,
) -> Result<TaskExecution, TaskError> {
    let local_kernel = local_kernel(execution_core)?;
    validate_cpsr(cpsr)?;
    let depth = word(memory, local_kernel.context_cell())?;
    if depth != 0 { return Err(TaskError::ContextNonzero { kernel: local_kernel, depth }); }
    let (identity, creation_words) = current_task(memory, execution_core)?;
    let stack = StackExtent(PhysicalExtent::new(Guest(creation_words[5]), creation_words[4])?);
    memory.validate_stack(stack).map_err(TaskError::Memory)?;
    stack.validate_sp(sp)?;
    Ok(TaskExecution { identity, stack, creation_words, execution_core })
}

/// The current task of `core`'s kernel, without the task-execution mode and context-depth checks of [`sample`].
pub fn current_task_id(memory: &impl ValidatedTaskMemory, core: CoreId) -> Result<TaskId, TaskError> {
    current_task(memory, core).map(|(identity, _)| identity.id)
}

/// Parses a lazy-VFP owner word: the loader's unowned value or the ID of a created task of `kernel`.
pub fn vfp_owner(memory: &impl ValidatedTaskMemory, kernel: KernelId, value: u32) -> Result<VfpOwner, TaskError> {
    if value == VfpOwner::UNOWNED { return Ok(VfpOwner::Unowned); }
    if value == 0 || value > TASK_COUNT { return Err(TaskError::InvalidVfpOwner(value)); }
    let (identity, _) = created_task(memory, kernel, value - 1)?;
    Ok(VfpOwner::Task(identity.id))
}

fn local_kernel(core: CoreId) -> Result<KernelId, TaskError> {
    match core.0 {
        0 => Ok(KernelId::Primary),
        1 => Ok(KernelId::Secondary),
        _ => Err(TaskError::InvalidCore(core)),
    }
}

fn current_task(memory: &impl ValidatedTaskMemory, core: CoreId) -> Result<(TaskIdentity, [u32; 10]), TaskError> {
    let local_kernel = local_kernel(core)?;
    let pointer = Guest(word(memory, local_kernel.current_cell())?);
    if pointer.0 == 0 { return Err(TaskError::NullCurrent); }
    if pointer.0 % 4 != 0 { return Err(TaskError::MisalignedNode(pointer)); }
    let kernel = [KernelId::Primary, KernelId::Secondary].into_iter().find(|kernel| {
        let base = kernel.nodes().0;
        base <= pointer.0 && pointer.0 < base + NODE_BYTES * TASK_COUNT
    }).ok_or(TaskError::OutsideNodeArrays(pointer))?;
    let offset = pointer.0 - kernel.nodes().0;
    if offset % NODE_BYTES != 0 { return Err(TaskError::MisalignedNode(pointer)); }
    if kernel != local_kernel { return Err(TaskError::WrongKernel { core, observed: kernel }); }
    created_task(memory, kernel, offset / NODE_BYTES)
}

fn created_task(memory: &impl ValidatedTaskMemory, kernel: KernelId, index: u32) -> Result<(TaskIdentity, [u32; 10]), TaskError> {
    let node = Guest(kernel.nodes().0 + NODE_BYTES * index);
    let id = TaskId(NonZeroU16::new((index + 1) as u16).ok_or(TaskError::NullCurrent)?);
    let creation = Guest(kernel.creations().0 + CREATION_BYTES * index);
    let backlink = Guest(word(memory, Guest(node.0 + 8))?);
    if backlink != creation { return Err(TaskError::WrongBacklink { expected: creation, observed: backlink }); }
    let mut bytes = [0; CREATION_BYTES as usize];
    memory.read(creation, &mut bytes).map_err(TaskError::Memory)?;
    let creation_words = std::array::from_fn(|index| {
        let offset = index * 4;
        u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
    });
    if creation_words[8] != u32::from(id.get()) {
        return Err(TaskError::WrongCreationId { expected: id, observed: creation_words[8] });
    }
    if creation_words[9] != 1 { return Err(TaskError::NotCreated(creation_words[9])); }
    Ok((TaskIdentity { kernel, id, node: TaskNode(node), creation: CreationRecord(creation) }, creation_words))
}
