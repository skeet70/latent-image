//! Configuration action 17 and its actual queue-98 command 0x7217 consumer.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, 0x00dbb258 and 0x006bcbec.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};
use crate::configuration_serial_actors::{PeripheralFlags, PeripheralStatus, PeripheralStatusBytes};
use crate::lens_queue_runtime::{LensQueueRuntime, Publication, Status};
use crate::lens_queue_runtime::packets::{CallbackId, QueueRequest};
use crate::lens_queue_runtime::replies::ReplyData;
use xm1_lens_transport::wire::{InterruptController, Mmio, Platform};
use xm1_power_services::shutdown::RequestId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusStart { Pending, Complete(ActionCompletion) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusError { Busy, Configuration(BackendError) }

impl From<BackendError> for StatusError {
    fn from(value: BackendError) -> Self { Self::Configuration(value) }
}

pub struct StatusActor {
    status: PeripheralStatus,
    callback: CallbackId,
    pending: Option<[ConfigurationKey; 8]>,
    cancelled: bool,
}

impl StatusActor {
    pub const fn adopt(status: PeripheralStatusBytes, flags: PeripheralFlags, callback: CallbackId) -> Self {
        Self { status: PeripheralStatus::adopt(status, flags), callback, pending: None, cancelled: false }
    }

    pub fn begin<M: Mmio, P: Platform, I: InterruptController>(&mut self, runtime: &mut LensQueueRuntime<M, P, I>, keys: [ConfigurationKey; 8]) -> Result<StatusStart, StatusError> {
        if self.pending.is_some() { return Err(StatusError::Busy); }
        let mut packet = [0; 48];
        packet[..4].copy_from_slice(&self.callback.0.to_le_bytes());
        packet[4..6].copy_from_slice(&0x0fc0u16.to_le_bytes());
        packet[44] = 1;
        let sent = match QueueRequest::parse(RequestId(0x7217), &packet) {
            Ok(request) => runtime.submit(request, None).is_ok(),
            Err(_) => false,
        };
        if !sent { return Ok(StatusStart::Complete(ActionCompletion::service_rejected())); }
        self.pending = Some(keys);
        self.cancelled = false;
        Ok(StatusStart::Pending)
    }

    pub fn consume(&mut self, images: &mut ConfigImages<'_>, publication: Publication, completion_saved_value: u32) -> Result<Option<ActionCompletion>, StatusError> {
        let Some(keys) = self.pending else { return Ok(None); };
        let Publication::Command { callback, operation, status, data } = publication else { return Ok(None); };
        if callback != self.callback || operation.request_id() != RequestId(0x7217) { return Ok(None); }
        self.pending = None;
        if status == Status::Complete {
            if let ReplyData::ObserverFlags(flags) = data {
                self.status.accumulate(PeripheralStatusBytes(flags.map(u8::from)));
            }
        }
        if self.cancelled { return Ok(None); }
        self.status.publish(images, &keys, completion_saved_value).map(Some).map_err(StatusError::from)
    }

    pub fn cancel(&mut self) -> Option<ActionCompletion> {
        if self.pending.is_none() || self.cancelled { return None; }
        self.cancelled = true;
        Some(ActionCompletion::service_rejected())
    }
}
