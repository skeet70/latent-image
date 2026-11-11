//! X-M1 1.50 actuator adapters at 0x00743de8..0x00744c90.
//! Packet fields are authenticated against image 4; see lens_actuator_cpu_evidence.json.

use super::lens_queue_runtime::{LensQueueRuntime, Publication, RuntimeError, Status};
use super::lens_queue_runtime::packets::{CallbackId, ControlOperation, ControlPacket, PacketError, QueueRequest};
use super::lens_queue_runtime::replies::ReplyData;
use xm1_lens_transport::frame::CommandWord;
use xm1_lens_transport::wire::{Error as WireError, InterruptController, Mmio, Platform};
use xm1_power_services::shutdown::RequestId;
use core::sync::atomic::{AtomicU32, Ordering};
pub use xm1_lens_transport::control_types::FirmwareVersion as LensFirmwareVersion;
use xm1_lens_transport::control_runtime::Task99Client;
use xm1_lens_transport::control_device::PeriodicIndex;
use xm1_lens_transport::control_types::{CallbackToken, ControlError, ModeStatus, ObserverKind, PeriodicModes, TickCount};
use super::eeprom_detection::{DetectionIo, DetectionKind, DetectionMode, DetectionProvider, ServiceStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerToken(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensPosition(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensRate(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensPercentage(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selector(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionFlags { pub first: bool, pub second: bool, pub third: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusOptions {
    pub first: bool, pub second: bool, pub third: bool,
    pub mode: Selector, pub speed: Selector,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrisOptions { pub first: bool, pub second: bool, pub third: bool, pub fourth: bool }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActuatorRequest {
    QuerySigned { selector: Selector, first: bool, second: bool },
    QueryUnsigned { selector: Selector, first: bool, second: bool },
    QueryBytes { selector: Selector, first: bool, second: bool },
    Initialize { first: bool, second: bool },
    Home,
    Focus { drive: LensRate, target: LensPosition, options: FocusOptions, firmware: LensFirmwareVersion },
    FocusMaximum { target: LensPosition, options: FocusOptions, firmware: LensFirmwareVersion },
    FocusPosition(LensPosition),
    FocusEnabled(bool),
    Iris,
    IrisMove { position: LensPosition, rate: LensRate, options: IrisOptions },
    AuxiliaryHome,
    AuxiliaryPosition(LensPosition),
    AuxiliaryFlags { first: bool, second: bool },
    Control { enabled: bool, selector: Selector },
    PositionRate { position: LensPosition, rate: LensRate, flags: MotionFlags },
    QueryFlags,
    QueryPosition,
    PositionEnabled(bool),
    PercentageEnabled(bool),
    PositionPercentage { percentage: LensPercentage, position: LensPosition },
    AuxiliaryMode(Selector),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActuatorOperation {
    QuerySigned, QueryUnsigned, QueryBytes, Initialize, Home, Focus, FocusMaximum,
    FocusPosition, FocusEnabled, Iris, IrisMove, AuxiliaryHome, AuxiliaryPosition,
    AuxiliaryFlags, Control, PositionRate, QueryFlags, QueryPosition,
    PositionEnabled, PercentageEnabled, PositionPercentage, AuxiliaryMode,
}
impl ActuatorOperation {
    const fn index(self) -> usize { self as usize }
    pub const fn request_id(self) -> RequestId {
        RequestId(match self {
            Self::QuerySigned => 0x720d, Self::QueryUnsigned => 0x720e,
            Self::QueryBytes => 0x7210, Self::Initialize => 0x7218, Self::Home => 0x721a,
            Self::Focus | Self::FocusMaximum => 0x721d, Self::FocusPosition => 0x721e,
            Self::FocusEnabled => 0x721f, Self::Iris => 0x7221, Self::IrisMove => 0x7222,
            Self::AuxiliaryHome => 0x7224, Self::AuxiliaryPosition => 0x7226,
            Self::AuxiliaryFlags => 0x7227, Self::Control => 0x722b,
            Self::PositionRate => 0x7241, Self::QueryFlags => 0x7212,
            Self::QueryPosition => 0x723a, Self::PositionEnabled => 0x7240,
            Self::PercentageEnabled => 0x7246, Self::PositionPercentage => 0x7242,
            Self::AuxiliaryMode => 0x722d,
        })
    }
}
impl ActuatorRequest {
    pub const fn operation(self) -> ActuatorOperation {
        match self {
            Self::QuerySigned { .. } => ActuatorOperation::QuerySigned,
            Self::QueryUnsigned { .. } => ActuatorOperation::QueryUnsigned,
            Self::QueryBytes { .. } => ActuatorOperation::QueryBytes,
            Self::Initialize { .. } => ActuatorOperation::Initialize, Self::Home => ActuatorOperation::Home,
            Self::Focus { .. } => ActuatorOperation::Focus, Self::FocusMaximum { .. } => ActuatorOperation::FocusMaximum,
            Self::FocusPosition(_) => ActuatorOperation::FocusPosition, Self::FocusEnabled(_) => ActuatorOperation::FocusEnabled,
            Self::Iris => ActuatorOperation::Iris, Self::IrisMove { .. } => ActuatorOperation::IrisMove,
            Self::AuxiliaryHome => ActuatorOperation::AuxiliaryHome, Self::AuxiliaryPosition(_) => ActuatorOperation::AuxiliaryPosition,
            Self::AuxiliaryFlags { .. } => ActuatorOperation::AuxiliaryFlags, Self::Control { .. } => ActuatorOperation::Control,
            Self::PositionRate { .. } => ActuatorOperation::PositionRate, Self::QueryFlags => ActuatorOperation::QueryFlags,
            Self::QueryPosition => ActuatorOperation::QueryPosition, Self::PositionEnabled(_) => ActuatorOperation::PositionEnabled,
            Self::PercentageEnabled(_) => ActuatorOperation::PercentageEnabled,
            Self::PositionPercentage { .. } => ActuatorOperation::PositionPercentage, Self::AuxiliaryMode(_) => ActuatorOperation::AuxiliaryMode,
        }
    }
    pub fn packet(self, callback: CallbackId) -> Result<ControlPacket, PacketError> {
        let mut words = [CommandWord::new(0); 3];
        let count = match self {
            Self::PositionRate { position, rate, flags } => {
                words[0] = command(0x5440, position.0);
                words[1] = command(0x5480, (rate.0 << 3) | u16::from(flags.first) | (u16::from(flags.second) << 1) | (u16::from(flags.third) << 2));
                2
            }
            Self::PositionPercentage { percentage, position } => {
                words[0] = command(0x54c0, u16::from(percentage.0));
                words[1] = command(0x5500, position.0);
                2
            }
            Self::Focus { drive, target, options, firmware } => {
                focus_words(&mut words, drive, target, options, firmware); 3
            }
            Self::FocusMaximum { target, options, firmware } => {
                focus_words(&mut words, LensRate(0x7fff), target, options, firmware); 3
            }
            Self::IrisMove { position, rate, options } => {
                words[0] = command(0x1840, position.0 & 0x7fff);
                words[1] = command(0x1880, (rate.0 & 0xfff) | (u16::from(options.first) << 15) | (u16::from(options.second) << 13) | (u16::from(options.third) << 14) | (u16::from(options.fourth) << 12));
                2
            }
            other => {
                let (code, value) = match other {
                    Self::QuerySigned { selector, first, second } => (0x840, (u16::from(selector.0) << 8) | (u16::from(first) << 4) | u16::from(second)),
                    Self::QueryUnsigned { selector, first, second } => (0x880, (u16::from(selector.0 & 15) << 8) | (u16::from(first) << 4) | u16::from(second)),
                    Self::QueryBytes { selector, first, second } => (0x900, (u16::from(selector.0 & 15) << 8) | (u16::from(first) << 4) | u16::from(second)),
                    Self::Initialize { first, second } => (0x1000, u16::from(first) | (u16::from(second) << 1)),
                    Self::Home => (0x1400, 0), Self::FocusPosition(value) => (0x1500, value.0 & 0x7fff),
                    Self::FocusEnabled(value) => (0x1600, u16::from(value)), Self::Iris => (0x1800, 0),
                    Self::AuxiliaryHome => (0x1c00, 0), Self::AuxiliaryPosition(value) => (0x1d40, value.0),
                    Self::AuxiliaryFlags { first, second } => (0x1e00, (u16::from(first) << 8) | u16::from(second)),
                    Self::Control { enabled, selector } => (0x2800, (u16::from(enabled) << 15) | u16::from(selector.0 & 0x7f)),
                    Self::QueryFlags => (0x9c0, 0), Self::QueryPosition => (0x4800, 0),
                    Self::PositionEnabled(value) => (0x5400, u16::from(value)), Self::PercentageEnabled(value) => (0x5800, u16::from(value)),
                    Self::AuxiliaryMode(value) => (0x2a40, u16::from(value.0 & 15)),
                    Self::PositionRate { .. } | Self::PositionPercentage { .. } | Self::Focus { .. } | Self::FocusMaximum { .. } | Self::IrisMove { .. } => unreachable!(),
                };
                words[0] = command(code, value); 1
            }
        };
        let operation = ControlOperation::parse(self.operation().request_id()).ok_or(PacketError::Operation)?;
        ControlPacket::new(operation, callback, &words[..count])
    }
}
fn command(code: u16, value: u16) -> CommandWord { CommandWord::new(u32::from(code) | (u32::from(value) << 16)) }
fn focus_words(words: &mut [CommandWord; 3], drive: LensRate, target: LensPosition, options: FocusOptions, firmware: LensFirmwareVersion) {
    let mut flags = u16::from(options.first) | (u16::from(options.second) << 1) | (u16::from(options.third) << 8) | (u16::from(options.speed.0 & 3) << 12);
    if firmware.0 < 200 {
        if options.mode.0 < 3 { flags |= u16::from(options.mode.0) << 14; }
    } else { flags |= u16::from(options.mode.0 & 15) << 4; }
    words[0] = command(0x1500, drive.0 & 0x7fff);
    words[1] = command(0x1540, flags);
    words[2] = command(0x1580, target.0);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActuatorData {
    StatusOnly, SignedPosition(i16), FlaggedPosition { flag: bool, position: LensPosition },
    FlaggedBytes { flag: bool, first: u8, second: u8 }, Flags([bool; 3]),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActuatorCompletion {
    pub owner: OwnerToken, pub operation: ActuatorOperation, pub failed: bool,
    pub status: Status, pub data: ActuatorData,
}
#[derive(Debug, Eq, PartialEq)]
pub enum ActuatorError<E> { Queue(RuntimeError<E>), Packet(PacketError) }
#[derive(Clone, Copy)]
struct Slot { callback: CallbackId, owner: Option<OwnerToken>, operation: ActuatorOperation }

pub struct LensActuatorCpu { slots: [Option<Slot>; 22], observer: Option<(CallbackId, OwnerToken)> }
impl Default for LensActuatorCpu { fn default() -> Self { Self::new() } }
impl LensActuatorCpu {
    pub const fn new() -> Self { Self { slots: [None; 22], observer: None } }
    pub fn execute<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, owner: Option<OwnerToken>, request: ActuatorRequest) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> {
        let operation = request.operation();
        let index = operation.index();
        let callback = match self.slots[index] {
            Some(slot) => slot.callback,
            None => queue.allocate_callback().map_err(ActuatorError::Queue)?,
        };
        let packet = request.packet(callback).map_err(ActuatorError::Packet)?;
        self.slots[index] = Some(Slot { callback, owner, operation });
        queue.submit(QueueRequest::Control(packet), None).map_err(ActuatorError::Queue)
    }
    pub fn position_rate<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, owner: Option<OwnerToken>, position: LensPosition, rate: LensRate, flags: MotionFlags) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> {
        self.execute(queue, owner, ActuatorRequest::PositionRate { position, rate, flags })
    }
    pub fn position_percentage<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, owner: Option<OwnerToken>, percentage: LensPercentage, position: LensPosition) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> {
        self.execute(queue, owner, ActuatorRequest::PositionPercentage { percentage, position })
    }
    pub fn home<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, owner: Option<OwnerToken>) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> { self.execute(queue, owner, ActuatorRequest::Home) }
    pub fn iris<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, owner: Option<OwnerToken>) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> { self.execute(queue, owner, ActuatorRequest::Iris) }
    pub fn control<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, owner: Option<OwnerToken>, enabled: bool, selector: Selector) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> { self.execute(queue, owner, ActuatorRequest::Control { enabled, selector }) }
    pub fn focus<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, owner: Option<OwnerToken>, drive: LensRate, target: LensPosition, options: FocusOptions, firmware: LensFirmwareVersion) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> {
        self.execute(queue, owner, ActuatorRequest::Focus { drive, target, options, firmware })
    }
    pub fn set_detection_mode<B: DetectionIo>(&self, detection: &DetectionProvider<'_, B>, enabled: bool) -> Result<(), ServiceStatus> {
        detection.set_mode(if enabled { DetectionMode::On } else { DetectionMode::Off })
    }
    pub fn register_observer<M: Mmio, P: Platform, I: InterruptController, B: DetectionIo>(&mut self, queue: &mut LensQueueRuntime<M, P, I>, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents, owner: OwnerToken) -> Result<(), ObserverError<WireError<P::Error, I::Error>>> {
        detection.register(DetectionMode::On, None, DetectionKind::Once).map_err(ObserverError::Detection)?;
        let callback = queue.allocate_callback().map_err(ObserverError::Queue)?;
        self.observer = Some((callback, owner));
        events.arm(callback);
        if let Err(error) = detection.register(DetectionMode::On, Some(callback), DetectionKind::Once) {
            events.cancel();
            self.observer = None;
            return Err(ObserverError::Detection(error));
        }
        Ok(())
    }
    pub fn unregister_observer<B: DetectionIo>(&mut self, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents) -> Result<(), ServiceStatus> {
        self.observer = None;
        events.cancel();
        detection.register(DetectionMode::On, None, DetectionKind::Once)
    }
    pub fn observer_completion(&mut self, callback: CallbackId) -> Option<ObserverCompletion> {
        let (active, owner) = self.observer?;
        if callback != active { return None; }
        self.observer = None;
        Some(ObserverCompletion { owner })
    }
    /// 0x744ab4 clears registrations without stopping pending queue requests.
    pub fn reset(&mut self) { self.slots.fill(None); self.observer = None; }
    pub fn cancel<M: Mmio, P: Platform, I: InterruptController>(&mut self, queue: &mut LensQueueRuntime<M, P, I>) -> Result<(), ActuatorError<WireError<P::Error, I::Error>>> {
        self.reset();
        queue.cancel().map_err(ActuatorError::Queue)
    }
    pub fn publication(&self, publication: Publication) -> Option<ActuatorCompletion> {
        let Publication::Command { callback, operation, status, data } = publication else { return None; };
        let slot = self.slots.iter().flatten().find(|slot| slot.callback == callback && slot.operation.request_id() == operation.request_id())?;
        let owner = slot.owner?;
        let data = match (slot.operation, data) {
            (ActuatorOperation::QuerySigned, ReplyData::Halfword(value)) => ActuatorData::SignedPosition(value.0 as i16),
            (ActuatorOperation::QueryUnsigned | ActuatorOperation::QueryPosition, ReplyData::FlaggedUnsigned { flag, value }) => ActuatorData::FlaggedPosition { flag, position: LensPosition(value.0) },
            (ActuatorOperation::QueryBytes, ReplyData::FlaggedBytes { flag, upper, lower }) => ActuatorData::FlaggedBytes { flag, first: upper, second: lower },
            (ActuatorOperation::QueryFlags, ReplyData::ThreeFlags([first, second, third])) => ActuatorData::Flags([third, first, second]),
            _ => ActuatorData::StatusOnly,
        };
        Some(ActuatorCompletion { owner, operation: slot.operation, failed: status.driver_word() != 0, status, data })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObserverCompletion { pub owner: OwnerToken }
#[derive(Debug, Eq, PartialEq)]
pub enum ObserverError<E> { Queue(RuntimeError<E>), Detection(ServiceStatus) }

pub struct ActuatorEvents { active: AtomicU32, pending: AtomicU32 }
impl Default for ActuatorEvents { fn default() -> Self { Self::new() } }
impl ActuatorEvents {
    pub const fn new() -> Self { Self { active: AtomicU32::new(0), pending: AtomicU32::new(0) } }
    pub fn arm(&self, callback: CallbackId) {
        self.active.store(0, Ordering::Release);
        self.pending.store(0, Ordering::Release);
        self.active.store(callback.0, Ordering::Release);
    }
    pub fn cancel(&self) { self.active.store(0, Ordering::Release); self.pending.store(0, Ordering::Release); }
    pub fn signal(&self, callback: CallbackId) -> bool {
        callback.0 != 0 && self.active.load(Ordering::Acquire) == callback.0
            && self.pending.compare_exchange(0, callback.0, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
    pub fn take(&self) -> Option<CallbackId> {
        let callback = self.pending.swap(0, Ordering::AcqRel);
        if callback == 0 { return None; }
        self.active.compare_exchange(callback, 0, Ordering::AcqRel, Ordering::Acquire).ok().map(CallbackId)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterSubmission { Accepted, Rejected }
impl AdapterSubmission {
    pub const fn firmware_status(self) -> u32 { match self { Self::Accepted => 0, Self::Rejected => 0x86 } }
    fn from_result(result: Result<(), ControlError>) -> Self { if result.is_ok() { Self::Accepted } else { Self::Rejected } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationPollInterval(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollCadence { Fast, Normal }

pub fn not_suspended(client: &Task99Client<'_>) -> bool { client.mode_status() != ModeStatus::Suspended }
pub fn inactive(client: &Task99Client<'_>) -> bool { client.mode_status() == ModeStatus::Unavailable }
pub fn start_position_poll(client: &Task99Client<'_>, cadence: PollCadence, callback: Option<CallbackToken>) -> AdapterSubmission {
    let ticks = match cadence { PollCadence::Fast => 1, PollCadence::Normal => 400 };
    AdapterSubmission::from_result(client.configure_periodic(PeriodicIndex::new(6).unwrap(), TickCount(ticks), PeriodicModes::new(2).unwrap(), callback))
}
pub fn stop_position_poll(client: &Task99Client<'_>) -> AdapterSubmission {
    AdapterSubmission::from_result(client.cancel_periodic(PeriodicIndex::new(6).unwrap()))
}
pub fn start_calibration_poll(client: &Task99Client<'_>, interval: CalibrationPollInterval, callback: Option<CallbackToken>) -> AdapterSubmission {
    AdapterSubmission::from_result(client.configure_periodic(PeriodicIndex::new(31).unwrap(), TickCount(u32::from(interval.0)), PeriodicModes::new(3).unwrap(), callback))
}
pub fn stop_calibration_poll(client: &Task99Client<'_>) -> AdapterSubmission {
    AdapterSubmission::from_result(client.cancel_periodic(PeriodicIndex::new(31).unwrap()))
}
pub async fn synchronize(client: &Task99Client<'_>) -> Result<xm1_lens_transport::control_types::CompletionStatus, ControlError> { client.synchronize().await }
pub fn release(client: &Task99Client<'_>) -> Result<(), ControlError> { client.release() }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensStatusByte(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CachedByteStatus { pub flagged: bool, pub value: LensStatusByte }
pub fn cached_byte_status(client: &Task99Client<'_>) -> CachedByteStatus {
    let [value, flag] = client.cached_two_bytes();
    CachedByteStatus { flagged: flag != 0, value: LensStatusByte(value) }
}
pub fn request_two_byte_snapshot(client: &Task99Client<'_>, callback: CallbackToken) -> Result<(), ControlError> { client.request_snapshot(ObserverKind::TwoBytes, callback) }
pub fn request_three_byte_snapshot(client: &Task99Client<'_>, callback: CallbackToken) -> Result<(), ControlError> { client.request_snapshot(ObserverKind::ThreeBytes, callback) }
pub fn request_nine_byte_snapshot(client: &Task99Client<'_>, callback: CallbackToken) -> Result<(), ControlError> { client.request_snapshot(ObserverKind::NineBytes, callback) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MissingFlagsSnapshot {
    pub status: xm1_lens_transport::control_types::CompletionStatus,
    pub failed: bool,
    pub flags: [bool; 3],
}
impl MissingFlagsSnapshot {
    pub fn from_bytes(status: xm1_lens_transport::control_types::CompletionStatus, bytes: [u8; 9]) -> Self {
        Self { status, failed: status.0 != 0, flags: [bytes[7] != 0, bytes[0] != 0, bytes[4] != 0] }
    }
}
pub async fn missing_flags(client: &Task99Client<'_>) -> Result<MissingFlagsSnapshot, ControlError> {
    let result = client.snapshot(ObserverKind::NineBytes).await?;
    let xm1_lens_transport::control_types::SnapshotData::NineBytes(bytes) = result.data else { return Err(ControlError::InvalidPayload); };
    Ok(MissingFlagsSnapshot::from_bytes(result.status, bytes))
}

#[cfg(test)]
#[path = "lens_actuator_cpu_tests.rs"]
mod tests;
