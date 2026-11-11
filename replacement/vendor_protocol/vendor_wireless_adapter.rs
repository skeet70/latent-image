//! X-M1 adapter ownership and descriptor teardown CPU.
//! Evidence and source qualification: vendor_wireless_adapter.evidence.json.

use super::vendor_wireless_network::MacAddress;
use super::vendor_wireless_parameter_driver::{TrafficClass, TransmitCredits, TransmitError};
use super::vendor_wireless_radio_program::CommandSequence;

#[path = "vendor_wireless_adapter_protocols.rs"]
pub mod protocols;
#[path = "vendor_wireless_adapter_regulatory.rs"]
pub mod regulatory;
#[path = "vendor_wireless_adapter_transmit.rs"]
pub mod transmit;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameOwner { Network, Supplicant }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameStatus { Sent, PendingCancelled, DescriptorCancelled, QueuedCancelled, Failed, DeviceCompleted(transmit::DeviceTxStatus) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionDisposition { Applied, Late }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalWrite { Written, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterError { FrameCapacity, DescriptorCapacity, IdentityExhausted, InvalidFrame, InvalidQueue, InvalidHardwareRevision, InvalidTransmitMetadata, PayloadTooLong }

pub struct FrameCompletion<'a> {
    pub identity: FrameId,
    pub owner: FrameOwner,
    pub status: FrameStatus,
    pub bytes: &'a mut [u8],
}

pub enum AdapterEvent<'a> {
    Frame(FrameCompletion<'a>),
    Request { identity: RequestId, cancelled: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingQueue(u8);
impl PendingQueue {
    pub const UNCLASSIFIED: Self = Self(100);
    pub const fn station(station: u8, class: u8) -> Result<Self, AdapterError> {
        if station < 20 && class < 5 { Ok(Self(station * 5 + class)) } else { Err(AdapterError::InvalidQueue) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameLocation { Queued, Pending(PendingQueue), Descriptor, DevicePending, Finished(FrameStatus) }
struct Frame<'a> {
    identity: FrameId,
    owner: FrameOwner,
    bytes: &'a mut [u8],
    location: FrameLocation,
    order: u64,
    device_status: Option<transmit::DeviceTxStatus>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DescriptorKind { Configuration, Address(MacAddress), Request(RequestId), Frame(FrameId) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DescriptorState { Queued, Transferring, Aggregated(DescriptorId), Finished { cancelled: bool } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareRequest<'a> {
    InterfaceParameters(&'a [u8; 80]),
    PeerParameters(&'a [u8; 40]),
    FlushInterface(&'a [u8; 4]),
    BeaconTemplate(&'a [u8]),
    BssParameters(&'a [u8; 16]),
}

pub struct FirmwareCommand {
    identity: DescriptorId,
    bytes: [u8; 4096],
    length: usize,
    class: TrafficClass,
}
impl FirmwareCommand {
    pub const fn identity(&self) -> DescriptorId { self.identity }
    pub fn argument(&self) -> u32 {
        let fifo = if self.class == TrafficClass::Command { 0x2c } else { 0x28 };
        if self.length >= 512 { 0x9800_0000 | (fifo << 9) | (self.length as u32 / 512) }
        else { 0x9000_0000 | (fifo << 9) | self.length as u32 }
    }
    pub fn bytes(&self) -> &[u8] { &self.bytes[..self.length] }
    pub fn bytes_mut(&mut self) -> &mut [u8] { &mut self.bytes[..self.length] }
}
struct Descriptor {
    kind: DescriptorKind,
    state: DescriptorState,
    order: u64,
    command: FirmwareCommand,
    report_status: bool,
    tx_metadata: Option<transmit::TransmitMetadata>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HardwareRevision { pub major: u16, pub minor: u16 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CountryCode(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateGroup { pub index: u8, pub band: RateBand, pub step: u8, pub first: u8, pub count: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateBand { Unused, TwoGhz, FiveGhz, Unsupported(u8) }
impl RateBand {
    pub const fn from_wire(value: u8) -> Self {
        match value { 0 => Self::Unused, 1 => Self::TwoGhz, 2 => Self::FiveGhz, other => Self::Unsupported(other) }
    }
    pub const fn wire(self) -> u8 {
        match self { Self::Unused => 0, Self::TwoGhz => 1, Self::FiveGhz => 2, Self::Unsupported(value) => value }
    }
}

pub struct HardwareConfiguration {
    pub first_revision: HardwareRevision,
    pub second_revision: HardwareRevision,
    pub use_calibration: bool,
    pub calibration: [u8; 40],
    pub feature_requested: bool,
    pub feature_supported: bool,
    pub feature_disabled: bool,
    pub country: CountryCode,
    pub radio_parameters: [u8; 144],
    pub regulatory: regulatory::RegulatoryInput,
    pub antenna: Option<[u8; 3]>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioCapabilities(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressConfiguration {
    pub address: Option<MacAddress>,
    pub preserve_invalid: bool,
    pub timestamp: u32,
    pub capabilities: RadioCapabilities,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationDisposition { Unchanged, Pending }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterPublication {
    pub address: MacAddress,
    pub country: CountryCode,
    pub feature_enabled: bool,
    pub outstanding_network: usize,
    pub outstanding_supplicant: usize,
}

pub struct Adapter<'a, const F: usize, const D: usize> {
    frames: [Option<Frame<'a>>; F],
    descriptors: [Option<Descriptor>; D],
    next_identity: u64,
    next_order: u64,
    sequence: CommandSequence,
    credits: TransmitCredits,
    publication: AdapterPublication,
    protocols: protocols::Protocols,
    regulatory: regulatory::RegulatorySelector,
    last_physical: Option<PhysicalWrite>,
    device_packets: [Option<FrameId>; 256],
    retired_packets: [bool; 256],
    last_device_packet: u8,
    transmit_capabilities: RadioCapabilities,
}

impl<'a, const F: usize, const D: usize> Default for Adapter<'a, F, D> {
    fn default() -> Self { Self::new() }
}
impl<'a, const F: usize, const D: usize> Adapter<'a, F, D> {
    pub const fn new() -> Self {
        Self {
            frames: [const { None }; F], descriptors: [const { None }; D],
            next_identity: 0, next_order: 0,
            sequence: CommandSequence::from_last_issued(0), credits: TransmitCredits::new(),
            publication: AdapterPublication { address: MacAddress([0; 6]), country: CountryCode(0),
                feature_enabled: false, outstanding_network: 0, outstanding_supplicant: 0 },
            protocols: protocols::Protocols::new(), last_physical: None,
            regulatory: regulatory::RegulatorySelector::new(),
            device_packets: [None; 256], retired_packets: [false; 256], last_device_packet: 0,
            transmit_capabilities: RadioCapabilities(0),
        }
    }
    fn identity(&mut self) -> Result<u64, AdapterError> {
        self.next_identity = self.next_identity.checked_add(1).ok_or(AdapterError::IdentityExhausted)?;
        Ok(self.next_identity)
    }
    fn order(&mut self) -> u64 {
        self.next_order += 1;
        self.next_order
    }
    pub const fn publication(&self) -> AdapterPublication { self.publication }
    pub fn protocols(&mut self) -> &mut protocols::Protocols { &mut self.protocols }
    pub const fn last_physical(&self) -> Option<PhysicalWrite> { self.last_physical }

    pub fn enqueue_frame(&mut self, owner: FrameOwner, bytes: &'a mut [u8]) -> Result<FrameId, (AdapterError, &'a mut [u8])> {
        let Some(index) = self.frames.iter().position(Option::is_none) else { return Err((AdapterError::FrameCapacity, bytes)); };
        let identity = match self.identity() { Ok(id) => FrameId(id), Err(error) => return Err((error, bytes)) };
        let order = self.order();
        self.frames[index] = Some(Frame { identity, owner, bytes, location: FrameLocation::Queued, order, device_status: None });
        match owner {
            FrameOwner::Network => self.publication.outstanding_network += 1,
            FrameOwner::Supplicant => self.publication.outstanding_supplicant += 1,
        }
        Ok(identity)
    }

    pub fn mark_pending(&mut self, identity: FrameId) -> Result<(), AdapterError> {
        self.mark_pending_in(identity, PendingQueue::UNCLASSIFIED)
    }
    pub fn mark_pending_in(&mut self, identity: FrameId, queue: PendingQueue) -> Result<(), AdapterError> {
        let order = self.order();
        let frame = self.frames.iter_mut().flatten().find(|frame| frame.identity == identity).ok_or(AdapterError::InvalidFrame)?;
        if frame.location != FrameLocation::Queued { return Err(AdapterError::InvalidFrame); }
        frame.location = FrameLocation::Pending(queue);
        frame.order = order;
        Ok(())
    }

    pub fn enqueue_frame_descriptor(&mut self, identity: FrameId) -> Result<DescriptorId, AdapterError> {
        self.enqueue_transmit_frame(identity, transmit::TransmitMetadata::Ethernet(transmit::DataMetadata::ethernet()))
    }

    pub fn enqueue_request(&mut self, identity: RequestId) -> Result<DescriptorId, AdapterError> {
        self.enqueue(DescriptorKind::Request(identity), 0, &[])
    }

    fn finish_frame(&mut self, identity: FrameId, status: FrameStatus) -> CompletionDisposition {
        let Some(index) = self.frames.iter().position(|frame| frame.as_ref().is_some_and(|frame| frame.identity == identity && !matches!(frame.location, FrameLocation::Finished(_)))) else { return CompletionDisposition::Late; };
        let order = self.order();
        self.retire_device_packet(identity);
        if let Some(frame) = self.frames[index].as_mut() {
            frame.location = FrameLocation::Finished(status);
            frame.order = order;
            match frame.owner {
                FrameOwner::Network => self.publication.outstanding_network -= 1,
                FrameOwner::Supplicant => self.publication.outstanding_supplicant -= 1,
            }
        }
        CompletionDisposition::Applied
    }
    pub fn complete_frame(&mut self, identity: FrameId, status: FrameStatus) -> CompletionDisposition {
        for slot in &mut self.descriptors {
            if slot.as_ref().is_some_and(|descriptor| descriptor.kind == DescriptorKind::Frame(identity)) { *slot = None; }
        }
        self.finish_frame(identity, status)
    }
    fn cancel_frames(&mut self, location: FrameLocation, status: FrameStatus) {
        loop {
            let next = self.frames.iter().flatten().filter(|frame| frame.location == location).min_by_key(|frame| frame.order).map(|frame| frame.identity);
            let Some(identity) = next else { break; };
            self.finish_frame(identity, status);
        }
    }
    pub fn cancel_pending_frames(&mut self) {
        for index in 0..=100 { self.cancel_frames(FrameLocation::Pending(PendingQueue(index)), FrameStatus::PendingCancelled); }
        self.cancel_frames(FrameLocation::DevicePending, FrameStatus::PendingCancelled);
    }
    pub fn cancel_queued_frames(&mut self) { self.cancel_frames(FrameLocation::Queued, FrameStatus::QueuedCancelled); }

    pub fn cancel_commands(&mut self) {
        self.cancel_descriptors(false);
    }
    pub fn cancel_received_descriptors(&mut self) {
        self.cancel_descriptors(true);
    }
    pub fn complete_descriptor(&mut self, identity: DescriptorId) -> CompletionDisposition {
        let Some(index) = self.descriptors.iter().position(|slot| slot.as_ref().is_some_and(|descriptor| descriptor.command.identity == identity && matches!(descriptor.kind, DescriptorKind::Request(_)) && !matches!(descriptor.state, DescriptorState::Finished { .. }))) else { return CompletionDisposition::Late; };
        self.finish_descriptor(index, false);
        CompletionDisposition::Applied
    }
    pub fn cancel(&mut self) {
        self.cancel_pending_frames();
        self.cancel_commands();
        self.cancel_received_descriptors();
        self.cancel_queued_frames();
        for index in 0..D {
            if self.descriptors[index].as_ref().is_some_and(|descriptor| matches!(descriptor.kind, DescriptorKind::Configuration | DescriptorKind::Address(_))) {
                self.finish_descriptor(index, true);
            }
        }
        self.stop_protocols();
    }
    fn cancel_descriptors(&mut self, frames: bool) {
        loop {
            let next = self.descriptors.iter().enumerate().filter_map(|(index, slot)| {
                let descriptor = slot.as_ref()?;
                let selected = if frames { matches!(descriptor.kind, DescriptorKind::Frame(_)) } else { matches!(descriptor.kind, DescriptorKind::Request(_)) };
                (selected && !matches!(descriptor.state, DescriptorState::Finished { .. })).then_some((index, descriptor.order))
            }).min_by_key(|(_, order)| *order).map(|(index, _)| index);
            let Some(index) = next else { break; };
            self.finish_descriptor(index, true);
        }
    }
    fn finish_descriptor(&mut self, index: usize, cancelled: bool) {
        let Some(kind) = self.descriptors[index].as_ref().map(|descriptor| descriptor.kind) else { return; };
        match kind {
            DescriptorKind::Frame(identity) => {
                if cancelled { self.finish_frame(identity, FrameStatus::DescriptorCancelled); }
                else {
                    let reports = self.descriptors[index].as_ref().is_some_and(|descriptor| descriptor.report_status);
                    self.finish_transmitted_frame(identity, reports);
                }
                self.descriptors[index] = None;
            }
            DescriptorKind::Request(_) => {
                let order = self.order();
                if let Some(descriptor) = self.descriptors[index].as_mut() {
                    descriptor.state = DescriptorState::Finished { cancelled };
                    descriptor.order = order;
                }
            }
            DescriptorKind::Address(address) => {
                if !cancelled { self.publication.address = address; }
                self.descriptors[index] = None;
            }
            DescriptorKind::Configuration => self.descriptors[index] = None,
        }
    }

    pub fn has_pending_events(&self) -> bool {
        self.frames.iter().flatten().any(|frame| matches!(frame.location, FrameLocation::Finished(_)))
            || self.descriptors.iter().flatten().any(|descriptor| matches!(descriptor.kind, DescriptorKind::Request(_)) && matches!(descriptor.state, DescriptorState::Finished { .. }))
    }

    pub fn pop_event(&mut self) -> Option<AdapterEvent<'a>> {
        let frame = self.frames.iter().enumerate().filter_map(|(index, slot)| {
            let frame = slot.as_ref()?;
            matches!(frame.location, FrameLocation::Finished(_)).then_some((index, frame.order))
        }).min_by_key(|(_, order)| *order);
        let descriptor = self.descriptors.iter().enumerate().filter_map(|(index, slot)| {
            let descriptor = slot.as_ref()?;
            matches!(descriptor.state, DescriptorState::Finished { .. }).then_some((index, descriptor.order))
        }).min_by_key(|(_, order)| *order);
        match (frame, descriptor) {
            (Some((index, order)), other) if other.is_none_or(|(_, other_order)| order < other_order) => {
                let frame = self.frames[index].take()?;
                let FrameLocation::Finished(status) = frame.location else { return None; };
                Some(AdapterEvent::Frame(FrameCompletion { identity: frame.identity, owner: frame.owner, status, bytes: frame.bytes }))
            }
            (_, Some((index, _))) => {
                let descriptor = self.descriptors[index].take()?;
                let DescriptorKind::Request(identity) = descriptor.kind else { return None; };
                let DescriptorState::Finished { cancelled } = descriptor.state else { return None; };
                Some(AdapterEvent::Request { identity, cancelled })
            }
            _ => None,
        }
    }

    pub fn enqueue_firmware_request(&mut self, request: FirmwareRequest<'_>) -> Result<DescriptorId, AdapterError> {
        let (opcode, payload): (u8, &[u8]) = match request {
            FirmwareRequest::InterfaceParameters(bytes) => (0x16, bytes),
            FirmwareRequest::PeerParameters(bytes) => (0x17, bytes),
            FirmwareRequest::FlushInterface(bytes) => (0x1b, bytes),
            FirmwareRequest::BeaconTemplate(bytes) => {
                if bytes.len() < 6 || bytes.len() > 608 { return Err(AdapterError::PayloadTooLong); }
                (0x1c, bytes)
            }
            FirmwareRequest::BssParameters(bytes) => (0x1d, bytes),
        };
        self.enqueue(DescriptorKind::Configuration, opcode, payload)
    }
    fn enqueue(&mut self, kind: DescriptorKind, opcode: u8, payload: &[u8]) -> Result<DescriptorId, AdapterError> {
        let index = self.descriptors.iter().position(Option::is_none).ok_or(AdapterError::DescriptorCapacity)?;
        let identity = DescriptorId(self.identity()?);
        let order = self.order();
        let length = transmit::transfer_length(payload.len() + 8);
        let mut bytes = [0; 4096];
        bytes[..2].copy_from_slice(&(((payload.len() + 11) & !3) as u16).to_le_bytes());
        bytes[3] = 0x50;
        bytes[4] = opcode;
        bytes[5] = 1;
        bytes[8..8 + payload.len()].copy_from_slice(payload);
        self.descriptors[index] = Some(Descriptor { kind, state: DescriptorState::Queued, order,
            command: FirmwareCommand { identity, bytes, length, class: TrafficClass::Command }, report_status: false, tx_metadata: None });
        Ok(identity)
    }
    pub fn has_pending_commands(&self) -> bool {
        self.descriptors.iter().flatten().any(|descriptor| !matches!(descriptor.kind, DescriptorKind::Request(_)))
    }
    pub fn command(&mut self) -> Option<&mut FirmwareCommand> {
        if let Some(index) = self.descriptors.iter().position(|slot| slot.as_ref().is_some_and(|descriptor| descriptor.state == DescriptorState::Transferring)) {
            return self.descriptors[index].as_mut().map(|descriptor| &mut descriptor.command);
        }
        let index = self.descriptors.iter().enumerate().filter_map(|(index, slot)| {
            let descriptor = slot.as_ref()?;
            (descriptor.state == DescriptorState::Queued && !matches!(descriptor.kind, DescriptorKind::Request(_))).then_some((index, descriptor.order))
        }).min_by_key(|(_, order)| *order).map(|(index, _)| index)?;
        let descriptor = self.descriptors[index].as_ref()?;
        let class = descriptor.command.class;
        if self.credits.available(class) == 0 { return None; }
        let frame = if descriptor.report_status {
            match descriptor.kind { DescriptorKind::Frame(identity) => Some(identity), _ => None }
        } else { None };
        let packet = match frame { Some(identity) => Some(self.assign_device_packet(identity)?), None => None };
        self.credits.consume(class);
        let is_command = !matches!(self.descriptors[index].as_ref()?.kind, DescriptorKind::Frame(_));
        let sequence = if is_command { Some(self.sequence.issue_next().wire()) } else { None };
        let descriptor = self.descriptors[index].as_mut()?;
        descriptor.state = DescriptorState::Transferring;
        if let Some(sequence) = sequence { descriptor.command.bytes[6] = sequence; }
        if let Some(packet) = packet { descriptor.command.bytes[12] = packet.wire(); }
        self.prepare_frame_transfer(index);
        let descriptor = self.descriptors[index].as_mut()?;
        Some(&mut descriptor.command)
    }
    pub fn finish_command(&mut self, identity: DescriptorId, outcome: PhysicalWrite) -> CompletionDisposition {
        let Some(index) = self.descriptors.iter().position(|slot| slot.as_ref().is_some_and(|descriptor| descriptor.command.identity == identity && descriptor.state == DescriptorState::Transferring)) else { return CompletionDisposition::Late; };
        self.last_physical = Some(outcome);
        // 0x012a0fb4 ignores the physical write result.
        self.finish_descriptor(index, false);
        loop {
            let next = self.descriptors.iter().enumerate().filter_map(|(index, slot)| {
                let descriptor = slot.as_ref()?;
                (descriptor.state == DescriptorState::Aggregated(identity)).then_some((index, descriptor.order))
            }).min_by_key(|(_, order)| *order).map(|(index, _)| index);
            let Some(index) = next else { break; };
            self.finish_descriptor(index, false);
        }
        CompletionDisposition::Applied
    }
    pub fn observe_credit_returns(&mut self, bytes: [u8; 8]) -> Result<bool, TransmitError> { self.credits.observe_returns(bytes) }
    pub fn reset_credits(&mut self) { self.credits.reset(); }
    pub fn set_transmit_capabilities(&mut self, capabilities: RadioCapabilities) { self.transmit_capabilities = capabilities; }
    pub const fn last_sequence(&self) -> u8 { self.sequence.wire() }
    pub fn set_last_sequence(&mut self, sequence: u8) { self.sequence = CommandSequence::from_last_issued(sequence); }
    pub fn take_credits(&mut self) -> TransmitCredits { core::mem::take(&mut self.credits) }
    pub fn set_credits(&mut self, credits: TransmitCredits) { self.credits = credits; }

    pub fn configure_address(&mut self, configuration: AddressConfiguration) -> Result<ConfigurationDisposition, AdapterError> {
        self.transmit_capabilities = configuration.capabilities;
        let valid = configuration.address.filter(|address| address.0 != [0; 6] && address.0[0] & 1 == 0);
        if valid.is_none() && configuration.preserve_invalid { return Ok(ConfigurationDisposition::Unchanged); }
        let address = valid.unwrap_or_else(|| {
            let time = configuration.timestamp.to_le_bytes();
            MacAddress([0, 8, 0x22, time[0], time[1], time[2]])
        });
        let mut payload = [0; 12];
        payload[..6].copy_from_slice(&address.0);
        let flags = configuration.capabilities.0;
        payload[8] = ((flags & 8) >> 1) | ((flags & 16) >> 3) | u8::from(flags & 0x60 != 0);
        payload[10] = ((flags & 1) << 2) | (flags & 2) | ((flags & 4) >> 2);
        self.enqueue(DescriptorKind::Address(address), 0xc1, &payload)?;
        Ok(ConfigurationDisposition::Pending)
    }

    pub fn configure(&mut self, configuration: &HardwareConfiguration) -> Result<(), AdapterError> {
        if configuration.first_revision.minor >= 0x108 || configuration.second_revision.minor >= 0x108 { return Err(AdapterError::InvalidHardwareRevision); }
        let calibration = configuration.first_revision.major != 1 && configuration.use_calibration;
        let count = 2 + usize::from(calibration) + usize::from(configuration.antenna.is_some());
        if self.descriptors.iter().filter(|slot| slot.is_none()).count() < count { return Err(AdapterError::DescriptorCapacity); }
        if calibration { self.enqueue(DescriptorKind::Configuration, 0x28, &configuration.calibration)?; }
        self.publication.feature_enabled = configuration.feature_requested && configuration.feature_supported && !configuration.feature_disabled;
        self.enqueue(DescriptorKind::Configuration, 0x31, &configuration.radio_parameters)?;
        self.publication.country = configuration.country;
        let selected = self.regulatory.select(configuration.country, &configuration.regulatory);
        let mut rates = [0; 56];
        rates[..2].copy_from_slice(&configuration.country.0.to_le_bytes());
        for (index, group) in selected.groups.iter().enumerate() {
            let start = 4 + index * 8;
            rates[start] = group.index;
            rates[start + 1] = group.band.wire();
            if matches!(group.band, RateBand::TwoGhz | RateBand::FiveGhz) {
                rates[start + 2] = group.step;
                rates[start + 3] = group.first;
                rates[start + 4] = group.count;
            }
        }
        rates[52..54].copy_from_slice(&selected.flags);
        self.enqueue(DescriptorKind::Configuration, 0x13, &rates)?;
        if let Some(antenna) = configuration.antenna {
            self.enqueue(DescriptorKind::Configuration, 0x36, &[antenna[0], antenna[1], antenna[2], 0])?;
        }
        Ok(())
    }
    pub fn invalidate_regulatory_selection(&mut self) { self.regulatory.invalidate(); }
    pub fn stop_protocols(&mut self) { self.protocols.stop(); }
    pub fn reset_protocols(&mut self) { self.protocols.reset(self.publication.address); }
}

#[cfg(test)]
#[path = "vendor_wireless_adapter_tests.rs"]
mod tests;
