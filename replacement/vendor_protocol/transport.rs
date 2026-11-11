#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;

#[path = "../rtos_irq_control/src/lib.rs"]
pub mod irq_control;

#[path = "transport_cpu.rs"]
pub mod cpu;

#[path = "transport_scheduler.rs"]
pub mod rtos_scheduler;
pub use rtos_scheduler::s3d as scheduler;

pub use xm1_power_services::{
    primitives::ServiceStatus,
    shutdown_task7_8::RegistryHandle,
    shutdown_task7_8_transport::RegistrationRegistry,
};

#[cfg(test)]
#[path = "transport_teardown_tests.rs"]
mod teardown_tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError {
    AddressOverflow,
    EmptySpan,
    OutsideRegion,
    BufferTooSmall,
    TransferTooLong,
    InvalidChannel,
    ChannelBusy,
    InvalidSelector,
    InvalidTimer,
    Scheduler(ServiceStatus),
    Release(ServiceStatus),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishedRegion {
    base: u32,
    length: u32,
}

impl PublishedRegion {
    pub fn new(base: u32, length: u32) -> Result<Self, TransportError> {
        if length == 0 { return Err(TransportError::EmptySpan); }
        base.checked_add(length).ok_or(TransportError::AddressOverflow)?;
        Ok(Self { base, length })
    }

    pub const fn base(self) -> u32 { self.base }
    pub const fn length(self) -> u32 { self.length }

    pub fn span(self, offset: u32, length: u32) -> Result<RegionSpan, TransportError> {
        if length == 0 { return Err(TransportError::EmptySpan); }
        let end = offset.checked_add(length).ok_or(TransportError::AddressOverflow)?;
        if end > self.length { return Err(TransportError::OutsideRegion); }
        let address = self.base.checked_add(offset).ok_or(TransportError::AddressOverflow)?;
        Ok(RegionSpan { region: self, offset, length, address })
    }

    pub fn span_at(self, address: u32, length: u32) -> Result<RegionSpan, TransportError> {
        let offset = address.checked_sub(self.base).ok_or(TransportError::OutsideRegion)?;
        self.span(offset, length)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionSpan {
    region: PublishedRegion,
    offset: u32,
    length: u32,
    address: u32,
}

impl RegionSpan {
    pub const fn offset(self) -> u32 { self.offset }
    pub const fn length(self) -> u32 { self.length }
    pub const fn address(self) -> u32 { self.address }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackSlot {
    Setup,
    Reserved,
    ControlComplete,
    BulkOutSource,
    BulkOutComplete,
    BulkInSource,
    BulkInComplete,
}

pub const CALLBACK_TABLE: [CallbackSlot; 8] = [
    CallbackSlot::Setup, CallbackSlot::Reserved, CallbackSlot::ControlComplete,
    CallbackSlot::BulkOutSource, CallbackSlot::BulkOutComplete,
    CallbackSlot::BulkInSource, CallbackSlot::BulkInComplete, CallbackSlot::Reserved,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupAction { Reject, ReceiveControl { length: u32 } }
impl SetupAction {
    pub const fn controller_code(self) -> u32 {
        match self { Self::Reject => 2, Self::ReceiveControl { .. } => 0x0200_0000 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceAction { OutSuppressed, InNotReady, Transfer(RegionSpan), EmptyReply(PublishedRegion) }
impl SourceAction {
    pub const fn controller_code(self) -> u32 {
        match self { Self::OutSuppressed => 16, Self::InNotReady => 256, Self::Transfer(_) | Self::EmptyReply(_) => 0x0300_0000 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionStatus { Success, Failed(i32) }
impl From<i32> for CompletionStatus {
    fn from(value: i32) -> Self { if value == 0 { Self::Success } else { Self::Failed(value) } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseKind { None, Control, Bulk }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionAction {
    None,
    SendStatus([u8; 2]),
    ReceiveComplete { start: u32, length: u32, status: Option<[u8; 2]> },
    ProcessReceive { status: Option<[u8; 2]> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportEvent {
    QueueStatus,
    Disconnect,
}
impl TransportEvent {
    pub const fn wire_message(self) -> Option<(u16, u32, u32)> {
        match self {
            Self::QueueStatus => Some((0x340e, 0, 0)),
            Self::Disconnect => Some((1, 0x90, 0)),
        }
    }
}

pub struct PublishedAperture<'a> {
    aperture: &'a mut [u8],
    region: PublishedRegion,
}

impl<'a> PublishedAperture<'a> {
    pub fn publish(region: PublishedRegion, aperture: &'a mut [u8]) -> Result<Self, TransportError> {
        if aperture.len() != region.length as usize { return Err(TransportError::OutsideRegion); }
        Ok(Self { aperture, region })
    }

    pub const fn region(&self) -> PublishedRegion { self.region }

    pub fn bytes(&self, span: RegionSpan) -> Result<&[u8], TransportError> {
        if span.region != self.region { return Err(TransportError::OutsideRegion); }
        Ok(&self.aperture[span.offset as usize..(span.offset + span.length) as usize])
    }

    pub fn bytes_mut(&mut self, span: RegionSpan) -> Result<&mut [u8], TransportError> {
        if span.region != self.region { return Err(TransportError::OutsideRegion); }
        Ok(&mut self.aperture[span.offset as usize..(span.offset + span.length) as usize])
    }
}

pub struct TransportState {
    suppress_next: bool,
    out_span: Option<RegionSpan>,
    out_progress: u32,
    out_total: u32,
    out_start: u32,
    stream: bool,
    response: ResponseKind,
    status: [u8; 2],
    pending: bool,
}

impl TransportState {
    pub const fn new() -> Self {
        Self { suppress_next: false, out_span: None,
            out_progress: 0, out_total: 0, out_start: 0, stream: false,
            response: ResponseKind::None, status: [0; 2], pending: false }
    }

    pub fn setup(&mut self, request_type: u8) -> SetupAction {
        self.suppress_next = false;
        if request_type & 0x60 == 0x40 && request_type & 0x80 == 0 {
            SetupAction::ReceiveControl { length: 12 }
        } else { SetupAction::Reject }
    }

    pub fn set_response(&mut self, kind: ResponseKind, status: [u8; 2]) {
        self.response = kind;
        self.status = status;
    }

    pub fn control_complete(&mut self, completion: CompletionStatus) -> CompletionAction {
        if completion != CompletionStatus::Success || self.response != ResponseKind::Control {
            return CompletionAction::None;
        }
        self.suppress_next = true;
        self.response = ResponseKind::None;
        CompletionAction::SendStatus(self.status)
    }

    pub fn arm_receive(&mut self, region: PublishedRegion, offset: u32, total: u32, chunk: u32, stream: bool) -> Result<(), TransportError> {
        region.span(offset, total)?;
        if chunk > total { return Err(TransportError::TransferTooLong); }
        self.out_span = Some(region.span(offset, chunk)?);
        self.out_progress = 0;
        self.out_total = total;
        self.out_start = offset;
        self.stream = stream;
        Ok(())
    }

    pub fn clear_receive(&mut self) {
        self.stream = false;
        self.out_span = None;
        self.out_progress = 0;
        self.out_total = 0;
        self.out_start = 0;
    }

    pub fn bulk_out_source(&mut self) -> Result<SourceAction, TransportError> {
        if self.suppress_next {
            self.suppress_next = false;
            return Ok(SourceAction::OutSuppressed);
        }
        self.out_span.map(SourceAction::Transfer).ok_or(TransportError::BufferTooSmall)
    }

    pub fn bulk_in_source(&mut self, region: PublishedRegion, offset: u32, available: u32, requested: u32, stream: bool,
        armed: &mut bool, ready: &mut bool) -> Result<SourceAction, TransportError> {
        if self.suppress_next {
            self.suppress_next = false;
            return Ok(SourceAction::InNotReady);
        }
        if !stream && (!*armed || !*ready) { return Ok(SourceAction::InNotReady); }
        if !stream && core::cmp::min(available, requested) == 0 {
            *armed = false;
            *ready = false;
            return Ok(SourceAction::EmptyReply(region));
        }
        let span = region.span(offset, core::cmp::min(available, requested))?;
        if !stream { *armed = false; *ready = false; }
        Ok(SourceAction::Transfer(span))
    }

    fn take_bulk_status(&mut self) -> Option<[u8; 2]> {
        if self.response == ResponseKind::Bulk {
            self.response = ResponseKind::None;
            Some(self.status)
        } else { None }
    }

    pub fn bulk_in_complete(&mut self, completion: CompletionStatus) -> CompletionAction {
        if completion != CompletionStatus::Success { return CompletionAction::None; }
        match self.take_bulk_status() {
            Some(status) => CompletionAction::SendStatus(status),
            None => CompletionAction::None,
        }
    }

    pub fn bulk_out_complete(&mut self, completion: CompletionStatus) -> Result<CompletionAction, TransportError> {
        if completion != CompletionStatus::Success { return Ok(CompletionAction::None); }
        let status = self.take_bulk_status();
        if !self.stream { return Ok(CompletionAction::ProcessReceive { status }); }
        let span = self.out_span.ok_or(TransportError::BufferTooSmall)?;
        self.out_progress = self.out_progress.checked_add(span.length).ok_or(TransportError::AddressOverflow)?;
        if self.out_progress >= self.out_total {
            self.stream = false;
            self.out_span = None;
            return Ok(CompletionAction::ReceiveComplete { start: self.out_start, length: self.out_progress, status });
        }
        let offset = span.offset.checked_add(span.length).ok_or(TransportError::AddressOverflow)?;
        let length = core::cmp::min(span.length, self.out_total - self.out_progress);
        self.out_span = Some(span.region.span(offset, length)?);
        Ok(match status { Some(status) => CompletionAction::SendStatus(status), None => CompletionAction::None })
    }

    pub fn interrupt(&mut self, io: &mut impl ControllerIo, events: &mut impl EventSink) {
        io.write32(Register::InterruptReset, 0x8181_0303);
        io.write16(Register::EventClearLow, 0xffff);
        io.write16(Register::EventClearMiddle, 0xffff);
        io.write16(Register::EventClearHigh, 0xffff);
        self.status_interrupt(events);
    }

    pub fn status_interrupt(&mut self, events: &mut impl EventSink) {
        events.emit(TransportEvent::QueueStatus);
    }

    pub fn mark_pending(&mut self, events: &mut impl EventSink) {
        self.pending = true;
        self.status_interrupt(events);
    }

    pub fn take_pending(&mut self) -> bool { core::mem::replace(&mut self.pending, false) }

    pub fn request_callback(&mut self, endpoint: Endpoint, completion: CompletionStatus, phase: &mut RequestPhase) -> Result<CompletionAction, TransportError> {
        match endpoint {
            Endpoint::BulkOut if *phase == RequestPhase::Data => {
                let action = self.bulk_out_complete(completion)?;
                *phase = RequestPhase::Idle;
                Ok(action)
            }
            Endpoint::BulkIn => Ok(self.bulk_in_complete(completion)),
            _ => Ok(CompletionAction::None),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestPhase { Idle, Data }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransmitPhase { Idle, Busy }

pub struct StatusTransmitter {
    phase: TransmitPhase,
}
impl StatusTransmitter {
    pub const fn new() -> Self { Self { phase: TransmitPhase::Idle } }
    pub fn set_phase(&mut self, phase: TransmitPhase) { self.phase = phase; }

    pub fn submit(&mut self, data: &[u8], io: &mut impl ControllerIo) -> Result<usize, TransportError> {
        if self.phase == TransmitPhase::Busy { return Err(TransportError::ChannelBusy); }
        let length = core::cmp::min(data.len(), Endpoint::BulkIn.packet_capacity(io) as usize);
        write_fifo(Endpoint::BulkIn, &data[..length], io);
        Ok(length)
    }
}

pub trait EventSink { fn emit(&mut self, event: TransportEvent); }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    InterruptReset, EventClearLow, EventClearMiddle, EventClearHigh,
    OutMaskLow, OutMaskMiddle, OutMaskHigh,
    InMaskLow, InMaskMiddle, InMaskHigh, EndpointPulse,
    OutLength, OutControl, OutStart, OutInitialize,
    Fifo(Endpoint), FifoMode, FifoCommit, BusSpeed,
    ChannelControl(Channel), ChannelInitialize(Channel), ChannelFlags(Channel), ChannelReset(Channel),
    ChannelAddress(Channel), ChannelLength(Channel),
    ChannelStatus(Channel), ResetPulse, ControllerEnable, ControllerDisconnect,
    BusState, SetupPacket, SetupPacketHigh, ReceivedLength, OutData, InStatus,
}
impl Register {
    pub const fn address(self) -> u32 {
        match self {
            Self::InterruptReset => 0xfff5512c,
            Self::EventClearLow => 0xfff54040, Self::EventClearMiddle => 0xfff54044, Self::EventClearHigh => 0xfff54048,
            Self::OutMaskLow => 0xfff54054, Self::OutMaskMiddle => 0xfff54058, Self::OutMaskHigh => 0xfff5405c,
            Self::InMaskLow => 0xfff54068, Self::InMaskMiddle => 0xfff5406c, Self::InMaskHigh => 0xfff54070,
            Self::EndpointPulse => 0xfff54080,
            Self::OutLength => 0xfff55124, Self::OutControl => 0xfff55108, Self::OutStart => 0xfff5510c,
            Self::OutInitialize => 0xfff55114,
            Self::Fifo(endpoint) => 0xfff5418c + (endpoint.fifo_index() << 3),
            Self::FifoMode => 0xfff5511c,
            Self::FifoCommit => 0xfff54088,
            Self::BusSpeed => 0xfff5409c,
            Self::ChannelControl(c) => 0xfff55804 + ((c as u32) << 7),
            Self::ChannelInitialize(c) => 0xfff5580c + ((c as u32) << 7),
            Self::ChannelFlags(c) => 0xfff55820 + ((c as u32) << 7),
            Self::ChannelReset(c) => 0xfff55844 + ((c as u32) << 7),
            Self::ChannelAddress(c) => 0xfff55824 + ((c as u32) << 7),
            Self::ChannelLength(c) => 0xfff55828 + ((c as u32) << 7),
            Self::ChannelStatus(c) => 0xfff55810 + ((c as u32) << 7),
            Self::ResetPulse => 0xfff552fc,
            Self::ControllerEnable => 0xfff54090,
            Self::ControllerDisconnect => 0xfff54008,
            Self::BusState => 0xfff54018,
            Self::SetupPacket => 0xfff55188,
            Self::SetupPacketHigh => 0xfff5518c,
            Self::ReceivedLength => 0xfff54184,
            Self::OutData => 0xfff54194,
            Self::InStatus => 0xfff541f4,
        }
    }
}

pub trait ControllerIo {
    fn read32(&mut self, register: Register) -> u32;
    fn read16(&mut self, register: Register) -> u16;
    fn write32(&mut self, register: Register, value: u32);
    fn write16(&mut self, register: Register, value: u16);
}

pub trait ChannelPlatform: ControllerIo {
    fn register_interrupt(&mut self, channel: Channel, callback: ChannelCallback) -> Result<(), TransportError>;
    fn synchronize(&mut self);
    fn delay_start(&mut self) -> Result<(), TransportError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Endpoint { Control, One, BulkOut, BulkIn }
impl Endpoint {
    const fn fifo_index(self) -> u32 { ((self as u32 >> 1) & 1) + (self as u32 & 1) }

    pub fn packet_capacity(self, io: &mut impl ControllerIo) -> u32 {
        match self {
            Self::Control => 64,
            Self::BulkIn => 32,
            Self::One | Self::BulkOut => if io.read16(Register::BusSpeed) & (1 << 14) != 0 { 512 } else { 64 },
        }
    }
}

pub fn write_fifo(endpoint: Endpoint, bytes: &[u8], io: &mut impl ControllerIo) {
    let mut chunks = bytes.chunks_exact(4);
    for chunk in &mut chunks {
        io.write32(Register::Fifo(endpoint), u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    let tail = chunks.remainder();
    if !tail.is_empty() {
        let mode = match tail.len() { 1 => 0xe0000, 2 => 0xc0000, _ => 0x80000 };
        io.write32(Register::FifoMode, mode);
        let mut word = 0u32;
        for (index, byte) in tail.iter().enumerate() { word |= (*byte as u32) << (index * 8); }
        io.write32(Register::Fifo(endpoint), word);
    }
    io.write32(Register::FifoMode, 16 + tail.len() as u32);
    io.write16(Register::FifoCommit, 1 << endpoint.fifo_index());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Channel { Zero, One }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelState { Closed, Open, Armed, Stopping }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelCallback { Receive, Transmit }

pub struct DmaChannel {
    channel: Channel,
    state: ChannelState,
    callback: Option<ChannelCallback>,
}
impl DmaChannel {
    const fn open(channel: Channel) -> Self { Self { channel, state: ChannelState::Open, callback: None } }

    pub fn arm(&mut self, callback: Option<ChannelCallback>, io: &mut impl ChannelPlatform) -> Result<(), TransportError> {
        if self.state != ChannelState::Open { return Err(TransportError::ChannelBusy); }
        self.state = ChannelState::Armed;
        self.callback = callback;
        io.write32(Register::ChannelInitialize(self.channel), 1);
        io.write32(Register::ChannelReset(self.channel), 1);
        let result = match callback {
            Some(callback) => io.register_interrupt(self.channel, callback),
            None => Ok(()),
        };
        let flags = io.read32(Register::ChannelFlags(self.channel));
        io.write32(Register::ChannelFlags(self.channel), if callback.is_some() { flags | (1 << 28) } else { flags & !(1 << 28) });
        io.synchronize();
        let control = io.read32(Register::ChannelControl(self.channel));
        io.write32(Register::ChannelControl(self.channel), control | 1);
        result
    }

    pub fn program_receive(&mut self, aperture: &PublishedAperture<'_>, span: RegionSpan, state: &mut ReceiveProgramming, io: &mut impl ChannelPlatform) -> Result<(), TransportError> {
        if self.state != ChannelState::Open { return Err(TransportError::ChannelBusy); }
        aperture.bytes(span)?;
        let rounded = span.length.checked_add(63).ok_or(TransportError::AddressOverflow)? & !63;
        span.region.span(span.offset, rounded)?;
        if span.address & 3 != 0 || rounded > 0x0fff_fffc {
            return Err(TransportError::OutsideRegion);
        }
        state.pending_events |= 0x40;
        state.dma_events |= 4;
        state.active = true;
        let flags = io.read32(Register::ChannelFlags(self.channel));
        let flags = (flags & !((3 << 12) | (7 << 20) | (1 << 28))) | (1 << 12) | (4 << 20) | (1 << 31);
        io.write32(Register::ChannelFlags(self.channel), flags);
        io.write32(Register::ChannelControl(self.channel), 1 << 16);
        io.write32(Register::ChannelAddress(self.channel), span.address & 0x3fff_fffc);
        io.write32(Register::ChannelLength(self.channel), rounded);
        self.arm(Some(ChannelCallback::Receive), io)?;
        io.write32(Register::OutInitialize, 0x101);
        io.delay_start()?;
        let low = io.read16(Register::InMaskLow);
        let middle = io.read16(Register::InMaskMiddle);
        let high = io.read16(Register::InMaskHigh);
        io.write16(Register::InMaskLow, low);
        io.write16(Register::InMaskMiddle, middle);
        io.write16(Register::InMaskHigh, high | 2);
        io.write16(Register::EndpointPulse, 16);
        io.write16(Register::EndpointPulse, 32);
        io.write32(Register::OutLength, span.length);
        let control = io.read32(Register::OutControl);
        io.write32(Register::OutControl, (control | 1) & !16);
        io.write32(Register::OutStart, 1);
        Ok(())
    }

    pub fn completion_interrupt(&mut self, io: &mut impl ControllerIo) -> Option<ChannelCallback> {
        io.write32(Register::ChannelInitialize(self.channel), 1);
        if self.state != ChannelState::Armed { return None; }
        self.callback
    }

    pub fn program_zero_receive(&mut self, state: &mut ReceiveProgramming, io: &mut impl ControllerIo) {
        io.write16(Register::OutMaskLow, 0xfff7);
        io.write16(Register::OutMaskMiddle, 0xffff);
        io.write16(Register::OutMaskHigh, 0xffff);
        state.endpoint_mask &= !8;
        write_fifo(Endpoint::BulkOut, &[], io);
    }

    pub fn begin_stop(&mut self, io: &mut impl ControllerIo) -> Result<(), TransportError> {
        if self.state == ChannelState::Stopping { return Ok(()); }
        if self.state != ChannelState::Armed { return Err(TransportError::ChannelBusy); }
        self.state = ChannelState::Stopping;
        io.write32(Register::ChannelInitialize(self.channel), 1);
        io.write32(Register::ChannelControl(self.channel), 0x10000);
        let control = io.read32(Register::ChannelControl(self.channel));
        io.write32(Register::ChannelControl(self.channel), control & !1);
        Ok(())
    }

    pub fn poll_stop(&mut self, io: &mut impl ControllerIo) -> Result<StopProgress, TransportError> {
        if self.state != ChannelState::Stopping { return Err(TransportError::ChannelBusy); }
        if io.read32(Register::ChannelStatus(self.channel)) & 7 != 0 { return Ok(StopProgress::Pending); }
        let flags = io.read32(Register::ChannelFlags(self.channel));
        io.write32(Register::ChannelFlags(self.channel), flags & !(1 << 28));
        self.callback = None;
        self.state = ChannelState::Open;
        Ok(StopProgress::Stopped)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopProgress { Pending, Stopped }

pub trait TeardownPlatform: ControllerIo {
    fn release_interrupt(&mut self, handle: cpu::DriverHandle) -> Result<(), TransportError>;
    fn release_channel_interrupt(&mut self, channel: Channel) -> Result<(), TransportError>;
    fn release_channel(&mut self, channel: Channel, handle: cpu::DriverHandle) -> Result<(), TransportError>;
}

pub struct ChannelLease {
    dma: DmaChannel,
    handle: cpu::DriverHandle,
}
impl ChannelLease {
    pub fn new(dma: DmaChannel, handle: cpu::DriverHandle) -> Self { Self { dma, handle } }
}

/// The DMA channel leases of one session. Stock opens channel zero then one at the first
/// transfer (0x00d7ca10/24 and 0x00d7ccec/d00); teardown takes each opened lease once.
pub trait SessionChannels {
    fn take(&mut self) -> [Option<ChannelLease>; 2];
    fn open(&mut self, _io: &mut impl ControllerIo) {}
}

pub struct SessionChannelSet<'a> {
    handles: cpu::HandleTable<'a>,
    sequence: cpu::HandleSequence,
    leases: [Option<ChannelLease>; 2],
}
impl<'a> SessionChannelSet<'a> {
    pub fn new(entries: &'a mut [Option<cpu::DriverHandle>], sequence: cpu::HandleSequence) -> Self {
        Self { handles: cpu::HandleTable::new(entries), sequence, leases: [None, None] }
    }
    pub const fn opened(&self) -> [bool; 2] { [self.leases[0].is_some(), self.leases[1].is_some()] }
}
impl SessionChannels for SessionChannelSet<'_> {
    fn take(&mut self) -> [Option<ChannelLease>; 2] { core::mem::replace(&mut self.leases, [None, None]) }
    fn open(&mut self, io: &mut impl ControllerIo) {
        for (index, channel) in [Channel::Zero, Channel::One].into_iter().enumerate() {
            if self.leases[index].is_some() { continue; }
            match cpu::open_channel(channel, &mut self.handles, &mut self.sequence, io) {
                Ok((dma, handle)) => self.leases[index] = Some(ChannelLease::new(dma, handle)),
                Err(_) => break,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TeardownPhase {
    Interrupt, StopZero, InterruptZero, StopOne, InterruptOne,
    CloseZero, CloseOne, AssertReset, DelayAsserted, DelayReleased, Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TeardownProgress { Pending, Complete }

pub struct ControllerTeardown {
    controller_irq: Option<cpu::DriverHandle>,
    channels: [Option<ChannelLease>; 2],
    reset_already_complete: bool,
    phase: TeardownPhase,
    first_error: Option<TransportError>,
}
impl ControllerTeardown {
    /// A channel is absent until its transfer opened it; stock 0x00d7c0f4 and 0x00d7bf24 return on a failed lookup.
    pub fn new(controller_irq: Option<cpu::DriverHandle>, channels: [Option<ChannelLease>; 2], reset_already_complete: bool) -> Self {
        Self { controller_irq, channels, reset_already_complete, phase: TeardownPhase::Interrupt, first_error: None }
    }

    fn note(&mut self, result: Result<(), TransportError>) {
        if let Err(error) = result { if self.first_error.is_none() { self.first_error = Some(error); } }
    }

    pub fn completion_interrupt(&mut self, channel: Channel, io: &mut impl ControllerIo) -> Option<ChannelCallback> {
        self.channels[channel as usize].as_mut()?.dma.completion_interrupt(io)
    }

    #[cfg(target_arch = "arm")]
    pub fn shutdown_cpu(
        &mut self,
        io: &mut impl TeardownPlatform,
        machine: &rtos_scheduler::cpu::Cpu,
    ) -> Result<(), TransportError> {
        self.shutdown(io, machine, machine.domain())
    }

    pub fn shutdown<M: scheduler::Machine>(
        &mut self,
        io: &mut impl TeardownPlatform,
        machine: &M,
        domain: scheduler::layout::Domain,
    ) -> Result<(), TransportError> {
        while self.resume(io, machine, domain)? == TeardownProgress::Pending {}
        Ok(())
    }

    pub fn resume<M: scheduler::Machine>(
        &mut self,
        io: &mut impl TeardownPlatform,
        machine: &M,
        domain: scheduler::layout::Domain,
    ) -> Result<TeardownProgress, TransportError> {
        loop {
            self.phase = match self.phase {
                TeardownPhase::Interrupt => {
                    if let Some(handle) = self.controller_irq {
                        let result = io.release_interrupt(handle);
                        self.controller_irq = None;
                        self.note(result);
                    }
                    TeardownPhase::StopZero
                }
                TeardownPhase::StopZero | TeardownPhase::StopOne => {
                    let index = if self.phase == TeardownPhase::StopZero { 0 } else { 1 };
                    let mut result = Ok(());
                    let mut pending = false;
                    match self.channels[index].as_mut() {
                        None => {}
                        Some(lease) => {
                            let dma = &mut lease.dma;
                            if dma.state == ChannelState::Armed { result = dma.begin_stop(io); }
                            if dma.state == ChannelState::Stopping {
                                match dma.poll_stop(io) {
                                    Ok(StopProgress::Pending) => pending = true,
                                    Ok(StopProgress::Stopped) => {}
                                    Err(error) => result = Err(error),
                                }
                            }
                        }
                    }
                    self.note(result);
                    if pending { return Ok(TeardownProgress::Pending); }
                    if index == 0 { TeardownPhase::InterruptZero } else { TeardownPhase::InterruptOne }
                }
                TeardownPhase::InterruptZero => {
                    let result = match self.channels[0].as_mut() {
                        Some(lease) => io.release_channel_interrupt(lease.dma.channel),
                        None => Ok(()),
                    };
                    self.note(result);
                    TeardownPhase::StopOne
                }
                TeardownPhase::InterruptOne => {
                    let result = match self.channels[1].as_mut() {
                        Some(lease) => io.release_channel_interrupt(lease.dma.channel),
                        None => Ok(()),
                    };
                    self.note(result);
                    TeardownPhase::CloseZero
                }
                TeardownPhase::CloseZero => {
                    let mut result = Ok(());
                    if let Some(lease) = self.channels[0].as_mut() {
                        result = io.release_channel(lease.dma.channel, lease.handle);
                        if result.is_ok() { lease.dma.state = ChannelState::Closed; }
                    }
                    self.note(result);
                    TeardownPhase::CloseOne
                }
                TeardownPhase::CloseOne => {
                    let mut result = Ok(());
                    if let Some(lease) = self.channels[1].as_mut() {
                        result = io.release_channel(lease.dma.channel, lease.handle);
                        if result.is_ok() { lease.dma.state = ChannelState::Closed; }
                    }
                    self.note(result);
                    if self.reset_already_complete { TeardownPhase::Complete } else { TeardownPhase::AssertReset }
                }
                TeardownPhase::AssertReset => {
                    let reset = io.read16(Register::ResetPulse);
                    io.write16(Register::ResetPulse, reset | 1);
                    TeardownPhase::DelayAsserted
                }
                TeardownPhase::DelayAsserted => {
                    let status = ServiceStatus(scheduler::delay(machine, domain, scheduler::Ticks(100)));
                    if status.0 != 0 { self.note(Err(TransportError::Scheduler(status))); }
                    let reset = io.read16(Register::ResetPulse);
                    io.write16(Register::ResetPulse, reset & !1);
                    TeardownPhase::DelayReleased
                }
                TeardownPhase::DelayReleased => {
                    let status = ServiceStatus(scheduler::delay(machine, domain, scheduler::Ticks(100)));
                    if status.0 != 0 { self.note(Err(TransportError::Scheduler(status))); }
                    io.write16(Register::ControllerEnable, 0);
                    let disconnect = io.read16(Register::ControllerDisconnect);
                    io.write16(Register::ControllerDisconnect, disconnect | 2);
                    self.reset_already_complete = true;
                    TeardownPhase::Complete
                }
                TeardownPhase::Complete => {
                    return match self.first_error {
                        Some(error) => Err(error),
                        None => Ok(TeardownProgress::Complete),
                    };
                }
            };
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferTimer { Short, Long }
impl TransferTimer {
    pub const fn address(self) -> u32 {
        match self { Self::Short => 0x0146a0b8, Self::Long => 0x0146a0bc }
    }
}

pub fn cancel_transfer_timer(handle: &mut Option<RegistryHandle>, registry: &mut impl RegistrationRegistry) -> ServiceStatus {
    let status = registry.release(handle.unwrap_or(RegistryHandle(0)));
    *handle = None;
    status
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveProgramming {
    endpoint_mask: u8,
    pending_events: u8,
    dma_events: u8,
    active: bool,
    receive_enabled: bool,
}
impl ReceiveProgramming {
    pub const fn new(endpoint_mask: u8, pending_events: u8, dma_events: u8) -> Self {
        Self { endpoint_mask, pending_events, dma_events, active: false, receive_enabled: false }
    }
    pub const fn endpoint_mask(self) -> u8 { self.endpoint_mask }
    pub const fn pending_events(self) -> u8 { self.pending_events }
    pub const fn dma_events(self) -> u8 { self.dma_events }
    pub const fn active(self) -> bool { self.active }
    pub fn begin_receive(&mut self) { self.receive_enabled = true; }
    pub fn clear_receive_state(&mut self) { self.receive_enabled = false; }
    pub const fn receive_enabled(self) -> bool { self.receive_enabled }
}

pub struct MmioController { _private: () }

impl MmioController {
    /// # Safety
    /// The caller holds exclusive access to the X-M1 USB controller MMIO aperture.
    pub unsafe fn claim() -> Self { Self { _private: () } }
}

impl ControllerIo for MmioController {
    fn read32(&mut self, register: Register) -> u32 {
        unsafe { core::ptr::read_volatile(register.address() as *const u32) }
    }
    fn read16(&mut self, register: Register) -> u16 {
        unsafe { core::ptr::read_volatile(register.address() as *const u16) }
    }
    fn write32(&mut self, register: Register, value: u32) {
        unsafe { core::ptr::write_volatile(register.address() as *mut u32, value) }
    }
    fn write16(&mut self, register: Register, value: u16) {
        unsafe { core::ptr::write_volatile(register.address() as *mut u16, value) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndpointTransfer {
    span: RegionSpan,
    sent: u32,
    endpoint: Endpoint,
}

impl EndpointTransfer {
    pub fn new(aperture: &PublishedAperture<'_>, span: RegionSpan, endpoint: Endpoint) -> Result<Self, TransportError> {
        aperture.bytes(span)?;
        if endpoint == Endpoint::BulkOut { return Err(TransportError::InvalidChannel); }
        Ok(Self { span, sent: 0, endpoint })
    }

    pub const fn remaining(&self) -> u32 { self.span.length - self.sent }

    pub fn program_next(&mut self, aperture: &PublishedAperture<'_>, io: &mut impl ControllerIo) -> Result<u32, TransportError> {
        let length = core::cmp::min(self.remaining(), self.endpoint.packet_capacity(io));
        if length == 0 { return Ok(0); }
        let span = self.span.region.span(self.span.offset + self.sent, length)?;
        write_fifo(self.endpoint, aperture.bytes(span)?, io);
        self.sent += length;
        Ok(length)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileBank { First, Second }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileIndex(u8);
impl ProfileIndex {
    pub fn new(value: u8) -> Result<Self, TransportError> {
        if value < 16 { Ok(Self(value)) } else { Err(TransportError::InvalidSelector) }
    }
    pub const fn offset(self, bank: ProfileBank) -> u16 {
        self.0 as u16 + match bank { ProfileBank::First => 0, ProfileBank::Second => 16 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileAction { Read { bank: ProfileBank, index: ProfileIndex }, Write { bank: ProfileBank, index: ProfileIndex, value: u8 }, ResetModes }

pub fn profile_selector(selector: u8, index: u8, value: u8) -> Result<ProfileAction, TransportError> {
    if selector == 5 { return Ok(ProfileAction::ResetModes); }
    let index = ProfileIndex::new(index)?;
    match selector {
        1 => Ok(ProfileAction::Read { bank: ProfileBank::First, index }),
        2 => Ok(ProfileAction::Read { bank: ProfileBank::Second, index }),
        3 => Ok(ProfileAction::Write { bank: ProfileBank::First, index, value }),
        4 => Ok(ProfileAction::Write { bank: ProfileBank::Second, index, value }),
        _ => Err(TransportError::InvalidSelector),
    }
}
