//! Bounded TCP sockets and IPv4 fragment assembly. See vendor_wireless_network_transport.evidence.json.

use super::packet::{InternetChecksum, PacketError};
use super::state::Ipv4Address;
pub use super::packet::TrafficClass;
#[path = "vendor_wireless_network_transport_fragments.rs"]
pub mod fragments;
pub use fragments::{FragmentReassembler, ReassemblyOutcome};
#[path = "vendor_wireless_network_transport_policy.rs"]
pub mod policy;
pub use policy::{SackRange, SegmentSize, TcpOptions, TcpTimestamp, TimestampPair, TransportTiming, WindowShift};
use policy::Recovery;
#[path = "vendor_wireless_network_transport_socket_task.rs"]
pub mod socket_task;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportTick(pub u32);
impl TransportTick {
    pub fn reached(self, deadline: Self) -> bool { self.0.wrapping_sub(deadline.0) < 0x8000_0000 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpPort(u16);
impl TcpPort {
    pub const fn parse(port: u16) -> Option<Self> { if port == 0 { None } else { Some(Self(port)) } }
    pub const fn get(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpSequence(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpEndpoint { pub address: Ipv4Address, pub port: TcpPort }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketBinding { Unbound(Ipv4Address), Bound(TcpEndpoint) }
impl SocketBinding {
    pub const fn address(self) -> Ipv4Address { match self { Self::Unbound(address) => address, Self::Bound(endpoint) => endpoint.address } }
    pub const fn port(self) -> Option<TcpPort> { match self { Self::Unbound(_) => None, Self::Bound(endpoint) => Some(endpoint.port) } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketHandle { index: usize, generation: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketPhase { Closed, Listen, SynSent, SynReceived, Established, FinWait1, FinWait2, Closing, CloseWait, LastAck, TimeWait, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError { Packet(PacketError), Capacity, Full, InvalidHandle, AddressInUse, NotConnected, WouldBlock, Reset, TimedOut, Cancelled, InvalidState }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportOutcome { Packet { length: usize }, Buffered, Ignored }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiveOutcome { Bytes(usize), End }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketStatus { pub phase: SocketPhase, pub local: SocketBinding, pub peer: Option<TcpEndpoint>, pub failure: Option<TransportError>, pub readable: usize, pub write_pending: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UrgentMode { Inline, OutOfBand }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UrgentRead { Peek, Consume }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketBufferSize(usize);
impl SocketBufferSize {
    pub fn parse(bytes: usize) -> Option<Self> { (bytes != 0 && bytes <= 128 * 1024 * 1024).then_some(Self(bytes)) }
    pub fn get(self) -> usize { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketConfiguration {
    pub no_delay: bool,
    pub maximum_segment: Option<SegmentSize>,
    pub receive_buffer: SocketBufferSize,
    pub send_buffer: SocketBufferSize,
    pub traffic_class: TrafficClass,
    pub urgent_mode: UrgentMode,
}

const FIN: u8 = 1;
const SYN: u8 = 2;
const RST: u8 = 4;
const PSH: u8 = 8;
const ACK: u8 = 16;
const URG: u8 = 32;

struct Socket<const BUFFER: usize> {
    used: bool,
    generation: u32,
    phase: SocketPhase,
    local: SocketBinding,
    peer: Option<TcpEndpoint>,
    parent: Option<SocketHandle>,
    accepted: bool,
    send_next: u32,
    send_unacked: u32,
    receive_next: u32,
    peer_window: u32,
    maximum_segment: usize,
    peer_segment: usize,
    fast_retransmit: bool,
    peer_shift: WindowShift,
    scaled_window: bool,
    timestamps: bool,
    selective_ack: bool,
    timestamp_echo: TcpTimestamp,
    timestamp_seen: TransportTick,
    recovery: Recovery,
    sent_at: TransportTick,
    tx_sent: usize,
    send_high: u32,
    peer_sacks: [Option<SackRange>; 4],
    urgent_end: Option<u32>,
    urgent_sequence: Option<u32>,
    urgent_byte: Option<u8>,
    urgent_mode: UrgentMode,
    stream_read_sequence: u32,
    no_delay: bool,
    segment_limit: Option<SegmentSize>,
    receive_capacity: usize,
    send_capacity: usize,
    traffic_class: TrafficClass,
    tx: [u8; BUFFER],
    tx_length: usize,
    tx_start: usize,
    tx_sequence: u32,
    tx_flags: u8,
    pending: bool,
    due: TransportTick,
    attempts: u8,
    ack_pending: bool,
    probe_due: TransportTick,
    probe_pending: bool,
    rx: [u8; BUFFER],
    rx_present: [bool; BUFFER],
    pending_fin: Option<u32>,
    rx_length: usize,
    rx_head: usize,
    eof: bool,
    failure: Option<TransportError>,
    detached: bool,
}
impl<const BUFFER: usize> Socket<BUFFER> {
    const fn new() -> Self {
        Self { used: false, generation: 0, phase: SocketPhase::Closed,
            local: SocketBinding::Unbound(Ipv4Address([0; 4])), peer: None,
            parent: None, accepted: false, send_next: 0, send_unacked: 0, receive_next: 0,
            peer_window: 0, maximum_segment: 536, tx: [0; BUFFER], tx_length: 0, tx_start: 0, tx_sequence: 0,
            peer_segment: 536, fast_retransmit: false,
            peer_shift: policy::local_shift(0), scaled_window: true, timestamps: true, selective_ack: true,
            timestamp_echo: TcpTimestamp(0), timestamp_seen: TransportTick(0), recovery: Recovery::new(),
            sent_at: TransportTick(0), tx_sent: 0, send_high: 0, peer_sacks: [None; 4],
            urgent_end: None, urgent_sequence: None, urgent_byte: None, urgent_mode: UrgentMode::OutOfBand,
            stream_read_sequence: 0, rx_present: [false; BUFFER], pending_fin: None,
            no_delay: false, segment_limit: None, receive_capacity: BUFFER, send_capacity: BUFFER, traffic_class: TrafficClass(0),
            tx_flags: 0, pending: false, due: TransportTick(0), attempts: 0, ack_pending: false,
            probe_due: TransportTick(0), probe_pending: false,
            rx: [0; BUFFER], rx_length: 0, rx_head: 0, eof: false, failure: None, detached: false }
    }
    fn fail(&mut self, error: TransportError) {
        self.phase = SocketPhase::Failed;
        self.failure = Some(error);
        self.pending = false;
        self.ack_pending = false;
        self.tx[self.tx_start..self.tx_start + self.tx_length].fill(0);
        self.tx_length = 0;
    }
    fn queue(&mut self, flags: u8, length: usize, now: TransportTick) {
        self.tx_sequence = self.send_next;
        self.tx_length = length;
        self.tx_start = 0;
        self.tx_flags = flags;
        self.tx_sent = 0;
        self.peer_sacks = [None; 4];
        self.urgent_end = None;
        self.send_next = self.send_next.wrapping_add(length as u32 + u32::from(flags & SYN != 0) + u32::from(flags & FIN != 0));
        self.pending = true;
        self.attempts = 0;
        self.due = now;
    }
    fn negotiate(&mut self, options: TcpOptions, now: TransportTick) {
        self.peer_segment = options.maximum_segment.map_or(536, SegmentSize::get);
        self.maximum_segment = self.peer_segment.min(self.segment_limit.map_or(65455, SegmentSize::get)).min(65455);
        self.peer_shift = options.window_shift.unwrap_or(policy::local_shift(0));
        self.scaled_window = options.window_shift.is_some();
        self.timestamps = options.timestamp.is_some();
        self.selective_ack = options.selective_ack;
        self.timestamp_echo = options.timestamp.map_or(TcpTimestamp(0), |pair| pair.value);
        self.timestamp_seen = now;
        self.recovery.initialize(self.maximum_segment);
    }
    fn transmit_ready(&self, now: TransportTick) -> bool {
        self.pending && (now.reached(self.due) || (self.tx_flags & (SYN | FIN | RST) == 0 &&
            self.tx_sent < self.tx_length && self.tx_sent < self.recovery.congestion.min(self.peer_window as usize) &&
            (self.no_delay || self.tx_sent == 0 || self.tx_length - self.tx_sent >= self.maximum_segment)))
    }
    fn wire_options(&self, now: TransportTick, syn: bool) -> TcpOptions {
        let mut options = TcpOptions::empty();
        if syn {
            options.maximum_segment = SegmentSize::parse(self.segment_limit.map_or(1460, SegmentSize::get).min(self.receive_capacity).max(1) as u16);
            options.window_shift = self.scaled_window.then(|| policy::local_shift(self.receive_capacity));
            options.selective_ack = self.selective_ack;
        }
        if self.timestamps { options.timestamp = Some(policy::timestamp(now, self.timestamp_echo)); }
        if !syn && self.selective_ack && BUFFER != 0 {
            let tail = (self.rx_head + self.rx_length) % BUFFER;
            let mut position = 0;
            let mut count = 0;
            while position < self.receive_capacity - self.rx_length && count < 4 {
                if !self.rx_present[(tail + position) % BUFFER] { position += 1; continue; }
                let start = position;
                while position < self.receive_capacity - self.rx_length && self.rx_present[(tail + position) % BUFFER] { position += 1; }
                options.sacks[count] = Some(SackRange { start: TcpSequence(self.receive_next.wrapping_add(start as u32)),
                    end: TcpSequence(self.receive_next.wrapping_add(position as u32)) });
                count += 1;
            }
        }
        options
    }
}

pub struct TransportRuntime<const SOCKETS: usize, const BUFFER: usize> {
    sockets: [Socket<BUFFER>; SOCKETS],
    now: TransportTick,
    identification: u16,
    next_ephemeral: u16,
}
impl<const SOCKETS: usize, const BUFFER: usize> TransportRuntime<SOCKETS, BUFFER> {
    pub const fn new() -> Self { Self { sockets: [const { Socket::new() }; SOCKETS], now: TransportTick(0), identification: 0, next_ephemeral: 49152 } }
    fn socket(&self, handle: SocketHandle) -> Result<&Socket<BUFFER>, TransportError> {
        self.sockets.get(handle.index).filter(|socket| socket.used && !socket.detached && socket.generation == handle.generation).ok_or(TransportError::InvalidHandle)
    }
    fn socket_mut(&mut self, handle: SocketHandle) -> Result<&mut Socket<BUFFER>, TransportError> {
        self.sockets.get_mut(handle.index).filter(|socket| socket.used && !socket.detached && socket.generation == handle.generation).ok_or(TransportError::InvalidHandle)
    }
    fn allocate(&mut self, local: SocketBinding) -> Result<SocketHandle, TransportError> {
        let index = self.sockets.iter().position(|socket| !socket.used).ok_or(TransportError::Full)?;
        let generation = self.sockets[index].generation.wrapping_add(1);
        self.sockets[index] = Socket::new();
        let socket = &mut self.sockets[index];
        socket.used = true;
        socket.generation = generation;
        socket.local = local;
        Ok(SocketHandle { index, generation })
    }
    pub fn open_unbound(&mut self, address: Ipv4Address) -> Result<SocketHandle, TransportError> {
        if BUFFER == 0 { return Err(TransportError::Capacity); }
        self.allocate(SocketBinding::Unbound(address))
    }
    pub fn open(&mut self, local: TcpEndpoint) -> Result<SocketHandle, TransportError> {
        if BUFFER == 0 { return Err(TransportError::Capacity); }
        if self.sockets.iter().any(|socket| socket.used && socket.local.port() == Some(local.port) &&
            (socket.local.address() == local.address || socket.local.address().0 == [0; 4] || local.address.0 == [0; 4])) {
            return Err(TransportError::AddressInUse);
        }
        self.allocate(SocketBinding::Bound(local))
    }
    fn bind_handle(&mut self, handle: SocketHandle) -> Result<(), TransportError> {
        let socket = self.socket(handle)?;
        if socket.phase != SocketPhase::Closed || socket.peer.is_some() { return Err(TransportError::InvalidState); }
        let local = match socket.local {
            SocketBinding::Bound(endpoint) => endpoint,
            SocketBinding::Unbound(address) => {
                let mut selected = None;
                for _ in 0..16384 {
                    let port = TcpPort(self.next_ephemeral);
                    self.next_ephemeral = if self.next_ephemeral == 65535 { 49152 } else { self.next_ephemeral + 1 };
                    if !self.sockets.iter().any(|socket| socket.used && socket.local.port() == Some(port) &&
                        (socket.local.address() == address || socket.local.address().0 == [0; 4])) {
                        selected = Some(TcpEndpoint { address, port });
                        break;
                    }
                }
                selected.ok_or(TransportError::AddressInUse)?
            }
        };
        self.socket_mut(handle)?.local = SocketBinding::Bound(local);
        Ok(())
    }
    pub fn listen_handle(&mut self, handle: SocketHandle, sequence: TcpSequence) -> Result<(), TransportError> {
        self.bind_handle(handle)?;
        let socket = self.socket_mut(handle)?;
        socket.send_next = sequence.0;
        socket.phase = SocketPhase::Listen;
        Ok(())
    }
    pub fn connect_handle(&mut self, handle: SocketHandle, peer: TcpEndpoint, sequence: TcpSequence) -> Result<(), TransportError> {
        self.bind_handle(handle)?;
        let now = self.now;
        let socket = self.socket_mut(handle)?;
        socket.peer = Some(peer);
        socket.send_next = sequence.0;
        socket.send_unacked = sequence.0;
        socket.send_high = sequence.0;
        socket.phase = SocketPhase::SynSent;
        socket.queue(SYN, 0, now);
        Ok(())
    }
    pub fn listen(&mut self, local: TcpEndpoint, initial_sequence: TcpSequence) -> Result<SocketHandle, TransportError> {
        if BUFFER == 0 { return Err(TransportError::Capacity); }
        if self.sockets.iter().any(|socket| socket.used && socket.local.port() == Some(local.port) &&
            (socket.local.address() == local.address || socket.local.address().0 == [0; 4] || local.address.0 == [0; 4])) {
            return Err(TransportError::AddressInUse);
        }
        let handle = self.allocate(SocketBinding::Bound(local))?;
        self.sockets[handle.index].phase = SocketPhase::Listen;
        self.sockets[handle.index].send_next = initial_sequence.0;
        Ok(handle)
    }
    pub fn connect(&mut self, local: TcpEndpoint, peer: TcpEndpoint, initial_sequence: TcpSequence) -> Result<SocketHandle, TransportError> {
        if BUFFER == 0 { return Err(TransportError::Capacity); }
        if self.sockets.iter().any(|socket| socket.used && socket.local == SocketBinding::Bound(local) &&
            (socket.peer == Some(peer) || socket.phase == SocketPhase::Listen)) { return Err(TransportError::AddressInUse); }
        let handle = self.allocate(SocketBinding::Bound(local))?;
        let socket = &mut self.sockets[handle.index];
        socket.peer = Some(peer);
        socket.send_next = initial_sequence.0;
        socket.send_unacked = initial_sequence.0;
        socket.send_high = initial_sequence.0;
        socket.phase = SocketPhase::SynSent;
        socket.queue(SYN, 0, self.now);
        Ok(handle)
    }
    pub fn accept(&mut self, listener: SocketHandle) -> Result<SocketHandle, TransportError> {
        if self.socket(listener)?.phase != SocketPhase::Listen { return Err(TransportError::InvalidState); }
        let index = self.sockets.iter().position(|socket| socket.used && socket.parent == Some(listener) &&
            !socket.accepted && matches!(socket.phase, SocketPhase::Established | SocketPhase::CloseWait)).ok_or(TransportError::WouldBlock)?;
        self.sockets[index].accepted = true;
        Ok(SocketHandle { index, generation: self.sockets[index].generation })
    }
    pub fn status(&self, handle: SocketHandle) -> Result<SocketStatus, TransportError> {
        let socket = self.socket(handle)?;
        Ok(SocketStatus { phase: socket.phase, local: socket.local, peer: socket.peer, failure: socket.failure,
            readable: socket.rx_length, write_pending: socket.pending })
    }
    pub fn configuration(&self, handle: SocketHandle) -> Result<SocketConfiguration, TransportError> {
        let socket = self.socket(handle)?;
        Ok(SocketConfiguration { no_delay: socket.no_delay, maximum_segment: socket.segment_limit,
            receive_buffer: SocketBufferSize(socket.receive_capacity), send_buffer: SocketBufferSize(socket.send_capacity),
            traffic_class: socket.traffic_class, urgent_mode: socket.urgent_mode })
    }
    pub fn configure(&mut self, handle: SocketHandle, configuration: SocketConfiguration) -> Result<(), TransportError> {
        let socket = self.socket_mut(handle)?;
        if configuration.receive_buffer.0 > BUFFER || configuration.send_buffer.0 > BUFFER ||
            configuration.receive_buffer.0 < socket.rx_length || configuration.send_buffer.0 < socket.tx_start + socket.tx_length {
            return Err(TransportError::Capacity);
        }
        if configuration.receive_buffer.0 < socket.receive_capacity && socket.rx_present.iter().any(|present| *present) {
            return Err(TransportError::WouldBlock);
        }
        socket.no_delay = configuration.no_delay;
        socket.segment_limit = configuration.maximum_segment;
        socket.maximum_segment = socket.peer_segment.min(configuration.maximum_segment.map_or(65455, SegmentSize::get)).min(65455);
        socket.receive_capacity = configuration.receive_buffer.0;
        socket.send_capacity = configuration.send_buffer.0;
        socket.traffic_class = configuration.traffic_class;
        socket.urgent_mode = configuration.urgent_mode;
        socket.ack_pending = socket.peer.is_some();
        Ok(())
    }
    pub fn timing(&self, handle: SocketHandle) -> Result<TransportTiming, TransportError> { Ok(self.socket(handle)?.recovery.status()) }
    pub fn negotiated_options(&self, handle: SocketHandle) -> Result<TcpOptions, TransportError> {
        let socket = self.socket(handle)?;
        Ok(TcpOptions { maximum_segment: SegmentSize::parse(socket.maximum_segment as u16),
            window_shift: socket.scaled_window.then_some(socket.peer_shift), selective_ack: socket.selective_ack,
            timestamp: socket.timestamps.then(|| policy::timestamp(self.now, socket.timestamp_echo)), sacks: socket.peer_sacks })
    }
    pub fn send_urgent(&mut self, handle: SocketHandle, bytes: &[u8]) -> Result<usize, TransportError> {
        let length = self.send(handle, bytes)?;
        if length != 0 {
            let socket = self.socket_mut(handle)?;
            socket.urgent_end = Some(socket.send_next);
            socket.tx_flags |= URG;
        }
        Ok(length)
    }
    pub fn recv_urgent(&mut self, handle: SocketHandle, mode: UrgentRead) -> Result<u8, TransportError> {
        let socket = self.socket_mut(handle)?;
        if socket.urgent_mode == UrgentMode::Inline { return Err(TransportError::InvalidState); }
        if let Some(byte) = socket.urgent_byte {
            if mode == UrgentRead::Consume { socket.urgent_byte = None; }
            return Ok(byte);
        }
        if let Some(error) = socket.failure { return Err(error); }
        Err(TransportError::WouldBlock)
    }
    pub fn send(&mut self, handle: SocketHandle, bytes: &[u8]) -> Result<usize, TransportError> {
        let now = self.now;
        let socket = self.socket_mut(handle)?;
        if let Some(error) = socket.failure { return Err(error); }
        if !matches!(socket.phase, SocketPhase::Established | SocketPhase::CloseWait) { return Err(TransportError::NotConnected); }
        if socket.pending && socket.tx_flags & (SYN | FIN | RST) != 0 { return Err(TransportError::WouldBlock); }
        if bytes.is_empty() { return Ok(0); }
        let capacity = socket.send_capacity.saturating_sub(socket.tx_start + socket.tx_length);
        let length = bytes.len().min(capacity).min(socket.peer_window as usize).min(65455);
        if length == 0 { return Err(TransportError::WouldBlock); }
        if socket.pending {
            let tail = socket.tx_start + socket.tx_length;
            socket.tx[tail..tail + length].copy_from_slice(&bytes[..length]);
            socket.tx_length += length;
            socket.send_next = socket.send_next.wrapping_add(length as u32);
        } else {
            socket.tx[..length].copy_from_slice(&bytes[..length]);
            socket.queue(ACK | PSH, length, now);
        }
        Ok(length)
    }
    pub fn recv(&mut self, handle: SocketHandle, output: &mut [u8]) -> Result<ReceiveOutcome, TransportError> {
        let socket = self.socket_mut(handle)?;
        if output.is_empty() { return Ok(ReceiveOutcome::Bytes(0)); }
        if socket.urgent_mode == UrgentMode::OutOfBand && socket.rx_length != 0 &&
            socket.urgent_sequence == Some(socket.stream_read_sequence) {
            socket.rx[socket.rx_head] = 0;
            socket.rx_head = (socket.rx_head + 1) % BUFFER;
            socket.rx_length -= 1;
            socket.stream_read_sequence = socket.stream_read_sequence.wrapping_add(1);
            socket.urgent_sequence = None;
            socket.ack_pending = true;
        }
        if socket.rx_length != 0 {
            let until_urgent = if socket.urgent_mode == UrgentMode::OutOfBand {
                socket.urgent_sequence.filter(|sequence| !after(socket.stream_read_sequence, *sequence))
                    .map_or(usize::MAX, |sequence| sequence.wrapping_sub(socket.stream_read_sequence) as usize)
            } else { usize::MAX };
            let length = output.len().min(socket.rx_length).min(until_urgent);
            let first = length.min(BUFFER - socket.rx_head);
            output[..first].copy_from_slice(&socket.rx[socket.rx_head..socket.rx_head + first]);
            output[first..length].copy_from_slice(&socket.rx[..length - first]);
            socket.rx[socket.rx_head..socket.rx_head + first].fill(0);
            socket.rx[..length - first].fill(0);
            socket.rx_head = (socket.rx_head + length) % BUFFER;
            socket.rx_length -= length;
            socket.stream_read_sequence = socket.stream_read_sequence.wrapping_add(length as u32);
            socket.ack_pending = true;
            return Ok(ReceiveOutcome::Bytes(length));
        }
        if let Some(error) = socket.failure { return Err(error); }
        if socket.eof { return Ok(ReceiveOutcome::End); }
        Err(TransportError::WouldBlock)
    }
    pub fn close(&mut self, handle: SocketHandle) -> Result<(), TransportError> {
        let now = self.now;
        let socket = self.socket_mut(handle)?;
        match socket.phase {
            SocketPhase::Established | SocketPhase::CloseWait => {
                if socket.pending { return Err(TransportError::WouldBlock); }
                socket.phase = if socket.phase == SocketPhase::CloseWait { SocketPhase::LastAck } else { SocketPhase::FinWait1 };
                socket.queue(FIN | ACK, 0, now);
            }
            SocketPhase::Listen => {
                socket.used = false;
                for child in &mut self.sockets {
                    if child.used && child.parent == Some(handle) && !child.accepted { child.fail(TransportError::Cancelled); child.used = false; }
                }
            }
            SocketPhase::SynSent | SocketPhase::SynReceived => { socket.fail(TransportError::Cancelled); }
            SocketPhase::Closed | SocketPhase::Failed => { socket.used = false; }
            _ => return Err(TransportError::InvalidState),
        }
        Ok(())
    }
    /// 0x01311770 issues the close and returns; the slot frees when the time-wait expires.
    pub fn release(&mut self, handle: SocketHandle) -> Result<(), TransportError> {
        let now = self.now;
        let socket = self.socket_mut(handle)?;
        match socket.phase {
            SocketPhase::Established | SocketPhase::CloseWait => {
                if socket.pending { return Err(TransportError::WouldBlock); }
                socket.phase = if socket.phase == SocketPhase::CloseWait { SocketPhase::LastAck } else { SocketPhase::FinWait1 };
                socket.queue(FIN | ACK, 0, now);
            }
            SocketPhase::Listen => {
                socket.used = false;
                for child in &mut self.sockets {
                    if child.used && child.parent == Some(handle) && !child.accepted { child.fail(TransportError::Cancelled); child.used = false; }
                }
                return Ok(());
            }
            SocketPhase::SynSent | SocketPhase::SynReceived => socket.fail(TransportError::Cancelled),
            SocketPhase::Closed | SocketPhase::Failed => { socket.used = false; return Ok(()); }
            _ => {}
        }
        socket.detached = true;
        Ok(())
    }
    pub fn cancel(&mut self, handle: SocketHandle) -> Result<(), TransportError> {
        let now = self.now;
        let socket = self.socket_mut(handle)?;
        let synchronized = socket.peer.is_some() && !matches!(socket.phase, SocketPhase::SynSent | SocketPhase::Closed | SocketPhase::Failed);
        socket.fail(TransportError::Cancelled);
        if synchronized { socket.queue(RST | ACK, 0, now); }
        else { socket.used = false; }
        for child in &mut self.sockets {
            if child.used && child.parent == Some(handle) && !child.accepted { child.fail(TransportError::Cancelled); child.used = false; }
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        for socket in &mut self.sockets {
            if socket.used {
                socket.fail(TransportError::Cancelled);
                socket.rx.fill(0);
                socket.rx_present.fill(false);
                socket.urgent_byte = None;
                socket.urgent_sequence = None;
                socket.rx_length = 0;
                socket.used = false;
            }
        }
    }
    pub fn poll(&mut self, now: TransportTick, output: &mut [u8]) -> Result<TransportOutcome, TransportError> {
        self.now = now;
        for index in 0..SOCKETS {
            let socket = &mut self.sockets[index];
            if !socket.used { continue; }
            if socket.phase == SocketPhase::TimeWait && now.reached(socket.due) { socket.phase = SocketPhase::Closed; }
            if socket.detached && matches!(socket.phase, SocketPhase::Closed | SocketPhase::Failed) { socket.used = false; continue; }
            if socket.pending && now.reached(socket.due) && socket.attempts >= 6 { socket.fail(TransportError::TimedOut); }
            if socket.parent.is_some() && !socket.accepted && matches!(socket.phase, SocketPhase::Failed | SocketPhase::Closed) {
                socket.used = false;
                continue;
            }
            if socket.peer_window == 0 && !socket.pending && matches!(socket.phase, SocketPhase::Established | SocketPhase::CloseWait) &&
                now.reached(socket.probe_due) { socket.probe_pending = true; socket.ack_pending = true; }
            if socket.ack_pending || socket.transmit_ready(now) { return self.emit(index, output); }
        }
        Ok(TransportOutcome::Buffered)
    }
    fn emit(&mut self, index: usize, output: &mut [u8]) -> Result<TransportOutcome, TransportError> {
        let socket = &mut self.sockets[index];
        let peer = socket.peer.ok_or(TransportError::NotConnected)?;
        let transmit = socket.transmit_ready(self.now);
        let control = socket.tx_flags & (SYN | FIN | RST) != 0;
        let budget = socket.recovery.congestion.min(socket.peer_window as usize);
        let fresh = transmit && !control && socket.tx_sent < socket.tx_length && socket.tx_sent < budget &&
            (socket.no_delay || socket.tx_sent == 0 || socket.tx_length - socket.tx_sent >= socket.maximum_segment);
        let mut offset = if fresh { socket.tx_sent } else { 0 };
        if transmit && !fresh && !control {
            for range in socket.peer_sacks.iter().flatten() {
                let start = range.start.0.wrapping_sub(socket.tx_sequence) as usize;
                let end = range.end.0.wrapping_sub(socket.tx_sequence) as usize;
                if start <= offset && offset < end && end <= socket.tx_sent { offset = end; }
            }
            if offset == socket.tx_sent { offset = 0; }
        }
        let payload_length = if !transmit || control { 0 } else {
            (socket.tx_length - offset).min(socket.maximum_segment).min(if fresh { budget - offset } else { socket.tx_sent.saturating_sub(offset).max(1) })
        };
        let sequence = if transmit { socket.tx_sequence.wrapping_add(offset as u32) }
            else if socket.probe_pending { socket.send_high.wrapping_sub(1) } else { socket.send_high };
        let mut flags = if transmit { socket.tx_flags } else { ACK };
        if transmit && offset + payload_length < socket.tx_length { flags &= !PSH; }
        let urgent = socket.urgent_end.map_or(0, |end| end.wrapping_sub(sequence));
        if urgent == 0 || urgent > 65535 { flags &= !URG; }
        let mut option_bytes = [0; 40];
        let option_length = socket.wire_options(self.now, flags & SYN != 0).encode(&mut option_bytes);
        let shift = if flags & SYN == 0 && socket.scaled_window { policy::local_shift(socket.receive_capacity).get() } else { 0 };
        let window = ((socket.receive_capacity - socket.rx_length) >> shift).min(65535) as u16;
        let payload = &socket.tx[socket.tx_start + offset..socket.tx_start + offset + payload_length];
        let SocketBinding::Bound(local) = socket.local else { return Err(TransportError::NotConnected); };
        let length = encode_segment(local, peer, sequence, socket.receive_next, flags, window, payload,
            self.identification, &option_bytes[..option_length], urgent as u16, socket.traffic_class, output)?;
        self.identification = self.identification.wrapping_add(1);
        socket.ack_pending = false;
        if socket.probe_pending {
            socket.probe_pending = false;
            socket.probe_due = TransportTick(self.now.0.wrapping_add(1000));
        }
        if transmit {
            let first = socket.attempts == 0;
            if first || !fresh {
                if !first && !socket.fast_retransmit && flags & (SYN | FIN | RST) == 0 { socket.recovery.timeout(socket.tx_sent, socket.maximum_segment); }
                let backoff = if socket.fast_retransmit { 1 } else { 1u32 << socket.attempts.min(5) };
                socket.due = TransportTick(self.now.0.wrapping_add(socket.recovery.timeout.saturating_mul(backoff).min(60_000)));
                socket.fast_retransmit = false;
                socket.attempts += 1;
            }
            if first { socket.sent_at = self.now; }
            if !control { socket.tx_sent = socket.tx_sent.max(offset + payload_length); }
            let end = sequence.wrapping_add(payload_length as u32 + u32::from(flags & SYN != 0) + u32::from(flags & FIN != 0));
            if after(end, socket.send_high) { socket.send_high = end; }
            if flags & RST != 0 { socket.pending = false; socket.used = false; }
        }
        Ok(TransportOutcome::Packet { length })
    }
    pub fn receive_ipv4(&mut self, packet: &[u8], output: &mut [u8]) -> Result<TransportOutcome, TransportError> {
        let segment = Segment::parse(packet)?;
        if output.len() < 80 { return Err(TransportError::Capacity); }
        let index = self.sockets.iter().position(|socket| socket.used && socket.peer == Some(segment.source) &&
            socket.local == SocketBinding::Bound(segment.destination) && !matches!(socket.phase, SocketPhase::Closed | SocketPhase::Failed));
        let index = match index {
            Some(index) => index,
            None => {
                if segment.flags & RST != 0 { return Ok(TransportOutcome::Ignored); }
                let listener = self.sockets.iter().position(|socket| socket.used && socket.phase == SocketPhase::Listen &&
                    socket.local.port() == Some(segment.destination.port) && (socket.local.address() == segment.destination.address || socket.local.address().0 == [0; 4]));
                if segment.flags & (SYN | ACK) == SYN {
                    if let Some(parent) = listener {
                        let handle = self.allocate(SocketBinding::Bound(segment.destination))?;
                        let parent_handle = SocketHandle { index: parent, generation: self.sockets[parent].generation };
                        let initial_sequence = self.sockets[parent].send_next;
                        self.sockets[parent].send_next = initial_sequence.wrapping_add(65537);
                        let configuration = self.configuration(parent_handle)?;
                        self.configure(handle, configuration)?;
                        let socket = &mut self.sockets[handle.index];
                        socket.parent = Some(parent_handle);
                        socket.peer = Some(segment.source);
                        socket.receive_next = segment.sequence.wrapping_add(1);
                        socket.stream_read_sequence = socket.receive_next;
                        socket.send_next = initial_sequence;
                        socket.send_unacked = initial_sequence;
                        socket.send_high = initial_sequence;
                        socket.peer_window = u32::from(segment.window);
                        socket.negotiate(segment.options, self.now);
                        socket.phase = SocketPhase::SynReceived;
                        socket.queue(SYN | ACK, 0, self.now);
                        return self.emit(handle.index, output);
                    }
                }
                let (sequence, acknowledgment, flags) = if segment.flags & ACK != 0 { (segment.acknowledgment, 0, RST) }
                    else { (0, segment.sequence.wrapping_add(segment.payload.len() as u32 + u32::from(segment.flags & SYN != 0) + u32::from(segment.flags & FIN != 0)), RST | ACK) };
                let length = encode_segment(segment.destination, segment.source, sequence, acknowledgment, flags, 0, &[],
                    self.identification, &[], 0, TrafficClass(0), output)?;
                self.identification = self.identification.wrapping_add(1);
                return Ok(TransportOutcome::Packet { length });
            }
        };
        if self.sockets[index].pending && output.len() < 80 + self.sockets[index].tx_length.min(self.sockets[index].maximum_segment) {
            return Err(TransportError::Capacity);
        }
        let socket = &mut self.sockets[index];
        if socket.phase == SocketPhase::SynSent {
            if segment.flags & ACK != 0 && segment.acknowledgment != socket.send_high { return Ok(TransportOutcome::Ignored); }
            if segment.flags & RST != 0 {
                if segment.flags & ACK != 0 { socket.fail(TransportError::Reset); }
                return Ok(TransportOutcome::Buffered);
            }
            if segment.flags & SYN == 0 { return Ok(TransportOutcome::Ignored); }
            socket.receive_next = segment.sequence.wrapping_add(1);
            socket.stream_read_sequence = socket.receive_next;
            socket.negotiate(segment.options, self.now);
            socket.peer_window = u32::from(segment.window);
            if segment.flags & ACK != 0 {
                if socket.attempts == 1 { socket.recovery.sample(self.now.0.wrapping_sub(socket.sent_at.0)); }
                socket.pending = false;
                socket.send_unacked = segment.acknowledgment;
                socket.phase = SocketPhase::Established;
                socket.ack_pending = true;
            } else {
                socket.phase = SocketPhase::SynReceived;
                socket.tx_flags = SYN | ACK;
                socket.due = self.now;
            }
            return self.emit(index, output);
        }
        if segment.flags & RST != 0 {
            if segment.sequence == socket.receive_next { socket.fail(TransportError::Reset); }
            else { socket.ack_pending = true; return self.emit(index, output); }
            return Ok(TransportOutcome::Buffered);
        }
        if socket.timestamps {
            let Some(pair) = segment.options.timestamp else { return Ok(TransportOutcome::Ignored); };
            if after(socket.timestamp_echo.0, pair.value.0) && self.now.0.wrapping_sub(socket.timestamp_seen.0) < 2_073_600_000 {
                socket.ack_pending = true;
                return self.emit(index, output);
            }
            if !after(segment.sequence, socket.receive_next) {
                socket.timestamp_echo = pair.value;
                socket.timestamp_seen = self.now;
            }
        }
        if socket.phase == SocketPhase::SynReceived && segment.flags & SYN != 0 && segment.sequence.wrapping_add(1) == socket.receive_next {
            if segment.flags & ACK != 0 && segment.acknowledgment == socket.send_high {
                socket.pending = false;
                socket.send_unacked = segment.acknowledgment;
                socket.peer_window = u32::from(segment.window);
                socket.phase = SocketPhase::Established;
                socket.ack_pending = true;
            } else { socket.due = self.now; }
            return self.emit(index, output);
        }
        if socket.phase == SocketPhase::TimeWait && segment.flags & FIN != 0 { socket.due = TransportTick(self.now.0.wrapping_add(120_000)); }
        if segment.flags & ACK == 0 { return Ok(TransportOutcome::Ignored); }
        if after(segment.acknowledgment, socket.send_high) { socket.ack_pending = true; return self.emit(index, output); }
        let available = socket.receive_capacity - socket.rx_length;
        if (after(segment.sequence, socket.receive_next) && segment.sequence.wrapping_sub(socket.receive_next) as usize >= available) ||
            (segment.sequence != socket.receive_next && segment.payload.is_empty() && segment.flags & FIN == 0) {
            socket.ack_pending = true;
            return self.emit(index, output);
        }
        if !after(socket.send_unacked, segment.acknowledgment) {
            let shift = if socket.scaled_window && segment.flags & SYN == 0 { socket.peer_shift.get() } else { 0 };
            let window = u32::from(segment.window) << shift;
            let duplicate = socket.pending && segment.acknowledgment == socket.send_unacked &&
                window == socket.peer_window && segment.payload.is_empty() && segment.flags & (SYN | FIN) == 0;
            socket.peer_window = window;
            if socket.selective_ack {
                socket.peer_sacks = segment.options.sacks.map(|range| range.filter(|range|
                    !after(socket.send_unacked, range.start.0) && !after(range.end.0, socket.send_high)));
            }
            if duplicate && socket.recovery.duplicate(socket.tx_sent, socket.maximum_segment, socket.send_high) {
                socket.due = self.now;
                socket.fast_retransmit = true;
            }
            if socket.pending && after(segment.acknowledgment, socket.send_unacked) {
                let acknowledged = segment.acknowledgment.wrapping_sub(socket.tx_sequence) as usize;
                if let Some(pair) = segment.options.timestamp.filter(|_| socket.timestamps) {
                    let sample = self.now.0.wrapping_sub(pair.echo.0);
                    if sample <= 60_000 { socket.recovery.sample(sample); }
                } else if socket.attempts == 1 { socket.recovery.sample(self.now.0.wrapping_sub(socket.sent_at.0)); }
                socket.recovery.acknowledgment(acknowledged, segment.acknowledgment, socket.maximum_segment);
                socket.send_unacked = segment.acknowledgment;
                if segment.acknowledgment == socket.send_next {
                    socket.pending = false;
                    socket.tx[socket.tx_start..socket.tx_start + socket.tx_length].fill(0);
                    socket.tx_length = 0;
                    socket.tx_start = 0;
                    socket.tx_sent = 0;
                    socket.phase = match socket.phase {
                        SocketPhase::SynReceived => SocketPhase::Established,
                        SocketPhase::FinWait1 => SocketPhase::FinWait2,
                        SocketPhase::Closing => SocketPhase::TimeWait,
                        SocketPhase::LastAck => SocketPhase::Closed,
                        other => other,
                    };
                    if socket.phase == SocketPhase::TimeWait { socket.due = TransportTick(self.now.0.wrapping_add(120_000)); }
                } else if socket.tx_flags & (SYN | FIN) == 0 && acknowledged <= socket.tx_sent {
                    socket.tx[socket.tx_start..socket.tx_start + acknowledged].fill(0);
                    socket.tx_start += acknowledged;
                    socket.tx_length -= acknowledged;
                    socket.tx_sent -= acknowledged;
                    socket.tx_sequence = segment.acknowledgment;
                    socket.attempts = 1;
                    socket.sent_at = self.now;
                    socket.due = TransportTick(self.now.0.wrapping_add(socket.recovery.timeout));
                }
            }
        }
        if socket.phase == SocketPhase::SynReceived { return Ok(TransportOutcome::Buffered); }
        if segment.flags & URG != 0 && segment.urgent != 0 {
            let point = segment.sequence.wrapping_add(u32::from(segment.urgent) - 1);
            if !after(socket.stream_read_sequence, point) && socket.urgent_sequence.is_none_or(|old| after(point, old)) {
                socket.urgent_sequence = Some(point);
                socket.urgent_byte = None;
                let offset = point.wrapping_sub(socket.stream_read_sequence) as usize;
                if offset < socket.rx_length { socket.urgent_byte = Some(socket.rx[(socket.rx_head + offset) % BUFFER]); }
            }
        }
        if !segment.payload.is_empty() && matches!(socket.phase, SocketPhase::Established | SocketPhase::FinWait1 | SocketPhase::FinWait2) {
            let skipped = if after(socket.receive_next, segment.sequence) { socket.receive_next.wrapping_sub(segment.sequence) as usize } else { 0 };
            let relative = if after(segment.sequence, socket.receive_next) { segment.sequence.wrapping_sub(socket.receive_next) as usize } else { 0 };
            let payload = segment.payload.get(skipped..).unwrap_or(&[]);
            let length = payload.len().min(available.saturating_sub(relative));
            if length != 0 {
                let tail = (socket.rx_head + socket.rx_length + relative) % BUFFER;
                for (offset, byte) in payload[..length].iter().enumerate() {
                    let position = (tail + offset) % BUFFER;
                    if !socket.rx_present[position] { socket.rx[position] = *byte; socket.rx_present[position] = true; }
                    if socket.urgent_sequence == Some(socket.receive_next.wrapping_add((relative + offset) as u32)) { socket.urgent_byte = Some(*byte); }
                }
                while socket.rx_length < socket.receive_capacity && socket.rx_present[(socket.rx_head + socket.rx_length) % BUFFER] {
                    socket.rx_present[(socket.rx_head + socket.rx_length) % BUFFER] = false;
                    socket.rx_length += 1;
                    socket.receive_next = socket.receive_next.wrapping_add(1);
                }
            }
        }
        if segment.flags & FIN != 0 {
            let end = segment.sequence.wrapping_add(segment.payload.len() as u32);
            if !after(socket.receive_next, end) && end.wrapping_sub(socket.receive_next) as usize <= socket.receive_capacity - socket.rx_length {
                socket.pending_fin = Some(end);
            }
        }
        if socket.pending_fin == Some(socket.receive_next) {
            socket.pending_fin = None;
            socket.receive_next = socket.receive_next.wrapping_add(1);
            socket.eof = true;
            socket.phase = match socket.phase {
                SocketPhase::Established => SocketPhase::CloseWait,
                SocketPhase::FinWait1 => SocketPhase::Closing,
                SocketPhase::FinWait2 => SocketPhase::TimeWait,
                other => other,
            };
            if socket.phase == SocketPhase::TimeWait { socket.due = TransportTick(self.now.0.wrapping_add(120_000)); }
        }
        if !segment.payload.is_empty() || segment.flags & (SYN | FIN) != 0 { socket.ack_pending = true; }
        if socket.ack_pending || socket.transmit_ready(self.now) { self.emit(index, output) } else { Ok(TransportOutcome::Buffered) }
    }
}
impl<const SOCKETS: usize, const BUFFER: usize> Default for TransportRuntime<SOCKETS, BUFFER> {
    fn default() -> Self { Self::new() }
}
fn after(first: u32, second: u32) -> bool { (first.wrapping_sub(second) as i32) > 0 }

struct Segment<'a> {
    source: TcpEndpoint, destination: TcpEndpoint, sequence: u32, acknowledgment: u32,
    flags: u8, window: u16, options: TcpOptions, urgent: u16, payload: &'a [u8],
}
impl<'a> Segment<'a> {
    fn parse(packet: &'a [u8]) -> Result<Self, TransportError> {
        let (header, total) = fragments::validate_ipv4(packet)?;
        if packet[9] != 6 { return Err(TransportError::Packet(PacketError::Protocol)); }
        if u16::from_be_bytes([packet[6], packet[7]]) & 0x3fff != 0 { return Err(TransportError::Packet(PacketError::Fragment)); }
        if total - header < 20 { return Err(TransportError::Packet(PacketError::Truncated)); }
        let tcp = &packet[header..total];
        let length = usize::from(tcp[12] >> 4) * 4;
        if length < 20 || length > tcp.len() { return Err(TransportError::Packet(PacketError::HeaderLength)); }
        let source_address = Ipv4Address([packet[12], packet[13], packet[14], packet[15]]);
        let destination_address = Ipv4Address([packet[16], packet[17], packet[18], packet[19]]);
        if tcp_checksum(source_address, destination_address, tcp) != 0 { return Err(TransportError::Packet(PacketError::Checksum)); }
        let source_port = TcpPort::parse(u16::from_be_bytes([tcp[0], tcp[1]])).ok_or(TransportError::Packet(PacketError::Protocol))?;
        let destination_port = TcpPort::parse(u16::from_be_bytes([tcp[2], tcp[3]])).ok_or(TransportError::Packet(PacketError::Protocol))?;
        let options = TcpOptions::parse(&tcp[20..length])?;
        Ok(Self { source: TcpEndpoint { address: source_address, port: source_port },
            destination: TcpEndpoint { address: destination_address, port: destination_port },
            sequence: u32::from_be_bytes([tcp[4], tcp[5], tcp[6], tcp[7]]),
            acknowledgment: u32::from_be_bytes([tcp[8], tcp[9], tcp[10], tcp[11]]), flags: tcp[13],
            window: u16::from_be_bytes([tcp[14], tcp[15]]), options,
            urgent: u16::from_be_bytes([tcp[18], tcp[19]]), payload: &tcp[length..] })
    }
}
fn tcp_checksum(source: Ipv4Address, destination: Ipv4Address, bytes: &[u8]) -> u16 {
    let mut checksum = InternetChecksum::new();
    checksum.update(&source.0);
    checksum.update(&destination.0);
    checksum.update(&[0, 6]);
    checksum.update(&(bytes.len() as u16).to_be_bytes());
    checksum.update(bytes);
    checksum.finish()
}
fn encode_segment(local: TcpEndpoint, peer: TcpEndpoint, sequence: u32, acknowledgment: u32,
    flags: u8, window: u16, payload: &[u8], identification: u16, options: &[u8], urgent: u16,
    traffic_class: TrafficClass, output: &mut [u8]) -> Result<usize, TransportError> {
    let header = 40 + options.len();
    let length = payload.len().checked_add(header).ok_or(TransportError::Capacity)?;
    if length > output.len() || length > 65535 { return Err(TransportError::Capacity); }
    output[..40].fill(0);
    output[0] = 0x45;
    output[1] = traffic_class.0;
    output[2..4].copy_from_slice(&(length as u16).to_be_bytes());
    output[4..6].copy_from_slice(&identification.to_be_bytes());
    output[8] = 64;
    output[9] = 6;
    output[12..16].copy_from_slice(&local.address.0);
    output[16..20].copy_from_slice(&peer.address.0);
    let mut checksum = InternetChecksum::new();
    checksum.update(&output[..20]);
    output[10..12].copy_from_slice(&checksum.finish().to_be_bytes());
    output[20..22].copy_from_slice(&local.port.0.to_be_bytes());
    output[22..24].copy_from_slice(&peer.port.0.to_be_bytes());
    output[24..28].copy_from_slice(&sequence.to_be_bytes());
    output[28..32].copy_from_slice(&acknowledgment.to_be_bytes());
    output[32] = (((20 + options.len()) / 4) as u8) << 4;
    output[33] = flags;
    output[34..36].copy_from_slice(&window.to_be_bytes());
    output[38..40].copy_from_slice(&urgent.to_be_bytes());
    output[40..header].copy_from_slice(options);
    output[header..length].copy_from_slice(payload);
    let checksum = tcp_checksum(local.address, peer.address, &output[20..length]);
    output[36..38].copy_from_slice(&checksum.to_be_bytes());
    Ok(length)
}

#[cfg(test)]
#[path = "vendor_wireless_network_transport_tests.rs"]
mod tests;
