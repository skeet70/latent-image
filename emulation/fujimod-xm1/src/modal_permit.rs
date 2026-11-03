//! Preexecution permits for the X-M1 1.50 modal entry seam.
//!
//! Feed the stock entry, every instruction in configured linked ranges and the stock
//! body, and every return site reported by `Observation::Entry`. Arm that return
//! site before advancing the guest. Stock-frame words are read at the observed SP
//! in ascending address order. Only downward-growing, LIFO contexts are admitted.
//! Owned observations require the source-bound task-execution boundary.

use crate::{
    modal_task::{TaskExecution, PhysicalExtent, TaskError, TaskIdentity},
    provenance::Guest,
};
use std::{fmt, num::NonZeroU64};

pub const STOCK_ENTRY: Guest = Guest(0x00bf_b028);
pub const STOCK_RESUME: Guest = Guest(0x00bf_b02c);
pub const STOCK_END: Guest = Guest(0x00bf_b124);
const SEAM_BYTES: u32 = 56;
const STOCK_FRAME_BYTES: u32 = 36;
const CORE_COUNT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cpsr(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Count(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Depth(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservationLimit(pub NonZeroU64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Owned75,
    StockFallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    StockBaseline,
    ScopedOwned75,
}

impl Policy {
    pub const fn route(self, controller: ControllerId) -> Route {
        match (self, controller.0) {
            (Self::ScopedOwned75, 75) => Route::Owned75,
            _ => Route::StockFallback,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    InvalidCodeRange,
    WrongSeamSize,
    OverlappingCode,
    HelpersWithoutLinkedLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeRange {
    start: Guest,
    end: Guest,
}

impl CodeRange {
    pub fn new(start: Guest, end: Guest) -> Result<Self, LayoutError> {
        if start.0 == 0 || start.0 % 4 != 0 || end.0 % 4 != 0 || start.0 >= end.0 {
            return Err(LayoutError::InvalidCodeRange);
        }
        Ok(Self { start, end })
    }

    pub const fn start(self) -> Guest { self.start }
    pub const fn end(self) -> Guest { self.end }

    fn contains(self, pc: Guest) -> bool { self.start <= pc && pc < self.end }
    fn overlaps(self, other: Self) -> bool { self.start < other.end && other.start < self.end }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinkedLayout {
    seam: CodeRange,
    owned: CodeRange,
    helpers: Option<[CodeRange; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    linked: Option<LinkedLayout>,
}

impl Layout {
    pub const fn stock_baseline() -> Self { Self { linked: None } }

    pub fn new(seam: CodeRange, owned: CodeRange) -> Result<Self, LayoutError> {
        if seam.end.0 - seam.start.0 != SEAM_BYTES {
            return Err(LayoutError::WrongSeamSize);
        }
        let stock = CodeRange { start: STOCK_ENTRY, end: STOCK_END };
        if seam.overlaps(owned) || seam.overlaps(stock) || owned.overlaps(stock) {
            return Err(LayoutError::OverlappingCode);
        }
        Ok(Self { linked: Some(LinkedLayout { seam, owned, helpers: None }) })
    }

    pub fn with_helpers(self, helpers: [CodeRange; 2]) -> Result<Self, LayoutError> {
        let mut linked = self.linked.ok_or(LayoutError::HelpersWithoutLinkedLayout)?;
        let stock = CodeRange { start: STOCK_ENTRY, end: STOCK_END };
        if helpers[0].overlaps(helpers[1]) || helpers.iter()
            .any(|helper| [stock, linked.seam, linked.owned].into_iter().any(|range| helper.overlaps(range))) {
            return Err(LayoutError::OverlappingCode);
        }
        linked.helpers = Some(helpers);
        Ok(Self { linked: Some(linked) })
    }

    /// The configured admission ceiling, not the currently installed entry word.
    pub const fn policy(self) -> Policy {
        match self.linked {
            None => Policy::StockBaseline,
            Some(_) => Policy::ScopedOwned75,
        }
    }

    pub fn seam(self) -> Option<CodeRange> { self.linked.map(|linked| linked.seam) }
    pub fn owned(self) -> Option<CodeRange> { self.linked.map(|linked| linked.owned) }
    pub fn helpers(self) -> impl Iterator<Item = CodeRange> {
        self.linked.and_then(|linked| linked.helpers).into_iter().flatten()
    }

    fn in_seam(self, pc: Guest) -> bool { self.seam().is_some_and(|seam| seam.contains(pc)) }

    fn body_route(self, pc: Guest) -> Option<Route> {
        if self.owned().is_some_and(|owned| owned.contains(pc))
            || self.helpers().any(|helper| helper.contains(pc)) {
            Some(Route::Owned75)
        } else if STOCK_RESUME <= pc && pc < STOCK_END {
            Some(Route::StockFallback)
        } else {
            None
        }
    }

    fn handoff(self, route: Route) -> Option<Guest> {
        match route {
            Route::Owned75 => self.owned().map(CodeRange::start),
            Route::StockFallback => Some(STOCK_RESUME),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StockFrame(pub [u32; 9]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryInstruction { NotEntry, OriginalPush, ScopedBranch }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub core: CoreId,
    pub pc: Guest,
    pub registers: [u32; 13],
    pub sp: Guest,
    pub lr: Guest,
    pub cpsr: Cpsr,
    pub saved_stock_frame: Option<StockFrame>,
    pub task: Result<TaskExecution, TaskError>,
    pub entry_instruction: EntryInstruction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneralRegister { R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12 }

const GENERAL_REGISTERS: [GeneralRegister; 13] = [
    GeneralRegister::R0, GeneralRegister::R1, GeneralRegister::R2,
    GeneralRegister::R3, GeneralRegister::R4, GeneralRegister::R5,
    GeneralRegister::R6, GeneralRegister::R7, GeneralRegister::R8,
    GeneralRegister::R9, GeneralRegister::R10, GeneralRegister::R11,
    GeneralRegister::R12,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateComponent {
    General(GeneralRegister),
    LinkRegister,
    Cpsr,
    StockFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    InvalidCore(CoreId),
    MisalignedPc(Guest),
    InvalidArmState(Cpsr),
    MisalignedStack(Guest),
    WrappingStack(Guest),
    InvalidReturnSite(Guest),
    InvalidEntryInstruction { pc: Guest, instruction: EntryInstruction },
    MissingPermit { core: CoreId, pc: Guest },
    WrongRoute { core: CoreId, expected: Route, observed: Route },
    UnexpectedHandoff(CoreId),
    UnresolvedHandoff(CoreId),
    NonNestedContext(CoreId),
    HandoffState { core: CoreId, component: StateComponent },
    HandoffStack { core: CoreId, expected: Guest, observed: Guest },
    MissingStockFrame(CoreId),
    WrongReturnStack { core: CoreId, expected: Guest, observed: Guest },
    WrongReturnOrder(CoreId),
    DepthOverflow(CoreId),
    TaskAttribution { core: CoreId, error: TaskError },
    OwnerMismatch { core: CoreId, expected: TaskIdentity, observed: TaskIdentity },
    CrossCoreStack {
        owner: TaskIdentity, observed: TaskIdentity, active_core: CoreId, observed_core: CoreId,
    },
    IdentityMetadataChanged(TaskIdentity),
    ActiveIdentityWrite { owner: TaskIdentity, write: PhysicalExtent },
    CounterOverflow,
    ActiveFrames(Depth),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for Failure {}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for LayoutError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Continue,
    Entry { core: CoreId, route: Route, arm_return: Guest },
    Handoff { core: CoreId, route: Route },
    Return { core: CoreId, route: Route },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub observations: Count,
    pub entries: Count,
    pub owned_handoffs: Count,
    pub fallback_handoffs: Count,
    pub returns: Count,
    pub active_frames: Depth,
    pub peak_core_depth: Depth,
}

impl Receipt {
    const EMPTY: Self = Self {
        observations: Count(0), entries: Count(0), owned_handoffs: Count(0),
        fallback_handoffs: Count(0), returns: Count(0), active_frames: Depth(0),
        peak_core_depth: Depth(0),
    };
}

#[derive(Clone, Copy)]
enum Phase { AwaitingHandoff, Running }

#[derive(Clone, Copy)]
struct EntryState {
    registers: [u32; 13],
    sp: Guest,
    lr: Guest,
    cpsr: Cpsr,
}

#[derive(Clone, Copy)]
struct Frame {
    entry: EntryState,
    route: Route,
    handoff_sp: Guest,
    phase: Phase,
    owner: Option<TaskExecution>,
}

pub struct Monitor<const CAPACITY: usize> {
    layout: Layout,
    frames: [[Option<Frame>; CAPACITY]; CORE_COUNT],
    depths: [usize; CORE_COUNT],
    limit: ObservationLimit,
    receipt: Receipt,
    failure: Option<Failure>,
}

impl<const CAPACITY: usize> Monitor<CAPACITY> {
    pub fn new(layout: Layout, limit: ObservationLimit) -> Self {
        Self {
            layout, frames: [[None; CAPACITY]; CORE_COUNT], depths: [0; CORE_COUNT],
            limit, receipt: Receipt::EMPTY, failure: None,
        }
    }

    pub fn receipt(&self) -> &Receipt { &self.receipt }
    pub fn failure(&self) -> Option<Failure> { self.failure }

    pub fn has_owned_frame(&self, core: CoreId) -> bool {
        self.frames.get(core.0).is_some_and(|frames| {
            frames.iter().take(self.depths[core.0]).flatten().any(|frame| frame.route == Route::Owned75)
        })
    }

    pub fn overlaps_active_identity(&self, write: PhysicalExtent) -> bool {
        self.frames.iter().flatten().flatten().filter_map(|frame| frame.owner.as_ref())
            .any(|owner| owner.identity_extents().into_iter().any(|extent| extent.overlaps(write)))
    }

    /// Every overlapping write invalidates continuity, including equal-value writes.
    pub fn observe_identity_write(&mut self, write: PhysicalExtent) -> Result<(), Failure> {
        if let Some(failure) = self.failure { return Err(failure); }
        let owner = self.frames.iter().flatten().flatten().filter_map(|frame| frame.owner.as_ref())
            .find(|owner| owner.identity_extents().into_iter().any(|extent| extent.overlaps(write)));
        if let Some(owner) = owner {
            let failure = Failure::ActiveIdentityWrite { owner: owner.identity(), write };
            self.failure = Some(failure);
            return Err(failure);
        }
        Ok(())
    }

    pub fn finish(&self) -> Result<&Receipt, Failure> {
        if let Some(failure) = self.failure { return Err(failure); }
        if self.receipt.active_frames.0 != 0 {
            return Err(Failure::ActiveFrames(self.receipt.active_frames));
        }
        Ok(&self.receipt)
    }

    /// A failure is sticky; rejected observations do not change the receipt.
    pub fn observe(&mut self, snapshot: Snapshot) -> Result<Observation, Failure> {
        if let Some(failure) = self.failure { return Err(failure); }
        let result = self.observe_inner(&snapshot);
        if let Err(failure) = result { self.failure = Some(failure); }
        result
    }

    fn observe_inner(&mut self, snapshot: &Snapshot) -> Result<Observation, Failure> {
        let core = snapshot.core;
        if core.0 >= CORE_COUNT { return Err(Failure::InvalidCore(core)); }
        if snapshot.pc.0 % 4 != 0 { return Err(Failure::MisalignedPc(snapshot.pc)); }
        if snapshot.cpsr.0 & (1 << 5 | 1 << 24) != 0 {
            return Err(Failure::InvalidArmState(snapshot.cpsr));
        }
        if snapshot.sp.0 % 4 != 0 { return Err(Failure::MisalignedStack(snapshot.sp)); }
        let valid_entry = match (snapshot.pc == STOCK_ENTRY, snapshot.entry_instruction) {
            (false, EntryInstruction::NotEntry) | (true, EntryInstruction::OriginalPush) => true,
            (true, EntryInstruction::ScopedBranch) => self.layout.policy() == Policy::ScopedOwned75,
            _ => false,
        };
        if !valid_entry {
            return Err(Failure::InvalidEntryInstruction { pc: snapshot.pc, instruction: snapshot.entry_instruction });
        }
        let observations = self.receipt.observations.0.checked_add(1)
            .filter(|count| *count <= self.limit.0.get()).ok_or(Failure::CounterOverflow)?;
        let depth = self.depths[core.0];
        let top = depth.checked_sub(1).and_then(|index| self.frames[core.0][index].as_ref());
        self.check_ownership(snapshot)?;
        let observation = if snapshot.pc == STOCK_ENTRY {
            self.enter(snapshot)?
        } else if let Some(frame) = top {
            if snapshot.pc == frame.entry.lr {
                if self.frames[core.0][..depth - 1].iter().flatten().any(|older| {
                    older.entry.lr == snapshot.pc && older.entry.sp == snapshot.sp
                }) && snapshot.sp != frame.entry.sp {
                    return Err(Failure::WrongReturnOrder(core));
                }
                if snapshot.sp != frame.entry.sp {
                    return Err(Failure::WrongReturnStack { core, expected: frame.entry.sp, observed: snapshot.sp });
                }
                if matches!(frame.phase, Phase::AwaitingHandoff) {
                    return Err(Failure::UnresolvedHandoff(core));
                }
                self.check_parent_destination(snapshot, depth)?;
                let returns = self.receipt.returns.0.checked_add(1).ok_or(Failure::CounterOverflow)?;
                let route = frame.route;
                self.frames[core.0][depth - 1] = None;
                self.depths[core.0] -= 1;
                self.receipt.active_frames.0 -= 1;
                self.receipt.returns = Count(returns);
                Observation::Return { core, route }
            } else if self.frames[core.0][..depth - 1].iter().flatten()
                .any(|older| older.entry.lr == snapshot.pc) {
                return Err(Failure::WrongReturnOrder(core));
            } else {
                self.in_context(snapshot, depth)?
            }
        } else if self.layout.in_seam(snapshot.pc) || self.layout.body_route(snapshot.pc).is_some() {
            return Err(Failure::MissingPermit { core, pc: snapshot.pc });
        } else {
            Observation::Continue
        };
        self.receipt.observations = Count(observations);
        Ok(observation)
    }

    fn enter(&mut self, snapshot: &Snapshot) -> Result<Observation, Failure> {
        let core = snapshot.core;
        let depth = self.depths[core.0];
        let parent = depth.checked_sub(1).and_then(|index| self.frames[core.0][index].as_ref());
        if snapshot.sp.0 % 8 != 0 { return Err(Failure::MisalignedStack(snapshot.sp)); }
        let route = match snapshot.entry_instruction {
            EntryInstruction::OriginalPush => Route::StockFallback,
            EntryInstruction::ScopedBranch => self.layout.policy().route(ControllerId(snapshot.registers[0])),
            EntryInstruction::NotEntry => return Err(Failure::InvalidEntryInstruction {
                pc: snapshot.pc, instruction: snapshot.entry_instruction,
            }),
        };
        let stack_bytes = match route { Route::Owned75 => 8, Route::StockFallback => STOCK_FRAME_BYTES };
        let lowest_sp = snapshot.sp.0.checked_sub(stack_bytes).ok_or(Failure::WrappingStack(snapshot.sp))?;
        let handoff_sp = match route {
            Route::Owned75 => snapshot.sp,
            Route::StockFallback => Guest(lowest_sp),
        };
        if snapshot.lr.0 == 0 || snapshot.lr.0 % 4 != 0 || snapshot.lr == STOCK_ENTRY
            || self.layout.in_seam(snapshot.lr) {
            return Err(Failure::InvalidReturnSite(snapshot.lr));
        }
        if let Some(parent) = parent {
            if matches!(parent.phase, Phase::AwaitingHandoff) { return Err(Failure::UnresolvedHandoff(core)); }
            if snapshot.sp >= parent.handoff_sp || snapshot.sp >= parent.entry.sp {
                return Err(Failure::NonNestedContext(core));
            }
            if let Some(observed) = self.layout.body_route(snapshot.lr) {
                if observed != parent.route {
                    return Err(Failure::WrongRoute { core, expected: parent.route, observed });
                }
            }
        } else if self.layout.body_route(snapshot.lr).is_some() {
            return Err(Failure::InvalidReturnSite(snapshot.lr));
        }
        if depth == CAPACITY { return Err(Failure::DepthOverflow(core)); }
        let entries = self.receipt.entries.0.checked_add(1).ok_or(Failure::CounterOverflow)?;
        let active = self.receipt.active_frames.0.checked_add(1).ok_or(Failure::CounterOverflow)?;
        let entry = EntryState {
            registers: snapshot.registers, sp: snapshot.sp, lr: snapshot.lr, cpsr: snapshot.cpsr,
        };
        let owner = if route == Route::Owned75 { Some(*self.task(snapshot)?) } else { None };
        self.frames[core.0][depth] = Some(Frame { entry, route, handoff_sp, phase: Phase::AwaitingHandoff, owner });
        self.depths[core.0] += 1;
        self.receipt.entries = Count(entries);
        self.receipt.active_frames = Depth(active);
        self.receipt.peak_core_depth.0 = self.receipt.peak_core_depth.0.max(depth + 1);
        Ok(Observation::Entry { core, route, arm_return: snapshot.lr })
    }

    fn in_context(&mut self, snapshot: &Snapshot, depth: usize) -> Result<Observation, Failure> {
        let core = snapshot.core;
        let frame = self.frames[core.0][depth - 1].as_ref()
            .ok_or(Failure::MissingPermit { core, pc: snapshot.pc })?;
        if let Some(observed) = self.layout.body_route(snapshot.pc) {
            if observed != frame.route {
                return Err(Failure::WrongRoute { core, expected: frame.route, observed });
            }
            if matches!(frame.phase, Phase::AwaitingHandoff) {
                if Some(snapshot.pc) != self.layout.handoff(frame.route) {
                    return Err(Failure::MissingPermit { core, pc: snapshot.pc });
                }
                self.check_handoff(snapshot, frame)?;
                let route = frame.route;
                let target = self.frames[core.0][depth - 1].as_mut()
                    .ok_or(Failure::MissingPermit { core, pc: snapshot.pc })?;
                let counter = match route {
                    Route::Owned75 => &mut self.receipt.owned_handoffs,
                    Route::StockFallback => &mut self.receipt.fallback_handoffs,
                };
                let count = counter.0.checked_add(1).ok_or(Failure::CounterOverflow)?;
                counter.0 = count;
                target.phase = Phase::Running;
                Ok(Observation::Handoff { core, route })
            } else if Some(snapshot.pc) == self.layout.handoff(frame.route) {
                Err(Failure::UnexpectedHandoff(core))
            } else {
                Ok(Observation::Continue)
            }
        } else if self.layout.in_seam(snapshot.pc) {
            if matches!(frame.phase, Phase::AwaitingHandoff) {
                Ok(Observation::Continue)
            } else {
                Err(Failure::MissingPermit { core, pc: snapshot.pc })
            }
        } else if matches!(frame.phase, Phase::AwaitingHandoff) {
            Err(Failure::UnresolvedHandoff(core))
        } else {
            Ok(Observation::Continue)
        }
    }

    fn task<'a>(&self, snapshot: &'a Snapshot) -> Result<&'a TaskExecution, Failure> {
        let task = snapshot.task.as_ref().map_err(|error| Failure::TaskAttribution { core: snapshot.core, error: *error })?;
        task.validate_execution(snapshot.core, snapshot.sp, snapshot.cpsr)
            .map_err(|error| Failure::TaskAttribution { core: snapshot.core, error })?;
        Ok(task)
    }

    fn check_ownership(&self, snapshot: &Snapshot) -> Result<(), Failure> {
        let core = snapshot.core;
        if let Ok(task) = snapshot.task.as_ref() {
            if task.execution_core() != core {
                return Err(Failure::TaskAttribution { core, error: TaskError::SampledCoreMismatch {
                    sampled: task.execution_core(), observed: core,
                } });
            }
            for (active_core, frames) in self.frames.iter().enumerate() {
                if active_core == core.0 { continue; }
                for owner in frames.iter().flatten().filter_map(|frame| frame.owner.as_ref()) {
                    if owner.stack().physical().overlaps(task.stack().physical()) {
                        return Err(Failure::CrossCoreStack {
                            owner: owner.identity(), observed: task.identity(),
                            active_core: CoreId(active_core), observed_core: core,
                        });
                    }
                }
            }
        }
        let mut observed = None;
        for owner in self.frames[core.0].iter().flatten().filter_map(|frame| frame.owner.as_ref()) {
            let task = match observed {
                Some(task) => task,
                None => {
                    let task = self.task(snapshot)?;
                    observed = Some(task);
                    task
                }
            };
            if task.identity() != owner.identity() {
                return Err(Failure::OwnerMismatch { core, expected: owner.identity(), observed: task.identity() });
            }
            if task != owner { return Err(Failure::IdentityMetadataChanged(owner.identity())); }
        }
        Ok(())
    }

    fn check_handoff(&self, snapshot: &Snapshot, frame: &Frame) -> Result<(), Failure> {
        let core = snapshot.core;
        if let Some(index) = snapshot.registers.iter().zip(frame.entry.registers.iter())
            .position(|(observed, expected)| observed != expected) {
            return Err(Failure::HandoffState { core, component: StateComponent::General(GENERAL_REGISTERS[index]) });
        }
        if snapshot.lr != frame.entry.lr {
            return Err(Failure::HandoffState { core, component: StateComponent::LinkRegister });
        }
        if snapshot.cpsr != frame.entry.cpsr {
            return Err(Failure::HandoffState { core, component: StateComponent::Cpsr });
        }
        if snapshot.sp != frame.handoff_sp {
            return Err(Failure::HandoffStack { core, expected: frame.handoff_sp, observed: snapshot.sp });
        }
        if frame.route == Route::StockFallback {
            let observed = snapshot.saved_stock_frame.as_ref().ok_or(Failure::MissingStockFrame(core))?;
            if observed.0[..8] != frame.entry.registers[4..12] || observed.0[8] != frame.entry.lr.0 {
                return Err(Failure::HandoffState { core, component: StateComponent::StockFrame });
            }
        }
        Ok(())
    }

    fn check_parent_destination(&self, snapshot: &Snapshot, depth: usize) -> Result<(), Failure> {
        if let Some(observed) = self.layout.body_route(snapshot.pc) {
            let parent = depth.checked_sub(2).and_then(|index| self.frames[snapshot.core.0][index].as_ref())
                .ok_or(Failure::MissingPermit { core: snapshot.core, pc: snapshot.pc })?;
            if observed != parent.route {
                return Err(Failure::WrongRoute { core: snapshot.core, expected: parent.route, observed });
            }
        }
        Ok(())
    }
}
