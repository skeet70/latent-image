#![no_std]

pub mod client;
pub mod protocol;
pub mod receiver;
pub mod sender;
#[cfg(target_arch = "arm")]
mod producer;
#[cfg(target_arch = "arm")]
pub mod native;

/// The single NAND owner of this composition: the standalone unit, or the copy
/// the Root bank already links inside power services.
#[cfg(not(loading_nand_in_power_services))]
pub use ::xm1_application_nand as nand;
#[cfg(loading_nand_in_power_services)]
pub use ::xm1_power_services::application_nand as nand;

use protocol::{CommandKind, ProtocolError, Request, RequestId};
use nand::dma_read::{Deadline, Failure, Instant, ReadReport};

pub const QUEUE_ID: u32 = 60;
/// Stock queue60 geometry (`dispatch_create_channel`): 0x30 messages of 16 bytes.
pub const QUEUE_MESSAGES: u32 = 0x30;
pub const QUEUE_MESSAGE_BYTES: u32 = 0x10;

pub trait Clock {
    fn now(&mut self) -> Instant;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retention { Released, HeldQuiescent, Unproved }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionOutcome {
    Loaded(ReadReport),
    Skipped(ReadReport),
    Delegated,
    Failed { failure: Failure, report: ReadReport, retention: Retention },
    Rejected(ProtocolError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    pub id: RequestId,
    pub kind: CommandKind,
    pub outcome: CompletionOutcome,
}

/// Construction requires the producer's completed-read and executable-validation proofs.
#[derive(Debug)]
pub struct LoadedProof { report: ReadReport }
impl LoadedProof {
    /// # Safety
    /// The complete executable was read and validated under this request's original deadline.
    /// DMA and all lower resources have been released.
    pub unsafe fn from_validated_read(report: ReadReport) -> Self { Self { report } }
    pub fn report(self) -> ReadReport { self.report }
}

#[derive(Debug)]
pub enum ProducerCompletion {
    Loaded(LoadedProof),
    Skipped(ReadReport),
    Delegated,
    Failed { failure: Failure, report: ReadReport, retention: Retention },
}

pub trait Producer {
    fn execute(&mut self, request: Request) -> ProducerCompletion;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackResult { Succeeded, Failed }
impl CallbackResult {
    pub const fn legacy_failed(self) -> u32 { match self { Self::Succeeded => 0, Self::Failed => 1 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Publication {
    pub completion: Completion,
    pub completed_latch: bool,
    pub busy: bool,
    pub callback_result: CallbackResult,
}

pub trait ReceiverKernel {
    /// A successful publication commits the result before notifying the waiter and callback.
    /// An error commits nothing. Loaded and delegated commits check the deadline atomically.
    fn publish(&mut self, request: Request, publication: Publication) -> Result<(), ProtocolError>;
}

pub trait SenderKernel {
    fn initialize_queue(&mut self, queue: u32, messages: u32, message_bytes: u32) -> Result<(), ProtocolError>;
    fn start_receiver(&mut self) -> Result<(), ProtocolError>;
    /// Queue admission is bounded by the supplied absolute deadline.
    fn post(&mut self, queue: u32, operation: u16, envelope: &[u8; protocol::ENVELOPE_BYTES], deadline: Deadline) -> Result<(), ProtocolError>;
    /// Inspect a committed completion without consuming its typed result.
    fn peek_completion(&mut self, id: RequestId, kind: CommandKind) -> Option<Completion>;
    /// The completion slot is consumed only for the supplied request and command.
    fn take_completion(&mut self, id: RequestId, kind: CommandKind) -> Option<Completion>;
    /// A wake is not a completion. This wait is bounded by the absolute deadline.
    fn wait(&mut self, id: RequestId, kind: CommandKind, deadline: Deadline) -> Result<(), ProtocolError>;
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
