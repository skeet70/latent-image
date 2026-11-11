//! X-M1 device receive framing, aggregate FIFO transfers and pending-command correlation.

use super::super::sd_controller::{Argument, Buffer, Execution, Operation, Request};
#[path = "vendor_wireless_station_driver_receive_events.rs"]
pub mod events;
#[path = "vendor_wireless_station_driver_receive_gate.rs"]
pub mod gate;
#[path = "vendor_wireless_station_driver_receive_reorder.rs"]
pub mod reorder;
#[path = "vendor_wireless_station_driver_receive_interrupt.rs"]
pub mod interrupt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiveError { Truncated, InvalidLength, Capacity, Busy, Stale, Physical, Cancelled, DuplicateSequence, InvalidPacketType }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceivePort { Zero, One }
impl ReceivePort {
    const fn register(self) -> u32 { match self { Self::Zero => 0x30, Self::One => 0x34 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSequence(u8);
impl CommandSequence { pub const fn get(self) -> u8 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventId(u8);
impl EventId { pub const fn get(self) -> u8 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareEventKind {
    CommandResult, Signal, Statistics, MicFailure, Service, Network, Sleepy, StatisticsExtended,
    AddBlockAck, DeleteBlockAck, ScanComplete, TxDone, ChannelGrant, Event25, Event26,
    StationAbort, Event28, Event29, Event30, Roaming, PeerDisconnect, Deauthenticate,
    CommandResponse(EventId),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareEvent<'a> { wire: &'a [u8], kind: FirmwareEventKind, sequence: CommandSequence }
impl<'a> FirmwareEvent<'a> {
    pub const fn kind(&self) -> FirmwareEventKind { self.kind }
    pub const fn sequence(&self) -> CommandSequence { self.sequence }
    pub fn body(&self) -> &'a [u8] { &self.wire[8..] }
    pub const fn wire(&self) -> &'a [u8] { self.wire }
    pub fn transmission(&self) -> Result<Option<(super::super::vendor_wireless_adapter::transmit::DevicePacketId, super::super::vendor_wireless_adapter::transmit::DeviceTxStatus)>, ReceiveError> {
        use super::super::vendor_wireless_adapter::transmit::{DevicePacketId, DeviceTxStatus};
        if self.kind != FirmwareEventKind::TxDone { return Ok(None); }
        let body = self.body();
        if body.len() < 2 { return Err(ReceiveError::Truncated); }
        Ok(Some((DevicePacketId::from_wire(body[0]), DeviceTxStatus::from_wire(body[1]))))
    }
    fn parse(wire: &'a [u8]) -> Result<Self, ReceiveError> {
        if wire.len() < 8 { return Err(ReceiveError::Truncated); }
        let kind = match wire[4] {
            1 => FirmwareEventKind::CommandResult, 5 => FirmwareEventKind::Signal,
            6 => FirmwareEventKind::Statistics, 7 => FirmwareEventKind::MicFailure,
            8 => FirmwareEventKind::Service, 12 => FirmwareEventKind::Network,
            14 => FirmwareEventKind::Sleepy, 15 => FirmwareEventKind::StatisticsExtended,
            17 => FirmwareEventKind::AddBlockAck, 18 => FirmwareEventKind::DeleteBlockAck,
            21 => FirmwareEventKind::ScanComplete, 23 => FirmwareEventKind::TxDone,
            24 => FirmwareEventKind::ChannelGrant, 25 => FirmwareEventKind::Event25,
            26 => FirmwareEventKind::Event26, 27 => FirmwareEventKind::StationAbort,
            28 => FirmwareEventKind::Event28, 29 => FirmwareEventKind::Event29,
            30 => FirmwareEventKind::Event30, 32 => FirmwareEventKind::Roaming,
            33 => FirmwareEventKind::PeerDisconnect, 35 => FirmwareEventKind::Deauthenticate,
            value => FirmwareEventKind::CommandResponse(EventId(value)),
        };
        Ok(Self { wire, kind, sequence: CommandSequence(wire[5]) })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerIndex(u8);
impl PeerIndex { pub const fn get(self) -> u8 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveSignal(u8);
impl ReceiveSignal { pub const fn raw(self) -> u8 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame<'a> { wire: &'a [u8], bytes: &'a [u8], header_length: u8, peer: PeerIndex, signal: ReceiveSignal, status: u32 }
impl<'a> Frame<'a> {
    pub const fn bytes(&self) -> &'a [u8] { self.bytes }
    pub const fn wire(&self) -> &'a [u8] { self.wire }
    pub const fn header_length(&self) -> u8 { self.header_length }
    pub const fn peer(&self) -> PeerIndex { self.peer }
    pub const fn signal(&self) -> ReceiveSignal { self.signal }
    pub const fn status(&self) -> u32 { self.status }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Packet<'a> { Ethernet(Frame<'a>), Event(FirmwareEvent<'a>), Loopback(&'a [u8]), Management(Frame<'a>) }
impl<'a> Packet<'a> {
    pub fn parse(wire: &'a [u8]) -> Result<Self, ReceiveError> {
        let length = usize::from(half(wire, 0)?);
        if length < 4 || length > wire.len() { return Err(ReceiveError::InvalidLength); }
        let kind = wire[2] & 3;
        let packet = &wire[..length];
        if kind == 1 { return FirmwareEvent::parse(packet).map(Self::Event); }
        if kind == 2 { return Ok(Self::Loopback(packet)); }
        if length < 12 { return Err(ReceiveError::Truncated); }
        let offset = 12 + usize::from(packet[4] & 3);
        if offset > length { return Err(ReceiveError::InvalidLength); }
        let trailer = (length + 3) & !3;
        let status = word(wire, trailer)?;
        let frame = Frame { wire, bytes: &packet[offset..], header_length: packet[4] >> 2,
            peer: PeerIndex(packet[8]), signal: ReceiveSignal(packet[9]), status };
        Ok(if kind == 0 { Self::Ethernet(frame) } else { Self::Management(frame) })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationRoute { Applied, NotAssociation }
pub fn dispatch_association(event: &FirmwareEvent<'_>, association: &mut super::association::Association, abort: super::super::vendor_wireless_secondary::runtime::AbortAdmission) -> Result<AssociationRoute, super::association::AssociationError> {
    use super::association::AssociationError;
    match event.kind {
        FirmwareEventKind::ScanComplete => association.receive_scan_event(event.body())?,
        FirmwareEventKind::ChannelGrant => association.receive_channel_event(event.body())?,
        FirmwareEventKind::Roaming => association.receive_roaming_event(event.body())?,
        FirmwareEventKind::MicFailure => association.receive_mic_failure(event.body())?,
        FirmwareEventKind::Deauthenticate => association.reject_class3(event.body())?,
        FirmwareEventKind::StationAbort => {
            if abort == super::super::vendor_wireless_secondary::runtime::AbortAdmission::Suppressed { return Ok(AssociationRoute::Applied); }
            let index = *event.body().first().ok_or(AssociationError::TruncatedFrame)?;
            if index != 0 { return Ok(AssociationRoute::NotAssociation); }
            association.abort_link()?;
        }
        _ => return Ok(AssociationRoute::NotAssociation),
    }
    Ok(AssociationRoute::Applied)
}
pub fn dispatch_management(frame: Frame<'_>, association: &mut super::association::Association) -> Result<(), super::association::AssociationError> {
    use super::association::{AssociationError, ReceiveMetadata};
    if frame.wire[2] & 3 != 3 { return Err(AssociationError::InvalidResponse); }
    let signal = super::super::vendor_wireless_network::SignalDbm((i32::from(frame.signal.raw()) >> 1) - 110);
    let metadata = ReceiveMetadata::parse(signal, frame.wire)?;
    association.receive_management_with_metadata(frame.bytes, metadata)
}
fn half(bytes: &[u8], offset: usize) -> Result<u16, ReceiveError> {
    let bytes = bytes.get(offset..offset + 2).ok_or(ReceiveError::Truncated)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}
fn word(bytes: &[u8], offset: usize) -> Result<u32, ReceiveError> {
    let bytes = bytes.get(offset..offset + 4).ok_or(ReceiveError::Truncated)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceivePlan { port: ReceivePort, lengths: [u16; 16], offsets: [u16; 16], count: usize, transferred: usize, physical: usize }
impl ReceivePlan {
    pub fn from_interrupt_snapshot(snapshot: &[u8], port: ReceivePort, available_packets: usize) -> Result<Self, ReceiveError> {
        let (count_offset, lengths_offset) = match port { ReceivePort::Zero => (12, 16), ReceivePort::One => (14, 48) };
        let count = usize::from(half(snapshot, count_offset)?);
        if count > 16 || count > available_packets { return Err(ReceiveError::Capacity); }
        let mut plan = Self { port, lengths: [0; 16], offsets: [0; 16], count, transferred: 0, physical: 0 };
        for index in 0..count {
            let length = half(snapshot, lengths_offset + index * 2)?;
            if length == 0 { return Err(ReceiveError::InvalidLength); }
            let stride = (usize::from(length) + 7) & !3;
            plan.offsets[index] = plan.transferred as u16;
            plan.transferred += stride;
            if plan.transferred >= 0x9c30 { return Err(ReceiveError::Capacity); }
            plan.lengths[index] = length;
        }
        plan.physical = if plan.transferred >= 512 { (plan.transferred + 511) & !511 } else { plan.transferred };
        Ok(plan)
    }
    pub const fn packet_count(&self) -> usize { self.count }
    pub const fn buffer_length(&self) -> usize { self.physical }
    pub fn packet_extent(&self, index: usize) -> Option<PacketExtent> {
        if index >= self.count { return None; }
        let length = usize::from(self.lengths[index]);
        Some(PacketExtent { offset: usize::from(self.offsets[index]), length, stride: (length + 7) & !3 })
    }
    pub const fn argument(&self) -> u32 {
        let (block, count) = if self.transferred >= 512 { (0x0800_0000, self.physical / 512) } else { (0, self.physical) };
        0x1000_0000 | block | (self.port.register() << 9) | count as u32
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketExtent { offset: usize, length: usize, stride: usize }
impl PacketExtent {
    pub const fn offset(self) -> usize { self.offset }
    pub const fn length(self) -> usize { self.length }
    pub const fn stride(self) -> usize { self.stride }
    pub fn range(self) -> core::ops::Range<usize> { self.offset..self.offset + self.stride }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferState { Prepared, Issued, Completed, Failed }
pub struct ReceiveTransfer<'a> { plan: ReceivePlan, buffer: &'a mut [u8], state: TransferState }
impl<'a> ReceiveTransfer<'a> {
    pub fn new(plan: ReceivePlan, buffer: &'a mut [u8]) -> Result<Self, (ReceiveError, &'a mut [u8])> {
        if buffer.len() < plan.physical { return Err((ReceiveError::Capacity, buffer)); }
        Ok(Self { plan, buffer, state: TransferState::Prepared })
    }
    pub fn request(&mut self) -> Result<Request<'_>, ReceiveError> {
        if self.state != TransferState::Prepared { return Err(ReceiveError::Stale); }
        if self.plan.physical == 0 { return Err(ReceiveError::InvalidLength); }
        self.state = TransferState::Issued;
        Ok(Request { operation: Operation::Extended53, argument: Argument::from_wire(self.plan.argument()),
            execution: Execution::Interrupt, buffer: Buffer::Fifo(&mut self.buffer[..self.plan.physical]) })
    }
    pub fn finish(&mut self, result: Result<(), ReceiveError>) -> Result<(), ReceiveError> {
        if self.state != TransferState::Issued { return Err(ReceiveError::Stale); }
        self.state = if result.is_ok() { TransferState::Completed } else { TransferState::Failed };
        result
    }
    pub fn into_batch(self) -> Result<ReceiveBatch<'a>, (ReceiveError, &'a mut [u8])> {
        if self.state != TransferState::Completed { return Err((ReceiveError::Physical, self.buffer)); }
        Ok(ReceiveBatch { plan: self.plan, buffer: self.buffer, next: 0, offset: 0 })
    }
    pub fn cancel(self) -> &'a mut [u8] { self.buffer }
}
pub struct ReceiveBatch<'a> { plan: ReceivePlan, buffer: &'a mut [u8], next: usize, offset: usize }
impl ReceiveBatch<'_> {
    pub fn next_packet(&mut self) -> Option<Result<Packet<'_>, ReceiveError>> {
        if self.next == self.plan.count { return None; }
        let length = usize::from(self.plan.lengths[self.next]);
        let stride = (length + 7) & !3;
        let offset = self.offset;
        self.offset += stride;
        self.next += 1;
        let bytes = &self.buffer[offset..offset + stride];
        if half(bytes, 0).ok().map(usize::from) != Some(length) { return Some(Err(ReceiveError::InvalidLength)); }
        Some(Packet::parse(bytes))
    }
}
impl<'a> ReceiveBatch<'a> {
    pub fn release(self) -> &'a mut [u8] { self.buffer }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingCommandId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandResult { Accepted, Rejected, Unsupported, Indeterminate(FirmwareStatus) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareStatus(u8);
impl FirmwareStatus { pub const fn get(self) -> u8 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandCompletion<T> { id: PendingCommandId, owner: T, result: CommandResult }
impl<T: Copy> CommandCompletion<T> {
    pub const fn id(&self) -> PendingCommandId { self.id }
    pub const fn owner(&self) -> T { self.owner }
    pub const fn result(&self) -> CommandResult { self.result }
}
#[derive(Clone, Copy)]
struct Pending<T> { id: PendingCommandId, owner: T, sequence: CommandSequence }
pub struct PendingCommands<T: Copy, const N: usize> { entries: [Option<Pending<T>>; N], serial: u64 }
impl<T: Copy, const N: usize> PendingCommands<T, N> {
    pub const fn new() -> Self { Self { entries: [None; N], serial: 0 } }
    pub fn register(&mut self, owner: T, transmitted: &[u8]) -> Result<PendingCommandId, ReceiveError> {
        if transmitted.len() < 8 || usize::from(half(transmitted, 0)?) > transmitted.len() { return Err(ReceiveError::Truncated); }
        let sequence = CommandSequence(transmitted[6]);
        if self.entries.iter().flatten().any(|v| v.sequence == sequence) { return Err(ReceiveError::DuplicateSequence); }
        let slot = self.entries.iter_mut().find(|v| v.is_none()).ok_or(ReceiveError::Capacity)?;
        self.serial = self.serial.wrapping_add(1);
        let id = PendingCommandId(self.serial);
        *slot = Some(Pending { id, owner, sequence });
        Ok(id)
    }
    pub fn receive(&mut self, event: &FirmwareEvent<'_>) -> Result<Option<CommandCompletion<T>>, ReceiveError> {
        let result = match event.kind {
            FirmwareEventKind::CommandResult => match *event.body().get(1).ok_or(ReceiveError::Truncated)? {
                0 => CommandResult::Accepted, 1 => CommandResult::Rejected, 2 => CommandResult::Unsupported,
                value => CommandResult::Indeterminate(FirmwareStatus(value)),
            },
            FirmwareEventKind::CommandResponse(_) | FirmwareEventKind::Signal | FirmwareEventKind::Statistics | FirmwareEventKind::StatisticsExtended => CommandResult::Accepted,
            _ => return Ok(None),
        };
        let Some(slot) = self.entries.iter_mut().find(|v| v.as_ref().map(|v| v.sequence) == Some(event.sequence)) else { return Ok(None); };
        let pending = slot.take().unwrap();
        Ok(Some(CommandCompletion { id: pending.id, owner: pending.owner, result }))
    }
    pub fn cancel(&mut self, id: PendingCommandId) -> Option<T> {
        let slot = self.entries.iter_mut().find(|v| v.as_ref().map(|v| v.id) == Some(id))?;
        slot.take().map(|v| v.owner)
    }
    pub fn teardown(&mut self) -> [Option<T>; N] {
        core::array::from_fn(|index| self.entries[index].take().map(|pending| pending.owner))
    }
}

#[cfg(test)]
#[path = "vendor_wireless_station_driver_receive_tests.rs"]
mod tests;
