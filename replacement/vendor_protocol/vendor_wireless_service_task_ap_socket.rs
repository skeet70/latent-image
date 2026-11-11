use super::{ModeCompletionTarget, ModeNotification, ModeState, packets::{DeviceModel, DeviceName, DiscoveryPacket, DiscoveryPort, PacketError, ServiceName, ServiceRegistry}};
use crate::vendor_wireless_service_task::{CompletionStatus, TaskMessage, PayloadError, interface_modes::{InterfaceModes, InterfaceMode, InterfaceError}, discovery::{NetworkDiscoveryRuntime, NetworkPacketLease, PacketPoolError, DiscoveryError}, station::{SlotId, SocketConfiguration, SocketPublication, SocketServiceRuntime, ServiceSocketKind, StationError}};
use crate::vendor_wireless_network::{dhcp::Milliseconds, lan::{LanRuntime, LanState}, packet::UdpEndpoint, dhcp::UdpPort, transport::{TcpPort, socket_task::{SocketTaskRuntime, SocketTaskOperationId, SocketTaskSocketCompletion, SocketTaskSocketResult, SocketTaskError, SocketTaskSlot, SocketServiceKind}}};
use crate::filesystem_runtime::SectorIo;
use crate::filesystem_catalog::Catalog;
use super::super::interface_modes::{InterfaceCompletion, InterfaceCompletionKind, InterfaceStatus};

#[derive(Debug)]
pub enum SocketModeError { Payload(PayloadError), Slot, Port, PacketMissing, PacketLength, Packet(PacketPoolError), Encoding(PacketError), Service(StationError), Socket(SocketTaskError), Discovery(DiscoveryError), Interface(InterfaceError), InterfaceStatus(InterfaceStatus), LanDown, NotificationCapacity, UnexpectedCompletion }
impl From<PayloadError> for SocketModeError { fn from(error: PayloadError) -> Self { Self::Payload(error) } }
impl From<StationError> for SocketModeError { fn from(error: StationError) -> Self { Self::Service(error) } }
impl From<SocketTaskError> for SocketModeError { fn from(error: SocketTaskError) -> Self { Self::Socket(error) } }
impl From<PacketError> for SocketModeError { fn from(error: PacketError) -> Self { Self::Encoding(error) } }
impl From<PacketPoolError> for SocketModeError { fn from(error: PacketPoolError) -> Self { Self::Packet(error) } }
impl From<DiscoveryError> for SocketModeError { fn from(error: DiscoveryError) -> Self { Self::Discovery(error) } }
impl From<InterfaceError> for SocketModeError { fn from(error: InterfaceError) -> Self { Self::Interface(error) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketState { Idle, Closing, Opening, Connected, Sending }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeState { Idle, Discovering, Transferring, Registering, Importing, Closing }
#[derive(Clone, Copy)]
enum PendingKind { Open, Close, Send }
struct Pending { id: SocketTaskOperationId, kind: PendingKind }

pub struct SocketModeRuntime {
    state: SocketState,
    handshake: HandshakeState,
    slot: Option<SlotId>,
    pending: Option<Pending>,
    close_seen: bool,
    interface_seen: bool,
    port: DiscoveryPort,
    name: Option<DeviceName>,
    target: Option<DeviceName>,
    peer: Option<ServiceName>,
    model: Option<DeviceModel>,
    registry: ServiceRegistry,
    registration_rejected: bool,
    failure: Option<SocketTaskError>,
    interface_failure: Option<InterfaceStatus>,
    notifications: [Option<ModeNotification>; 16],
    read: usize,
    length: usize,
    publication: Option<SocketPublication>,
    mode_state: ModeState,
    service_failure: Option<CompletionStatus>,
}
impl SocketModeRuntime {
    pub const fn new() -> Self {
        Self { state: SocketState::Idle, handshake: HandshakeState::Idle, slot: None, pending: None, close_seen: false, interface_seen: false,
            port: DiscoveryPort::Register, name: None, target: None, peer: None, model: None, registry: ServiceRegistry::new(), registration_rejected: false, failure: None, interface_failure: None,
            notifications: [None; 16], read: 0, length: 0, publication: None, mode_state: ModeState::Idle, service_failure: None }
    }
    pub fn configure(&mut self, slot: SlotId, name: Option<DeviceName>, model: DeviceModel, port: DiscoveryPort) -> Result<(), SocketModeError> {
        if self.state != SocketState::Idle || self.handshake != HandshakeState::Idle { return Err(SocketModeError::UnexpectedCompletion); }
        self.slot = Some(slot); self.name = name; self.model = Some(model); self.port = port; Ok(())
    }
    pub fn configure_target(&mut self, target: Option<DeviceName>, peer: ServiceName) { self.target = target; self.peer = Some(peer); }
    pub fn registry(&mut self) -> &mut ServiceRegistry { &mut self.registry }
    pub fn set_mode_state(&mut self, state: ModeState) { self.mode_state = state; }
    pub const fn state(&self) -> SocketState { self.state }
    pub const fn handshake_state(&self) -> HandshakeState { self.handshake }
    pub const fn registration_rejected(&self) -> bool { self.registration_rejected }
    pub fn publication(&mut self) -> Option<SocketPublication> { self.publication.take() }
    pub fn notification(&mut self) -> Option<ModeNotification> {
        if self.length == 0 { return None; }
        let result = self.notifications[self.read].take(); self.read = (self.read + 1) % self.notifications.len(); self.length -= 1; result
    }
    fn reserve(&self) -> Result<(), SocketModeError> { if self.length == self.notifications.len() { Err(SocketModeError::NotificationCapacity) } else { Ok(()) } }
    fn notify(&mut self, target: ModeCompletionTarget) {
        let index = (self.read + self.length) % self.notifications.len();
        self.notifications[index] = Some(ModeNotification { target, status: if self.failure.is_some() || self.interface_failure.is_some() || self.service_failure.is_some() { CompletionStatus::Failed } else { CompletionStatus::Success }, state: self.mode_state }); self.length += 1;
    }
    pub fn receive_interface_completion(&mut self, completion: InterfaceCompletion, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime) -> Result<bool, SocketModeError> {
        if self.handshake != HandshakeState::Importing || completion.kind != InterfaceCompletionKind::Started { return Ok(false); }
        self.reserve()?;
        self.interface_seen = true;
        if completion.status != InterfaceStatus::Success { self.interface_failure = Some(completion.status); }
        let handled = self.finish_import(slots, socket)?;
        self.publish_state(slots);
        if completion.status != InterfaceStatus::Success { return Err(SocketModeError::InterfaceStatus(completion.status)); }
        Ok(handled)
    }
    pub fn receive_service_discovery(&mut self, entries: &[super::packets::ServiceEntry], status: CompletionStatus, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime) -> Result<bool, SocketModeError> {
        if self.handshake != HandshakeState::Transferring { return Ok(false); }
        self.registry.replace(entries)?;
        if status != CompletionStatus::Success { self.service_failure = Some(status); }
        self.close(slots, socket)?; self.publish_state(slots); Ok(true)
    }
    fn slot(&self) -> Result<SlotId, SocketModeError> { self.slot.ok_or(SocketModeError::Slot) }
    fn open(&mut self, kind: ServiceSocketKind, port: u16, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime) -> Result<(), SocketModeError> {
        let id = slots.open_service(self.slot()?, kind, TcpPort::parse(port).ok_or(SocketModeError::Port)?, socket)?;
        self.pending = Some(Pending { id, kind: PendingKind::Open }); self.state = SocketState::Opening; Ok(())
    }
    fn close(&mut self, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime) -> Result<(), SocketModeError> {
        let id = slots.close(self.slot()?, socket)?; self.pending = Some(Pending { id, kind: PendingKind::Close }); self.state = SocketState::Closing; Ok(())
    }
    fn send(&mut self, bytes: &[u8], destination: Option<UdpEndpoint>, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime) -> Result<(), SocketModeError> {
        let slot = self.slot()?;
        let id = match destination { Some(destination) => socket.send_to(SocketTaskSlot::parse(slot.get() as u8).ok_or(SocketModeError::Slot)?, destination, bytes)?, None => slots.send(slot, bytes, socket)? };
        self.pending = Some(Pending { id, kind: PendingKind::Send }); self.state = SocketState::Sending; Ok(())
    }
    pub fn reset(&mut self, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), SocketModeError> {
        discovery.cancel()?;
        *self = Self::new(); Ok(())
    }
    pub fn configure_channel<I: SectorIo>(&mut self, message: &mut TaskMessage, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>, lan: &mut LanRuntime<'_>, modes: &mut InterfaceModes, mode: InterfaceMode, catalog: &mut Catalog<'_, I>, now: Milliseconds) -> Result<bool, SocketModeError> {
        let slot = self.slot()?;
        let words = [message.payload.word(0)?, message.payload.word(4)?, message.payload.word(8)?, message.payload.word(12)?, message.payload.word(16)?];
        if slots.configuration(slot).words != words { slots.configure_slot(slot, SocketConfiguration { words }); self.publication = slots.select_socket(slot, false, discovery.active_peer_count()); }
        if words[2] != 0 { return Ok(false); }
        if discovery.phase() != super::super::discovery::DiscoveryPhase::Initial || mode == InterfaceMode::Mode5 {
            self.publication = slots.select_socket(slot, true, discovery.active_peer_count());
        }
        let result = self.handshake_dispatch(0x6624, None, slots, socket, discovery, lan, modes, mode, catalog, now);
        self.publish_state(slots); result
    }
    fn publish_state(&self, slots: &mut SocketServiceRuntime) {
        if let Some(slot) = self.slot {
            use super::super::station::SocketPhase;
            slots.set_station_socket(match self.state { SocketState::Idle => SocketPhase::Inactive, SocketState::Closing => SocketPhase::Opening, SocketState::Opening => SocketPhase::AwaitingOpen, SocketState::Connected => SocketPhase::Connected, SocketState::Sending => SocketPhase::Transferring }, slot);
        }
    }
    pub fn dispatch<I: SectorIo>(&mut self, message: &mut TaskMessage, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>, lan: &mut LanRuntime<'_>, modes: &mut InterfaceModes, mode: InterfaceMode, catalog: &mut Catalog<'_, I>, now: Milliseconds) -> Result<bool, SocketModeError> {
        let result = self.dispatch_inner(message, slots, socket, discovery, lan, modes, mode, catalog, now);
        self.publish_state(slots); result
    }
    fn dispatch_inner<I: SectorIo>(&mut self, message: &mut TaskMessage, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>, lan: &mut LanRuntime<'_>, modes: &mut InterfaceModes, mode: InterfaceMode, catalog: &mut Catalog<'_, I>, now: Milliseconds) -> Result<bool, SocketModeError> {
        self.reserve()?;
        let kind = message.kind.0;
        match (kind, self.state) {
            (0x6610 | 0x6611 | 0x6614 | 0x662a, _) => Ok(false),
            (0x6620, SocketState::Idle) => {
                self.slot = Some(SlotId::new(message.payload.word(0)?).ok_or(SocketModeError::Slot)?);
                let kind = ServiceSocketKind::parse(message.payload.word(4)?).ok_or(SocketModeError::Port)?;
                self.open(kind, u16::try_from(message.payload.word(8)?).map_err(|_| SocketModeError::Port)?, slots, socket)?; Ok(true)
            }
            (0x6621, SocketState::Opening | SocketState::Connected | SocketState::Sending) => { self.close(slots, socket)?; Ok(true) }
            (0x6607, SocketState::Connected) => {
                let lease = message.packet.as_ref().ok_or(SocketModeError::PacketMissing)?;
                let bytes = discovery.packets.bytes(lease)?;
                if bytes.len() != message.payload.word(8)? as usize { return Err(SocketModeError::PacketLength); }
                let destination = if message.payload.word(12)? == 0 { None } else { Some(UdpEndpoint { address: crate::vendor_wireless_network::state::Ipv4Address(message.payload.word(12)?.to_le_bytes()), port: UdpPort(u16::try_from(message.payload.word(16)?).map_err(|_| SocketModeError::Port)?) }) };
                self.send(bytes, destination, slots, socket)?; Ok(true)
            }
            (0x6607, SocketState::Sending) => {
                let slot = SlotId::new(message.payload.word(0)?).ok_or(SocketModeError::Slot)?;
                let lease = message.packet.take().ok_or(SocketModeError::PacketMissing)?;
                if let Some(old) = slots.retain_primary_packet(slot, lease) { discovery.packets.release(old)?; } Ok(true)
            }
            (0x6613, SocketState::Sending) => {
                let slot = SlotId::new(message.payload.word(0)?).ok_or(SocketModeError::Slot)?;
                let lease = message.packet.take().ok_or(SocketModeError::PacketMissing)?;
                if let Some(old) = slots.retain_secondary_packet(slot, lease) { discovery.packets.release(old)?; } Ok(true)
            }
            (0x6613, SocketState::Connected) => {
                let lease = message.packet.take().ok_or(SocketModeError::PacketMissing)?;
                self.handshake_dispatch(kind, Some(lease), slots, socket, discovery, lan, modes, mode, catalog, now)
            }
            (0x6603, SocketState::Closing) => Ok(true),
            (0x6603, SocketState::Opening | SocketState::Connected | SocketState::Sending) => {
                let handled = self.handshake_dispatch(kind, None, slots, socket, discovery, lan, modes, mode, catalog, now)?;
                if !handled { self.close(slots, socket)?; return Ok(true); } Ok(handled)
            }
            (0x6624, SocketState::Opening) => { self.close(slots, socket)?; Ok(true) }
            (0x6624, SocketState::Connected | SocketState::Sending) => {
                let handled = self.handshake_dispatch(0x6622, None, slots, socket, discovery, lan, modes, mode, catalog, now)?;
                if !handled { self.close(slots, socket)?; return Ok(true); } Ok(handled)
            }
            (0x6612, SocketState::Connected | SocketState::Sending) => {
                let handled = self.handshake_dispatch(0x6622, None, slots, socket, discovery, lan, modes, mode, catalog, now)?;
                if !handled { self.state = SocketState::Idle; } Ok(handled)
            }
            (0x6603, SocketState::Idle) => Ok(false),
            (_, SocketState::Idle | SocketState::Opening | SocketState::Connected | SocketState::Sending) => self.handshake_dispatch(kind, None, slots, socket, discovery, lan, modes, mode, catalog, now),
            _ => Ok(false),
        }
    }
    pub fn receive_completion<I: SectorIo>(&mut self, completion: SocketTaskSocketCompletion, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>, lan: &mut LanRuntime<'_>, modes: &mut InterfaceModes, mode: InterfaceMode, catalog: &mut Catalog<'_, I>, now: Milliseconds) -> Result<bool, SocketModeError> {
        let result = self.receive_inner(completion, slots, socket, discovery, lan, modes, mode, catalog, now);
        self.publish_state(slots); result
    }
    fn receive_inner<I: SectorIo>(&mut self, completion: SocketTaskSocketCompletion, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>, lan: &mut LanRuntime<'_>, modes: &mut InterfaceModes, mode: InterfaceMode, catalog: &mut Catalog<'_, I>, now: Milliseconds) -> Result<bool, SocketModeError> {
        let pending = self.pending.as_ref().ok_or(SocketModeError::UnexpectedCompletion)?;
        if pending.id != completion.operation || self.slot()?.get() != completion.slot.get() as u32 { return Err(SocketModeError::UnexpectedCompletion); }
        self.reserve()?;
        let kind = pending.kind;
        let valid = matches!((kind, completion.status), (PendingKind::Open, Ok(SocketTaskSocketResult::Opened(_))) | (PendingKind::Close, Ok(SocketTaskSocketResult::Closed)) | (PendingKind::Send, Ok(SocketTaskSocketResult::Sent(_))) | (_, Err(_)));
        if !valid { return Err(SocketModeError::UnexpectedCompletion); }
        self.pending = None;
        if let Err(error) = completion.status { self.failure = Some(error); }
        match kind {
            PendingKind::Open => {
                if completion.status.is_err() { self.close(slots, socket)?; return Err(SocketModeError::Socket(self.failure.unwrap())); }
                slots.set_handle(self.slot()?, completion.operation.get());
                self.handshake_dispatch(0x6610, None, slots, socket, discovery, lan, modes, mode, catalog, now)?;
                if self.state == SocketState::Opening { self.state = SocketState::Connected; } Ok(false)
            }
            PendingKind::Close => {
                let handled = self.handshake_dispatch(0x6611, None, slots, socket, discovery, lan, modes, mode, catalog, now)?;
                if self.state == SocketState::Closing { self.state = SocketState::Idle; }
                if let Err(error) = completion.status { return Err(SocketModeError::Socket(error)); } Ok(handled)
            }
            PendingKind::Send => {
                let handled = self.handshake_dispatch(0x6614, None, slots, socket, discovery, lan, modes, mode, catalog, now)?;
                if handled { return Ok(true); }
                let slot = self.slot()?;
                if completion.status.is_err() {
                    if let Some(packet) = slots.take_primary_packet(slot) { discovery.packets.release(packet)?; }
                }
                if let Some(packet) = slots.take_primary_packet(slot) {
                    let result = self.send(discovery.packets.bytes(&packet)?, None, slots, socket);
                    discovery.packets.release(packet)?; result?; return Ok(true);
                }
                self.state = SocketState::Connected;
                if let Some(packet) = slots.take_secondary_packet(slot) {
                    self.handshake_dispatch(0x6613, Some(packet), slots, socket, discovery, lan, modes, mode, catalog, now)?;
                }
                if let Err(error) = completion.status { return Err(SocketModeError::Socket(error)); } Ok(false)
            }
        }
    }
    fn handshake_dispatch<I: SectorIo>(&mut self, kind: u16, packet: Option<NetworkPacketLease>, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>, lan: &mut LanRuntime<'_>, modes: &mut InterfaceModes, mode: InterfaceMode, catalog: &mut Catalog<'_, I>, now: Milliseconds) -> Result<bool, SocketModeError> {
        if self.handshake == HandshakeState::Closing && kind == 0x6613 {
            return discovery.process_transfer(kind, self.slot()?, packet, socket).map_err(SocketModeError::Discovery);
        }
        let result = self.handshake_inner(kind, packet.as_ref(), slots, socket, discovery, lan, modes, mode, catalog, now);
        if let Some(packet) = packet { discovery.packets.release(packet)?; }
        result
    }
    fn handshake_inner<I: SectorIo>(&mut self, kind: u16, packet: Option<&NetworkPacketLease>, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>, lan: &mut LanRuntime<'_>, modes: &mut InterfaceModes, mode: InterfaceMode, catalog: &mut Catalog<'_, I>, now: Milliseconds) -> Result<bool, SocketModeError> {
        self.reserve()?;
        match (kind, self.handshake) {
            (0x6618, HandshakeState::Idle) => { self.open(ServiceSocketKind::Datagram, self.port.number(), slots, socket)?; self.failure = None; self.interface_failure = None; self.service_failure = None; self.handshake = HandshakeState::Discovering; Ok(true) }
            (0x6619, HandshakeState::Idle) => { self.open(ServiceSocketKind::Stream, 51542, slots, socket)?; self.failure = None; self.interface_failure = None; self.service_failure = None; self.handshake = HandshakeState::Registering; Ok(true) }
            (0x661e, HandshakeState::Idle) => {
                self.close_seen = false; self.interface_seen = false; self.failure = None; self.interface_failure = None; self.service_failure = None;
                self.open(ServiceSocketKind::Stream, 51541, slots, socket)?;
                modes.start(mode, discovery, catalog)?;
                self.handshake = HandshakeState::Importing; Ok(true)
            }
            (0x6610, HandshakeState::Discovering | HandshakeState::Registering | HandshakeState::Importing) => {
                let lease = match lan.state() { LanState::Ready(lease) => lease, _ => return Err(SocketModeError::LanDown) };
                let (packet, destination) = match self.handshake {
                    HandshakeState::Discovering => {
                        let packet = DiscoveryPacket::discover(lease.address, lease.mask, self.target, self.port)?;
                        let mut broadcast = lease.address.0; for (out, mask) in broadcast.iter_mut().zip(lease.mask.0) { *out |= !mask; }
                        (packet, Some(UdpEndpoint { address: crate::vendor_wireless_network::state::Ipv4Address(broadcast), port: UdpPort(self.port.number()) }))
                    }
                    HandshakeState::Registering | HandshakeState::Importing => {
                        let Some(peer) = self.peer.and_then(|name| self.registry.find(name)) else { return Ok(false); };
                        let address = crate::vendor_wireless_network::state::Ipv4Address(peer.0.to_le_bytes());
                        let packet = if self.handshake == HandshakeState::Registering {
                            DiscoveryPacket::register(address, self.name.ok_or(SocketModeError::Encoding(PacketError::Name))?, self.model.ok_or(SocketModeError::Encoding(PacketError::Model))?)?
                        } else { DiscoveryPacket::import(address, self.name.ok_or(SocketModeError::Encoding(PacketError::Name))?, self.port)? };
                        (packet, None)
                    }
                    _ => return Ok(false),
                };
                self.send(packet.as_bytes(), destination, slots, socket)?; Ok(true)
            }
            (0x6611, HandshakeState::Discovering) => { self.open(ServiceSocketKind::Control, 51540, slots, socket)?; self.handshake = HandshakeState::Transferring; Ok(true) }
            (0x6611, HandshakeState::Transferring) => { self.notify(ModeCompletionTarget::Discover); self.handshake = HandshakeState::Idle; Ok(false) }
            (0x6611, HandshakeState::Registering) => { self.notify(ModeCompletionTarget::Register); self.handshake = HandshakeState::Idle; Ok(false) }
            (0x6611, HandshakeState::Closing) => { self.handshake = HandshakeState::Idle; Ok(false) }
            (0x6611, HandshakeState::Importing) => { self.close_seen = true; self.finish_import(slots, socket) }
            (0x6606, HandshakeState::Discovering | HandshakeState::Registering | HandshakeState::Importing) => {
                let target = match self.handshake { HandshakeState::Discovering => ModeCompletionTarget::Discover, HandshakeState::Registering => ModeCompletionTarget::Register, _ => ModeCompletionTarget::Import };
                self.close(slots, socket)?; self.notify(target); self.handshake = HandshakeState::Idle; Ok(false)
            }
            (0x6606, HandshakeState::Transferring) => { discovery.cancel()?; self.close(slots, socket)?; Ok(true) }
            (0x6629, HandshakeState::Transferring) => { self.close(slots, socket)?; Ok(true) }
            (0x6614, HandshakeState::Discovering) => { self.close(slots, socket)?; Ok(true) }
            (0x6614, HandshakeState::Closing) => self.forward(kind, packet, discovery, socket),
            (0x6613, HandshakeState::Registering | HandshakeState::Importing) => {
                let packet = packet.ok_or(SocketModeError::PacketMissing)?;
                let bytes = discovery.packets.bytes(packet)?;
                if self.handshake == HandshakeState::Registering {
                    if bytes.starts_with(b"HTTP/1.1 200 OK") { self.registration_rejected = false; }
                    else if bytes.starts_with(b"HTTP/1.1 403 Fo") { self.registration_rejected = true; }
                }
                self.close(slots, socket)?; Ok(true)
            }
            (0x6613, HandshakeState::Closing) => self.forward(kind, packet, discovery, socket),
            (0x6603 | 0x6622, HandshakeState::Closing) => {
                let handled = self.forward(kind, packet, discovery, socket)?;
                if !handled { self.close(slots, socket)?; return Ok(true); } Ok(handled)
            }
            (_, HandshakeState::Closing) => self.forward(kind, packet, discovery, socket),
            _ => { let _ = now; Ok(false) }
        }
    }
    fn finish_import(&mut self, slots: &mut SocketServiceRuntime, socket: &mut SocketTaskRuntime) -> Result<bool, SocketModeError> {
        if !self.close_seen || !self.interface_seen { return Ok(true); }
        self.notify(ModeCompletionTarget::Import);
        let id = socket.open_service(SocketTaskSlot::parse(self.slot()?.get() as u8).ok_or(SocketModeError::Slot)?, SocketServiceKind::Listener, TcpPort::parse(55740).ok_or(SocketModeError::Port)?)?;
        self.pending = Some(Pending { id, kind: PendingKind::Open }); self.state = SocketState::Opening;
        self.handshake = HandshakeState::Closing; Ok(false)
    }
    fn forward(&mut self, kind: u16, _packet: Option<&NetworkPacketLease>, discovery: &mut NetworkDiscoveryRuntime<'_>, socket: &mut SocketTaskRuntime) -> Result<bool, SocketModeError> {
        discovery.process_transfer(kind, self.slot()?, None, socket).map_err(SocketModeError::Discovery)
    }
}
impl Default for SocketModeRuntime { fn default() -> Self { Self::new() } }
