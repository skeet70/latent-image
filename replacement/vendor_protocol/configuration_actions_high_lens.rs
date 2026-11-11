//! X-M1 high configuration lens transactions, pinned at 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey, CpuReturn};
use crate::configuration_actions_high::{CallbackNotification, CallbackState, CallbackStatus};
use crate::lens_queue_runtime::{LensQueueRuntime, Publication, RuntimeError, Status};
use crate::lens_queue_runtime::packets::{CallbackId, ControlOperation, ControlPacket, PacketError, QueueRequest};
use crate::lens_queue_runtime::replies::ReplyData;
use xm1_lens_transport::frame::CommandWord;
use xm1_lens_transport::wire::{InterruptController, Mmio, Platform, Error as WireError};
use xm1_power_services::shutdown::RequestId;

const STATUS_CALLBACK: CallbackId = CallbackId(0x00dc6374);
const WORD_CALLBACK: CallbackId = CallbackId(0x00dc6314);
const TRANSFER_WORD_CALLBACK: CallbackId = CallbackId(0x00dc4e94);
const TRANSFER_BITS_CALLBACK: CallbackId = CallbackId(0x00dc4ee0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferQuery { Bits, Word }
impl TransferQuery {
    pub const fn from_wrapper(address: u32) -> Option<Self> {
        match address { 0x00eccee4 => Some(Self::Bits), 0x00eccefc => Some(Self::Word), _ => None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensAction { ReadSelectedPairs, SetControlPair, SetLevelSelection, ReadBothPairs }
impl LensAction {
    pub const fn from_wrapper(address: u32) -> Option<Self> {
        match address {
            0x00eccf44 => Some(Self::ReadSelectedPairs),
            0x00eccf5c => Some(Self::SetControlPair),
            0x00eccfbc => Some(Self::SetLevelSelection),
            0x00ecd04c => Some(Self::ReadBothPairs),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PairWindow { At81, At83 }
impl PairWindow {
    const fn base(self) -> u16 { match self { Self::At81 => 0x81, Self::At83 => 0x83 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending {
    Idle,
    PairSetup { first_output: PairWindow },
    FirstPair { first_output: PairWindow },
    SecondPair { first_output: PairWindow },
    ControlSetup,
    ControlValue,
    LevelSelection,
    TransferQuery,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error<E> { Configuration(BackendError), Packet(PacketError), Runtime(RuntimeError<E>), Busy, Reply }
impl<E> From<BackendError> for Error<E> { fn from(error: BackendError) -> Self { Self::Configuration(error) } }
impl<E> From<PacketError> for Error<E> { fn from(error: PacketError) -> Self { Self::Packet(error) } }

pub struct LensActions { pending: Pending }
impl LensActions {
    pub const fn new() -> Self { Self { pending: Pending::Idle } }

    pub const fn waiting_notification(&self) -> Option<CallbackNotification> {
        match self.pending {
            Pending::Idle => None,
            Pending::TransferQuery => Some(CallbackNotification::Transfer),
            _ => Some(CallbackNotification::Level),
        }
    }

    pub fn begin_query<M: Mmio, P: Platform, I: InterruptController>(&mut self, query: TransferQuery, configuration: &ConfigImages<'_>, callbacks: &mut CallbackState, runtime: &mut LensQueueRuntime<M, P, I>) -> Result<Option<ActionCompletion>, Error<WireError<P::Error, I::Error>>> {
        if self.pending != Pending::Idle { return Err(Error::Busy); }
        callbacks.high_byte = 0;
        callbacks.low_byte = 0;
        callbacks.result = CpuReturn::SUCCESS;
        let selector = byte(configuration, 0x81)?;
        let (operation, word, callback) = match query {
            TransferQuery::Bits => (0x7238, 0x4640 | (u32::from(selector & 0xf) << 16), TRANSFER_BITS_CALLBACK),
            TransferQuery::Word => (0x7239, 0x4680 | (u32::from(selector & 0x3f) << 16), TRANSFER_WORD_CALLBACK),
        };
        submit(runtime, operation, word, callback)?;
        self.pending = Pending::TransferQuery;
        Ok(None)
    }

    pub fn begin<M: Mmio, P: Platform, I: InterruptController>(&mut self, action: LensAction, configuration: &ConfigImages<'_>, runtime: &mut LensQueueRuntime<M, P, I>) -> Result<Option<ActionCompletion>, Error<WireError<P::Error, I::Error>>> {
        if self.pending != Pending::Idle { return Err(Error::Busy); }
        let (operation, word, pending) = match action {
            LensAction::ReadSelectedPairs => {
                let first = byte(configuration, 0x81)?;
                let second = byte(configuration, 0x82)?;
                if first > 1 || second > 1 { return Ok(Some(ActionCompletion::service_rejected())); }
                (0x7248, 0x6040 | (u32::from(second | first << 1) << 16), Pending::PairSetup { first_output: PairWindow::At83 })
            }
            LensAction::ReadBothPairs => (0x7248, 0x0004_6040, Pending::PairSetup { first_output: PairWindow::At81 }),
            LensAction::SetControlPair => {
                let mode_input = byte(configuration, 0x81)?;
                let mode: u8 = match mode_input { 0 => 3, 1 | 2 => 0, _ => return Ok(Some(ActionCompletion::service_rejected())) };
                let flag: u8 = match byte(configuration, 0x82)? { 0 => 1, 1 => 0, _ => return Ok(Some(ActionCompletion::service_rejected())) };
                let extra = u8::from(mode_input == 2);
                (0x724b, 0x6100 | (u32::from(mode << 3 | flag << 1 | extra) << 16), Pending::ControlSetup)
            }
            LensAction::SetLevelSelection => {
                let enabled = byte(configuration, 0x81)?;
                if enabled > 1 { return Ok(Some(ActionCompletion::service_rejected())); }
                let selection: u8 = match byte(configuration, 0x82)? { 0 => 1, 1 => 0, 2 => 2, 3 => 3, _ => return Ok(Some(ActionCompletion::service_rejected())) };
                (0x724f, 0x6200 | (u32::from(selection << 1 | enabled) << 16), Pending::LevelSelection)
            }
        };
        submit(runtime, operation, word, STATUS_CALLBACK)?;
        self.pending = pending;
        Ok(None)
    }

    pub fn publication<M: Mmio, P: Platform, I: InterruptController>(&mut self, publication: Publication, configuration: &mut ConfigImages<'_>, callbacks: &mut CallbackState, runtime: &mut LensQueueRuntime<M, P, I>) -> Result<Option<ActionCompletion>, Error<WireError<P::Error, I::Error>>> {
        let Publication::Command { callback, status, data, .. } = publication else { return Ok(None); };
        let callback_status = if status == Status::Complete { CallbackStatus::Success } else { CallbackStatus::Failure };
        if callback == WORD_CALLBACK {
            let ReplyData::Halfword(word) = data else { return Err(Error::Reply); };
            callbacks.level_word(callback_status, word.0);
        } else if callback == STATUS_CALLBACK {
            callbacks.level_complete(callback_status);
        } else if callback == TRANSFER_WORD_CALLBACK {
            let ReplyData::Halfword(word) = data else { return Err(Error::Reply); };
            callbacks.transfer_word(callback_status, word.0);
        } else if callback == TRANSFER_BITS_CALLBACK {
            let ReplyData::Flags(flags) = data else { return Err(Error::Reply); };
            let bits = flags.0.map(u8::from);
            callbacks.transfer_bits(callback_status, &bits);
        } else { return Ok(None); }
        // The stock callbacks publish state even after their waiter has returned.
        if self.pending == Pending::Idle { return Ok(None); }
        if callbacks.result != CpuReturn::SUCCESS {
            self.pending = Pending::Idle;
            return Ok(Some(ActionCompletion::service_rejected()));
        }
        let next = match self.pending {
            Pending::PairSetup { first_output } => {
                submit(runtime, 0x723c, 0x0001_4880, WORD_CALLBACK)?;
                Pending::FirstPair { first_output }
            }
            Pending::FirstPair { first_output } => {
                write_pair(configuration, first_output.base(), callbacks.high_byte, callbacks.low_byte)?;
                submit(runtime, 0x723c, 0x4880, WORD_CALLBACK)?;
                Pending::SecondPair { first_output }
            }
            Pending::SecondPair { first_output } => {
                write_pair(configuration, first_output.base() + 2, callbacks.high_byte, callbacks.low_byte)?;
                Pending::Idle
            }
            Pending::ControlSetup => {
                let value = u16::from_be_bytes([byte(configuration, 0x83)?, byte(configuration, 0x84)?]);
                submit(runtime, 0x724c, 0x6140 | (u32::from(value) << 16), STATUS_CALLBACK)?;
                Pending::ControlValue
            }
            Pending::ControlValue | Pending::LevelSelection => Pending::Idle,
            Pending::TransferQuery => {
                write_pair(configuration, 0x82, callbacks.high_byte, callbacks.low_byte)?;
                Pending::Idle
            }
            Pending::Idle => return Ok(None),
        };
        self.pending = next;
        Ok(if next == Pending::Idle { Some(ActionCompletion::service_success()) } else { None })
    }

    pub fn wait_failed(&mut self) -> Option<ActionCompletion> {
        if self.pending == Pending::Idle { None } else {
            self.pending = Pending::Idle;
            Some(ActionCompletion::service_rejected())
        }
    }
}

fn byte(configuration: &ConfigImages<'_>, offset: u16) -> Result<u8, BackendError> { ConfigurationKey::from_abi(offset).read(configuration) }
fn write_pair(configuration: &mut ConfigImages<'_>, offset: u16, high: u8, low: u8) -> Result<(), BackendError> {
    ConfigurationKey::from_abi(offset).write(configuration, high)?;
    ConfigurationKey::from_abi(offset + 1).write(configuration, low)
}
fn submit<M: Mmio, P: Platform, I: InterruptController>(runtime: &mut LensQueueRuntime<M, P, I>, operation: u16, word: u32, callback: CallbackId) -> Result<(), Error<WireError<P::Error, I::Error>>> {
    let operation = ControlOperation::parse(RequestId(operation)).ok_or(PacketError::Operation)?;
    let packet = ControlPacket::new(operation, callback, &[CommandWord::new(word)])?;
    runtime.submit(QueueRequest::Control(packet), None).map_err(Error::Runtime)
}

#[cfg(test)]
#[path = "configuration_actions_high_lens_tests.rs"]
mod tests;
