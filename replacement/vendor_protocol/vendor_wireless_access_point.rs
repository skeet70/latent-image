//! X-M1 1.50 AP management and canonical station publication.
//! Source pin: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::vendor_wireless_network::{MacAddress, Ssid};
use crate::vendor_wireless_peers::{NetIndex, PeerLease, PeerPool, PEER_CAPACITY};
#[path = "vendor_wireless_access_point_beacon.rs"]
pub mod beacon;
pub use beacon::{BeaconTemplate, HardwareCapabilities, Protection};
#[path = "vendor_wireless_access_point_security.rs"]
mod security;
pub use security::{SecurityEffect, WpsMethod};
#[path = "vendor_wireless_access_point_management.rs"]
mod management;
pub use management::{ManagementId, ManagementTransmission, PeerInstallation, StationParameters};
#[path = "vendor_wireless_access_point_timers.rs"]
mod timers;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Security { Open, Wep, WpaTkip, Wpa2Ccmp }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channel { Automatic, Number(ChannelNumber) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelNumber(u8);
impl ChannelNumber {
    pub fn new(number: u8) -> Result<Self, Error> {
        if (1..=14).contains(&number) { Ok(Self(number)) } else { Err(Error::Channel) }
    }
    pub fn get(self) -> u8 { self.0 }
}
impl Channel {
    pub fn frequency_mhz(self) -> u16 {
        match self { Self::Automatic => 2412, Self::Number(ChannelNumber(14)) => 2484,
            Self::Number(number) => 2407 + 5 * u16::from(number.0) }
    }
    pub fn number(self) -> u8 { match self { Self::Automatic => 1, Self::Number(number) => number.0 } }
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct NetworkKey { bytes: [u8; 64], length: u8 }
impl core::fmt::Debug for NetworkKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("NetworkKey([redacted])") }
}
impl NetworkKey {
    pub fn new(security: Security, input: &[u8]) -> Result<Self, Error> {
        let mut result = Self { bytes: [0; 64], length: 0 };
        match security {
            Security::Open => { if !input.is_empty() { return Err(Error::Key); } }
            Security::Wep => match input.len() {
                5 | 13 => { result.bytes[..input.len()].copy_from_slice(input); result.length = input.len() as u8; }
                10 | 26 => {
                    for (index, pair) in input.chunks_exact(2).enumerate() {
                        result.bytes[index] = hex(pair[0])?.checked_mul(16).ok_or(Error::Key)? | hex(pair[1])?;
                    }
                    result.length = (input.len() / 2) as u8;
                }
                _ => return Err(Error::Key),
            },
            Security::WpaTkip | Security::Wpa2Ccmp => {
                if !(8..=64).contains(&input.len()) || input.contains(&0) { return Err(Error::Key); }
                if input.len() == 64 { for byte in input { hex(*byte)?; } }
                result.bytes[..input.len()].copy_from_slice(input); result.length = input.len() as u8;
            }
        }
        Ok(result)
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..usize::from(self.length)] }
}
fn hex(byte: u8) -> Result<u8, Error> {
    match byte { b'0'..=b'9' => Ok(byte - b'0'), b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10), _ => Err(Error::Key) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyIndex(u8);
impl KeyIndex {
    pub fn new(index: u8) -> Result<Self, Error> { if index < 4 { Ok(Self(index)) } else { Err(Error::KeyIndex) } }
    pub fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configuration {
    pub ssid: Ssid,
    pub channel: Channel,
    pub security: Security,
    pub key: NetworkKey,
    pub key_index: KeyIndex,
    pub country: Option<[u8; 3]>,
    pub beacon_interval: Option<u16>,
    pub disable_11n: bool,
}
impl Configuration {
    /// Payload of driver message kind 10, excluding its twelve-byte header.
    pub fn parse(payload: &[u8]) -> Result<Self, Error> {
        if payload.len() < 136 { return Err(Error::Truncated); }
        let ssid_length = word(payload, 32)? as usize;
        let ssid = Ssid::parse(payload.get(..ssid_length).filter(|_| ssid_length <= 32).ok_or(Error::Ssid)?).map_err(|_| Error::Ssid)?;
        if ssid.as_bytes().is_empty() { return Err(Error::Ssid); }
        let channel = match word(payload, 36)? { 0 => Channel::Automatic, n if n <= 14 => Channel::Number(ChannelNumber::new(n as u8)?), _ => return Err(Error::Channel) };
        let security = match word(payload, 44)? { 0 => Security::Open, 2 => Security::WpaTkip, 3 => Security::Wpa2Ccmp, _ => Security::Wep };
        let key_length = word(payload, 112)? as usize;
        let key = NetworkKey::new(security, payload.get(48..48usize.checked_add(key_length).ok_or(Error::Key)?).filter(|_| key_length <= 64).ok_or(Error::Key)?)?;
        let country = if payload[116] != 0 && payload[117] != 0 { Some([payload[116], payload[117], payload[118]]) } else { None };
        let beacon = word(payload, 120)?;
        Ok(Self { ssid, channel, security, key, key_index: KeyIndex::new(payload[132])?, country,
            beacon_interval: if (20..=2000).contains(&beacon) { Some(beacon as u16) } else { None },
            disable_11n: word(payload, 40)? == 2 })
    }
}
fn word(input: &[u8], offset: usize) -> Result<u32, Error> {
    let bytes: [u8; 4] = input.get(offset..offset + 4).ok_or(Error::Truncated)?.try_into().map_err(|_| Error::Truncated)?;
    Ok(u32::from_le_bytes(bytes))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceMode { Off, Station, AccessPoint, Monitor }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioProfile { Legacy, HighThroughput }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceConfiguration { bytes: [u8; 44], configuration: Configuration }
impl ServiceConfiguration {
    pub fn resolve(input: &[u8], ssid: &[u8], key: &[u8], country: Option<&[u8]>) -> Result<Self, Error> {
        let bytes: [u8; 44] = input.get(..44).ok_or(Error::Truncated)?.try_into().map_err(|_| Error::Truncated)?;
        if word(&bytes, 4)? as usize != ssid.len() || word(&bytes, 24)? as usize != key.len() { return Err(Error::Truncated); }
        let mut payload = [0; 136];
        if ssid.len() > 32 || key.len() > 64 { return Err(Error::Capacity); }
        payload[..ssid.len()].copy_from_slice(ssid);
        payload[32..36].copy_from_slice(&bytes[4..8]);
        payload[36..48].copy_from_slice(&bytes[8..20]);
        payload[48..48 + key.len()].copy_from_slice(key);
        payload[112..116].copy_from_slice(&bytes[24..28]);
        if let Some(country) = country {
            let count = country.iter().position(|byte| *byte == 0).unwrap_or(country.len()).min(3);
            payload[116..116 + count].copy_from_slice(&country[..count]);
        }
        payload[120..124].copy_from_slice(&bytes[32..36]);
        Ok(Self { bytes, configuration: Configuration::parse(&payload)? })
    }
    pub fn configuration(&self) -> Configuration { self.configuration }
    pub fn bytes(&self) -> &[u8; 44] { &self.bytes }
    pub fn profile(&self) -> Option<RadioProfile> {
        match u32::from_le_bytes(self.bytes[12..16].try_into().unwrap()) { 2 => Some(RadioProfile::Legacy), 3 => Some(RadioProfile::HighThroughput), _ => None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Truncated, Ssid, Channel, Key, KeyIndex, Busy, NotRunning, UnexpectedCompletion, PeerCapacity, UnknownPeer, PeerLease, Capabilities, HardwareMissing, Capacity, Management, Security, Random, Driver(DriverError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverError(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State { Stopped, Starting, Running, Stopping, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation { Associate, Country, Frequency, FlushStations, Privacy, ClearKey(KeyIndex), InstallWep, Ssid, Beacon, UpdateBeacon, Operstate, Commit, Deauthenticate, StopBeacon, StopBss }
#[derive(Debug, Eq, PartialEq)]
pub enum Command {
    Associate { ssid: Ssid, frequency_mhz: u16, security: Security },
    Country([u8; 3]), Frequency { mhz: u16, channel: u8, ht: bool }, FlushStations,
    Privacy(bool), ClearKey(KeyIndex), InstallWep { index: KeyIndex, key: NetworkKey },
    Ssid(Ssid), Beacon(BeaconTemplate),
    Operstate(bool), Commit, DeauthenticateAll, StopBeacon, StopBss,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Peer { pub address: MacAddress, pub signal: u8 }
#[derive(Debug)]
pub enum Effect {
    Driver { operation: Operation, command: Command }, Started, Stopped, Cancelled,
    Failed(Error), PeerConnected(Peer), PeerDisconnected(MacAddress),
    RemovePeer { address: MacAddress, lease: PeerLease },
}
#[derive(Clone, Copy)]
struct PeerEntry {
    peer: Peer, published: bool, authenticated: bool, associated: bool, aid: u16, lease: Option<PeerLease>, wps: bool,
    deadline: Option<crate::vendor_wireless_access_point_authenticator::Milliseconds>, expiry: PeerExpiry,
}
#[derive(Clone, Copy)]
enum PeerExpiry { FirstDisassociation, SecondDisassociation, Deauthentication }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, Associate, Country, Frequency, Flush, Privacy, Clear(u8), Wep, Ssid, Beacon, Operstate, Commit, Ready, Deauthenticate, StopFlush, StopBeacon, StopClear(u8), StopBss, Finished }

pub struct AccessPoint {
    local: MacAddress,
    configuration: Option<Configuration>,
    state: State,
    phase: Phase,
    pending: Option<Operation>,
    peers: [Option<PeerEntry>; PEER_CAPACITY],
    notifications: [Option<Effect>; PEER_CAPACITY * 3 + 4],
    read: usize,
    write: usize,
    failure: Option<Error>,
    cancelled: bool,
    interval: u16,
    hardware: Option<HardwareCapabilities>,
    protection: Protection,
    authenticator: Option<crate::vendor_wireless_access_point_authenticator::Authenticator>,
    registrar: Option<crate::wireless_association::station::wps::registrar::Registrar>,
    wps_elements: crate::vendor_wireless_station_driver::association::frames::Packet,
    auth_initialized: bool,
    beacon_dirty: bool,
    management_serial: u64,
    management_pending: Option<management::PendingManagement>,
    installation_pending: Option<management::PendingInstallation>,
    net: NetIndex,
    retiring: [Option<PeerLease>; PEER_CAPACITY],
    registrar_io_pending: bool,
}
impl AccessPoint {
    pub fn new(local: MacAddress) -> Self {
        Self { local, configuration: None, state: State::Stopped, phase: Phase::Idle, pending: None,
            peers: [None; PEER_CAPACITY], notifications: core::array::from_fn(|_| None), read: 0, write: 0, failure: None, cancelled: false, interval: 100,
            hardware: None, protection: Protection::default(), authenticator: None, registrar: None,
            wps_elements: crate::vendor_wireless_station_driver::association::frames::Packet::new(),
            auth_initialized: false, beacon_dirty: false, management_serial: 0, management_pending: None,
            installation_pending: None, net: NetIndex::PRIMARY, retiring: [None; PEER_CAPACITY], registrar_io_pending: false }
    }
    pub fn state(&self) -> State { self.state }
    pub fn configuration(&self) -> Option<&Configuration> { self.configuration.as_ref() }
    pub fn supply_hardware(&mut self, hardware: HardwareCapabilities) -> Result<(), Error> {
        if !matches!(self.state, State::Stopped | State::Failed) { return Err(Error::Busy); }
        self.hardware = Some(hardware); Ok(())
    }
    pub fn start(&mut self, configuration: Configuration) -> Result<(), Error> {
        if !matches!(self.state, State::Stopped | State::Failed) || self.pending.is_some() || self.read != self.write { return Err(Error::Busy); }
        if self.hardware.is_none() { return Err(Error::HardwareMissing); }
        if configuration.channel.number() == 14 { return Err(Error::Channel); }
        if self.peers.iter().flatten().any(|entry| entry.lease.is_some()) || self.installation_pending.is_some() || self.retiring.iter().any(Option::is_some) { return Err(Error::Busy); }
        NetworkKey::new(configuration.security, configuration.key.as_bytes())?;
        self.prepare_security(&configuration)?;
        self.configuration = Some(configuration);
        if let Some(interval) = configuration.beacon_interval { self.interval = interval; }
        self.state = State::Starting; self.phase = Phase::Associate; self.failure = None; self.cancelled = false;
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), Error> {
        match self.state {
            State::Stopped => return Ok(()), State::Stopping => return Ok(()),
            State::Failed if self.phase == Phase::Idle && self.peers.iter().flatten().all(|entry| entry.lease.is_none()) && self.retiring.iter().all(Option::is_none) => { self.state = State::Stopped; return Ok(()); }
            _ => {}
        }
        self.state = State::Stopping;
        self.management_pending = None;
        if self.pending.is_none() { self.phase = Phase::Deauthenticate; }
        if let Some(authenticator) = self.authenticator.as_mut() {
            if authenticator.cancel().is_err() { self.failure = Some(Error::Security); return Err(Error::Security); }
        }
        if let Some(registrar) = self.registrar.as_mut() { registrar.cancel(); }
        Ok(())
    }
    pub fn cancel(&mut self) -> Result<(), Error> { self.cancelled = true; self.stop() }
    pub fn peers(&self) -> impl Iterator<Item = Peer> + '_ {
        self.peers.iter().flatten().filter(|entry| entry.published).map(|entry| entry.peer)
    }
    pub fn peer_signal(&self, address: MacAddress) -> Option<u8> { self.peers().find(|peer| peer.address == address).map(|peer| peer.signal) }
    fn authorized(&mut self, address: MacAddress, pool: &mut PeerPool) -> Result<(), Error> {
        if self.state != State::Running { return Err(Error::NotRunning); }
        let entry = self.peers.iter_mut().flatten().find(|entry| entry.peer.address == address).ok_or(Error::UnknownPeer)?;
        pool.get_mut(entry.lease.ok_or(Error::PeerLease)?).map_err(|_| Error::PeerLease)?.security_authorized = true;
        if !entry.published { entry.published = true; let peer = entry.peer; self.notify(Effect::PeerConnected(peer)); }
        Ok(())
    }
    pub fn peer_disconnected(&mut self, address: MacAddress, pool: &mut PeerPool) -> Result<(), Error> {
        if let Some(index) = self.peers.iter().position(|slot| slot.as_ref().is_some_and(|entry| entry.peer.address == address)) {
            let entry = self.peers[index].ok_or(Error::UnknownPeer)?;
            if entry.associated && !entry.wps {
                if let Some(authenticator) = self.authenticator.as_mut() { authenticator.disconnected(address).map_err(|_| Error::Security)?; }
            }
            if let Some(lease) = entry.lease {
                let slot = self.retiring.iter_mut().find(|slot| slot.is_none()).ok_or(Error::Capacity)?;
                pool.begin_release(lease).map_err(|_| Error::PeerLease)?;
                *slot = Some(lease);
                self.notify(Effect::RemovePeer { address, lease });
            }
            self.peers[index] = None;
            if entry.published { self.notify(Effect::PeerDisconnected(address)); }
        }
        Ok(())
    }
    pub fn complete_peer_removal(&mut self, lease: PeerLease, result: Result<(), DriverError>, pool: &mut PeerPool) -> Result<(), Error> {
        let slot = self.retiring.iter_mut().find(|slot| **slot == Some(lease)).ok_or(Error::UnexpectedCompletion)?;
        match result {
            Ok(()) => { pool.release(lease).map_err(|_| Error::PeerLease)?; *slot = None; Ok(()) }
            Err(error) => { self.failure = Some(Error::Driver(error)); self.stop()?; Err(Error::Driver(error)) }
        }
    }
    fn notify(&mut self, effect: Effect) {
        assert!(self.write - self.read < self.notifications.len());
        self.notifications[self.write % self.notifications.len()] = Some(effect); self.write += 1;
    }
    pub fn poll(&mut self) -> Option<Effect> {
        if self.read != self.write {
            let effect = self.notifications[self.read % self.notifications.len()].take(); self.read += 1;
            if self.read == self.write { self.read = 0; self.write = 0; }
            return effect;
        }
        if self.pending.is_some() { return None; }
        if self.phase == Phase::Ready && self.beacon_dirty {
            let security = self.authenticator.as_ref().map(|authenticator| authenticator.information_element());
            let bytes = security.as_ref().map_or(&[][..], |element| element.as_bytes());
            let template = match BeaconTemplate::build(self.local, self.configuration.as_ref()?, self.hardware.as_ref()?, self.protection, self.interval, bytes, self.wps_elements.as_bytes()) {
                Ok(template) => template,
                Err(error) => { self.failure = Some(error); self.stop().ok()?; return self.poll(); }
            };
            self.pending = Some(Operation::UpdateBeacon); self.beacon_dirty = false;
            return Some(Effect::Driver { operation: Operation::UpdateBeacon, command: Command::Beacon(template) });
        }
        let config = self.configuration.as_ref();
        let (operation, command) = match self.phase {
            Phase::Associate => { let c = config?; (Operation::Associate, Command::Associate { ssid: c.ssid, frequency_mhz: c.channel.frequency_mhz(), security: c.security }) }
            Phase::Country => (Operation::Country, Command::Country(config?.country?)),
            Phase::Frequency => { let c = config?; (Operation::Frequency, Command::Frequency { mhz: c.channel.frequency_mhz(), channel: c.channel.number(), ht: !c.disable_11n && c.security != Security::WpaTkip && self.hardware.as_ref()?.ht.is_some() }) }
            Phase::Flush => (Operation::FlushStations, Command::FlushStations),
            Phase::Privacy => (Operation::Privacy, Command::Privacy(false)),
            Phase::Clear(index) => (Operation::ClearKey(KeyIndex(index)), Command::ClearKey(KeyIndex(index))),
            Phase::Wep => { let c = config?; (Operation::InstallWep, Command::InstallWep { index: c.key_index, key: c.key }) }
            Phase::Ssid => (Operation::Ssid, Command::Ssid(config?.ssid)),
            Phase::Beacon => {
                if self.authenticator.as_ref().is_some_and(|authenticator| !authenticator.is_ready()) { return None; }
                let security = self.authenticator.as_ref().map(|authenticator| authenticator.information_element());
                let bytes = security.as_ref().map_or(&[][..], |element| element.as_bytes());
                match BeaconTemplate::build(self.local, config?, self.hardware.as_ref()?, self.protection, self.interval, bytes, self.wps_elements.as_bytes()) {
                    Ok(template) => (Operation::Beacon, Command::Beacon(template)),
                    Err(error) => { self.failure = Some(error); self.stop().ok()?; return self.poll(); }
                }
            }
            Phase::Operstate => (Operation::Operstate, Command::Operstate(true)),
            Phase::Commit => (Operation::Commit, Command::Commit),
            Phase::Deauthenticate => (Operation::Deauthenticate, Command::DeauthenticateAll),
            Phase::StopFlush => (Operation::FlushStations, Command::FlushStations),
            Phase::StopBeacon => (Operation::StopBeacon, Command::StopBeacon),
            Phase::StopClear(index) => (Operation::ClearKey(KeyIndex(index)), Command::ClearKey(KeyIndex(index))),
            Phase::StopBss => {
                if self.authenticator.as_ref().is_some_and(|authenticator| !authenticator.is_drained()) { return None; }
                if self.registrar_io_pending || self.registrar.as_ref().is_some_and(|registrar| !registrar.is_drained()) { return None; }
                (Operation::StopBss, Command::StopBss)
            }
            Phase::Finished => {
                self.configuration = None; self.phase = Phase::Idle;
                self.state = if self.failure.is_some() { State::Failed } else { State::Stopped };
                return Some(if let Some(error) = self.failure.take() { Effect::Failed(error) } else if self.cancelled { Effect::Cancelled } else { Effect::Stopped });
            }
            _ => return None,
        };
        self.pending = Some(operation);
        Some(Effect::Driver { operation, command })
    }
    pub fn complete(&mut self, operation: Operation, result: Result<(), DriverError>, pool: &mut PeerPool) -> Result<(), Error> {
        if self.pending != Some(operation) { return Err(Error::UnexpectedCompletion); }
        self.pending = None;
        if operation == Operation::UpdateBeacon {
            if let Err(error) = result { self.failure = Some(Error::Driver(error)); self.stop()?; }
            return Ok(());
        }
        let teardown = matches!(self.phase, Phase::Deauthenticate | Phase::StopFlush | Phase::StopBeacon | Phase::StopClear(_) | Phase::StopBss);
        if let Err(error) = result {
            if self.failure.is_none() { self.failure = Some(Error::Driver(error)); }
            if !teardown { self.stop()?; self.phase = Phase::Deauthenticate; return Ok(()); }
        }
        if self.state == State::Stopping && !teardown { self.phase = Phase::Deauthenticate; return Ok(()); }
        self.phase = match self.phase {
            Phase::Associate => if self.configuration.unwrap().country.is_some() { Phase::Country } else { Phase::Frequency },
            Phase::Country => Phase::Frequency, Phase::Frequency => Phase::Flush, Phase::Flush => Phase::Privacy,
            Phase::Privacy => Phase::Clear(0), Phase::Clear(index) if index < 3 => Phase::Clear(index + 1),
            Phase::Clear(_) => if self.configuration.unwrap().security == Security::Wep { Phase::Wep } else { Phase::Ssid },
            Phase::Wep => Phase::Ssid, Phase::Ssid => Phase::Beacon, Phase::Beacon => Phase::Operstate,
            Phase::Operstate => Phase::Commit, Phase::Commit => { self.state = State::Running; self.notify(Effect::Started); Phase::Ready }
            Phase::Deauthenticate => Phase::StopFlush,
            Phase::StopFlush => {
                if result.is_ok() {
                    if let Some(installation) = self.installation_pending.take() {
                        let lease = installation.lease();
                        if !self.peers.iter().flatten().any(|entry| entry.lease == Some(lease)) { pool.release(lease).map_err(|_| Error::PeerLease)?; }
                    }
                    for index in 0..self.peers.len() {
                        if let Some(entry) = self.peers[index].take() {
                            if let Some(lease) = entry.lease { pool.release(lease).map_err(|_| Error::PeerLease)?; }
                            if entry.published { self.notify(Effect::PeerDisconnected(entry.peer.address)); }
                        }
                    }
                    for slot in &mut self.retiring { if let Some(lease) = slot.take() { pool.release(lease).map_err(|_| Error::PeerLease)?; } }
                }
                Phase::StopBeacon
            }
            Phase::StopBeacon => Phase::StopClear(0),
            Phase::StopClear(index) if index < 3 => Phase::StopClear(index + 1),
            Phase::StopClear(_) => Phase::StopBss, Phase::StopBss => Phase::Finished,
            _ => return Err(Error::UnexpectedCompletion),
        };
        Ok(())
    }
}

#[cfg(test)]
#[path = "vendor_wireless_access_point_tests.rs"]
mod tests;
