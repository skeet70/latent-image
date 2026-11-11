//! X-M1 1.50 service at 0x013ef2f0. The firmware labels this mode STA.

use crate::vendor_wireless_network::{MacAddress, Ssid};
use crate::wireless_association::station::{StationCompletion, StationError, StationOperationId, StationOutcome, StationProfile, StationRequest, StationRuntime};
use crate::vendor_wireless_service_task::{CompletionStatus, PayloadError, TaskMessage};
pub use crate::vendor_wireless_network::dhcp::Milliseconds;
#[path = "vendor_wireless_service_task_ap_collect.rs"]
pub mod collect;
#[path = "vendor_wireless_service_task_ap_packets.rs"]
pub mod packets;
#[path = "vendor_wireless_service_task_ap_socket.rs"]
pub mod socket;
#[cfg(test)]
#[path = "vendor_wireless_service_task_ap_tests.rs"]
mod tests;


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ModeState { Idle, Scanning, Discovering, Listing, Connecting, Ready, Cancelling }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeCompletionTarget { Configure, Reset, Start, Discover, Register, Import }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeNotification { pub target: ModeCompletionTarget, pub status: CompletionStatus, pub state: ModeState }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationStatus { Selected, NotFound, Rejected }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Security { Open, Wep, Wpa, Rsn }
impl Security {
    fn from_wire(value: u32) -> Self { match value { 2 => Self::Wep, 3 => Self::Wpa, 4 => Self::Rsn, _ => Self::Open } }
    fn profile_code(self) -> u8 { match self { Self::Open => 0, Self::Wep => 1, Self::Wpa => 2, Self::Rsn => 3 } }
}
#[derive(Clone, Eq, PartialEq)]
struct DiscoveryKey([u8; 64]);
impl Drop for DiscoveryKey { fn drop(&mut self) { use xm1_wireless_crypto::zeroize::Zeroize; self.0.zeroize(); } }
#[derive(Clone, Eq, PartialEq)]
pub struct DiscoveryRecord { pub ssid: Ssid, pub security: Security, key: DiscoveryKey }
impl DiscoveryRecord {
    fn profile(&self) -> Result<StationProfile, ModeError> {
        StationProfile::parse(self.ssid, self.security.profile_code(), nul_terminated(&self.key.0), 0)
            .map_err(|error| ModeError::Station(StationError::Profile(error)))
    }
}
#[derive(Debug)]
pub enum ModeError { Busy, Unconfigured, NotificationCapacity, Payload(PayloadError), Station(StationError), InvalidSsid, Socket(socket::SocketModeError) }
impl From<StationError> for ModeError { fn from(error: StationError) -> Self { Self::Station(error) } }
impl From<PayloadError> for ModeError { fn from(error: PayloadError) -> Self { Self::Payload(error) } }
impl From<socket::SocketModeError> for ModeError { fn from(error: socket::SocketModeError) -> Self { Self::Socket(error) } }

pub struct ModeRuntime {
    state: ModeState,
    profile: Option<StationProfile>,
    prefix: Option<Ssid>,
    operation: Option<StationOperationId>,
    retry_at: Option<Milliseconds>,
    scan_started: Milliseconds,
    cancel_target: ModeCompletionTarget,
    records: [Option<DiscoveryRecord>; 16],
    count: usize,
    selected: Option<MacAddress>,
    scan_verified: bool,
    configuration_status: ConfigurationStatus,
    notifications: [Option<ModeNotification>; 16],
    read: usize,
    length: usize,
    socket: socket::SocketModeRuntime,
}
impl Default for ModeRuntime { fn default() -> Self { Self::new() } }
impl ModeRuntime {
    pub const fn new() -> Self {
        Self { state: ModeState::Idle, profile: None, prefix: None, operation: None, retry_at: None, scan_started: Milliseconds(0), cancel_target: ModeCompletionTarget::Configure,
            records: [const { None }; 16], count: 0, selected: None, scan_verified: false, configuration_status: ConfigurationStatus::NotFound,
            notifications: [None; 16], read: 0, length: 0, socket: socket::SocketModeRuntime::new() }
    }
    pub fn reset(&mut self) { *self = Self::new(); }
    pub fn initialize(&mut self) -> Result<(), ModeError> {
        if self.operation.is_some() { return Err(ModeError::Busy); }
        self.reset(); Ok(())
    }
    pub const fn state(&self) -> ModeState { self.state }
    pub const fn configuration_status(&self) -> ConfigurationStatus { self.configuration_status }
    pub const fn selected(&self) -> Option<MacAddress> { self.selected }
    pub fn records(&self) -> impl Iterator<Item = &DiscoveryRecord> { self.records[..self.count].iter().flatten() }
    pub fn profile(&self) -> Option<&StationProfile> { self.profile.as_ref() }
    pub fn configure(&mut self, profile: StationProfile, prefix: Ssid) -> Result<(), ModeError> {
        if self.state != ModeState::Idle { return Err(ModeError::Busy); }
        self.profile = Some(profile); self.prefix = Some(prefix); Ok(())
    }
    pub fn notification(&mut self) -> Option<ModeNotification> {
        if self.length == 0 { return self.socket.notification(); }
        let notification = self.notifications[self.read].take(); self.read = (self.read + 1) % self.notifications.len(); self.length -= 1;
        notification
    }
    fn reserve(&self) -> Result<(), ModeError> {
        if self.length == self.notifications.len() { Err(ModeError::NotificationCapacity) } else { Ok(()) }
    }
    fn notify(&mut self, target: ModeCompletionTarget, status: CompletionStatus) {
        let index = (self.read + self.length) % self.notifications.len();
        self.notifications[index] = Some(ModeNotification { target, status, state: self.state }); self.length += 1;
    }
    pub fn start(&mut self, station: &mut StationRuntime, now: Milliseconds) -> Result<bool, ModeError> {
        if self.state != ModeState::Idle { return Ok(false); }
        let operation = station.submit(StationRequest::Scan)?;
        self.records.fill(None); self.count = 0; self.selected = None; self.scan_verified = false; self.scan_started = now;
        self.operation = Some(operation); self.state = ModeState::Scanning; Ok(true)
    }
    pub fn list(&mut self, station: &mut StationRuntime) -> Result<bool, ModeError> {
        if self.state != ModeState::Idle { return Ok(false); }
        self.operation = Some(station.submit(StationRequest::Scan)?); self.state = ModeState::Listing; Ok(true)
    }
    pub fn connect(&mut self, station: &mut StationRuntime) -> Result<bool, ModeError> {
        if self.state != ModeState::Idle { return Ok(false); }
        let profile = self.profile.as_ref().ok_or(ModeError::Unconfigured)?;
        self.operation = Some(station.submit(StationRequest::Select(profile.clone()))?); self.state = ModeState::Connecting; Ok(true)
    }
    pub fn cancel(&mut self, station: &mut StationRuntime) -> Result<bool, ModeError> {
        match self.state {
            ModeState::Scanning | ModeState::Listing => {
                self.cancel_target = if self.state == ModeState::Scanning { ModeCompletionTarget::Configure } else { ModeCompletionTarget::Reset };
            }
            ModeState::Discovering | ModeState::Connecting => {
                if let Some(operation) = self.operation { station.cancel(operation)?; }
                self.cancel_target = if self.state == ModeState::Connecting { ModeCompletionTarget::Start } else { ModeCompletionTarget::Configure };
            }
            _ => return Ok(false),
        }
        self.state = ModeState::Cancelling; Ok(true)
    }
    pub fn stop(&mut self, station: &mut StationRuntime) -> Result<bool, ModeError> { self.cancel(station) }
    pub fn poll_station(&mut self, station: &mut StationRuntime, now: Milliseconds) -> Result<bool, ModeError> {
        if let Some(deadline) = self.retry_at {
            if now.0.wrapping_sub(deadline.0) >= 0x8000_0000 { return Ok(false); }
            self.reserve()?;
            self.retry_at = None;
            if self.state == ModeState::Cancelling {
                self.notify(self.cancel_target, CompletionStatus::Success); self.state = ModeState::Idle; return Ok(false);
            }
            if now.0.wrapping_sub(self.scan_started.0) <= 109_999 {
                self.operation = Some(station.submit(StationRequest::Scan)?); return Ok(true);
            }
            self.configuration_status = ConfigurationStatus::NotFound;
            self.state = ModeState::Idle; self.notify(ModeCompletionTarget::Configure, CompletionStatus::Success); return Ok(false);
        }
        let Some(completion) = station.completion() else { return Ok(false); };
        if self.operation != Some(completion.operation()) { return Ok(false); }
        self.accept_completion(completion, station, now)
    }
    fn accept_completion(&mut self, completion: StationCompletion, station: &mut StationRuntime, now: Milliseconds) -> Result<bool, ModeError> {
        self.reserve()?;
        match (self.state, completion.outcome()) {
            (ModeState::Scanning, StationOutcome::ScanComplete) => {
                if !station.scan_results().candidates().any(|candidate| candidate.privacy) {
                    self.operation = None;
                    self.retry_at = Some(Milliseconds(now.0.wrapping_add(500))); return Ok(false);
                }
                self.operation = Some(station.submit(StationRequest::ScanPrefix(self.prefix.ok_or(ModeError::Unconfigured)?))?);
                self.state = ModeState::Discovering; Ok(true)
            }
            (ModeState::Listing, StationOutcome::ScanComplete) => {
                self.operation = None; self.notify(ModeCompletionTarget::Reset, CompletionStatus::Success); self.state = ModeState::Idle; Ok(false)
            }
            (ModeState::Discovering, StationOutcome::ScanComplete) => {
                self.operation = None; self.scan_verified = true; Ok(false)
            }
            (ModeState::Connecting, StationOutcome::Connected(address)) => {
                self.operation = None; self.selected = Some(address); self.notify(ModeCompletionTarget::Start, CompletionStatus::Success); self.state = ModeState::Ready; Ok(false)
            }
            (ModeState::Cancelling, StationOutcome::Failed(error)) => {
                self.operation = None; self.notify(self.cancel_target, CompletionStatus::Failed); self.state = ModeState::Idle; Err(ModeError::Station(error))
            }
            (ModeState::Cancelling, _) => {
                self.operation = None; self.notify(self.cancel_target, CompletionStatus::Success); self.state = ModeState::Idle; Ok(false)
            }
            (_, StationOutcome::Failed(error)) => {
                self.operation = None;
                let target = match self.state { ModeState::Connecting => ModeCompletionTarget::Start, ModeState::Listing => ModeCompletionTarget::Reset, _ => ModeCompletionTarget::Configure };
                self.notify(target, CompletionStatus::Failed); self.state = ModeState::Idle; Err(ModeError::Station(error))
            }
            _ => Ok(false),
        }
    }
    pub fn process(&mut self, message: &TaskMessage, station: &mut StationRuntime, now: Milliseconds) -> Result<bool, ModeError> {
        self.reserve()?;
        match message.kind.0 {
            0x6615 => self.start(station, now),
            0x6616 => self.list(station),
            0x6617 => self.connect(station),
            0x6606 => self.cancel(station),
            0x6627 | 0x6628 => self.poll_station(station, now),
            0x6626 => {
                self.poll_station(station, now)?;
                if self.state != ModeState::Discovering || !self.scan_verified { return Ok(false); }
                let bytes = message.payload.bytes();
                let address = MacAddress(bytes[2..8].try_into().map_err(|_| ModeError::InvalidSsid)?);
                let candidate = station.scan_results().candidates().find(|candidate| candidate.access_point.address == address);
                let Some(candidate) = candidate else { return Ok(false); };
                let profile = self.records().find(|record| record.ssid == candidate.ssid).map(DiscoveryRecord::profile).transpose()?;
                let Some(profile) = profile else { return Ok(false); };
                if message.payload.halfword(0)? == 0 {
                    self.profile = Some(profile); self.selected = Some(address); self.configuration_status = ConfigurationStatus::Selected;
                } else {
                    self.profile = None; self.selected = None; self.configuration_status = ConfigurationStatus::NotFound;
                }
                self.reserve()?; self.notify(ModeCompletionTarget::Configure, CompletionStatus::Success); self.state = ModeState::Ready; Ok(false)
            }
            0x6625 if self.state == ModeState::Discovering => {
                let status = message.payload.halfword(0)?;
                if status == 0 {
                    if self.count == self.records.len() { return Ok(false); }
                    let bytes = message.payload.bytes();
                    let ssid = Ssid::parse(nul_terminated(&bytes[71..103])).map_err(|_| ModeError::InvalidSsid)?;
                    let mut key = [0; 64]; key[..63].copy_from_slice(&bytes[8..71]);
                    self.records[self.count] = Some(DiscoveryRecord { ssid, security: Security::from_wire(message.payload.word(4)?), key: DiscoveryKey(key) }); self.count += 1;
                } else {
                    self.configuration_status = if status == 3 { ConfigurationStatus::Rejected } else { ConfigurationStatus::NotFound };
                    self.operation = None; self.notify(ModeCompletionTarget::Configure, CompletionStatus::Success); self.state = ModeState::Idle;
                }
                Ok(false)
            }
            0x6625 if self.state == ModeState::Cancelling => {
                self.notify(ModeCompletionTarget::Configure, CompletionStatus::Success); self.state = ModeState::Idle; Ok(false)
            }
            _ => Ok(false),
        }
    }
    pub fn socket(&mut self) -> &mut socket::SocketModeRuntime { &mut self.socket }
    pub fn dispatch<I: crate::filesystem_runtime::SectorIo>(&mut self, message: &mut TaskMessage, station: &mut StationRuntime, slots: &mut super::station::SocketServiceRuntime, socket: &mut crate::vendor_wireless_network::transport::socket_task::SocketTaskRuntime, discovery: &mut super::discovery::NetworkDiscoveryRuntime<'_>, lan: &mut crate::vendor_wireless_network::lan::LanRuntime<'_>, modes: &mut super::interface_modes::InterfaceModes, mode: super::interface_modes::InterfaceMode, catalog: &mut crate::filesystem_catalog::Catalog<'_, I>, now: crate::vendor_wireless_network::dhcp::Milliseconds) -> Result<bool, ModeError> {
        self.socket.set_mode_state(self.state);
        let known = matches!(message.kind.0, 0x6603 | 0x6606 | 0x660f | 0x6615 | 0x6625 | 0x6626 | 0x6616 | 0x6627 | 0x6617 | 0x6628);
        if self.state == ModeState::Ready && message.kind.0 == 0x660f {
            let handled = self.socket.configure_channel(message, slots, socket, discovery, lan, modes, mode, catalog, now)?;
            if message.payload.word(8)? == 0 { self.state = ModeState::Idle; }
            return Ok(handled);
        }
        if (self.state == ModeState::Ready && matches!(message.kind.0, 0x6603 | 0x6606)) || (!known && matches!(self.state, ModeState::Idle | ModeState::Ready)) {
            return self.socket.dispatch(message, slots, socket, discovery, lan, modes, mode, catalog, now).map_err(ModeError::Socket);
        }
        self.process(message, station, now)
    }
    pub fn receive_socket_completion<I: crate::filesystem_runtime::SectorIo>(&mut self, completion: crate::vendor_wireless_network::transport::socket_task::SocketTaskSocketCompletion, slots: &mut super::station::SocketServiceRuntime, socket: &mut crate::vendor_wireless_network::transport::socket_task::SocketTaskRuntime, discovery: &mut super::discovery::NetworkDiscoveryRuntime<'_>, lan: &mut crate::vendor_wireless_network::lan::LanRuntime<'_>, modes: &mut super::interface_modes::InterfaceModes, mode: super::interface_modes::InterfaceMode, catalog: &mut crate::filesystem_catalog::Catalog<'_, I>, now: crate::vendor_wireless_network::dhcp::Milliseconds) -> Result<bool, ModeError> {
        self.socket.set_mode_state(self.state);
        self.socket.receive_completion(completion, slots, socket, discovery, lan, modes, mode, catalog, now).map_err(ModeError::Socket)
    }
    pub fn receive_interface_completion(&mut self, completion: super::interface_modes::InterfaceCompletion, slots: &mut super::station::SocketServiceRuntime, socket: &mut crate::vendor_wireless_network::transport::socket_task::SocketTaskRuntime) -> Result<bool, ModeError> {
        self.socket.set_mode_state(self.state);
        self.socket.receive_interface_completion(completion, slots, socket).map_err(ModeError::Socket)
    }
    pub fn receive_service_discovery(&mut self, entries: &[packets::ServiceEntry], status: CompletionStatus, slots: &mut super::station::SocketServiceRuntime, socket: &mut crate::vendor_wireless_network::transport::socket_task::SocketTaskRuntime) -> Result<bool, ModeError> {
        self.socket.set_mode_state(self.state);
        self.socket.receive_service_discovery(entries, status, slots, socket).map_err(ModeError::Socket)
    }
}
fn nul_terminated(bytes: &[u8]) -> &[u8] { &bytes[..bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len())] }
