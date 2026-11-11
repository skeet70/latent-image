//! Framing and TCP execution established by X-M1 1.50 0x01405a14.
use super::super::vendor_wireless_network::{lan::{LanRuntime, LanError}, state::Ipv4Address, transport::{SocketHandle, SocketPhase, ReceiveOutcome, TcpEndpoint, TcpPort, TcpSequence, TransportError}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestIdentity(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestArgument(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestTime(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestCommand { ReadPage, BeginTransfer, DescribeTransfer, WriteBlock, FinishTransfer, TransferStatus, SetMode, QueryIdentity, QueryStatus }
impl TestCommand {
    pub const fn code(self) -> u8 { match self { Self::ReadPage => 0x40, Self::BeginTransfer => 0x50, Self::DescribeTransfer => 0x51, Self::WriteBlock => 0x52, Self::FinishTransfer => 0x53, Self::TransferStatus => 0xb0, Self::SetMode => 0xb3, Self::QueryIdentity => 0xc1, Self::QueryStatus => 0xc3 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestError { Busy, Capacity, Timeout, Closed, RemoteStatus(u8), Network(LanError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestResult { pub status: u8, pub length: usize }
#[derive(Clone, Copy)]
enum Phase { Connecting, Sending, Receiving }
struct Active { socket: SocketHandle, phase: Phase, deadline: TestTime, sent: usize, length: usize, received: usize, expected: Option<usize> }
pub struct NetworkSelftest { active: Option<Active>, session: Option<SocketHandle>, retained: bool, request: [u8; 2048], response: [u8; 1024], result: Option<Result<TestResult, TestError>> }
impl NetworkSelftest {
    pub const fn new() -> Self { Self { active: None, session: None, retained: false, request: [0; 2048], response: [0; 1024], result: None } }
    pub fn begin(&mut self, lan: &mut LanRuntime<'_>, local_port: TcpPort, sequence: TcpSequence, identity: TestIdentity, argument: TestArgument, command: TestCommand, payload: &[u8], now: TestTime) -> Result<(), TestError> {
        if self.active.is_some() { return Err(TestError::Busy); }
        let length = encode_request(identity, argument, command, payload, &mut self.request)?;
        let socket = match self.session.take() {
            Some(socket) => socket,
            None => lan.connect_tcp(local_port, TcpEndpoint { address: Ipv4Address([192, 168, 0, 251]), port: TcpPort::parse(8080).ok_or(TestError::Capacity)? }, sequence).map_err(TestError::Network)?,
        };
        self.active = Some(Active { socket, phase: Phase::Connecting, deadline: TestTime(now.0.saturating_add(10_000)), sent: 0, length, received: 0, expected: None });
        self.result = None;
        Ok(())
    }
    pub fn connect(&mut self, lan: &mut LanRuntime<'_>, local_port: TcpPort, sequence: TcpSequence, now: TestTime) -> Result<(), TestError> {
        if self.active.is_some() { return Err(TestError::Busy); }
        let socket = match self.session.take() {
            Some(socket) => socket,
            None => lan.connect_tcp(local_port, TcpEndpoint { address: Ipv4Address([192, 168, 0, 251]), port: TcpPort::parse(8080).ok_or(TestError::Capacity)? }, sequence).map_err(TestError::Network)?,
        };
        self.active = Some(Active { socket, phase: Phase::Connecting, deadline: TestTime(now.0.saturating_add(10_000)), sent: 0, length: 0, received: 0, expected: None });
        self.result = None;
        Ok(())
    }
    pub fn retain_session(&mut self) { self.retained = true; }
    pub fn close_session(&mut self, lan: &mut LanRuntime<'_>) -> Result<(), TestError> {
        if self.active.is_some() { return Err(TestError::Busy); }
        self.retained = false;
        if let Some(socket) = self.session.take() { lan.close_tcp(socket).map_err(TestError::Network)?; }
        Ok(())
    }
    pub fn poll(&mut self, lan: &mut LanRuntime<'_>, now: TestTime) -> bool {
        let Some(active) = self.active.as_mut() else { return false; };
        let result = (|| {
            if now.0 >= active.deadline.0 { return Err(TestError::Timeout); }
            let status = lan.tcp_status(active.socket).map_err(TestError::Network)?;
            if matches!(status.phase, SocketPhase::Closed | SocketPhase::Failed) { return Err(TestError::Closed); }
            if matches!(active.phase, Phase::Connecting) {
                if status.phase != SocketPhase::Established { return Ok(None); }
                if active.length == 0 { return Ok(Some(TestResult { status: 0, length: 0 })); }
                active.phase = Phase::Sending;
                active.deadline = TestTime(now.0.saturating_add(10_000));
            }
            if matches!(active.phase, Phase::Sending) {
                let count = match lan.send_tcp(active.socket, &self.request[active.sent..active.length]) {
                    Ok(count) => count,
                    Err(LanError::Transport(TransportError::WouldBlock)) => return Ok(None),
                    Err(error) => return Err(TestError::Network(error)),
                };
                active.sent += count;
                if active.sent != active.length { return Ok(None); }
                active.phase = Phase::Receiving;
                active.deadline = TestTime(now.0.saturating_add(10_000));
            }
            let limit = active.expected.unwrap_or(20).min(self.response.len());
            match lan.receive_tcp(active.socket, &mut self.response[active.received..limit]) {
                Err(LanError::Transport(TransportError::WouldBlock)) => return Ok(None),
                Err(error) => return Err(TestError::Network(error)),
                Ok(ReceiveOutcome::End) => return Err(TestError::Closed),
                Ok(ReceiveOutcome::Bytes(count)) => {
                    active.received += count;
                    if count != 0 { active.deadline = TestTime(now.0.saturating_add(10_000)); }
                }
            }
            if active.received > 4 {
                let expected = usize::from(u16::from_be_bytes([self.response[2], self.response[3]]));
                if expected > self.response.len() || expected < active.received || expected < 13 { return Err(TestError::Capacity); }
                active.expected = Some(expected);
                if active.received == expected {
                    return Ok(Some(TestResult { status: self.response[12], length: expected }));
                }
            }
            Ok(None)
        })();
        match result {
            Ok(None) => false,
            Ok(Some(result)) => {
                let terminal = if self.retained { self.session = Some(active.socket); Ok(result) } else { lan.close_tcp(active.socket).map(|()| result).map_err(TestError::Network) };
                self.active = None;
                self.result = Some(terminal);
                true
            }
            Err(error) => {
                let cleanup = lan.cancel_tcp(active.socket);
                self.active = None;
                self.result = Some(Err(cleanup.err().map_or(error, TestError::Network)));
                true
            }
        }
    }
    pub fn result(&self) -> Option<Result<TestResult, TestError>> { self.result }
    pub fn response(&self) -> Option<&[u8]> { match self.result { Some(Ok(result)) => Some(&self.response[..result.length]), _ => None } }
    pub fn cancel(&mut self, lan: &mut LanRuntime<'_>) -> Result<(), TestError> {
        if let Some(active) = self.active.take() { lan.cancel_tcp(active.socket).map_err(TestError::Network)?; }
        if let Some(socket) = self.session.take() { lan.cancel_tcp(socket).map_err(TestError::Network)?; }
        self.result = Some(Err(TestError::Closed)); Ok(())
    }
}
impl Default for NetworkSelftest { fn default() -> Self { Self::new() } }

pub fn encode_request(identity: TestIdentity, argument: TestArgument, command: TestCommand, payload: &[u8], output: &mut [u8]) -> Result<usize, TestError> {
    let length = payload.len().checked_add(16).ok_or(TestError::Capacity)?;
    if length > 2048 || output.len() < length { return Err(TestError::Capacity); }
    output[0] = 0x24; output[1] = command.code();
    output[2..4].copy_from_slice(&(length as u16).to_be_bytes());
    output[4..8].copy_from_slice(&identity.0.to_be_bytes());
    output[8..10].copy_from_slice(&argument.0.to_be_bytes());
    output[10..12].fill(0);
    output[12..12 + payload.len()].copy_from_slice(payload);
    let sum = output[..12 + payload.len()].iter().fold(0u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    output[12 + payload.len()..14 + payload.len()].copy_from_slice(&(!sum).to_be_bytes());
    output[14 + payload.len()..length].copy_from_slice(b"\r\n");
    Ok(length)
}
