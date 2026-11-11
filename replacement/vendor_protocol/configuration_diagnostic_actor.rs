//! Configuration action 16, composed with the owned queue-98 CPU runtime.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, 0x00dbb3a4.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};
use crate::configuration_diagnostic_registers::{DiagnosticRegisters, DiagnosticTick};
use crate::configuration_i2c_actor::RegisterIo;
use crate::lens_queue_runtime::{LensQueueRuntime, Publication, Status};
use crate::lens_queue_runtime::packets::{CallbackId, QueueRequest};
use xm1_lens_transport::wire::{InterruptController, Mmio, Platform};
use xm1_power_services::shutdown::RequestId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticOutcome { Pending, Complete(ActionCompletion) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, Programming, Settling(DiagnosticTick), Waiting { start: DiagnosticTick, callback: CallbackId, remaining: u8 } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticError { Busy, Configuration(BackendError) }

impl From<BackendError> for DiagnosticError {
    fn from(error: BackendError) -> Self { Self::Configuration(error) }
}

pub struct DiagnosticActor {
    registers: DiagnosticRegisters,
    phase: Phase,
    output: Option<ConfigurationKey>,
    generation: u32,
}

impl DiagnosticActor {
    pub const fn new() -> Self { Self { registers: DiagnosticRegisters::new(), phase: Phase::Idle, output: None, generation: 0 } }

    fn complete(&mut self, images: &mut ConfigImages<'_>, success: bool) -> Result<DiagnosticOutcome, DiagnosticError> {
        self.phase = Phase::Idle;
        if let Some(output) = self.output.take() { output.write(images, u8::from(!success))?; }
        Ok(DiagnosticOutcome::Complete(if success { ActionCompletion::service_success() } else { ActionCompletion::service_rejected() }))
    }

    pub fn begin(&mut self, io: &mut impl RegisterIo, images: &mut ConfigImages<'_>, keys: &[ConfigurationKey; 2], now: DiagnosticTick) -> Result<DiagnosticOutcome, DiagnosticError> {
        if self.phase != Phase::Idle { return Err(DiagnosticError::Busy); }
        self.output = Some(keys[1]);
        match keys[0].read(images)? {
            0 => { self.registers.disable(io); self.complete(images, true) }
            1 => {
                self.registers.enable(io, now).map_err(|_| DiagnosticError::Busy)?;
                self.phase = Phase::Programming;
                Ok(DiagnosticOutcome::Pending)
            }
            _ => self.complete(images, false),
        }
    }

    fn submit_sample<M: Mmio, P: Platform, I: InterruptController>(&mut self, runtime: &mut LensQueueRuntime<M, P, I>, images: &mut ConfigImages<'_>, now: DiagnosticTick, remaining: u8) -> Result<DiagnosticOutcome, DiagnosticError> {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 { self.generation = 1; }
        let callback = CallbackId(self.generation);
        let mut packet = [0; 48];
        packet[..4].copy_from_slice(&callback.0.to_le_bytes());
        packet[4..6].copy_from_slice(&0x3fc0u16.to_le_bytes());
        packet[44] = 1;
        let request = QueueRequest::parse(RequestId(0x7202), &packet);
        let sent = match request { Ok(request) => runtime.submit(request, None).is_ok(), Err(_) => false };
        if !sent { return self.complete(images, false); }
        self.phase = Phase::Waiting { start: now, callback, remaining };
        Ok(DiagnosticOutcome::Pending)
    }

    pub fn poll<M: Mmio, P: Platform, I: InterruptController>(&mut self, io: &mut impl RegisterIo, runtime: &mut LensQueueRuntime<M, P, I>, images: &mut ConfigImages<'_>, now: DiagnosticTick) -> Result<DiagnosticOutcome, DiagnosticError> {
        match self.phase {
            Phase::Programming if self.registers.poll(io, now) => {
                if runtime.submit(QueueRequest::Initialize { callback: CallbackId(0), observer: CallbackId(0) }, None).is_err() { return self.complete(images, false); }
                self.phase = Phase::Settling(now);
                Ok(DiagnosticOutcome::Pending)
            }
            Phase::Settling(start) if now.0.wrapping_sub(start.0) >= 3000 => self.submit_sample(runtime, images, now, 3),
            Phase::Waiting { start, .. } if now.0.wrapping_sub(start.0) >= 5000 => self.complete(images, false),
            _ => Ok(DiagnosticOutcome::Pending),
        }
    }

    pub fn consume<M: Mmio, P: Platform, I: InterruptController>(&mut self, runtime: &mut LensQueueRuntime<M, P, I>, images: &mut ConfigImages<'_>, publication: Publication, now: DiagnosticTick) -> Result<Option<DiagnosticOutcome>, DiagnosticError> {
        let Phase::Waiting { callback: expected, remaining, .. } = self.phase else { return Ok(None); };
        let Publication::Command { callback, operation, status, .. } = publication else { return Ok(None); };
        if callback != expected || operation.request_id() != RequestId(0x7202) { return Ok(None); }
        if status != Status::Complete { return self.complete(images, false).map(Some); }
        if remaining == 1 { self.complete(images, true).map(Some) }
        else { self.submit_sample(runtime, images, now, remaining - 1).map(Some) }
    }

    pub fn cancel<M: Mmio, P: Platform, I: InterruptController>(&mut self, io: &mut impl RegisterIo, runtime: &mut LensQueueRuntime<M, P, I>, images: &mut ConfigImages<'_>) -> Result<Option<DiagnosticOutcome>, DiagnosticError> {
        if self.phase == Phase::Idle { return Ok(None); }
        let _ = runtime.cancel();
        self.registers.disable(io);
        self.complete(images, false).map(Some)
    }
}
