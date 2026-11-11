//! Middle configuration property transactions through the owned queue 0x62 consumer.

use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::{ActionCompletion, ConfigurationKey, CpuReturn}, configuration_actions_middle::{CallbackDisposition, MiddleAction, MiddleArguments, publish_byte_pair}, lens_queue_runtime::{LensQueueRuntime, Publication, RuntimeError, packets::{CallbackId, ControlOperation, ControlPacket, QueueRequest}, replies::ReplyData}};
use xm1_lens_transport::{control_actor::ControlActor, control_types::ModeStatus, frame::CommandWord, wire::{Error as WireError, InterruptController, Mmio, Platform}};
use xm1_power_services::shutdown::RequestId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyProgress { Waiting, Completed(ActionCompletion) }
#[derive(Debug, Eq, PartialEq)]
pub enum PropertyError<E> { Configuration(BackendError), Driver(RuntimeError<E>), Busy, Action, Reply }
#[derive(Clone, Copy)]
struct OptionWait { high: ConfigurationKey, low: ConfigurationKey, status: ConfigurationKey }
#[derive(Clone, Copy)]
enum Sequence { BankMode { enabled: bool, selection: ConfigurationKey }, Banks { selection: bool }, Halfword { value: u16 } }
#[derive(Clone, Copy)]
enum Continuation { Option(OptionWait), Sequence { sequence: Sequence, step: Step } }
#[derive(Clone, Copy)]
struct Pending { callback: CallbackId, operation: ControlOperation, continuation: Continuation }
#[derive(Clone, Copy)]
enum Step { Mode, Bank00, Bank01, Bank10, Bank11, Channel, Value }
#[derive(Clone, Copy)]
#[repr(u16)]
enum PropertyOperation { ReadOption = 0x723b, ReadBank = 0x723e, Mode = 0x7243, Channel = 0x7244, Value = 0x7245 }
#[derive(Default)]
pub struct MiddlePropertyActions { pending: Option<Pending> }

impl MiddlePropertyActions {
    fn submit<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        operation: PropertyOperation,
        word: CommandWord,
        continuation: Continuation,
        runtime: &mut LensQueueRuntime<M, P, I>,
    ) -> Result<PropertyProgress, PropertyError<WireError<P::Error, I::Error>>> {
        let callback = runtime.allocate_callback().map_err(PropertyError::Driver)?;
        let operation = ControlOperation::parse(RequestId(operation as u16)).ok_or(PropertyError::Action)?;
        let packet = ControlPacket::new(operation, callback, &[word]).map_err(|_| PropertyError::Reply)?;
        runtime.submit(QueueRequest::Control(packet), None).map_err(PropertyError::Driver)?;
        self.pending = Some(Pending { callback, operation, continuation });
        Ok(PropertyProgress::Waiting)
    }

    pub fn begin<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        action: MiddleAction,
        arguments: MiddleArguments<'_>,
        configuration: &mut ConfigImages<'_>,
        device: &ControlActor,
        runtime: &mut LensQueueRuntime<M, P, I>,
    ) -> Result<PropertyProgress, PropertyError<WireError<P::Error, I::Error>>> {
        if self.pending.is_some() { return Err(PropertyError::Busy); }
        match action.queue_action() {
            64 | 74 => {
                let bank = u32::from(action.queue_action() == 74);
                let channel = arguments.read(0, configuration).map_err(PropertyError::Configuration)? & 1;
                let high = arguments.key(1).map_err(PropertyError::Configuration)?;
                let low = arguments.key(2).map_err(PropertyError::Configuration)?;
                let status = arguments.key(3).map_err(PropertyError::Configuration)?;
                let command = CommandWord::new((0x121 << 6) | (u32::from(channel) << 16) | (bank << 20));
                self.submit(PropertyOperation::ReadOption, command, Continuation::Option(OptionWait { high, low, status }), runtime)
            }
            66 => {
                for key in 0x83..=0x8a { ConfigurationKey::from_abi(key).write(configuration, 0).map_err(PropertyError::Configuration)?; }
                if device.mode_status() != ModeStatus::Suspended { return Ok(PropertyProgress::Completed(ActionCompletion::service_success())); }
                let enabled = match arguments.read(0, configuration).map_err(PropertyError::Configuration)? {
                    0 => false, 1 => true, _ => return Ok(PropertyProgress::Completed(ActionCompletion::service_success())),
                };
                let selection = arguments.key(1).map_err(PropertyError::Configuration)?;
                let sequence = Sequence::BankMode { enabled, selection };
                self.submit(PropertyOperation::Mode, CommandWord::new((0x155 << 6) | ((enabled as u32) << 16)), Continuation::Sequence { sequence, step: Step::Mode }, runtime)
            }
            67 => {
                if device.mode_status() != ModeStatus::Suspended { return Ok(PropertyProgress::Completed(ActionCompletion::service_success())); }
                let enabled = match arguments.read(0, configuration).map_err(PropertyError::Configuration)? {
                    0 => false, 1 => true, _ => return Ok(PropertyProgress::Completed(ActionCompletion::service_success())),
                };
                let value = arguments.read_pair(1, configuration).map_err(PropertyError::Configuration)?;
                self.submit(PropertyOperation::Channel, CommandWord::new((0x156 << 6) | ((enabled as u32) << 16)), Continuation::Sequence { sequence: Sequence::Halfword { value }, step: Step::Channel }, runtime)
            }
            _ => Err(PropertyError::Action),
        }
    }

    pub fn publication<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        publication: Publication,
        configuration: &mut ConfigImages<'_>,
        device: &ControlActor,
        runtime: &mut LensQueueRuntime<M, P, I>,
    ) -> Result<Option<PropertyProgress>, PropertyError<WireError<P::Error, I::Error>>> {
        let Some(wait) = self.pending else { return Ok(None); };
        let Publication::Command { callback, operation, status, data } = publication else { return Ok(None); };
        if callback != wait.callback || operation != wait.operation { return Ok(None); }
        self.pending = None;
        match wait.continuation {
            Continuation::Option(wait) => {
                let ReplyData::Halfword(value) = data else { return Err(PropertyError::Reply); };
                let failed = status.driver_word() != 0;
                wait.status.write(configuration, u8::from(failed)).map_err(PropertyError::Configuration)?;
                let [high, low] = value.0.to_be_bytes();
                wait.high.write(configuration, high).map_err(PropertyError::Configuration)?;
                wait.low.write(configuration, low).map_err(PropertyError::Configuration)?;
                let mut completion = ActionCompletion::service_success();
                if failed { completion.return_code = CpuReturn::REJECTED; }
                Ok(Some(PropertyProgress::Completed(completion)))
            }
            Continuation::Sequence { sequence, step } => {
                if let Some(high) = step.output_key() {
                    let ReplyData::Halfword(value) = data else { return Err(PropertyError::Reply); };
                    let disposition = if status.driver_word() == 0 { CallbackDisposition::Accepted } else { CallbackDisposition::Failed };
                    publish_byte_pair(disposition, value.0, ConfigurationKey::from_abi(high), ConfigurationKey::from_abi(high + 1), configuration).map_err(PropertyError::Configuration)?;
                }
                if device.mode_status() != ModeStatus::Suspended { return Ok(Some(PropertyProgress::Completed(ActionCompletion::service_success()))); }
                let sequence = match (sequence, step) {
                    (Sequence::BankMode { enabled: true, .. }, Step::Mode) => return Ok(Some(PropertyProgress::Completed(ActionCompletion::service_success()))),
                    (Sequence::BankMode { selection, .. }, Step::Mode) => {
                        let selection = selection.read(configuration).map_err(PropertyError::Configuration)?;
                        if selection > 1 { return Ok(Some(PropertyProgress::Completed(ActionCompletion::service_success()))); }
                        Sequence::Banks { selection: selection != 0 }
                    }
                    _ => sequence,
                };
                let next = match (sequence, step) {
                    (Sequence::Banks { selection }, Step::Mode) => Some((PropertyOperation::ReadBank, bank_command(false, selection, false), Step::Bank00)),
                    (Sequence::Banks { selection }, Step::Bank00) => Some((PropertyOperation::ReadBank, bank_command(false, selection, true), Step::Bank01)),
                    (Sequence::Banks { selection }, Step::Bank01) => Some((PropertyOperation::ReadBank, bank_command(true, selection, false), Step::Bank10)),
                    (Sequence::Banks { selection }, Step::Bank10) => Some((PropertyOperation::ReadBank, bank_command(true, selection, true), Step::Bank11)),
                    (Sequence::Halfword { value }, Step::Channel) => Some((PropertyOperation::Value, CommandWord::new((0x157 << 6) | (u32::from(value) << 16)), Step::Value)),
                    _ => None,
                };
                match next {
                    Some((operation, command, step)) => self.submit(operation, command, Continuation::Sequence { sequence, step }, runtime).map(Some),
                    None => Ok(Some(PropertyProgress::Completed(ActionCompletion::service_success()))),
                }
            }
        }
    }

    pub fn cancel<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        runtime: &mut LensQueueRuntime<M, P, I>,
    ) -> Result<(), PropertyError<WireError<P::Error, I::Error>>> {
        if self.pending.take().is_some() { runtime.cancel().map_err(PropertyError::Driver)?; }
        Ok(())
    }

    pub fn is_waiting(&self) -> bool { self.pending.is_some() }
}

impl Step {
    const fn output_key(self) -> Option<u16> {
        match self { Self::Bank00 => Some(0x87), Self::Bank01 => Some(0x89), Self::Bank10 => Some(0x83), Self::Bank11 => Some(0x85), _ => None }
    }
}

fn bank_command(enabled: bool, selection: bool, lane: bool) -> CommandWord {
    let payload = (enabled as u32) | ((selection as u32) << 1) | ((lane as u32) << 2);
    CommandWord::new((0x124 << 6) | (payload << 16))
}

#[cfg(test)]
#[path = "configuration_actions_middle_property_tests.rs"]
mod tests;
