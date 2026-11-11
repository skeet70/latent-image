use super::MacAddress;
use super::client::{ClientEvent, ClientPhase, DhcpClient};
use super::dhcp::{DhcpError, DhcpServer, LeaseSlot, Milliseconds, ServerConfiguration, TransactionId, UdpPort};
use super::link::{EtherType, EthernetFrame, LinkState, ResolutionOutcome, RouteError, write_header};
use super::packet::{HopLimit, PacketError, PacketIdentification, TrafficClass, UdpDatagram, UdpEndpoint};
use super::multicast::{MulticastGroup, MulticastState};
use super::state::{ChannelLeases, ChannelSlot, Ipv4Address, Lease, LeaseBank, LeaseEntry, LeasePhase, PeerChange, PeerError, PeerNotification, PeerRoster, TaskId};
use super::transport::{FragmentReassembler, ReassemblyOutcome, ReceiveOutcome, SocketConfiguration, SocketHandle, SocketStatus, TcpEndpoint, TcpPort, TcpSequence, TransportError, TransportOutcome, TransportRuntime, TransportTick};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanMode { Station { transaction: TransactionId }, StaticStation { lease: Lease }, AccessPoint { configuration: ServerConfiguration } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanState { Down, Acquiring, Ready(Lease) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanOutcome { Ignored, Buffered, Frame { length: usize } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanError { Down, Busy, Socket, Capacity, Packet(PacketError), Dhcp(DhcpError), Route(RouteError), Transport(TransportError), Peer(PeerError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpSocket { index: u8, generation: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpReceived { pub source: UdpEndpoint, pub length: usize }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UdpReceiveMode { WholeDatagram, Truncate }
struct UdpQueue {
    port: Option<UdpPort>, generation: u32, source: Option<UdpEndpoint>, length: usize, bytes: [u8; 2048],
    bound: Option<Ipv4Address>, groups: [Option<MulticastGroup>; 8], traffic: TrafficClass, multicast_interface: Option<Ipv4Address>,
}
impl UdpQueue {
    const fn new() -> Self { Self { port: None, generation: 0, source: None, length: 0, bytes: [0; 2048], bound: None, groups: [None; 8], traffic: TrafficClass(0), multicast_interface: None } }
    fn clear(&mut self) { self.port = None; self.source = None; self.length = 0; self.bound = None; self.groups.fill(None); self.traffic = TrafficClass(0); self.multicast_interface = None; self.generation = self.generation.wrapping_add(1); }
}
pub struct LanRuntime<'a> {
    mode: Option<LanMode>, state: LanState, link: LinkState, client: DhcpClient, server: DhcpServer<'a>,
    leases: ChannelLeases, peers: PeerRoster, udp: [UdpQueue; 8], identification: u16,
    transport: TransportRuntime<8, 2048>, fragments: FragmentReassembler<4, 4096>, reassembled: [u8; 4096],
    pending: [u8; 4096], pending_length: usize,
    multicast: MulticastState,
    next_udp_port: UdpPort,
}
impl<'a> LanRuntime<'a> {
    pub fn new(hardware: MacAddress, server_slots: &'a mut [LeaseSlot]) -> Self {
        let mut server = DhcpServer::new(ServerConfiguration::camera_default(), server_slots); server.stop();
        Self { mode: None, state: LanState::Down, link: LinkState::new(hardware), client: DhcpClient::new(hardware), server,
            leases: ChannelLeases::new(), peers: PeerRoster::new(), udp: core::array::from_fn(|_| UdpQueue::new()), identification: 0,
            transport: TransportRuntime::new(), fragments: FragmentReassembler::new(), reassembled: [0; 4096], pending: [0; 4096], pending_length: 0, multicast: MulticastState::new(), next_udp_port: UdpPort(49152) }
    }
    pub fn state(&self) -> LanState { self.state }
    pub fn neighbor(&self, address: Ipv4Address) -> Option<MacAddress> { self.link.neighbor(address) }
    pub fn leases(&self) -> &ChannelLeases { &self.leases }
    pub fn peers(&self) -> &PeerRoster { &self.peers }
    pub fn peer_changed(&mut self, change: PeerChange, address: MacAddress, source: TaskId) -> Result<Option<PeerNotification>, LanError> {
        if !matches!(self.state, LanState::Ready(_)) || !matches!(self.mode, Some(LanMode::AccessPoint { .. })) { return Err(LanError::Down); }
        self.peers.change(change, address, source).map_err(LanError::Peer)
    }
    pub fn start(&mut self, mode: LanMode, now: Milliseconds, output: &mut [u8]) -> Result<LanOutcome, LanError> {
        if self.state != LanState::Down { return Err(LanError::Busy); }
        if matches!(mode, LanMode::Station { .. }) && output.len() < 342 { return Err(LanError::Capacity); }
        self.link.advance(now);
        self.mode = Some(mode);
        match mode {
            LanMode::AccessPoint { configuration } => {
                self.server.start(configuration);
                self.install(Lease { address: configuration.address, mask: configuration.mask, router: Ipv4Address([0; 4]) });
                Ok(LanOutcome::Ignored)
            }
            LanMode::StaticStation { lease } => { self.install(lease); Ok(LanOutcome::Ignored) }
            LanMode::Station { transaction } => {
                self.client.start(transaction, now); self.state = LanState::Acquiring;
                self.client_frame(output)
            }
        }
    }
    fn install(&mut self, lease: Lease) {
        if self.link.lease() != Some(lease) {
            self.transport.clear(); self.fragments.clear();
            self.multicast.clear();
            for socket in &mut self.udp { socket.clear(); }
            self.pending_length = 0; self.link.install(lease);
        }
        if let Some(slot) = ChannelSlot::parse(0) { self.leases.install(LeaseBank::Primary, slot, LeaseEntry { phase: LeasePhase::Bound, lease, usable: true }); }
        self.state = LanState::Ready(lease);
    }
    pub fn stop(&mut self) {
        self.server.stop(); self.client.stop(); self.transport.clear(); self.fragments.clear();
        self.multicast.clear();
        for socket in &mut self.udp { socket.clear(); }
        self.pending_length = 0; self.link.clear(); self.peers.clear();
        if let Some(slot) = ChannelSlot::parse(0) { self.leases.clear(slot); }
        self.mode = None; self.state = LanState::Down;
    }
    pub fn cancel(&mut self) { self.stop(); }
    pub fn fail(&mut self) { self.stop(); }
    fn identification(&mut self) -> PacketIdentification { let value = PacketIdentification(self.identification); self.identification = self.identification.wrapping_add(1); value }
    fn client_frame(&mut self, output: &mut [u8]) -> Result<LanOutcome, LanError> {
        if output.len() < 342 { return Err(LanError::Capacity); }
        let identification = self.identification();
        let length = self.client.encode_ipv4(identification, &mut output[14..]).map_err(LanError::Packet)?;
        self.route_output(length, output)
    }
    fn route_output(&mut self, length: usize, output: &mut [u8]) -> Result<LanOutcome, LanError> {
        let destination = Ipv4Address(output[30..34].try_into().map_err(|_| LanError::Capacity)?);
        match self.link.resolve(destination) {
            Ok(hardware) => { write_header(output, self.link.hardware(), hardware, EtherType::Ipv4); Ok(LanOutcome::Frame { length: length + 14 }) }
            Err(RouteError::Unresolved(address)) => {
                if self.pending_length != 0 { return Err(LanError::Busy); }
                if length > self.pending.len() { return Err(LanError::Capacity); }
                self.pending[..length].copy_from_slice(&output[14..14 + length]); self.pending_length = length;
                let length = match self.link.begin_resolution(address, output) {
                    Ok(length) => length,
                    Err(error) => { self.pending_length = 0; return Err(LanError::Packet(error)); }
                };
                Ok(LanOutcome::Frame { length })
            }
            Err(error) => Err(LanError::Route(error)),
        }
    }
    pub fn receive(&mut self, bytes: &[u8], now: Milliseconds, output: &mut [u8]) -> Result<LanOutcome, LanError> {
        if self.state == LanState::Down { return Err(LanError::Down); }
        if output.len() < 590 { return Err(LanError::Capacity); }
        self.link.advance(now);
        let frame = EthernetFrame::parse(bytes).map_err(LanError::Packet)?;
        if frame.destination != self.link.hardware() && frame.destination.0[0] & 1 == 0 { return Ok(LanOutcome::Ignored); }
        if frame.kind == EtherType::Arp {
            return self.link.receive_arp(&frame, output).map(|length| length.map_or(LanOutcome::Ignored, |length| LanOutcome::Frame { length })).map_err(LanError::Packet);
        }
        if frame.payload.len() < 20 { return Err(LanError::Packet(PacketError::Truncated)); }
        let packet = if u16::from_be_bytes([frame.payload[6], frame.payload[7]]) & 0x3fff == 0 {
            frame.payload
        } else {
            let length = match self.fragments.receive_ipv4(frame.payload, TransportTick(now.0), &mut self.reassembled).map_err(LanError::Transport)? {
                ReassemblyOutcome::Buffered => return Ok(LanOutcome::Buffered),
                ReassemblyOutcome::Complete { length } => length,
            };
            &self.reassembled[..length]
        };
        let destination = Ipv4Address(packet[16..20].try_into().map_err(|_| LanError::Capacity)?);
        if packet[9] == 2 && matches!(self.state, LanState::Ready(_)) {
            self.multicast.receive_ipv4(packet).map_err(LanError::Packet)?;
            return Ok(LanOutcome::Ignored);
        }
        let multicast = MulticastGroup::parse(destination);
        let accepted = self.link.accepts(destination) || multicast.is_some_and(|group| self.multicast.contains(group));
        if !accepted && self.state != LanState::Acquiring { return Ok(LanOutcome::Ignored); }
        if packet[9] == 6 {
            if !matches!(self.state, LanState::Ready(_)) || !self.link.accepts(destination) { return Ok(LanOutcome::Ignored); }
            let outcome = self.transport.receive_ipv4(packet, &mut output[14..]).map_err(LanError::Transport)?;
            return match outcome { TransportOutcome::Packet { length } => self.route_output(length, output), TransportOutcome::Buffered => Ok(LanOutcome::Buffered), TransportOutcome::Ignored => Ok(LanOutcome::Ignored) };
        }
        if packet[9] != 17 { return Ok(LanOutcome::Ignored); }
        let datagram = UdpDatagram::parse_ipv4(packet).map_err(LanError::Packet)?;
        if matches!(self.mode, Some(LanMode::Station { .. })) && datagram.destination.port == UdpPort(68) && datagram.source.port == UdpPort(67) {
            let event = self.client.receive(datagram.payload, now).map_err(LanError::Dhcp)?;
            return self.client_event(event, output);
        }
        if !accepted || !matches!(self.state, LanState::Ready(_)) { return Ok(LanOutcome::Ignored); }
        if let Some(LanMode::AccessPoint { configuration }) = self.mode {
            if datagram.destination.port == UdpPort(67) && matches!(datagram.source.port, UdpPort(67) | UdpPort(68)) {
                let mut body = [0; 548];
                let Some(destination) = self.server.receive(datagram.payload, now, &mut body).map_err(LanError::Dhcp)? else { return Ok(LanOutcome::Ignored); };
                self.identification = self.identification.wrapping_add(1);
                let length = UdpDatagram { source: UdpEndpoint { address: configuration.address, port: UdpPort(67) },
                    destination: UdpEndpoint { address: destination.address, port: destination.port }, payload: &body }
                    .encode_ipv4(PacketIdentification(self.identification), HopLimit(64), &mut output[14..]).map_err(LanError::Packet)?;
                return self.route_output(length, output);
            }
        }
        let Some(socket) = self.udp.iter_mut().find(|socket| socket.port == Some(datagram.destination.port)
            && socket.bound.is_none_or(|bound| bound == destination)
            && multicast.is_none_or(|group| socket.groups.contains(&Some(group)))) else { return Ok(LanOutcome::Ignored); };
        if socket.source.is_some() { return Err(LanError::Busy); }
        if datagram.payload.len() > socket.bytes.len() { return Err(LanError::Capacity); }
        socket.bytes[..datagram.payload.len()].copy_from_slice(datagram.payload); socket.length = datagram.payload.len(); socket.source = Some(datagram.source);
        Ok(LanOutcome::Buffered)
    }
    fn client_event(&mut self, event: ClientEvent, output: &mut [u8]) -> Result<LanOutcome, LanError> {
        match event {
            ClientEvent::Bound(lease) => { self.install(lease); Ok(LanOutcome::Ignored) }
            ClientEvent::Request => self.client_frame(output),
            ClientEvent::Lost => {
                self.link.clear(); if let Some(slot) = ChannelSlot::parse(0) { self.leases.clear(slot); }
                self.transport.clear(); self.fragments.clear(); self.multicast.clear(); for socket in &mut self.udp { socket.clear(); } self.pending_length = 0;
                self.state = LanState::Acquiring; self.client_frame(output)
            }
            ClientEvent::Ignored => Ok(LanOutcome::Ignored),
        }
    }
    pub fn poll(&mut self, now: Milliseconds, output: &mut [u8]) -> Result<LanOutcome, LanError> {
        if self.state == LanState::Down { return Err(LanError::Down); }
        if output.len() < 590 { return Err(LanError::Capacity); }
        self.server.expire(now);
        let event = self.client.poll(now);
        if event != ClientEvent::Ignored { self.link.advance(now); return self.client_event(event, output); }
        match self.link.poll_resolution(now, output).map_err(LanError::Packet)? {
            ResolutionOutcome::Frame { length } => return Ok(LanOutcome::Frame { length }),
            ResolutionOutcome::Failed(address) => { self.pending_length = 0; return Err(LanError::Route(RouteError::NeighborTimedOut(address))); }
            ResolutionOutcome::Idle => {}
        }
        if self.pending_length != 0 {
            let destination = Ipv4Address(self.pending[16..20].try_into().map_err(|_| LanError::Capacity)?);
            if let Ok(hardware) = self.link.resolve(destination) {
                let length = self.pending_length;
                if output.len() < length + 14 { return Err(LanError::Capacity); }
                write_header(output, self.link.hardware(), hardware, EtherType::Ipv4); output[14..14 + length].copy_from_slice(&self.pending[..length]);
                self.pending_length = 0; return Ok(LanOutcome::Frame { length: length + 14 });
            }
        }
        if self.pending_length == 0 && matches!(self.state, LanState::Ready(_)) {
            if let LanState::Ready(lease) = self.state {
                if let Some(length) = self.multicast.poll_ipv4(lease.address, PacketIdentification(self.identification), &mut output[14..]).map_err(LanError::Packet)? {
                    self.identification = self.identification.wrapping_add(1);
                    return self.route_output(length, output);
                }
            }
            if let TransportOutcome::Packet { length } = self.fragments.expire_ipv4(TransportTick(now.0), &mut output[14..]).map_err(LanError::Transport)? {
                return self.route_output(length, output);
            }
            return match self.transport.poll(TransportTick(now.0), &mut output[14..]).map_err(LanError::Transport)? {
                TransportOutcome::Packet { length } => self.route_output(length, output),
                TransportOutcome::Buffered => Ok(LanOutcome::Buffered),
                TransportOutcome::Ignored => Ok(LanOutcome::Ignored),
            };
        }
        Ok(LanOutcome::Ignored)
    }
    pub fn open_udp_ephemeral(&mut self) -> Result<UdpSocket, LanError> {
        if !matches!(self.state, LanState::Ready(_)) { return Err(LanError::Down); }
        if self.udp.iter().all(|socket| socket.port.is_some()) { return Err(LanError::Capacity); }
        for _ in 0..=self.udp.len() {
            let port = self.next_udp_port;
            self.next_udp_port = UdpPort(if port.0 == u16::MAX { 49152 } else { port.0 + 1 });
            if self.udp.iter().all(|socket| socket.port != Some(port)) { return self.open_udp(port); }
        }
        Err(LanError::Capacity)
    }
    pub fn udp_local_endpoint(&mut self, handle: UdpSocket) -> Result<UdpEndpoint, LanError> {
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        let port = self.socket(handle)?.port.ok_or(LanError::Socket)?;
        Ok(UdpEndpoint { address: lease.address, port })
    }
    pub fn open_udp(&mut self, port: UdpPort) -> Result<UdpSocket, LanError> {
        if !matches!(self.state, LanState::Ready(_)) { return Err(LanError::Down); }
        if port.0 == 0 || port == UdpPort(67) || port == UdpPort(68) || self.udp.iter().any(|socket| socket.port == Some(port)) { return Err(LanError::Socket); }
        let (index, socket) = self.udp.iter_mut().enumerate().find(|(_, socket)| socket.port.is_none()).ok_or(LanError::Capacity)?;
        socket.port = Some(port); Ok(UdpSocket { index: index as u8, generation: socket.generation })
    }
    fn socket(&mut self, handle: UdpSocket) -> Result<&mut UdpQueue, LanError> {
        self.udp.get_mut(usize::from(handle.index)).filter(|socket| socket.port.is_some() && socket.generation == handle.generation).ok_or(LanError::Socket)
    }
    pub fn close_udp(&mut self, handle: UdpSocket) -> Result<(), LanError> {
        let groups = self.socket(handle)?.groups;
        self.socket(handle)?.clear();
        for group in groups.into_iter().flatten() { self.multicast.leave(group); }
        Ok(())
    }
    pub fn bind_udp(&mut self, handle: UdpSocket, address: Ipv4Address) -> Result<(), LanError> {
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        if address != Ipv4Address([0; 4]) && address != lease.address && MulticastGroup::parse(address).is_none() { return Err(LanError::Route(RouteError::NoAddress)); }
        self.socket(handle)?.bound = (address != Ipv4Address([0; 4])).then_some(address);
        Ok(())
    }
    pub fn set_udp_traffic_class(&mut self, handle: UdpSocket, traffic: TrafficClass) -> Result<(), LanError> { self.socket(handle)?.traffic = traffic; Ok(()) }
    pub fn join_udp_multicast(&mut self, handle: UdpSocket, group: MulticastGroup) -> Result<(), LanError> {
        let socket = self.socket(handle)?;
        if socket.groups.contains(&Some(group)) { return Ok(()); }
        let index = socket.groups.iter().position(Option::is_none).ok_or(LanError::Capacity)?;
        self.multicast.join(group).map_err(LanError::Packet)?;
        self.socket(handle)?.groups[index] = Some(group); Ok(())
    }
    pub fn leave_udp_multicast(&mut self, handle: UdpSocket, group: MulticastGroup) -> Result<(), LanError> {
        if let Some(slot) = self.socket(handle)?.groups.iter_mut().find(|entry| **entry == Some(group)) {
            *slot = None; self.multicast.leave(group);
        }
        Ok(())
    }
    pub fn udp_received(&self, handle: UdpSocket) -> Result<Option<UdpReceived>, LanError> {
        let socket = self.udp.get(usize::from(handle.index)).filter(|socket| socket.port.is_some() && socket.generation == handle.generation).ok_or(LanError::Socket)?;
        Ok(socket.source.map(|source| UdpReceived { source, length: socket.length }))
    }
    pub fn receive_udp(&mut self, handle: UdpSocket, output: &mut [u8]) -> Result<Option<UdpReceived>, LanError> {
        self.receive_udp_with_mode(handle, output, UdpReceiveMode::WholeDatagram)
    }
    pub fn receive_udp_with_mode(&mut self, handle: UdpSocket, output: &mut [u8], mode: UdpReceiveMode) -> Result<Option<UdpReceived>, LanError> {
        let socket = self.socket(handle)?;
        let Some(source) = socket.source else { return Ok(None); };
        if mode == UdpReceiveMode::WholeDatagram && output.len() < socket.length { return Err(LanError::Capacity); }
        let length = socket.length.min(output.len());
        output[..length].copy_from_slice(&socket.bytes[..length]); socket.source = None; socket.length = 0;
        Ok(Some(UdpReceived { source, length }))
    }
    pub fn send_udp(&mut self, handle: UdpSocket, destination: UdpEndpoint, payload: &[u8], output: &mut [u8]) -> Result<LanOutcome, LanError> {
        if self.pending_length != 0 { return Err(LanError::Busy); }
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        let socket = self.socket(handle)?;
        let port = socket.port.ok_or(LanError::Socket)?;
        let traffic = socket.traffic;
        let multicast = MulticastGroup::parse(destination.address).is_some();
        let source = if multicast { socket.multicast_interface.unwrap_or(lease.address) } else { lease.address };
        let hops = if multicast { HopLimit(1) } else { HopLimit(64) };
        if output.len() < 42 { return Err(LanError::Capacity); }
        let identification = self.identification();
        let length = UdpDatagram { source: UdpEndpoint { address: source, port }, destination, payload }.encode_ipv4_with_traffic(identification, hops, traffic, &mut output[14..]).map_err(LanError::Packet)?;
        self.route_output(length, output)
    }
    pub fn open_tcp_unbound(&mut self) -> Result<SocketHandle, LanError> {
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        self.transport.open_unbound(lease.address).map_err(LanError::Transport)
    }
    pub fn open_tcp(&mut self, port: TcpPort) -> Result<SocketHandle, LanError> {
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        self.transport.open(TcpEndpoint { address: lease.address, port }).map_err(LanError::Transport)
    }
    pub fn connect_tcp_handle(&mut self, handle: SocketHandle, peer: TcpEndpoint, sequence: TcpSequence) -> Result<(), LanError> {
        if !matches!(self.state, LanState::Ready(_)) { return Err(LanError::Down); }
        self.transport.connect_handle(handle, peer, sequence).map_err(LanError::Transport)
    }
    pub fn listen_tcp_handle(&mut self, handle: SocketHandle, sequence: TcpSequence) -> Result<(), LanError> {
        if !matches!(self.state, LanState::Ready(_)) { return Err(LanError::Down); }
        self.transport.listen_handle(handle, sequence).map_err(LanError::Transport)
    }
    pub fn listen_tcp(&mut self, port: TcpPort, sequence: TcpSequence) -> Result<SocketHandle, LanError> {
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        self.transport.listen(TcpEndpoint { address: lease.address, port }, sequence).map_err(LanError::Transport)
    }
    pub fn connect_tcp(&mut self, port: TcpPort, peer: TcpEndpoint, sequence: TcpSequence) -> Result<SocketHandle, LanError> {
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        self.transport.connect(TcpEndpoint { address: lease.address, port }, peer, sequence).map_err(LanError::Transport)
    }
    pub fn accept_tcp(&mut self, handle: SocketHandle) -> Result<SocketHandle, LanError> { self.transport.accept(handle).map_err(LanError::Transport) }
    pub fn send_tcp(&mut self, handle: SocketHandle, payload: &[u8]) -> Result<usize, LanError> { self.transport.send(handle, payload).map_err(LanError::Transport) }
    pub fn receive_tcp(&mut self, handle: SocketHandle, output: &mut [u8]) -> Result<ReceiveOutcome, LanError> { self.transport.recv(handle, output).map_err(LanError::Transport) }
    pub fn close_tcp(&mut self, handle: SocketHandle) -> Result<(), LanError> { self.transport.close(handle).map_err(LanError::Transport) }
    pub fn release_tcp(&mut self, handle: SocketHandle) -> Result<(), LanError> { self.transport.release(handle).map_err(LanError::Transport) }
    pub fn cancel_tcp(&mut self, handle: SocketHandle) -> Result<(), LanError> { self.transport.cancel(handle).map_err(LanError::Transport) }
    pub fn tcp_status(&self, handle: SocketHandle) -> Result<SocketStatus, LanError> { self.transport.status(handle).map_err(LanError::Transport) }
    pub fn configure_tcp(&mut self, handle: SocketHandle, configuration: SocketConfiguration) -> Result<(), LanError> { self.transport.configure(handle, configuration).map_err(LanError::Transport) }
    pub fn tcp_configuration(&self, handle: SocketHandle) -> Result<SocketConfiguration, LanError> { self.transport.configuration(handle).map_err(LanError::Transport) }
    pub fn set_udp_multicast_interface(&mut self, handle: UdpSocket, address: Ipv4Address) -> Result<(), LanError> {
        let LanState::Ready(lease) = self.state else { return Err(LanError::Down); };
        if address != Ipv4Address([0; 4]) && address != lease.address { return Err(LanError::Route(RouteError::NoAddress)); }
        self.socket(handle)?.multicast_interface = (address != Ipv4Address([0; 4])).then_some(address);
        Ok(())
    }
    pub fn client_phase(&self) -> ClientPhase { self.client.phase() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame_length(outcome: LanOutcome) -> usize {
        match outcome { LanOutcome::Frame { length } => length, other => panic!("expected frame: {other:?}") }
    }
    #[test]
    fn dhcp_exchange_publishes_one_coherent_station_address_and_teardown_rejects_late_input() {
        let mut server_slots = [LeaseSlot::new(Ipv4Address([192, 168, 0, 2]))];
        let mut station_slots = [];
        let mut ap = LanRuntime::new(MacAddress([2; 6]), &mut server_slots);
        let mut station = LanRuntime::new(MacAddress([4; 6]), &mut station_slots);
        let mut first = [0; 1600];
        let mut second = [0; 1600];
        ap.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut first).unwrap();
        let discover = frame_length(station.start(LanMode::Station { transaction: TransactionId([1, 2, 3, 4]) }, Milliseconds(0), &mut first).unwrap());
        assert_eq!(station.state(), LanState::Acquiring);
        let offer = frame_length(ap.receive(&first[..discover], Milliseconds(1), &mut second).unwrap());
        let request = frame_length(station.receive(&second[..offer], Milliseconds(2), &mut first).unwrap());
        let ack = frame_length(ap.receive(&first[..request], Milliseconds(3), &mut second).unwrap());
        assert_eq!(station.receive(&second[..ack], Milliseconds(4), &mut first), Ok(LanOutcome::Ignored));
        let LanState::Ready(lease) = station.state() else { panic!("station not ready"); };
        assert_eq!(lease.address, Ipv4Address([192, 168, 0, 2]));
        assert_eq!(station.leases().lookup(ChannelSlot::parse(0).unwrap()), Some(&lease));
        let socket = station.open_udp(UdpPort(5000)).unwrap();
        station.cancel();
        assert_eq!(station.state(), LanState::Down);
        assert_eq!(station.leases().lookup(ChannelSlot::parse(0).unwrap()), None);
        assert_eq!(station.receive_udp(socket, &mut first), Err(LanError::Socket));
        assert_eq!(station.receive(&second[..ack], Milliseconds(5), &mut first), Err(LanError::Down));
    }
    #[test]
    fn udp_socket_transmits_queued_packet_after_arp_and_preserves_payload_on_small_receive() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        let mut output = [0; 1600];
        lan.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut output).unwrap();
        let socket = lan.open_udp(UdpPort(5000)).unwrap();
        let peer = UdpEndpoint { address: Ipv4Address([192, 168, 0, 2]), port: UdpPort(5001) };
        let request = frame_length(lan.send_udp(socket, peer, b"out", &mut output).unwrap());
        assert_eq!(EthernetFrame::parse(&output[..request]).unwrap().kind, EtherType::Arp);
        let mut peer_link = LinkState::new(MacAddress([4; 6]));
        peer_link.install(Lease { address: peer.address, mask: super::super::state::Ipv4Mask([255, 255, 0, 0]), router: Ipv4Address([0; 4]) });
        let mut incoming = [0; 1600];
        let reply = peer_link.receive_arp(&EthernetFrame::parse(&output[..request]).unwrap(), &mut incoming).unwrap().unwrap();
        lan.receive(&incoming[..reply], Milliseconds(1), &mut output).unwrap();
        let sent = frame_length(lan.poll(Milliseconds(2), &mut output).unwrap());
        let frame = EthernetFrame::parse(&output[..sent]).unwrap();
        assert_eq!(frame.destination, MacAddress([4; 6]));
        assert_eq!(UdpDatagram::parse_ipv4(frame.payload).unwrap().payload, b"out");
        let length = UdpDatagram { source: peer, destination: UdpEndpoint { address: Ipv4Address([192, 168, 0, 1]), port: UdpPort(5000) }, payload: b"incoming" }
            .encode_ipv4(PacketIdentification(1), HopLimit(64), &mut incoming[14..]).unwrap();
        write_header(&mut incoming, MacAddress([4; 6]), MacAddress([2; 6]), EtherType::Ipv4);
        assert_eq!(lan.receive(&incoming[..length + 14], Milliseconds(3), &mut output), Ok(LanOutcome::Buffered));
        assert_eq!(lan.receive_udp(socket, &mut [0; 1]), Err(LanError::Capacity));
        assert_eq!(lan.receive_udp(socket, &mut output), Ok(Some(UdpReceived { source: peer, length: 8 })));
        assert_eq!(&output[..8], b"incoming");
        assert_eq!(lan.receive_udp(socket, &mut output), Ok(None));
    }
    #[test]
    fn station_start_capacity_failure_leaves_lan_down() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        assert_eq!(lan.start(LanMode::Station { transaction: TransactionId([1; 4]) }, Milliseconds(0), &mut [0; 341]), Err(LanError::Capacity));
        assert_eq!(lan.state(), LanState::Down);
    }
    #[test]
    fn stopped_ap_rejects_late_peer_completion_without_republishing_a_peer() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        lan.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut []).unwrap();
        assert!(lan.peer_changed(PeerChange::Connected, MacAddress([4; 6]), TaskId(86)).unwrap().is_some());
        assert_eq!(lan.peers().count(), 1);
        lan.stop();
        assert_eq!(lan.peer_changed(PeerChange::Connected, MacAddress([4; 6]), TaskId(86)), Err(LanError::Down));
        assert_eq!(lan.peers().count(), 0);
    }
    #[test]
    fn multicast_socket_membership_controls_delivery_and_marks_outgoing_traffic() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        let mut output = [0; 1600];
        lan.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut output).unwrap();
        let socket = lan.open_udp(UdpPort(5000)).unwrap();
        let group = MulticastGroup::parse(Ipv4Address([239, 1, 2, 3])).unwrap();
        let remote = UdpEndpoint { address: Ipv4Address([192, 168, 0, 2]), port: UdpPort(5001) };
        let mut incoming = [0; 1600];
        let length = UdpDatagram { source: remote, destination: UdpEndpoint { address: group.address(), port: UdpPort(5000) }, payload: b"group" }
            .encode_ipv4(PacketIdentification(1), HopLimit(1), &mut incoming[14..]).unwrap();
        write_header(&mut incoming, MacAddress([4; 6]), MacAddress([1, 0, 94, 1, 2, 3]), EtherType::Ipv4);
        assert_eq!(lan.receive(&incoming[..length + 14], Milliseconds(1), &mut output), Ok(LanOutcome::Ignored));
        lan.join_udp_multicast(socket, group).unwrap();
        let report = frame_length(lan.poll(Milliseconds(2), &mut output).unwrap());
        assert_eq!(report, 46); assert_eq!(output[38], 0x16);
        assert_eq!(lan.receive(&incoming[..length + 14], Milliseconds(3), &mut output), Ok(LanOutcome::Buffered));
        assert_eq!(lan.receive_udp(socket, &mut output), Ok(Some(UdpReceived { source: remote, length: 5 })));
        lan.set_udp_traffic_class(socket, TrafficClass(0xb8)).unwrap();
        let sent = frame_length(lan.send_udp(socket, UdpEndpoint { address: group.address(), port: UdpPort(5001) }, b"reply", &mut output).unwrap());
        assert_eq!(output[15], 0xb8);
        assert_eq!(UdpDatagram::parse_ipv4(&output[14..sent]).unwrap().payload, b"reply");
        lan.leave_udp_multicast(socket, group).unwrap();
        assert_eq!(lan.receive(&incoming[..length + 14], Milliseconds(4), &mut output), Ok(LanOutcome::Ignored));
    }
    #[test]
    fn ephemeral_udp_avoids_live_ports_and_uses_assigned_endpoint_on_wire() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        let mut output = [0; 1600];
        lan.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut output).unwrap();
        let reserved = lan.open_udp(UdpPort(49152)).unwrap();
        let first = lan.open_udp_ephemeral().unwrap();
        let second = lan.open_udp_ephemeral().unwrap();
        let endpoint = lan.udp_local_endpoint(first).unwrap();
        assert!(endpoint.port.0 >= 49152);
        assert_ne!(endpoint.port, lan.udp_local_endpoint(reserved).unwrap().port);
        assert_ne!(endpoint.port, lan.udp_local_endpoint(second).unwrap().port);
        let length = frame_length(lan.send_udp(first, UdpEndpoint { address: Ipv4Address([255; 4]), port: UdpPort(5000) }, b"data", &mut output).unwrap());
        assert_eq!(UdpDatagram::parse_ipv4(&output[14..length]).unwrap().source, endpoint);
        lan.stop();
        assert_eq!(lan.udp_local_endpoint(first), Err(LanError::Down));
    }
    #[test]
    fn unbound_tcp_retains_its_handle_when_connect_assigns_a_local_port() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        let mut output = [0; 1600];
        lan.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut output).unwrap();
        let handle = lan.open_tcp_unbound().unwrap();
        assert!(matches!(lan.tcp_status(handle).unwrap().local, super::super::transport::SocketBinding::Unbound(Ipv4Address([192, 168, 0, 1]))));
        let peer = TcpEndpoint { address: Ipv4Address([192, 168, 0, 2]), port: TcpPort::parse(5000).unwrap() };
        lan.connect_tcp_handle(handle, peer, TcpSequence(123)).unwrap();
        let status = lan.tcp_status(handle).unwrap();
        assert_eq!(status.peer, Some(peer));
        assert!(matches!(status.local, super::super::transport::SocketBinding::Bound(endpoint) if endpoint.port.get() >= 49152));
        let length = frame_length(lan.poll(Milliseconds(1), &mut output).unwrap());
        assert_eq!(EthernetFrame::parse(&output[..length]).unwrap().kind, EtherType::Arp);
        lan.stop();
        assert_eq!(lan.tcp_status(handle), Err(LanError::Transport(TransportError::InvalidHandle)));
    }
    #[test]
    fn unbound_tcp_listeners_receive_distinct_ephemeral_bindings() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        lan.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut []).unwrap();
        let first = lan.open_tcp_unbound().unwrap();
        let second = lan.open_tcp_unbound().unwrap();
        lan.listen_tcp_handle(first, TcpSequence(1)).unwrap();
        lan.listen_tcp_handle(second, TcpSequence(2)).unwrap();
        let first_status = lan.tcp_status(first).unwrap();
        let second_status = lan.tcp_status(second).unwrap();
        assert_eq!(first_status.phase, super::super::transport::SocketPhase::Listen);
        assert_eq!(second_status.phase, super::super::transport::SocketPhase::Listen);
        assert_ne!(first_status.local.port(), second_status.local.port());
        assert!(first_status.local.port().unwrap().get() >= 49152);
    }
    #[test]
    fn udp_metadata_does_not_consume_and_truncating_receive_discards_the_remainder() {
        let mut slots = [];
        let mut lan = LanRuntime::new(MacAddress([2; 6]), &mut slots);
        let mut output = [0; 1600];
        lan.start(LanMode::AccessPoint { configuration: ServerConfiguration::camera_default() }, Milliseconds(0), &mut output).unwrap();
        let socket = lan.open_udp(UdpPort(5000)).unwrap();
        let source = UdpEndpoint { address: Ipv4Address([192, 168, 0, 2]), port: UdpPort(5001) };
        let mut incoming = [0; 1600];
        let length = UdpDatagram { source, destination: UdpEndpoint { address: Ipv4Address([192, 168, 0, 1]), port: UdpPort(5000) }, payload: &[0x5a; 1470] }
            .encode_ipv4(PacketIdentification(1), HopLimit(64), &mut incoming[14..]).unwrap();
        write_header(&mut incoming, MacAddress([4; 6]), MacAddress([2; 6]), EtherType::Ipv4);
        assert_eq!(lan.receive(&incoming[..length + 14], Milliseconds(1), &mut output), Ok(LanOutcome::Buffered));
        let queued = Some(UdpReceived { source, length: 1470 });
        assert_eq!(lan.udp_received(socket), Ok(queued));
        assert_eq!(lan.udp_received(socket), Ok(queued));
        let mut short = [0; 52];
        assert_eq!(lan.receive_udp(socket, &mut short), Err(LanError::Capacity));
        assert_eq!(lan.udp_received(socket), Ok(queued));
        assert_eq!(lan.receive_udp_with_mode(socket, &mut short, UdpReceiveMode::Truncate), Ok(Some(UdpReceived { source, length: 52 })));
        assert_eq!(short, [0x5a; 52]);
        assert_eq!(lan.udp_received(socket), Ok(None));
        assert_eq!(lan.receive_udp(socket, &mut output), Ok(None));
    }
}
