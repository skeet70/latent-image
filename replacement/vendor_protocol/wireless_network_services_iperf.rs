use crate::vendor_wireless_network::{dhcp::Milliseconds, state::Ipv4Address};
use crate::vendor_wireless_network::lan::{LanError, LanOutcome, LanRuntime, UdpReceiveMode, UdpSocket};
use crate::vendor_wireless_network::packet::{TrafficClass as IpTrafficClass, UdpEndpoint};
use crate::vendor_wireless_network::dhcp::UdpPort;
use crate::vendor_wireless_network::transport::{ReceiveOutcome, SocketHandle, SocketPhase, TcpEndpoint, TcpPort, TcpSequence, TransportError};
use crate::vendor_wireless_network::multicast::MulticastGroup;
use crate::vendor_wireless_network::transport::{SocketConfiguration, SocketBufferSize as TransportBufferSize, UrgentMode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role { Unselected, Client(Ipv4Address), Server }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol { Tcp, Udp }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteCount(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitsPerSecond(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockSize(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Seconds(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketBufferSize(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrafficClass(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServicePort(pub u32);
impl ServicePort {
    pub const fn get(self) -> u16 { self.0 as u16 }
    fn tcp(self) -> Result<TcpPort, IperfError> { TcpPort::parse(self.get()).ok_or(IperfError::Network(LanError::Socket)) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    pub role: Role, pub protocol: Protocol, pub port: ServicePort, pub duration: Seconds,
    pub block: BlockSize, pub rate: BitsPerSecond, pub byte_limit: ByteCount,
    pub window: SocketBufferSize, pub tradeoff: bool, pub dual: bool,
    pub bind: Option<Ipv4Address>, pub tos: TrafficClass,
}
impl Default for Options {
    fn default() -> Self {
        Self { role: Role::Unselected, protocol: Protocol::Tcp, port: ServicePort(5001),
            duration: Seconds(10), block: BlockSize(8192), rate: BitsPerSecond(1_048_576),
            byte_limit: ByteCount(0), window: SocketBufferSize(0), tradeoff: false, dual: false,
            bind: None, tos: TrafficClass(0) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError { ConflictingRoles, ClientOnly, Duration, Block, Window, Number, Address, CommandLength }
impl ParseError {
    pub const fn vendor_code(self) -> u8 {
        match self { Self::ConflictingRoles => 1, Self::ClientOnly => 3, Self::Duration => 4,
            Self::Block => 6, Self::Window => 7, _ => 11 }
    }
}
fn number(bytes: &[u8]) -> Result<u64, ParseError> {
    let mut value = 0u64;
    for &byte in bytes {
        if !byte.is_ascii_digit() { return Err(ParseError::Number); }
        value = value.checked_mul(10).and_then(|v| v.checked_add(u64::from(byte - b'0'))).ok_or(ParseError::Number)?;
    }
    Ok(value)
}
fn traffic_class(bytes: &[u8]) -> Result<TrafficClass, ParseError> {
    let (digits, radix) = if bytes.starts_with(b"0x") || bytes.starts_with(b"0X") { (&bytes[2..], 16u32) }
        else if bytes.starts_with(b"0") { (bytes, 8) } else { (bytes, 10) };
    let mut value = 0u32;
    for &byte in digits {
        let digit = match byte { b'0'..=b'9' => u32::from(byte - b'0'), b'a'..=b'f' => u32::from(byte - b'a' + 10), b'A'..=b'F' => u32::from(byte - b'A' + 10), _ => break };
        if digit >= radix { break; }
        value = value.checked_mul(radix).and_then(|value| value.checked_add(digit)).ok_or(ParseError::Number)?;
    }
    Ok(TrafficClass(value as u8))
}
fn scaled(bytes: &[u8]) -> Result<u64, ParseError> {
    let factor = match bytes.last() { Some(b'K') => 1024, Some(b'M') => 1_048_576,
        Some(b'G') => 1_073_741_824, Some(b'k') => 1000, Some(b'm') => 1_000_000,
        Some(b'g') => 1_000_000_000, _ => 1 };
    let digits = if factor == 1 { bytes } else { &bytes[..bytes.len() - 1] };
    number(digits)?.checked_mul(factor).ok_or(ParseError::Number)
}
fn address(bytes: &[u8]) -> Result<Ipv4Address, ParseError> {
    let mut octets = [0; 4];
    let mut parts = bytes.split(|&byte| byte == b'.');
    for octet in &mut octets {
        let part = parts.next().ok_or(ParseError::Address)?;
        if part.is_empty() || part.len() > 3 || part.iter().any(|byte| !byte.is_ascii_digit()) { return Err(ParseError::Address); }
        *octet = u8::try_from(number(part)?).map_err(|_| ParseError::Address)?;
    }
    if parts.next().is_some() { return Err(ParseError::Address); }
    Ok(Ipv4Address(octets))
}
impl Options {
    pub fn parse(command: &[u8]) -> Result<Self, ParseError> {
        let end = command.iter().position(|&byte| byte == 0).unwrap_or(command.len());
        if end > 80 { return Err(ParseError::CommandLength); }
        let command = &command[..end];
        let mut result = Self::default();
        let mut cursor = 0;
        while cursor < command.len() {
            let flag = command[cursor]; cursor += 1;
            if !b"BSbcdlnprstuw".contains(&flag) { continue; }
            let no_argument = b"drsu".contains(&flag);
            let argument = if no_argument { &command[0..0] } else {
                while cursor < command.len() && command[cursor] != b' ' && command[cursor] != b'-' { cursor += 1; }
                if cursor < command.len() && command[cursor] == b' ' {
                    cursor += 1;
                    let start = cursor;
                    while cursor < command.len() && command[cursor] != b' ' && command[cursor] != b'-' { cursor += 1; }
                    if cursor < command.len() && command[cursor] == b'-' { &command[0..0] } else { &command[start..cursor] }
                } else { &command[0..0] }
            };
            if matches!(result.role, Role::Server) && b"bdlntuw".contains(&flag) { return Err(ParseError::ClientOnly); }
            match flag {
                b'c' => { if result.role == Role::Server { return Err(ParseError::ConflictingRoles); } result.role = Role::Client(address(argument).unwrap_or(Ipv4Address([255; 4]))); }
                b's' => { if matches!(result.role, Role::Client(_)) { return Err(ParseError::ConflictingRoles); } result.role = Role::Server; }
                b'u' => { result.protocol = Protocol::Udp; result.block = BlockSize(1470); }
                b'p' => { if let Ok(value) = number(argument).and_then(|value| u32::try_from(value).map_err(|_| ParseError::Number)) { result.port = ServicePort(value); } }
                b't' => { if let Ok(value) = number(argument).and_then(|value| u32::try_from(value).map_err(|_| ParseError::Number)) { if value > 3600 { return Err(ParseError::Duration); } result.duration = Seconds(value); } }
                b'l' => { let value = scaled(argument)?; if value > 1_048_576 { return Err(ParseError::Block); } result.block = BlockSize(value as u32); }
                b'b' => result.rate = BitsPerSecond(scaled(argument)?),
                b'n' => result.byte_limit = ByteCount(scaled(argument)?),
                b'w' => { let value = scaled(argument)?; if value > 134_217_728 { return Err(ParseError::Window); } result.window = SocketBufferSize(value as u32); }
                b'r' => result.tradeoff = true,
                b'd' => result.dual = true,
                b'B' => result.bind = Some(address(argument).unwrap_or(Ipv4Address([255; 4]))),
                b'S' => result.tos = traffic_class(argument)?,
                _ => {}
            }
        }
        Ok(result)
    }
    pub fn description(&self) -> [u8; 24] {
        let mut output = [0; 24];
        output[..4].copy_from_slice(&(if self.tradeoff { 0x8000_0000u32 } else { 0 }).to_be_bytes());
        put_word(&mut output, 4, 1);
        put_word(&mut output, 8, if self.port.0 == 0 { 5001 } else { self.port.0 });
        put_word(&mut output, 12, self.block.0);
        put_word(&mut output, 16, if self.protocol == Protocol::Udp { self.rate.0 as u32 } else { self.window.0 });
        put_word(&mut output, 20, (self.duration.0 as i32).wrapping_mul(-100) as u32);
        output
    }
}
fn put_word(output: &mut [u8], offset: usize, value: u32) { output[offset..offset + 4].copy_from_slice(&value.to_be_bytes()); }
fn word(input: &[u8], offset: usize) -> u32 { u32::from_be_bytes(input[offset..offset + 4].try_into().unwrap()) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatagramHeader { pub sequence: i32, pub seconds: u32, pub microseconds: u32 }
impl DatagramHeader {
    pub fn parse(input: &[u8]) -> Option<Self> {
        if input.len() < 12 { return None; }
        Some(Self { sequence: word(input, 0) as i32, seconds: word(input, 4), microseconds: word(input, 8) })
    }
    fn write(self, output: &mut [u8]) { put_word(output, 0, self.sequence as u32); put_word(output, 4, self.seconds); put_word(output, 8, self.microseconds); }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IperfProgress {
    pub sent: u64, pub received: u64, pub datagrams: u32, pub lost: u32, pub out_of_order: u32,
    pub elapsed_ms: u32, pub jitter_us: u64, pub reports: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IperfError { Parse(ParseError), Capacity, Busy, Network(LanError), MalformedDatagram, PeerClosed, Timeout, Cancelled, MissingRole }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IperfResult { pub progress: IperfProgress, pub failure: Option<IperfError>, pub peer: Option<ServerReport>, pub parser_failed: bool }
impl IperfResult {
    pub const fn status(self) -> i32 { if self.parser_failed { -255 } else { 0 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerReport { pub bytes: ByteCount, pub duration_us: u64, pub lost: u32, pub out_of_order: u32, pub datagrams: u32, pub jitter_us: u64 }
impl ServerReport {
    pub fn parse(input: &[u8]) -> Option<Self> {
        if input.len() < 52 || word(input, 12) & 0x8000_0000 == 0 { return None; }
        Some(Self { bytes: ByteCount((u64::from(word(input, 16)) << 32) | u64::from(word(input, 20))),
            duration_us: u64::from(word(input, 24)) * 1_000_000 + u64::from(word(input, 28)),
            lost: word(input, 32), out_of_order: word(input, 36), datagrams: word(input, 40),
            jitter_us: u64::from(word(input, 44)) * 1_000_000 + u64::from(word(input, 48)) })
    }
    fn write(self, header: DatagramHeader, output: &mut [u8]) {
        header.write(output); put_word(output, 12, 0x8000_0000);
        put_word(output, 16, (self.bytes.0 >> 32) as u32); put_word(output, 20, self.bytes.0 as u32);
        put_word(output, 24, (self.duration_us / 1_000_000) as u32); put_word(output, 28, (self.duration_us % 1_000_000) as u32);
        put_word(output, 32, self.lost); put_word(output, 36, self.out_of_order); put_word(output, 40, self.datagrams);
        put_word(output, 44, (self.jitter_us / 1_000_000) as u32); put_word(output, 48, (self.jitter_us % 1_000_000) as u32);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IperfPoll { Pending, Frame { length: usize }, Complete(IperfResult) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, Open, Connect, Accept, Description, Transfer, Finish, AwaitReport, ReplyReport, ReplyWait, BoundComplete, Turnaround, Closing, Complete }

pub struct IperfRuntime<'a> {
    buffer: &'a mut [u8], options: Options, phase: Phase, tcp: Option<SocketHandle>, listener: Option<SocketHandle>, udp: Option<UdpSocket>,
    peer: Option<UdpEndpoint>, progress: IperfProgress, result: Option<IperfResult>, began: Milliseconds, deadline: Milliseconds,
    next_send_us: u64, leg_bytes: u64, offset: usize, sequence: i32, last_sequence: i32,
    last_transit: i64, retry: u8, second_leg: bool, last_header: DatagramHeader,
    configuration_failure: Option<IperfError>,
}
impl<'a> IperfRuntime<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, options: Options::default(), phase: Phase::Idle, tcp: None, listener: None, udp: None, peer: None,
            configuration_failure: None,
            progress: IperfProgress::default(), result: None, began: Milliseconds(0), deadline: Milliseconds(0),
            next_send_us: 0, leg_bytes: 0, offset: 0, sequence: 0, last_sequence: 0,
            last_transit: 0, retry: 0, second_leg: false, last_header: DatagramHeader { sequence: 0, seconds: 0, microseconds: 0 } }
    }
    pub fn options(&self) -> &Options { &self.options }
    pub fn progress(&self) -> IperfProgress { self.progress }
    pub fn result(&self) -> Option<IperfResult> { self.result }
    pub fn start(&mut self, command: &[u8], now: Milliseconds) -> Result<(), IperfError> {
        if !matches!(self.phase, Phase::Idle | Phase::Complete) { return Err(IperfError::Busy); }
        self.result = None;
        self.configuration_failure = None;
        self.progress = IperfProgress::default();
        self.options = match Options::parse(command) {
            Ok(options) => options,
            Err(error) => { let error = IperfError::Parse(error); self.result = Some(IperfResult { progress: self.progress, failure: Some(error), peer: None, parser_failed: true }); self.phase = Phase::Complete; return Err(error); }
        };
        let required = (self.options.block.0 as usize).max(if self.options.tradeoff { 1470 } else { 52 });
        if self.buffer.len() < required {
            self.result = Some(IperfResult { progress: self.progress, failure: Some(IperfError::Capacity), peer: None, parser_failed: true });
            self.phase = Phase::Complete; return Err(IperfError::Capacity);
        }
        self.began = now; self.deadline = now; self.next_send_us = 0; self.leg_bytes = 0;
        self.sequence = 0; self.last_sequence = 0; self.last_transit = 0; self.offset = 0; self.retry = 0; self.second_leg = false;
        self.peer = match self.options.role { Role::Client(address) => Some(UdpEndpoint { address, port: UdpPort(self.options.port.get()) }), _ => None };
        self.phase = Phase::Open;
        Ok(())
    }
    fn configure_tcp(&mut self, lan: &mut LanRuntime<'_>, socket: SocketHandle) {
        let size = if self.options.window.0 == 0 { 2048 } else { self.options.window.0 as usize };
        let configuration = SocketConfiguration {
            no_delay: false, maximum_segment: None,
            receive_buffer: TransportBufferSize::parse(size).unwrap(),
            send_buffer: TransportBufferSize::parse(size).unwrap(),
            traffic_class: IpTrafficClass(self.options.tos.0), urgent_mode: UrgentMode::OutOfBand,
        };
        if let Err(error) = lan.configure_tcp(socket, configuration) { self.configuration_failure = Some(IperfError::Network(error)); }
    }
    fn teardown(&mut self, lan: &mut LanRuntime<'_>, cancel: bool) {
        if let Some(socket) = self.udp.take() { let _ = lan.close_udp(socket); }
        if let Some(socket) = self.listener.take() { let _ = lan.cancel_tcp(socket); }
        if let Some(socket) = self.tcp.take() { if cancel { let _ = lan.cancel_tcp(socket); } else { let _ = lan.release_tcp(socket); } }
    }
    fn complete(&mut self, lan: &mut LanRuntime<'_>, failure: Option<IperfError>) -> IperfPoll {
        let failure = failure.or(self.configuration_failure);
        self.teardown(lan, failure.is_some()); self.phase = Phase::Complete;
        self.progress.reports = self.progress.reports.saturating_add(1);
        let result = IperfResult { progress: self.progress, failure, peer: self.result.and_then(|result| result.peer), parser_failed: false };
        self.result = Some(result); IperfPoll::Complete(result)
    }
    pub fn cancel(&mut self, lan: &mut LanRuntime<'_>) -> IperfResult {
        if self.phase == Phase::Complete { return self.result.unwrap(); }
        self.complete(lan, Some(IperfError::Cancelled)); self.result.unwrap()
    }
    pub fn network_failed(&mut self, lan: &mut LanRuntime<'_>, error: LanError) -> IperfResult {
        if self.phase == Phase::Complete { return self.result.unwrap(); }
        self.complete(lan, Some(IperfError::Network(error))); self.result.unwrap()
    }
    fn transfer_started(&mut self, now: Milliseconds) {
        self.began = now;
        self.next_send_us = if self.options.protocol == Protocol::Udp && self.options.rate.0 != 0 { u64::from(self.options.block.0) * 8_000_000 / self.options.rate.0 } else { 0 };
        self.phase = Phase::Transfer; self.offset = 0;
        for (index, byte) in self.buffer[..self.options.block.0 as usize].iter_mut().enumerate() { *byte = b'0' + if index == 0 { 0 } else { ((index - 1) % 9 + 1) as u8 }; }
        if self.options.protocol == Protocol::Udp { self.buffer[12..36].copy_from_slice(&self.options.description()); }
    }
    fn sending(&self) -> bool { matches!(self.options.role, Role::Client(_)) }
    fn ended(&self, now: Milliseconds) -> bool {
        self.options.byte_limit.0 != 0 && self.progress.sent.saturating_add(self.progress.received).saturating_sub(self.leg_bytes) >= self.options.byte_limit.0
            || self.options.byte_limit.0 == 0 && (self.sending() || self.options.protocol == Protocol::Udp)
                && self.options.duration.0 != 0 && now.0.wrapping_sub(self.began.0) > self.options.duration.0 * 1000
    }
    pub fn poll(&mut self, lan: &mut LanRuntime<'_>, now: Milliseconds, output: &mut [u8]) -> Result<IperfPoll, IperfError> {
        match self.poll_inner(lan, now, output) {
            Ok(value) => Ok(value),
            Err(error) => Ok(self.complete(lan, Some(error))),
        }
    }
    fn poll_inner(&mut self, lan: &mut LanRuntime<'_>, now: Milliseconds, output: &mut [u8]) -> Result<IperfPoll, IperfError> {
        if self.phase == Phase::BoundComplete { return self.finish_leg(lan, now); }
        if self.phase == Phase::Complete { return Ok(IperfPoll::Complete(self.result.unwrap())); }
        if self.phase == Phase::Idle { return Ok(IperfPoll::Pending); }
        if matches!(self.phase, Phase::Description | Phase::Transfer | Phase::Finish) {
            if let Some(socket) = self.tcp {
                let status = lan.tcp_status(socket).map_err(IperfError::Network)?;
                if let Some(error) = status.failure { return Err(IperfError::Network(LanError::Transport(error))); }
                if status.phase == SocketPhase::Closed { return Err(IperfError::PeerClosed); }
            }
        }
        if self.phase == Phase::Open {
            if self.options.role == Role::Unselected { return Ok(self.complete(lan, Some(IperfError::MissingRole))); }
            if self.options.protocol == Protocol::Udp {
                self.udp = Some(if (self.options.role == Role::Server || self.options.bind.is_some()) && self.options.port.get() != 0 {
                    lan.open_udp(UdpPort(self.options.port.get()))
                } else { lan.open_udp_ephemeral() }.map_err(IperfError::Network)?);
                let socket = self.udp.unwrap();
                if let Some(group) = self.options.bind.and_then(MulticastGroup::parse) {
                    if let Err(error) = lan.join_udp_multicast(socket, group) { self.configuration_failure = Some(IperfError::Network(error)); }
                }
                if let Err(error) = lan.set_udp_traffic_class(socket, IpTrafficClass(self.options.tos.0)) { self.configuration_failure = Some(IperfError::Network(error)); }
                if self.options.role == Role::Server { self.phase = Phase::Accept; } else { self.transfer_started(now); }
            } else if self.options.role == Role::Server {
                self.listener = Some(if self.options.port.get() == 0 { lan.open_tcp_unbound() }
                    else { lan.open_tcp(self.options.port.tcp()?) }.map_err(IperfError::Network)?);
                let socket = self.listener.unwrap();
                self.configure_tcp(lan, socket);
                lan.listen_tcp_handle(socket, TcpSequence(now.0)).map_err(IperfError::Network)?;
                self.phase = Phase::Accept;
            } else {
                let Role::Client(address) = self.options.role else { unreachable!() };
                let peer = TcpEndpoint { address, port: self.options.port.tcp()? };
                self.tcp = Some(if self.options.bind.is_some() && self.options.port.get() != 0 {
                    lan.open_tcp(self.options.port.tcp()?)
                } else { lan.open_tcp_unbound() }.map_err(IperfError::Network)?);
                let socket = self.tcp.unwrap();
                self.configure_tcp(lan, socket);
                lan.connect_tcp_handle(socket, peer, TcpSequence(now.0)).map_err(IperfError::Network)?;
                self.phase = Phase::Connect;
            }
            return Ok(IperfPoll::Pending);
        }
        if self.phase == Phase::Connect {
            let status = lan.tcp_status(self.tcp.unwrap()).map_err(IperfError::Network)?;
            if let Some(error) = status.failure { return Err(IperfError::Network(LanError::Transport(error))); }
            if status.phase == SocketPhase::Established { self.phase = Phase::Description; self.offset = 0; }
            return Ok(IperfPoll::Pending);
        }
        if self.phase == Phase::Accept && self.options.protocol == Protocol::Tcp {
            match lan.accept_tcp(self.listener.unwrap()) {
                Ok(socket) => { self.tcp = Some(socket); self.phase = Phase::Description; self.offset = 0; }
                Err(error) if waiting(error) => {}, Err(error) => return Err(IperfError::Network(error)),
            }
            return Ok(IperfPoll::Pending);
        }
        if self.phase == Phase::Description {
            let socket = self.tcp.unwrap();
            if self.options.role == Role::Server {
                match lan.receive_tcp(socket, &mut self.buffer[self.offset..24]) {
                    Ok(ReceiveOutcome::Bytes(length)) => self.offset += length,
                    Ok(ReceiveOutcome::End) => return Err(IperfError::PeerClosed),
                    Err(error) if waiting(error) => {}, Err(error) => return Err(IperfError::Network(error)),
                }
            } else {
                match lan.send_tcp(socket, &self.options.description()[self.offset..]) {
                    Ok(length) => self.offset += length,
                    Err(error) if waiting(error) => {}, Err(error) => return Err(IperfError::Network(error)),
                }
            }
            if self.offset == 24 { self.transfer_started(now); }
            return Ok(IperfPoll::Pending);
        }
        if matches!(self.phase, Phase::Accept | Phase::Transfer) {
            if self.phase == Phase::Transfer {
                self.progress.elapsed_ms = now.0.wrapping_sub(self.began.0);
                if (self.sending() && self.offset == 0 || self.options.protocol == Protocol::Tcp && !self.sending()) && self.ended(now) {
                    self.phase = Phase::Finish;
                    return Ok(IperfPoll::Pending);
                }
            }
            if self.options.protocol == Protocol::Tcp { return self.transfer_tcp(lan, now); }
            return self.transfer_udp(lan, now, output);
        }
        if self.phase == Phase::Finish {
            if self.options.protocol == Protocol::Tcp {
                if self.tcp.is_some_and(|socket| lan.tcp_status(socket).is_ok_and(|status| status.write_pending)) { return Ok(IperfPoll::Pending); }
                if let Some(socket) = self.tcp { lan.close_tcp(socket).map_err(IperfError::Network)?; }
                self.phase = Phase::Closing; return Ok(IperfPoll::Pending);
            }
            if self.options.bind.is_some() {
                match lan.send_udp(self.udp.unwrap(), self.peer.unwrap(), &[14], output) {
                    Ok(outcome) => { self.phase = Phase::BoundComplete; return Ok(frame(outcome)); }
                    Err(error) if waiting(error) => return Ok(IperfPoll::Pending),
                    Err(error) => return Err(IperfError::Network(error)),
                }
            }
            if self.retry == 0 { self.last_header = DatagramHeader { sequence: -self.sequence.max(1), seconds: now.0 / 1000, microseconds: now.0 % 1000 * 1000 }; }
            self.last_header.write(self.buffer);
            match lan.send_udp(self.udp.unwrap(), self.peer.unwrap(), &self.buffer[..self.options.block.0 as usize], output) {
                Ok(outcome) => { self.retry += 1; self.deadline = Milliseconds(now.0.wrapping_add(250)); self.phase = Phase::AwaitReport; return Ok(frame(outcome)); }
                Err(error) if waiting(error) => return Ok(IperfPoll::Pending), Err(error) => return Err(IperfError::Network(error)),
            }
        }
        if self.phase == Phase::AwaitReport {
            if let Some(received) = lan.receive_udp_with_mode(self.udp.unwrap(), &mut self.buffer[..self.options.block.0 as usize], UdpReceiveMode::Truncate).map_err(IperfError::Network)? {
                if Some(received.source) == self.peer {
                    if received.length >= 52 {
                        self.result = Some(IperfResult { progress: self.progress, failure: None, peer: ServerReport::parse(&self.buffer[..received.length]), parser_failed: false });
                        return self.finish_leg(lan, now);
                    }
                }
            }
            if due(now, self.deadline) {
                if self.retry == 10 { return Err(IperfError::Timeout); }
                self.phase = Phase::Finish;
            }
            return Ok(IperfPoll::Pending);
        }
        if self.phase == Phase::ReplyReport {
            let report = ServerReport { bytes: ByteCount(self.progress.received), duration_us: u64::from(self.progress.elapsed_ms / 1000) * 1_000_000,
                lost: self.progress.lost, out_of_order: self.progress.out_of_order, datagrams: self.last_sequence as u32,
                jitter_us: self.progress.jitter_us / 1_000_000 * 1_000_000 };
            let header = DatagramHeader { sequence: -self.last_sequence, ..self.last_header };
            report.write(header, self.buffer);
            match lan.send_udp(self.udp.unwrap(), self.peer.unwrap(), &self.buffer[..self.options.block.0 as usize], output) {
                Ok(outcome) => { self.deadline = Milliseconds(now.0.wrapping_add(250)); self.phase = Phase::ReplyWait; return Ok(frame(outcome)); }
                Err(error) if waiting(error) => return Ok(IperfPoll::Pending), Err(error) => return Err(IperfError::Network(error)),
            }
        }
        if self.phase == Phase::ReplyWait {
            if let Some(received) = lan.receive_udp_with_mode(self.udp.unwrap(), &mut self.buffer[..self.options.block.0 as usize], UdpReceiveMode::Truncate).map_err(IperfError::Network)? {
                if Some(received.source) == self.peer { return self.finish_leg(lan, now); }
            }
            if due(now, self.deadline) { return self.finish_leg(lan, now); }
            return Ok(IperfPoll::Pending);
        }
        if self.phase == Phase::Turnaround {
            if due(now, self.deadline) { self.phase = Phase::Open; }
            return Ok(IperfPoll::Pending);
        }
        if self.phase == Phase::Closing {
            let socket = self.tcp.unwrap();
            let status = lan.tcp_status(socket).map_err(IperfError::Network)?;
            if matches!(status.phase, SocketPhase::TimeWait | SocketPhase::Closed) { return self.finish_leg(lan, now); }
            if let Some(error) = status.failure { return Err(IperfError::Network(LanError::Transport(error))); }
            return Ok(IperfPoll::Pending);
        }
        Ok(IperfPoll::Pending)
    }
    fn transfer_tcp(&mut self, lan: &mut LanRuntime<'_>, now: Milliseconds) -> Result<IperfPoll, IperfError> {
        let socket = self.tcp.unwrap();
        if self.sending() {
            let end = (self.offset + 2048).min(self.options.block.0 as usize);
            match lan.send_tcp(socket, &self.buffer[self.offset..end]) {
                Ok(length) => { self.offset += length; self.progress.sent += length as u64; if self.offset == self.options.block.0 as usize { self.offset = 0; } }
                Err(error) if waiting(error) => {}, Err(error) => return Err(IperfError::Network(error)),
            }
        } else {
            match lan.receive_tcp(socket, self.buffer) {
                Ok(ReceiveOutcome::Bytes(length)) => self.progress.received += length as u64,
                Ok(ReceiveOutcome::End) => { self.phase = Phase::Finish; return Ok(IperfPoll::Pending); }
                Err(error) if waiting(error) => {}, Err(error) => return Err(IperfError::Network(error)),
            }
            if self.ended(now) { self.phase = Phase::Finish; }
        }
        Ok(IperfPoll::Pending)
    }
    fn transfer_udp(&mut self, lan: &mut LanRuntime<'_>, now: Milliseconds, output: &mut [u8]) -> Result<IperfPoll, IperfError> {
        if self.sending() && self.phase != Phase::Accept {
            if u64::from(now.0.wrapping_sub(self.began.0)) * 1000 < self.next_send_us { return Ok(IperfPoll::Pending); }
            if self.options.block.0 < 12 { return Err(IperfError::MalformedDatagram); }
            DatagramHeader { sequence: self.sequence, seconds: now.0 / 1000, microseconds: now.0 % 1000 * 1000 }.write(self.buffer);
            match lan.send_udp(self.udp.unwrap(), self.peer.unwrap(), &self.buffer[..self.options.block.0 as usize], output) {
                Ok(outcome) => {
                    self.sequence = self.sequence.checked_add(1).ok_or(IperfError::Capacity)?;
                    self.progress.sent += u64::from(self.options.block.0); self.progress.datagrams += 1;
                    let delay = if self.options.rate.0 == 0 { 0 } else { u64::from(self.options.block.0) * 8_000_000 / self.options.rate.0 };
                    self.next_send_us = u64::from(now.0.wrapping_sub(self.began.0)) * 1000 + delay;
                    return Ok(frame(outcome));
                }
                Err(error) if waiting(error) => return Ok(IperfPoll::Pending), Err(error) => return Err(IperfError::Network(error)),
            }
        }
        let Some(received) = lan.receive_udp_with_mode(self.udp.unwrap(), &mut self.buffer[..self.options.block.0 as usize], UdpReceiveMode::Truncate).map_err(IperfError::Network)? else {
            if self.phase == Phase::Transfer && self.ended(now) {
                if self.options.bind.is_some() { return self.finish_leg(lan, now); }
                self.phase = Phase::ReplyReport;
            }
            return Ok(IperfPoll::Pending);
        };
        if self.peer.is_some_and(|peer| peer != received.source) { return Ok(IperfPoll::Pending); }
        let header = DatagramHeader::parse(&self.buffer[..received.length]).ok_or(IperfError::MalformedDatagram)?;
        if self.phase == Phase::Accept {
            self.peer = Some(received.source); self.began = now; self.phase = Phase::Transfer; self.last_header = header;
            return Ok(IperfPoll::Pending);
        }
        self.progress.elapsed_ms = now.0.wrapping_sub(self.began.0);
        self.progress.received += received.length as u64; self.last_header = header;
        if header.sequence < 0 {
            self.progress.out_of_order = self.progress.out_of_order.saturating_add(1);
            if received.length >= 16 && word(self.buffer, 12) == 0x8000_0000 { self.options.tradeoff = true; }
            if self.options.bind.is_some() { return self.finish_leg(lan, now); }
            self.phase = Phase::ReplyReport; return Ok(IperfPoll::Pending);
        }
        self.progress.datagrams += 1;
        if header.sequence < self.last_sequence + 1 { self.progress.out_of_order += 1; }
        else { self.progress.lost += (header.sequence - self.last_sequence - 1) as u32; self.last_sequence = header.sequence; }
        let transit = i64::from(now.0) * 1000 - i64::from(header.seconds) * 1_000_000 - i64::from(header.microseconds);
        let variation = transit.abs_diff(self.last_transit); self.last_transit = transit;
        self.progress.jitter_us = if variation >= self.progress.jitter_us { self.progress.jitter_us + (variation - self.progress.jitter_us) / 16 }
            else { self.progress.jitter_us - (self.progress.jitter_us - variation) / 16 };
        Ok(IperfPoll::Pending)
    }
    fn finish_leg(&mut self, lan: &mut LanRuntime<'_>, now: Milliseconds) -> Result<IperfPoll, IperfError> {
        if self.options.tradeoff && self.options.protocol == Protocol::Udp && !self.second_leg {
            self.teardown(lan, true); self.second_leg = true; self.retry = 0; self.sequence = 0; self.last_sequence = 0;
            self.leg_bytes = self.progress.sent.saturating_add(self.progress.received);
            self.options.role = match self.options.role { Role::Client(_) => Role::Server, Role::Server => Role::Client(self.peer.unwrap().address), Role::Unselected => return Err(IperfError::MissingRole) };
            self.peer = match self.options.role {
                Role::Client(address) => Some(UdpEndpoint { address, port: UdpPort(self.options.port.get()) }),
                _ => None,
            };
            self.options.block = BlockSize(1470); self.deadline = Milliseconds(now.0.wrapping_add(if matches!(self.options.role, Role::Client(_)) { 500 } else { 0 }));
            self.phase = Phase::Turnaround; return Ok(IperfPoll::Pending);
        }
        Ok(self.complete(lan, None))
    }
}
fn due(now: Milliseconds, deadline: Milliseconds) -> bool { now.0.wrapping_sub(deadline.0) < 0x8000_0000 }
fn waiting(error: LanError) -> bool { matches!(error, LanError::Busy | LanError::Transport(TransportError::WouldBlock | TransportError::Full)) }
fn frame(outcome: LanOutcome) -> IperfPoll { match outcome { LanOutcome::Frame { length } => IperfPoll::Frame { length }, _ => IperfPoll::Pending } }

#[cfg(test)]
#[path = "wireless_network_services_iperf_tests.rs"]
mod tests;
