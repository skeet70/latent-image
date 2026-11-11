use super::{Adapter, AdapterError, CompletionDisposition, Descriptor, DescriptorId, DescriptorKind, DescriptorState, FirmwareCommand, FrameId, FrameLocation, FrameStatus};
use super::super::vendor_wireless_parameter_driver::TrafficClass;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevicePacketId(u8);
impl DevicePacketId {
    pub const fn from_wire(value: u8) -> Self { Self(value) }
    pub const fn wire(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceTxStatus(u8);
impl DeviceTxStatus {
    pub const fn from_wire(value: u8) -> Self { Self(value) }
    pub const fn wire(self) -> u8 { self.0 }
    pub const fn succeeded(self) -> bool { self.0 == 0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BssIndex(u8);
impl BssIndex {
    pub const PRIMARY: Self = Self(0);
    pub const fn parse(value: u8) -> Result<Self, AdapterError> {
        if value <= 2 { Ok(Self(value)) } else { Err(AdapterError::InvalidTransmitMetadata) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketPriority(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationIndex(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserPriority(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameSequence(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lifetime(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryLimit(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateSelection(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeaderLength(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorTag(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PacketKind { Zero, One, Two, Three }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataMetadata {
    pub queue: TrafficClass,
    pub bss: BssIndex,
    pub tag: DescriptorTag,
}
impl DataMetadata {
    pub const fn ethernet() -> Self {
        Self { queue: TrafficClass::One, bss: BssIndex::PRIMARY, tag: DescriptorTag(0) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagementMetadata {
    pub priority: PacketPriority,
    pub queue: TrafficClass,
    pub packet_kind: PacketKind,
    pub station: StationIndex,
    pub bss: BssIndex,
    pub user_priority: UserPriority,
    pub direct_link: bool,
    pub sequence: FrameSequence,
    pub lifetime: Lifetime,
    pub retry_limit: RetryLimit,
    pub rate: RateSelection,
    pub protected: bool,
    pub insert_timestamp: bool,
    pub burst: bool,
    pub header_length: HeaderLength,
    pub extra_header_length: HeaderLength,
    pub report_status: bool,
}
impl ManagementMetadata {
    pub const fn management() -> Self {
        Self { priority: PacketPriority(0), queue: TrafficClass::Command, packet_kind: PacketKind::Zero,
            station: StationIndex(0xff), bss: BssIndex::PRIMARY, user_priority: UserPriority(0),
            direct_link: false, sequence: FrameSequence(0), lifetime: Lifetime(0), retry_limit: RetryLimit(0),
            rate: RateSelection(0), protected: false, insert_timestamp: false, burst: false,
            header_length: HeaderLength(24), extra_header_length: HeaderLength(0), report_status: true }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransmitMetadata { Ethernet(DataMetadata), Management(ManagementMetadata), AggregatedData(ManagementMetadata) }
impl TransmitMetadata {
    pub(super) const fn class(self) -> TrafficClass {
        match self { Self::Ethernet(metadata) => metadata.queue, Self::Management(metadata) | Self::AggregatedData(metadata) => metadata.queue }
    }
    pub(super) const fn reports_status(self) -> bool {
        matches!(self, Self::Management(ManagementMetadata { report_status: true, .. }))
    }
    const fn aggregates(self) -> bool { !matches!(self, Self::Ethernet(_)) }
    fn write_header(self, length: usize, bytes: &mut [u8]) {
        bytes[..16].fill(0);
        let total = (length + 16) as u16;
        match self {
            Self::Ethernet(metadata) => {
                bytes[..2].copy_from_slice(&total.to_le_bytes());
                bytes[2] = 14;
                bytes[3] = (metadata.queue as u8) << 2;
                bytes[4] = 14;
                bytes[5] = (metadata.bss.0 << 4) | 0x40;
                bytes[10] = metadata.tag.0;
                bytes[11] = 0x20;
            }
            Self::Management(metadata) | Self::AggregatedData(metadata) => {
                bytes[..2].copy_from_slice(&(total | ((metadata.priority.0 as u16) << 12)).to_le_bytes());
                bytes[2] = ((metadata.header_length.0 as u16 + metadata.extra_header_length.0 as u16 + 16) >> 1) as u8;
                bytes[3] = ((metadata.queue as u8) << 2) | ((metadata.packet_kind as u8) << 6);
                bytes[4] = metadata.header_length.0 & 0x3f;
                bytes[5] = (metadata.user_priority.0 & 7) | ((metadata.bss.0 & 3) << 4) | (u8::from(metadata.direct_link) << 6) | 0x80;
                bytes[6..8].copy_from_slice(&metadata.sequence.0.to_le_bytes());
                bytes[8..10].copy_from_slice(&metadata.lifetime.0.to_le_bytes());
                bytes[10] = metadata.station.0;
                bytes[11] = metadata.retry_limit.0 | metadata.rate.0.wrapping_shl(2) | (u8::from(metadata.protected) << 5);
                bytes[13] = u8::from(metadata.report_status) | (u8::from(metadata.insert_timestamp) << 1) | (u8::from(metadata.burst) << 2);
                if matches!(self, Self::AggregatedData(_)) {
                    bytes[2] = 14;
                    bytes[5] &= 0x7f;
                    bytes[13] &= !1;
                }
            }
        }
    }
}

pub(super) const fn transfer_length(length: usize) -> usize {
    let aligned = (length + 3) & !3;
    if aligned >= 512 { (aligned + 511) & !511 } else { aligned }
}

impl<'a, const F: usize, const D: usize> Adapter<'a, F, D> {
    pub fn enqueue_transmit_frame(&mut self, identity: FrameId, metadata: TransmitMetadata) -> Result<DescriptorId, AdapterError> {
        let frame_index = self.frames.iter().position(|slot| slot.as_ref().is_some_and(|frame| frame.identity == identity && frame.location == FrameLocation::Queued)).ok_or(AdapterError::InvalidFrame)?;
        let index = self.descriptors.iter().position(Option::is_none).ok_or(AdapterError::DescriptorCapacity)?;
        let payload_length = self.frames[frame_index].as_ref().map_or(0, |frame| frame.bytes.len());
        if payload_length > 4079 { return Err(AdapterError::PayloadTooLong); }
        let descriptor_id = DescriptorId(self.identity()?);
        let order = self.order();
        let mut command = FirmwareCommand { identity: descriptor_id, bytes: [0; 4096],
            length: transfer_length(payload_length + 16), class: metadata.class() };
        metadata.write_header(payload_length, &mut command.bytes);
        if let Some(frame) = self.frames[frame_index].as_mut() {
            frame.location = FrameLocation::Descriptor;
        }
        self.descriptors[index] = Some(Descriptor { kind: DescriptorKind::Frame(identity), state: DescriptorState::Queued,
            order, command, report_status: metadata.reports_status(), tx_metadata: Some(metadata) });
        Ok(descriptor_id)
    }

    pub(super) fn prepare_frame_transfer(&mut self, primary: usize) {
        let Some(descriptor) = self.descriptors[primary].as_ref() else { return; };
        let DescriptorKind::Frame(frame_id) = descriptor.kind else { return; };
        let Some(metadata) = descriptor.tx_metadata else { return; };
        let aggregate = metadata.aggregates();
        let primary_id = descriptor.command.identity;
        let command_fifo = descriptor.command.class == TrafficClass::Command;
        let mut total = 0;
        if let Some(frame) = self.frames.iter().flatten().find(|frame| frame.identity == frame_id) {
            total = (frame.bytes.len() + 19) & !3;
            if let Some(descriptor) = self.descriptors[primary].as_mut() {
                descriptor.command.bytes[16..16 + frame.bytes.len()].copy_from_slice(frame.bytes);
                if matches!(metadata, TransmitMetadata::AggregatedData(_)) && self.transmit_capabilities.0 & 7 != 0 {
                    descriptor.command.bytes[3] |= 3;
                }
            }
        }
        if !aggregate { return; }
        loop {
            let next = self.descriptors.iter().enumerate().filter_map(|(index, slot)| {
                let descriptor = slot.as_ref()?;
                (descriptor.state == DescriptorState::Queued && !matches!(descriptor.kind, DescriptorKind::Request(_))).then_some((index, descriptor.order))
            }).min_by_key(|(_, order)| *order).map(|(index, _)| index);
            let Some(index) = next else { break; };
            let Some(descriptor) = self.descriptors[index].as_ref() else { break; };
            let DescriptorKind::Frame(frame_id) = descriptor.kind else { break; };
            let Some(metadata) = descriptor.tx_metadata else { break; };
            let class = descriptor.command.class;
            if !metadata.aggregates() || (class == TrafficClass::Command) != command_fifo || self.credits.available(class) == 0 { break; }
            let Some(frame_index) = self.frames.iter().position(|slot| slot.as_ref().is_some_and(|frame| frame.identity == frame_id)) else { break; };
            let payload_length = self.frames[frame_index].as_ref().map_or(0, |frame| frame.bytes.len());
            let packet_length = (payload_length + 19) & !3;
            if total + packet_length > 4096 { break; }
            let packet = if metadata.reports_status() {
                let Some(packet) = self.assign_device_packet(frame_id) else { break; };
                Some(packet)
            } else { None };
            self.credits.consume(class);
            if let (Some(frame), Some(primary)) = (self.frames[frame_index].as_ref(), self.descriptors[primary].as_mut()) {
                let output = &mut primary.command.bytes[total..total + packet_length];
                metadata.write_header(payload_length, output);
                if let Some(packet) = packet { output[12] = packet.wire(); }
                if matches!(metadata, TransmitMetadata::AggregatedData(_)) && self.transmit_capabilities.0 & 7 != 0 { output[3] |= 3; }
                output[16..16 + payload_length].copy_from_slice(frame.bytes);
            }
            if let Some(descriptor) = self.descriptors[index].as_mut() { descriptor.state = DescriptorState::Aggregated(primary_id); }
            total += packet_length;
        }
        if let Some(descriptor) = self.descriptors[primary].as_mut() { descriptor.command.length = transfer_length(total); }
    }

    pub(super) fn assign_device_packet(&mut self, frame: FrameId) -> Option<DevicePacketId> {
        for _ in 0..256 {
            self.last_device_packet = self.last_device_packet.wrapping_add(1);
            let index = self.last_device_packet as usize;
            if self.device_packets[index].is_none() && !self.retired_packets[index] {
                self.device_packets[index] = Some(frame);
                return Some(DevicePacketId(self.last_device_packet));
            }
        }
        None
    }

    pub(super) fn retire_device_packet(&mut self, identity: FrameId) {
        for (index, packet) in self.device_packets.iter_mut().enumerate() {
            if *packet == Some(identity) { *packet = None; self.retired_packets[index] = true; }
        }
    }

    pub fn complete_device_packet(&mut self, identity: DevicePacketId, status: DeviceTxStatus) -> CompletionDisposition {
        let index = identity.0 as usize;
        let Some(frame_id) = self.device_packets[index].take() else {
            self.retired_packets[index] = false;
            return CompletionDisposition::Late;
        };
        let Some(frame) = self.frames.iter_mut().flatten().find(|frame| frame.identity == frame_id) else { return CompletionDisposition::Late; };
        match frame.location {
            FrameLocation::Descriptor => { frame.device_status = Some(status); CompletionDisposition::Applied }
            FrameLocation::DevicePending => self.finish_frame(frame_id, FrameStatus::DeviceCompleted(status)),
            _ => CompletionDisposition::Late,
        }
    }

    pub(super) fn finish_transmitted_frame(&mut self, identity: FrameId, report_status: bool) {
        let early_status = self.frames.iter().flatten().find(|frame| frame.identity == identity).and_then(|frame| frame.device_status);
        if let Some(status) = early_status {
            self.finish_frame(identity, FrameStatus::DeviceCompleted(status));
        } else if report_status {
            if let Some(frame) = self.frames.iter_mut().flatten().find(|frame| frame.identity == identity) { frame.location = FrameLocation::DevicePending; }
        } else {
            self.finish_frame(identity, FrameStatus::Sent);
        }
    }
}

#[cfg(test)]
#[path = "vendor_wireless_adapter_transmit_tests.rs"]
mod tests;
