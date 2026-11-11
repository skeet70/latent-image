use super::{AssociationError, Milliseconds, frames::{Packet, half}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoamingState { Idle, Decision, Discovery, Roam }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Discovery { Scan, Cached }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoamingCommand { Start, Roam, Fail { connection_limit: bool }, Abort }
impl RoamingCommand {
    pub fn encode(self, sequence: u8, packet: &mut Packet) {
        let body = packet.device(0x30, sequence, 4);
        let (command, parameter): (u16, u16) = match self { Self::Start => (0,0), Self::Roam => (2,0), Self::Fail { connection_limit } => (3,u16::from(connection_limit)), Self::Abort => (4,0) };
        body[..2].copy_from_slice(&command.to_le_bytes());
        body[2..4].copy_from_slice(&parameter.to_le_bytes());
    }
}
pub struct Roaming { enabled: bool, state: RoamingState, scan_finished: Milliseconds }
impl Roaming {
    pub const fn new() -> Self { Self { enabled: false, state: RoamingState::Idle, scan_finished: Milliseconds(0) } }
    pub fn state(&self) -> RoamingState { self.state }
    pub fn configure(&mut self, enabled: bool) { self.enabled = enabled; }
    pub fn scanned(&mut self, now: Milliseconds) { if self.enabled { self.scan_finished = now; } }
    pub fn associated(&mut self, independent: bool) -> Option<RoamingCommand> {
        if self.enabled && !independent && matches!(self.state, RoamingState::Idle | RoamingState::Roam) {
            self.state = RoamingState::Decision; Some(RoamingCommand::Start)
        } else { None }
    }
    pub fn receive(&mut self, bytes: &[u8], now: Milliseconds) -> Result<Option<(u8, Discovery)>, AssociationError> {
        let event = half(bytes, 0)?;
        let signal = half(bytes, 2)? as u8;
        if event == 1 && self.enabled && self.state == RoamingState::Decision {
            self.state = RoamingState::Discovery;
            Ok(Some((signal, if now.0.wrapping_sub(self.scan_finished.0) as i32 >= 5000 { Discovery::Scan } else { Discovery::Cached })))
        } else { Ok(None) }
    }
    pub fn roam(&mut self) -> Option<RoamingCommand> {
        if self.enabled && self.state == RoamingState::Discovery { self.state = RoamingState::Roam; Some(RoamingCommand::Roam) } else { None }
    }
    pub fn failed(&mut self, connection_limit: bool) -> Option<RoamingCommand> {
        if self.enabled && self.state == RoamingState::Roam { self.state = RoamingState::Decision; Some(RoamingCommand::Fail { connection_limit }) } else { None }
    }
    pub fn abort(&mut self) -> Option<RoamingCommand> {
        if self.enabled && self.state != RoamingState::Idle { self.state = RoamingState::Idle; Some(RoamingCommand::Abort) } else { None }
    }
}
