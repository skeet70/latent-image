//! Selected queue 0x32 (decimal 50) lens-control requests, not task 99 or full AF.
//! X-M1 1.50 source/index pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Image 4 SHA-256: f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16.
//! Request roots: 0076f6d8/0076f7c4/0076f840; task: 0075fe54.
//! Selected transition rows: 00819834, selectors 26..28 and 31, width 16.

use core::num::NonZeroU32;
use super::lens_queue_runtime::{LensQueueRuntime, Publication, RuntimeError, Status};
use super::lens_queue_runtime::packets::{CallbackId, ControlOperation, ControlPacket, PacketError, QueueRequest};
use super::lens_queue_runtime::replies::ReplyData;
use xm1_lens_transport::frame::CommandWord;
use xm1_lens_transport::wire::{Error as WireError, InterruptController, Mmio, Platform};
use xm1_power_services::shutdown::RequestId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientId(NonZeroU32);
impl ClientId {
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensMachineState(u8);
impl LensMachineState {
    pub const fn from_snapshot(value: u16) -> Option<Self> {
        if value < 16 { Some(Self(value as u8)) } else { None }
    }
    pub const fn snapshot(self) -> u16 { self.0 as u16 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensRequest {
    ReadPosition { client: ClientId },
    SetOption { client: ClientId, enabled: bool },
    SetMode { client: ClientId, enabled: bool },
}
impl LensRequest {
    pub const fn client(self) -> ClientId {
        match self { Self::ReadPosition { client } | Self::SetOption { client, .. } | Self::SetMode { client, .. } => client }
    }
    pub const fn opcode(self) -> u16 {
        match self { Self::ReadPosition { .. } => 0x4218, Self::SetOption { .. } => 0x4219, Self::SetMode { .. } => 0x421a }
    }
    fn slot(self) -> usize {
        match self { Self::ReadPosition { .. } => 0, Self::SetOption { .. } => 1, Self::SetMode { .. } => 2 }
    }
    fn operation(self) -> RequestId {
        match self { Self::ReadPosition { .. } => RequestId(0x723a), Self::SetOption { .. } => RequestId(0x7240), Self::SetMode { .. } => RequestId(0x7246) }
    }
    fn command(self) -> CommandWord {
        let word = match self {
            Self::ReadPosition { .. } => 0x120 << 6,
            Self::SetOption { enabled, .. } => (0x150 << 6) | (u32::from(enabled) << 16),
            Self::SetMode { enabled, .. } => (0x160 << 6) | (u32::from(enabled) << 16),
        };
        CommandWord::new(word)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensOutcome {
    Position { flag: bool, position: LensPosition },
    Complete,
    RejectedBusy,
    Driver(Status),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensPosition(u16);
impl LensPosition { pub const fn get(self) -> u16 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensCompletion { pub client: ClientId, pub outcome: LensOutcome }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Submission { Waiting, Duplicate, Completed(LensCompletion) }
#[derive(Debug, Eq, PartialEq)]
pub enum ActorError<E> {
    Runtime(RuntimeError<E>),
    Packet(PacketError),
    Uninitialized(LensMachineState),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionError { UnexpectedReply }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateTransitionError { SelectedRequestPending, StaleState }
#[derive(Clone, Copy)]
struct Pending { request: LensRequest, callback: CallbackId }

pub struct LensRequestConsumer {
    state: LensMachineState,
    pending: [Option<Pending>; 3],
}
impl LensRequestConsumer {
    pub const fn from_machine_snapshot(state: LensMachineState) -> Self {
        Self { state, pending: [None; 3] }
    }
    pub const fn machine_state(&self) -> LensMachineState { self.state }

    pub fn transition_when_idle(&mut self, expected: LensMachineState, next: LensMachineState) -> Result<(), StateTransitionError> {
        if self.pending.iter().any(Option::is_some) { return Err(StateTransitionError::SelectedRequestPending); }
        if self.state != expected { return Err(StateTransitionError::StaleState); }
        self.state = next;
        Ok(())
    }

    pub fn submit<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        runtime: &mut LensQueueRuntime<M, P, I>,
        request: LensRequest,
    ) -> Result<Submission, ActorError<WireError<P::Error, I::Error>>> {
        let slot = request.slot();
        if self.pending[slot].is_some() { return Ok(Submission::Duplicate); }
        match self.state.0 {
            0 | 1 => return Err(ActorError::Uninitialized(self.state)),
            2 => (),
            _ => return Ok(Submission::Completed(LensCompletion { client: request.client(), outcome: LensOutcome::RejectedBusy })),
        }
        let callback = runtime.allocate_callback().map_err(ActorError::Runtime)?;
        let operation = ControlOperation::parse(request.operation()).ok_or(ActorError::Packet(PacketError::Operation))?;
        let packet = ControlPacket::new(operation, callback, &[request.command()]).map_err(ActorError::Packet)?;
        runtime.submit(QueueRequest::Control(packet), None).map_err(ActorError::Runtime)?;
        self.pending[slot] = Some(Pending { request, callback });
        self.state = LensMachineState(15);
        Ok(Submission::Waiting)
    }

    pub fn consume(&mut self, publication: Publication) -> Result<Option<LensCompletion>, CompletionError> {
        let Publication::Command { callback, operation, status, data } = publication else { return Ok(None); };
        let Some(slot) = self.pending.iter().position(|entry| entry.is_some_and(|entry| entry.callback == callback)) else { return Ok(None); };
        let Some(pending) = self.pending[slot] else { return Ok(None); };
        if pending.request.operation() != operation.request_id() { return Err(CompletionError::UnexpectedReply); }
        let outcome = if status != Status::Complete {
            LensOutcome::Driver(status)
        } else {
            match (pending.request, data) {
                (LensRequest::ReadPosition { .. }, ReplyData::FlaggedUnsigned { flag, value }) => LensOutcome::Position { flag, position: LensPosition(value.0) },
                (LensRequest::SetOption { .. } | LensRequest::SetMode { .. }, ReplyData::StatusOnly) => LensOutcome::Complete,
                _ => return Err(CompletionError::UnexpectedReply),
            }
        };
        self.pending[slot] = None;
        self.state = LensMachineState(2);
        Ok(Some(LensCompletion { client: pending.request.client(), outcome }))
    }
}

#[cfg(test)]
#[path = "af_capture_actor_tests.rs"]
mod tests;
