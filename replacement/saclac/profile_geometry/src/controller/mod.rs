#![forbid(unsafe_code)]

//! The DCF controller: its lifecycle, the operations stock starters run on the image pipeline,
//! and the parameter refresh that keeps their pipeline configuration current.

pub mod changes;
pub mod crop;
pub mod lifecycle;
pub mod operations;
pub mod payloads;
pub mod rectangles;
pub mod refresh;
pub mod scan;
pub mod scan_geometry;
pub mod stages;
pub mod windows;

use crate::memory::Memory;
use crate::numeric::FractionTables;
use crate::ports::{PipelineChannel, StockCode};
use crate::profile::{ProfileTables, Profiles};
use crate::stock::layout::controller as field;

/// The image tables the layer reads.
pub trait Tables: ProfileTables + FractionTables {}

impl<T: ProfileTables + FractionTables> Tables for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The controller is already where the request would take it.
    AlreadyThere,
    WrongState,
    /// The pipeline refused a stop or a configuration.
    PipelineRefused,
}

impl Refusal {
    pub const fn code(self) -> u32 {
        match self {
            Refusal::AlreadyThere => 0x11,
            Refusal::WrongState => 0x81,
            Refusal::PipelineRefused => 0x86,
        }
    }
}

pub type Status = Result<(), Refusal>;

pub const fn status_code(status: Status) -> u32 {
    match status {
        Ok(()) => 0,
        Err(refusal) => refusal.code(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    Terminated,
    Closed,
    Open,
    Started,
}

impl Lifecycle {
    pub const fn word(self) -> u32 {
        match self {
            Lifecycle::Terminated => 0,
            Lifecycle::Closed => 1,
            Lifecycle::Open => 2,
            Lifecycle::Started => 3,
        }
    }

    pub const fn from_word(word: u32) -> Option<Self> {
        match word {
            0 => Some(Lifecycle::Terminated),
            1 => Some(Lifecycle::Closed),
            2 => Some(Lifecycle::Open),
            3 => Some(Lifecycle::Started),
            _ => None,
        }
    }
}

/// The operations stock starters run through the controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Primary,
    First,
    Second,
    Third,
    Indexed,
    Auxiliary,
    Channel,
}

/// Stopping an operation stores `STOPPING` until the pipeline has stopped its stages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationState(pub u32);

impl OperationState {
    pub const IDLE: Self = Self(0);
    pub const RUNNING: Self = Self(1);
    pub const STOPPING: Self = Self(4);

    pub const fn idle(self) -> bool { self.0 == 0 }

    pub const fn running(self) -> bool { self.0 == 1 }
}

/// The layer's single owner of the controller state, the profile tables and the calls out.
pub struct ProfileGeometry<M, P, T> {
    pub(crate) memory: M,
    pub(crate) ports: P,
    pub(crate) profiles: Profiles<T>,
}

impl<M: Memory, P, T: ProfileTables> ProfileGeometry<M, P, T> {
    pub const fn new(memory: M, ports: P, profiles: Profiles<T>) -> Self { Self { memory, ports, profiles } }

    pub fn memory(&self) -> &M { &self.memory }

    pub fn ports(&mut self) -> &mut P { &mut self.ports }

    pub fn lifecycle(&self) -> Option<Lifecycle> { Lifecycle::from_word(self.memory.read_u32(field::LIFECYCLE)) }

    pub(crate) fn set_lifecycle(&mut self, lifecycle: Lifecycle) { self.memory.write_u32(field::LIFECYCLE, lifecycle.word()) }

    pub fn operation(&self, operation: Operation) -> OperationState {
        OperationState(self.memory.read_u32(field::state(operation)))
    }

    pub(crate) fn set_operation(&mut self, operation: Operation, state: OperationState) {
        self.memory.write_u32(field::state(operation), state.0)
    }

    pub(crate) fn completion(&self, operation: Operation) -> StockCode {
        field::completion(operation).map_or(StockCode(0), |at| StockCode(self.memory.read_u32(at)))
    }

    pub(crate) fn clear_completion(&mut self, operation: Operation) {
        if let Some(at) = field::completion(operation) {
            self.memory.write_u32(at, 0);
        }
    }

    pub(crate) fn pipeline_channel(&self) -> PipelineChannel { PipelineChannel(self.memory.read_u32(field::PIPELINE_CHANNEL)) }

    pub(crate) fn all_idle(&self, operations: &[Operation]) -> bool { operations.iter().all(|&operation| self.operation(operation).idle()) }
}
