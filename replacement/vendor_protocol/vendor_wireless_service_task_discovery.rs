#[path = "vendor_wireless_service_task_discovery_packet.rs"]
pub mod packet;
#[path = "vendor_wireless_service_task_discovery_pool.rs"]
pub mod pool;
#[path = "vendor_wireless_service_task_discovery_executor.rs"]
pub mod executor;
#[path = "vendor_wireless_service_task_discovery_executor_session.rs"]
pub mod executor_session;
#[path = "vendor_wireless_service_task_discovery_executor_objects.rs"]
pub mod executor_objects;
#[path = "vendor_wireless_service_task_discovery_executor_properties.rs"]
pub mod executor_properties;
#[path = "vendor_wireless_service_task_discovery_timer.rs"]
pub mod timer;

pub use packet::{CameraIdentity, PeerHandshake, PeerMac, PeerName, PeerRestriction, TransferClassification, TransferKind};
pub use pool::{NetworkPacketLease, NetworkPacketPool, PacketPoolError, PacketStorage};
pub use timer::{LogicalSocketHandle, SendTimer};
use packet::{ApplicationPacket, CommandPacket, PacketError};
use executor::{PtpError, PtpOutput, PtpPhase, PtpState};
use executor_session::RemoteSessionOperations;
use executor_objects::ConcreteRemoteObjectOperations;
use executor_properties::RemotePropertyOperations;
use super::interface_modes::{RemoteControl, catalog::CatalogRuntime};
use super::station::SlotId;
use super::super::vendor_wireless_network::transport::socket_task::{SocketTaskError, SocketTaskOperationId, SocketTaskRuntime, SocketTaskSlot, SocketTaskSocketCompletion, SocketTaskSocketResult};
use super::super::vendor_wireless_network::transport::TransportTick;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryPhase { Initial, Ready, Active }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryError {
    Packet(PacketError),
    Pool(PacketPoolError),
    Protocol(PtpError),
    Transport(SocketTaskError),
    Payload(super::PayloadError),
    QueueFull,
    NotConfigured,
    Busy,
    Slot,
    UnexpectedCompletion,
    TransferFailed,
}
impl From<PacketError> for DiscoveryError { fn from(value: PacketError) -> Self { Self::Packet(value) } }
impl From<PacketPoolError> for DiscoveryError { fn from(value: PacketPoolError) -> Self { Self::Pool(value) } }
impl From<PtpError> for DiscoveryError { fn from(value: PtpError) -> Self { Self::Protocol(value) } }
impl From<SocketTaskError> for DiscoveryError { fn from(value: SocketTaskError) -> Self { Self::Transport(value) } }

struct PendingPacket { slot: SlotId, lease: NetworkPacketLease }
struct PendingSend { slot: SlotId, packet: pool::PacketIdentity, operation: Option<SocketTaskOperationId> }

pub struct NetworkDiscoveryRuntime<'a> {
    pub packets: NetworkPacketPool<'a>,
    pub catalog: CatalogRuntime<'a>,
    pub interface_control: RemoteControl,
    pub state: PtpState,
    pub output: PtpOutput,
    pub send_timer: SendTimer,
    now: TransportTick,
    socket_handles: [Option<LogicalSocketHandle>; 10],
    peer_addresses: [Option<PeerMac>; 10],
    restriction: Option<PeerRestriction>,
    session_operations: RemoteSessionOperations,
    object_operations: ConcreteRemoteObjectOperations,
    property_operations: RemotePropertyOperations,
    identity: Option<CameraIdentity>,
    peer: Option<PeerHandshake>,
    peer_slot: Option<SlotId>,
    handshake_slot: Option<SlotId>,
    phase: DiscoveryPhase,
    blocked: bool,
    active_peers: u32,
    pending_packet: Option<PendingPacket>,
    pending_send: Option<PendingSend>,
    command: Option<CommandPacket>,
    continuation: Option<PtpPhase>,
    completion_flag: u8,
    error_flag: u8,
}

impl<'a> NetworkDiscoveryRuntime<'a> {
    pub fn new(packets: NetworkPacketPool<'a>, catalog: CatalogRuntime<'a>) -> Self {
        Self { packets, catalog, interface_control: RemoteControl::new(), state: PtpState::new(),
            send_timer: SendTimer::new(), now: TransportTick(0), socket_handles: [None; 10], peer_addresses: [None; 10], restriction: None,
            output: PtpOutput::new(), session_operations: RemoteSessionOperations::new(),
            object_operations: ConcreteRemoteObjectOperations::new(), property_operations: RemotePropertyOperations::new(),
            identity: None, peer: None, peer_slot: None, handshake_slot: None, phase: DiscoveryPhase::Initial,
            blocked: false, active_peers: 0, pending_packet: None, pending_send: None,
            command: None, continuation: None, completion_flag: 0, error_flag: 1 }
    }
    pub fn configure_identity(&mut self, identity: CameraIdentity) { self.identity = Some(identity); }
    pub fn set_blocked(&mut self, blocked: bool) { self.blocked = blocked; }
    pub fn set_peer_restriction(&mut self, restriction: Option<PeerRestriction>) { self.restriction = restriction; }
    pub fn bind_peer_address(&mut self, slot: SlotId, address: PeerMac) { self.peer_addresses[slot.get() as usize] = Some(address); }
    pub fn bind_socket_handle(&mut self, slot: SlotId, handle: LogicalSocketHandle) { self.socket_handles[slot.get() as usize] = Some(handle); }
    pub fn poll(&mut self, now: TransportTick, queue: &mut super::TaskQueue<'_>) -> Result<bool, DiscoveryError> {
        self.now = now;
        self.send_timer.poll(now, queue)
    }
    pub const fn handshake(&self) -> Option<PeerHandshake> { self.peer }
    pub const fn phase(&self) -> DiscoveryPhase { self.phase }
    pub const fn cancel_requested(&self) -> bool { self.error_flag != 0 }
    pub const fn peer_slot(&self) -> Option<SlotId> { self.handshake_slot }
    pub const fn active_peer_count(&self) -> u32 { self.active_peers }
    pub fn drop_peer(&mut self, slot: SlotId) {
        self.active_peers = self.active_peers.wrapping_sub(1);
        if self.peer_slot == Some(slot) { self.peer_slot = None; }
    }

    fn check_transfer(&mut self) -> Result<(), DiscoveryError> {
        if self.completion_flag < 2 { return Ok(()); }
        if self.completion_flag == 2 { self.completion_flag = 0; self.error_flag = 0; }
        Err(DiscoveryError::TransferFailed)
    }

    pub fn prepare_control(&mut self) {
        self.command = None;
        self.state.transaction = packet::TransactionId(0);
        self.completion_flag = 0;
        self.error_flag = 1;
    }

    pub fn release_control(&mut self) {
        self.session_operations.teardown(&mut self.state);
        self.output.discard();
        self.state = PtpState::new();
        self.command = None;
        self.continuation = None;
    }

    pub fn classify(&mut self, slot: SlotId, bytes: &[u8]) -> Result<TransferClassification, DiscoveryError> {
        self.check_transfer()?;
        if bytes.len() < 8 { return Err(PacketError::Truncated.into()); }
        if u32::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3]]) as usize != bytes.len() { return Err(PacketError::LengthMismatch.into()); }
        let kind = u32::from_le_bytes([bytes[4],bytes[5],bytes[6],bytes[7]]);
        if kind == u32::MAX { return Ok(TransferClassification { kind: TransferKind::Invalid, status: 0 }); }
        let Some(peer) = PeerHandshake::parse(bytes)? else {
            return Ok(TransferClassification { kind: TransferKind::Other, status: 0 });
        };
        self.classify_peer(slot, peer)
    }

    fn classify_peer(&mut self, slot: SlotId, peer: PeerHandshake) -> Result<TransferClassification, DiscoveryError> {
        if self.identity.is_none() { return Err(DiscoveryError::NotConfigured); }
        let mut status = if self.blocked { 0x2019 } else { 0 };
        if self.active_peers != 0 { status = 0x201e; }
        if peer.identity[..4] != [0xf2,0xe4,0x53,0x8f] { status = 0x201d; }
        if let Some(restriction) = self.restriction {
            let name_matches = peer.name.iter().copied().map(|character| character as u8).take_while(|character| *character != 0)
                .eq(restriction.name.0.iter().copied().take_while(|character| *character != 0));
            let address = self.peer_addresses[slot.get() as usize].ok_or(DiscoveryError::NotConfigured)?;
            if status == 0 && (!name_matches || restriction.address != address) { status = 0x2019; }
        }
        self.peer = Some(peer);
        self.handshake_slot = Some(slot);
        if status == 0 { self.active_peers = self.active_peers.wrapping_add(1); self.peer_slot = Some(slot); }
        Ok(TransferClassification { kind: TransferKind::Command, status })
    }

    pub fn classify_packet(&mut self, slot: SlotId, lease: &NetworkPacketLease) -> Result<TransferClassification, DiscoveryError> {
        self.check_transfer()?;
        let bytes = self.packets.bytes(lease)?;
        let peer = PeerHandshake::parse(bytes)?;
        if let Some(peer) = peer {
            self.classify_peer(slot, peer)
        } else {
            let kind = u32::from_le_bytes([bytes[4],bytes[5],bytes[6],bytes[7]]);
            Ok(TransferClassification { kind: if kind == u32::MAX { TransferKind::Invalid } else { TransferKind::Other }, status: 0 })
        }
    }

    pub fn dispatch_packet(&mut self, slot: SlotId, bytes: &[u8], socket: &mut SocketTaskRuntime) -> Result<(), DiscoveryError> {
        let lease = self.packets.insert(bytes)?;
        self.dispatch_packet_lease(slot, lease, socket)
    }

    pub fn dispatch_packet_lease(&mut self, slot: SlotId, lease: NetworkPacketLease, _socket: &mut SocketTaskRuntime) -> Result<(), DiscoveryError> {
        if self.pending_packet.is_some() {
            self.packets.release(lease)?;
            return Err(DiscoveryError::Busy);
        }
        if let Err(error) = ApplicationPacket::parse(self.packets.bytes(&lease)?) {
            self.packets.release(lease)?;
            return Err(error.into());
        }
        self.pending_packet = Some(PendingPacket { slot, lease });
        Ok(())
    }

    pub fn process_packet_lease(&mut self, slot: SlotId, lease: NetworkPacketLease, socket: &mut SocketTaskRuntime) -> Result<(), DiscoveryError> {
        self.process_transfer(0x6613, slot, Some(lease), socket).map(|_| ())
    }

    pub fn process_transfer(&mut self, kind: u16, slot: SlotId, mut lease: Option<NetworkPacketLease>, socket: &mut SocketTaskRuntime) -> Result<bool, DiscoveryError> {
        let result = match (self.phase, kind) {
            (DiscoveryPhase::Ready, 0x6603) => Ok(true),
            (DiscoveryPhase::Active, 0x6603) => {
                match self.socket_handles[slot.get() as usize] {
                    Some(handle) => {
                        self.send_timer.arm(self.now, handle);
                        self.finish_response(slot, socket).map(|_| false)
                    }
                    None => Err(DiscoveryError::NotConfigured),
                }
            }
            (DiscoveryPhase::Ready, 0x6622 | 0x6614) | (DiscoveryPhase::Active, 0x6622) => {
                if self.phase == DiscoveryPhase::Ready { self.send_timer.cancel(); }
                self.drop_peer(slot);
                self.phase = DiscoveryPhase::Initial;
                Ok(false)
            }
            (DiscoveryPhase::Active, 0x6614) => self.cancel().map(|_| false),
            (DiscoveryPhase::Initial, 0x6613) => {
                match lease.as_ref().ok_or(DiscoveryError::NotConfigured).and_then(|lease| self.classify_packet(slot, lease)) {
                    Ok(classification) => match classification.kind {
                        TransferKind::Invalid => self.send_response(slot, 0x2019, socket).map(|_| false),
                        TransferKind::Command => self.send_response(slot, classification.status, socket).map(|_| false),
                        TransferKind::Other => Ok(false),
                    },
                    Err(error) => Err(error),
                }
            }
            (DiscoveryPhase::Active, 0x6613) => {
                match lease.as_ref().ok_or(DiscoveryError::NotConfigured).and_then(|lease| self.classify_packet(slot, lease)) {
                    Ok(classification) if classification.kind == TransferKind::Invalid => {
                        self.drop_peer(slot);
                        self.phase = DiscoveryPhase::Initial;
                        Ok(false)
                    }
                    Ok(_) => match lease.take() {
                        Some(lease) => self.dispatch_packet_lease(slot, lease, socket).map(|_| false),
                        None => Err(DiscoveryError::NotConfigured),
                    },
                    Err(error) => Err(error),
                }
            }
            _ => Ok(false),
        };
        if let Some(lease) = lease { self.packets.release(lease)?; }
        result
    }

    pub fn send_response(&mut self, slot: SlotId, status: i32, _socket: &mut SocketTaskRuntime) -> Result<(), DiscoveryError> {
        if self.pending_send.is_some() || !self.output.is_empty() { return Err(DiscoveryError::Busy); }
        let identity = self.identity.ok_or(DiscoveryError::NotConfigured)?;
        let (frame, length) = packet::response_frame(status, identity);
        self.output.send_frame(&frame[..length])?;
        self.peer_slot = Some(slot);
        if status == 0 { self.phase = DiscoveryPhase::Active; }
        Ok(())
    }

    pub fn finish_response(&mut self, slot: SlotId, _socket: &mut SocketTaskRuntime) -> Result<(), DiscoveryError> {
        if self.pending_send.is_some() { return Err(DiscoveryError::Busy); }
        self.output.send_frame(&packet::finish_frame())?;
        self.peer_slot = Some(slot);
        self.phase = DiscoveryPhase::Ready;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), DiscoveryError> {
        self.error_flag = 1;
        if self.command.is_some() { self.continuation = Some(PtpPhase::Data); }
        else { self.state.response = executor::ResponseCode(0x2001); }
        self.check_transfer()
    }

    pub fn queue_output(&mut self, queue: &mut super::TaskQueue<'_>) -> Result<bool, DiscoveryError> {
        if self.pending_send.is_some() || self.output.is_empty() { return Ok(false); }
        let slot = self.peer_slot.ok_or(DiscoveryError::Slot)?;
        let handle = self.socket_handles[slot.get() as usize].ok_or(DiscoveryError::NotConfigured)?;
        let lease = self.packets.insert(self.output.queued_bytes())?;
        let identity = lease.identity();
        let mut bytes = [0; 200];
        bytes[..4].copy_from_slice(&handle.0.to_le_bytes());
        bytes[8..12].copy_from_slice(&(self.output.queued_bytes().len() as u32).to_le_bytes());
        let payload = super::TaskPayload::parse(&bytes).map_err(DiscoveryError::Payload)?;
        let message = super::TaskMessage::new(super::TaskMessageKind(0x6607), payload).with_packet(lease);
        match queue.enqueue(message) {
            Ok(()) => { self.pending_send = Some(PendingSend { slot, packet: identity, operation: None }); Ok(true) }
            Err(mut error) => {
                if let Some(lease) = error.message.packet.take() { self.packets.release(lease)?; }
                Err(DiscoveryError::QueueFull)
            }
        }
    }

    pub fn bind_output_operation(&mut self, slot: SlotId, lease: &NetworkPacketLease, operation: SocketTaskOperationId) -> bool {
        let Some(pending) = self.pending_send.as_mut() else { return false; };
        if pending.slot != slot || pending.packet != lease.identity() { return false; }
        pending.operation = Some(operation);
        true
    }

    pub fn complete_output(&mut self, completion: SocketTaskSocketCompletion) -> Result<bool, DiscoveryError> {
        let Some(pending) = self.pending_send.as_ref() else { return Ok(false); };
        if pending.operation != Some(completion.operation) || pending.slot.get() != u32::from(completion.slot.get()) { return Ok(false); }
        match completion.status {
            Ok(SocketTaskSocketResult::Sent(count)) => {
                self.output.acknowledge(count)?;
                if self.output.is_empty() { self.pending_send = None; }
                else if let Some(pending) = self.pending_send.as_mut() { pending.operation = None; }
                if self.output.is_empty() && self.command.is_some() {
                    match self.output.kind() {
                        executor::OutputKind::Data => {
                            if self.continuation.is_none() { self.continuation = Some(PtpPhase::Data); }
                        }
                        executor::OutputKind::Response => { self.command = None; self.continuation = None; }
                        executor::OutputKind::Control => {}
                    }
                }
                Ok(true)
            }
            Err(error) => { self.pending_send = None; Err(error.into()) }
            _ => Err(DiscoveryError::UnexpectedCompletion),
        }
    }

    pub fn teardown(&mut self, socket: &mut SocketTaskRuntime) -> Result<(), DiscoveryError> {
        if let Some(pending) = self.pending_packet.take() { self.packets.release(pending.lease)?; }
        if let Some(slot) = self.peer_slot {
            let slot = SocketTaskSlot::parse(slot.get() as u8).ok_or(DiscoveryError::Slot)?;
            socket.close(slot)?;
        }
        self.pending_send = None;
        self.release_control();
        self.active_peers = 0;
        self.peer_slot = None;
        self.phase = DiscoveryPhase::Initial;
        Ok(())
    }
}

#[cfg(test)]
#[path = "vendor_wireless_service_task_discovery_packet_tests.rs"]
mod packet_tests;
#[cfg(test)]
#[path = "vendor_wireless_service_task_discovery_pool_tests.rs"]
mod pool_tests;
#[cfg(test)]
#[path = "vendor_wireless_service_task_discovery_executor_tests.rs"]
mod executor_tests;
