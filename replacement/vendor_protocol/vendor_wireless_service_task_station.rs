use super::TaskMessage;
use super::discovery::{NetworkDiscoveryRuntime, DiscoveryError, TransferKind, NetworkPacketLease, PacketPoolError};
use super::super::vendor_wireless_network::transport::{
    TcpSequence, TcpPort, TransportTick,
    socket_task::{SocketTaskRuntime, SocketTaskSlot, SocketTaskOperationId, SocketTaskError, SocketTaskSocketCompletion, SocketTaskSocketResult, SocketTaskHandle, SocketServiceKind},
};

#[path = "vendor_wireless_service_task_station_engine.rs"]
mod engine;
pub use engine::StationServiceRuntime;

#[cfg(test)]
#[path = "vendor_wireless_service_task_station_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotId(u8);
impl SlotId {
    pub const fn new(value: u32) -> Option<Self> {
        if value < 10 { Some(Self(value as u8)) } else { None }
    }
    pub const fn get(self) -> u32 { self.0 as u32 }
    const fn index(self) -> usize { self.0 as usize }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SocketResult { pub status: i32, pub value: u32 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SocketPhase {
    #[default]
    Inactive,
    Opening,
    AwaitingOpen,
    Connected,
    Transferring,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransferPhase { #[default] Idle, Ready, Responding }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SocketConfiguration {
    pub words: [u32; 5],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SocketIdentity {
    pub name: [u8; 32],
    pub address: [u8; 6],
    pub kind: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketPublication {
    pub slot: SlotId,
    pub count: u32,
    pub identity: SocketIdentity,
    pub enabled: bool,
    pub disconnected: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct SocketSlot {
    configured: bool,
    configuration: SocketConfiguration,
    identity: SocketIdentity,
    phase: SocketPhase,
    transfer: TransferPhase,
    handle: u32,
    primary: SocketResult,
    secondary: SocketResult,
}

pub struct SocketServiceRuntime {
    slots: [SocketSlot; 10],
    primary_packets: [Option<NetworkPacketLease>; 10],
    secondary_packets: [Option<NetworkPacketLease>; 10],
    opening: [Option<SocketTaskOperationId>; 10],
    closing: [Option<SocketTaskOperationId>; 10],
    sending: [Option<SocketTaskOperationId>; 10],
    selected: Option<SlotId>,
    station_socket: Option<(SocketPhase, SlotId)>,
}
impl Default for SocketServiceRuntime { fn default() -> Self { Self::new() } }
impl SocketServiceRuntime {
    pub const fn new() -> Self {
        const EMPTY: SocketSlot = SocketSlot {
            configured: false,
            configuration: SocketConfiguration { words: [0; 5] },
            identity: SocketIdentity { name: [0; 32], address: [0; 6], kind: 0 },
            phase: SocketPhase::Inactive,
            transfer: TransferPhase::Idle,
            handle: 0,
            primary: SocketResult { status: 0, value: 0 },
            secondary: SocketResult { status: 0, value: 0 },
        };
        Self { slots: [EMPTY; 10], primary_packets: [const { None }; 10], secondary_packets: [const { None }; 10], opening: [None; 10], closing: [None; 10], sending: [None; 10], selected: None, station_socket: None }
    }
    pub fn reset(&mut self, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), StationError> {
        for index in 0..10 {
            if let Some(packet) = self.primary_packets[index].take() { discovery.packets.release(packet).map_err(StationError::Packet)?; }
            if let Some(packet) = self.secondary_packets[index].take() { discovery.packets.release(packet).map_err(StationError::Packet)?; }
        }
        *self = Self::new();
        Ok(())
    }
    pub fn phase(&self, slot: SlotId) -> SocketPhase { self.slots[slot.index()].phase }
    pub fn transfer_phase(&self, slot: SlotId) -> TransferPhase { self.slots[slot.index()].transfer }
    pub fn configuration(&self, slot: SlotId) -> SocketConfiguration { self.slots[slot.index()].configuration }
    pub fn handle(&self, slot: SlotId) -> u32 { self.slots[slot.index()].handle }
    pub fn configure_slot(&mut self, slot: SlotId, configuration: SocketConfiguration) {
        let entry = &mut self.slots[slot.index()];
        entry.configuration = configuration;
        entry.identity.address[..4].copy_from_slice(&configuration.words[0].to_le_bytes());
        entry.identity.address[4..].copy_from_slice(&configuration.words[1].to_le_bytes()[..2]);
        entry.identity.kind = configuration.words[3];
    }
    pub fn set_identity(&mut self, slot: SlotId, identity: SocketIdentity) { self.slots[slot.index()].identity = identity; }
    pub fn set_handle(&mut self, slot: SlotId, handle: u32) { self.slots[slot.index()].handle = handle; }
    pub fn set_phase(&mut self, slot: SlotId, phase: SocketPhase) { self.slots[slot.index()].phase = phase; }
    pub fn set_transfer_phase(&mut self, slot: SlotId, phase: TransferPhase) { self.slots[slot.index()].transfer = phase; }
    pub fn set_station_socket(&mut self, phase: SocketPhase, slot: SlotId) { self.station_socket = Some((phase, slot)); }
    pub fn store_primary(&mut self, slot: SlotId, result: SocketResult) { self.slots[slot.index()].primary = result; }
    pub fn take_primary(&mut self, slot: SlotId) -> SocketResult { core::mem::take(&mut self.slots[slot.index()].primary) }
    pub fn clear_primary(&mut self, slot: SlotId) { self.slots[slot.index()].primary = SocketResult::default(); }
    pub fn store_secondary(&mut self, slot: SlotId, result: SocketResult) { self.slots[slot.index()].secondary = result; }
    pub fn take_secondary(&mut self, slot: SlotId) -> SocketResult { core::mem::take(&mut self.slots[slot.index()].secondary) }
    pub fn retain_primary_packet(&mut self, slot: SlotId, packet: NetworkPacketLease) -> Option<NetworkPacketLease> { self.primary_packets[slot.index()].replace(packet) }
    pub fn take_primary_packet(&mut self, slot: SlotId) -> Option<NetworkPacketLease> { self.primary_packets[slot.index()].take() }
    pub fn retain_secondary_packet(&mut self, slot: SlotId, packet: NetworkPacketLease) -> Option<NetworkPacketLease> { self.secondary_packets[slot.index()].replace(packet) }
    pub fn take_secondary_packet(&mut self, slot: SlotId) -> Option<NetworkPacketLease> { self.secondary_packets[slot.index()].take() }
    pub fn clear_pending(&mut self, handle: u32) {
        if let Some(slot) = self.find_socket(handle) { self.clear_primary(slot); }
    }
    pub fn find_socket(&self, handle: u32) -> Option<SlotId> {
        if let Some((phase, slot)) = self.station_socket {
            return if matches!(phase, SocketPhase::Connected | SocketPhase::Transferring) { Some(slot) } else { None };
        }
        self.slots.iter().position(|slot| matches!(slot.phase, SocketPhase::Connected | SocketPhase::Transferring) && slot.handle == handle).map(|index| SlotId(index as u8))
    }
    pub fn select_socket(&mut self, slot: SlotId, enabled: bool, count: u32) -> Option<SocketPublication> {
        if self.selected.is_some_and(|selected| selected != slot) { return None; }
        self.selected = Some(slot);
        let mut identity = self.slots[slot.index()].identity;
        if identity.kind > 3 { identity.kind = 0; }
        Some(SocketPublication { slot, count, identity, enabled, disconnected: count == 0 })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StationError { InvalidSlot, InvalidPayload, QueueFull, Socket(SocketTaskError), Discovery(DiscoveryError), Packet(PacketPoolError) }

impl SocketServiceRuntime {
    fn transport_slot(slot: SlotId) -> SocketTaskSlot {
        SocketTaskSlot::parse(slot.0).expect("SlotId is within socket task capacity")
    }
    pub fn close(&mut self, slot: SlotId, socket: &mut SocketTaskRuntime) -> Result<SocketTaskOperationId, StationError> {
        let operation = socket.close(Self::transport_slot(slot)).map_err(StationError::Socket)?;
        self.closing[slot.index()] = Some(operation);
        Ok(operation)
    }
    pub fn send(&mut self, slot: SlotId, bytes: &[u8], socket: &mut SocketTaskRuntime) -> Result<SocketTaskOperationId, StationError> {
        let operation = socket.send(Self::transport_slot(slot), bytes).map_err(StationError::Socket)?;
        self.sending[slot.index()] = Some(operation);
        Ok(operation)
    }
    pub fn open_service(&mut self, slot: SlotId, kind: ServiceSocketKind, port: TcpPort, socket: &mut SocketTaskRuntime) -> Result<SocketTaskOperationId, StationError> {
        self.open_kind(slot, kind.source_kind(), port, socket)
    }
    pub fn open_default(&mut self, slot: SlotId, socket: &mut SocketTaskRuntime) -> Result<SocketTaskOperationId, StationError> {
        self.open_kind(slot, SocketServiceKind::Listener, TcpPort::parse(55740).expect("The service port is nonzero"), socket)
    }
    fn open_kind(&mut self, slot: SlotId, kind: SocketServiceKind, port: TcpPort, socket: &mut SocketTaskRuntime) -> Result<SocketTaskOperationId, StationError> {
        let operation = socket.open_service(Self::transport_slot(slot), kind, port).map_err(StationError::Socket)?;
        self.opening[slot.index()] = Some(operation);
        Ok(operation)
    }
    pub fn completion(&mut self, completion: SocketTaskSocketCompletion) -> Option<(SlotId, ServiceSocketCompletion)> {
        let slot = SlotId::new(u32::from(completion.slot.get()))?;
        let index = slot.index();
        if self.opening[index] == Some(completion.operation) {
            let status = match completion.status {
                Ok(SocketTaskSocketResult::Opened(handle)) => Ok(handle),
                Err(error) => Err(error),
                _ => return None,
            };
            self.opening[index] = None;
            return Some((slot, ServiceSocketCompletion::Opened(status)));
        }
        if self.closing[index] == Some(completion.operation) {
            let status = match completion.status { Ok(SocketTaskSocketResult::Closed) => Ok(()), Err(error) => Err(error), _ => return None };
            self.closing[index] = None;
            return Some((slot, ServiceSocketCompletion::Closed(status)));
        }
        if self.sending[index] == Some(completion.operation) {
            let status = match completion.status { Ok(SocketTaskSocketResult::Sent(_)) => Ok(()), Err(error) => Err(error), _ => return None };
            self.sending[index] = None;
            return Some((slot, ServiceSocketCompletion::Sent(status)));
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceSocketCompletion {
    Opened(Result<SocketTaskHandle, SocketTaskError>),
    Closed(Result<(), SocketTaskError>),
    Sent(Result<(), SocketTaskError>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceSocketKind { Stream, Control, Datagram }
impl ServiceSocketKind {
    pub const fn parse(value: u32) -> Option<Self> {
        match value { 1 => Some(Self::Stream), 3 => Some(Self::Control), 5 => Some(Self::Datagram), _ => None }
    }
    fn source_kind(self) -> SocketServiceKind {
        match self { Self::Stream => SocketServiceKind::Client, Self::Control => SocketServiceKind::AcceptingListener, Self::Datagram => SocketServiceKind::BroadcastDatagram }
    }
}
