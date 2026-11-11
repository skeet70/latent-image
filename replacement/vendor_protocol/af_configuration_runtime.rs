//! X-M1 application AF callback ownership and queue50 requests.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::af_capture_actor::{ActorError, ClientId, CompletionError, LensCompletion, LensOutcome, LensRequest, LensRequestConsumer, Submission};
use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::ConfigurationKey;
use crate::lens_queue_runtime::{LensQueueRuntime, Publication};
use xm1_lens_transport::wire::{Error as WireError, InterruptController, Mmio, Platform};
use core::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackOwner(NonZeroU32);
impl CallbackOwner {
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistrationId(ClientId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationRoot { Initialize, Select, Operation1, Operation2, Operation3, Operation4, Operation5, Operation6 }
impl RegistrationRoot {
    pub const fn from_address(address: u32) -> Option<Self> {
        match address {
            0x00efc2f8 => Some(Self::Initialize), 0x00efc360 => Some(Self::Select),
            0x00efc3a4 => Some(Self::Operation1), 0x00efc3e8 => Some(Self::Operation2),
            0x00efc42c => Some(Self::Operation3), 0x00efc470 => Some(Self::Operation4),
            0x00efc4b4 => Some(Self::Operation5), 0x00efc4f8 => Some(Self::Operation6), _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request { Register(RegistrationRoot), ReadPosition, ClearOption, ClearMode, PrimaryParameter, SecondaryParameter }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackStatus { Success, Busy }
impl CallbackStatus {
    pub const fn abi(self) -> i32 { match self { Self::Success => 0, Self::Busy => -95 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    pub owner: CallbackOwner,
    pub status: CallbackStatus,
    pub registration: Option<RegistrationId>,
    pub waiter_attached: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress { Waiting(RegistrationId), Callback(Completion), NoCallback }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifetimeError { Stale, DeliveryPending, IdentifiersExhausted, CallbackRequired }
#[derive(Debug, Eq, PartialEq)]
pub enum Error<E> { Lifetime(LifetimeError), Lens(ActorError<E>), Reply(CompletionError), Configuration(BackendError) }

#[derive(Clone, Copy)]
struct Pending { id: RegistrationId, owner: CallbackOwner, request: Request, attached: bool, delivering: bool }

pub struct AfRuntime { pending: Option<Pending>, next: u32 }
impl Default for AfRuntime { fn default() -> Self { Self::new() } }
impl AfRuntime {
    pub const fn new() -> Self { Self { pending: None, next: 1 } }
    pub const fn busy(&self) -> bool { self.pending.is_some() }

    pub fn begin<M: Mmio, P: Platform, I: InterruptController>(
        &mut self, request: Request, owner: Option<CallbackOwner>, consumer: &mut LensRequestConsumer,
        lens: &mut LensQueueRuntime<M, P, I>, configuration: &mut ConfigImages<'_>,
    ) -> Result<Progress, Error<WireError<P::Error, I::Error>>> {
        if matches!(request, Request::PrimaryParameter | Request::SecondaryParameter) {
            return Ok(match owner {
                Some(owner) => Progress::Callback(Completion { owner, status: CallbackStatus::Success, registration: None, waiter_attached: true }),
                None => Progress::NoCallback,
            });
        }
        let owner = owner.ok_or(Error::Lifetime(LifetimeError::CallbackRequired))?;
        if self.pending.is_some() {
            return Ok(Progress::Callback(Completion { owner, status: CallbackStatus::Busy, registration: None, waiter_attached: true }));
        }
        let client = ClientId::new(self.next).ok_or(Error::Lifetime(LifetimeError::IdentifiersExhausted))?;
        self.next = self.next.checked_add(1).unwrap_or(0);
        let id = RegistrationId(client);
        self.pending = Some(Pending { id, owner, request, attached: true, delivering: false });
        let queue_request = match request {
            Request::Register(_) => return Ok(Progress::Waiting(id)),
            Request::ReadPosition => LensRequest::ReadPosition { client },
            Request::ClearOption => LensRequest::SetOption { client, enabled: false },
            Request::ClearMode => LensRequest::SetMode { client, enabled: false },
            Request::PrimaryParameter | Request::SecondaryParameter => unreachable!(),
        };
        match consumer.submit(lens, queue_request).map_err(Error::Lens)? {
            Submission::Waiting | Submission::Duplicate => Ok(Progress::Waiting(id)),
            Submission::Completed(completion) => self.lens_completion(completion, configuration).map(|completion| completion.map_or(Progress::Waiting(id), Progress::Callback)).map_err(Error::Configuration),
        }
    }

    pub fn publication(&mut self, publication: Publication, consumer: &mut LensRequestConsumer, configuration: &mut ConfigImages<'_>) -> Result<Option<Completion>, PublicationError> {
        let completion = consumer.consume(publication).map_err(PublicationError::Reply)?;
        match completion { Some(completion) => self.lens_completion(completion, configuration).map_err(PublicationError::Configuration), None => Ok(None) }
    }

    fn lens_completion(&mut self, completion: LensCompletion, configuration: &mut ConfigImages<'_>) -> Result<Option<Completion>, BackendError> {
        let Some(pending) = self.pending else { return Ok(None); };
        if pending.id.0 != completion.client || pending.delivering { return Ok(None); }
        if pending.request == Request::ReadPosition {
            let value = match completion.outcome { LensOutcome::Position { position, .. } => position.get(), _ => 0 };
            let [high, low] = value.to_be_bytes();
            ConfigurationKey::from_abi(0x82).write(configuration, high)?;
            ConfigurationKey::from_abi(0x83).write(configuration, low)?;
        }
        self.pending = Some(Pending { delivering: true, ..pending });
        Ok(Some(Completion { owner: pending.owner, status: CallbackStatus::Success, registration: Some(pending.id), waiter_attached: pending.attached }))
    }

    // Stock clears busy after invoking the registered callback.
    pub fn delivered(&mut self, id: RegistrationId) -> Result<(), LifetimeError> {
        match self.pending {
            Some(pending) if pending.id == id && pending.delivering => { self.pending = None; Ok(()) },
            Some(pending) if pending.id == id => Err(LifetimeError::DeliveryPending),
            _ => Err(LifetimeError::Stale),
        }
    }

    // A service wait timeout does not write the application AF busy word.
    pub fn detach_waiter(&mut self, id: RegistrationId) -> Result<(), LifetimeError> {
        match self.pending {
            Some(pending) if pending.id == id => { self.pending = Some(Pending { attached: false, ..pending }); Ok(()) },
            _ => Err(LifetimeError::Stale),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum PublicationError { Reply(CompletionError), Configuration(BackendError) }

#[cfg(test)]
#[path = "af_configuration_runtime_tests.rs"]
mod tests;
