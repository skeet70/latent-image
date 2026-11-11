//! X-M1 station selector CPU and command FIFO requests.
//! Source pin and instruction extents are recorded in the adjacent evidence file.

use super::vendor_wireless_network::{MacAddress, Ssid};
use super::vendor_wireless_parameter_driver::SdioWriteOutcome;

#[path = "vendor_wireless_station_driver_association.rs"]
pub mod association;
#[path = "vendor_wireless_station_driver_receive.rs"]
pub mod receive;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum InterfaceMode { Adhoc = 0, Infrastructure = 1, Auto = 2, Dedicated = 3 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Authentication { Open = 0, Shared = 1, Auto = 2, Wpa = 3, WpaPsk = 4, WpaNone = 5, Wpa2 = 6, Wpa2Psk = 7 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Encryption { Wep = 0, Disabled = 1, Tkip = 4, Ccmp = 6 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationDriverError { Busy, Stopped, InvalidKey, InvalidAuthenticationMode, Cancelled, Association(association::AssociationError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyIndex(u8);
impl KeyIndex {
    pub const fn new(value: u8) -> Result<Self, StationDriverError> {
        if value < 4 { Ok(Self(value)) } else { Err(StationDriverError::InvalidKey) }
    }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyMaterial { Wep40([u8; 5]), Wep104([u8; 13]), Temporal128([u8; 16]), Temporal256([u8; 32]) }
impl KeyMaterial {
    pub fn as_bytes(&self) -> &[u8] {
        match self { Self::Wep40(v) => v, Self::Wep104(v) => v, Self::Temporal128(v) => v, Self::Temporal256(v) => v }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationKeySlot { pub index: KeyIndex, pub peer: MacAddress, pub pairwise: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationKey { pub slot: StationKeySlot, pub transmit: bool, pub authenticator: bool, pub material: KeyMaterial }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationDriverRequest {
    SetInterfaceMode(InterfaceMode), SetAuthentication(Authentication), SetEncryption(Encryption),
    SelectSsid(Ssid), SelectBssid(MacAddress), InstallKey(StationKey), RemoveKey(StationKeySlot), StopStation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationCompletion(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationReply { Applied, Late }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationRequestId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationDriverCompletion {
    operation: StationRequestId,
    outcome: Result<(), StationDriverError>,
}
impl StationDriverCompletion {
    pub const fn operation(self) -> StationRequestId { self.operation }
    pub const fn outcome(self) -> Result<(), StationDriverError> { self.outcome }
}

pub struct FirmwareCommand {
    pub identity: StationCompletion,
    bytes: [u8; 72],
    length: usize,
}
impl FirmwareCommand {
    pub const fn argument(&self) -> u32 { 0x9000_5800 | self.length as u32 }
    pub fn bytes(&self) -> &[u8] { &self.bytes[..self.length] }
    pub fn bytes_mut(&mut self) -> &mut [u8] { &mut self.bytes[..self.length] }
    pub const fn sequence(&self) -> u8 { self.bytes[6] }
}
impl Drop for FirmwareCommand {
    fn drop(&mut self) {
        for byte in &mut self.bytes {
            unsafe { core::ptr::write_volatile(byte, 0); }
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityPublication {
    pub mode: InterfaceMode,
    pub authentication: Authentication,
    pub encryption: Encryption,
    pub transmit_key_present: bool,
}
impl SecurityPublication {
    pub const fn authentication_suite(self) -> Option<AuthenticationSuite> {
        match self.authentication {
            Authentication::Wpa => Some(AuthenticationSuite::WpaEnterprise),
            Authentication::WpaPsk => Some(AuthenticationSuite::WpaPersonal),
            Authentication::WpaNone => Some(AuthenticationSuite::WpaNone),
            Authentication::Wpa2 => Some(AuthenticationSuite::RsnEnterprise),
            Authentication::Wpa2Psk => Some(AuthenticationSuite::RsnPersonal),
            _ => None,
        }
    }
    pub const fn cipher_suites(self) -> CipherSuites {
        match self.encryption {
            Encryption::Wep | Encryption::Disabled => CipherSuites { wep40: true, wep104: true, tkip: false, ccmp: false },
            Encryption::Tkip => CipherSuites { wep40: true, wep104: true, tkip: true, ccmp: false },
            Encryption::Ccmp => CipherSuites { wep40: true, wep104: true, tkip: true, ccmp: true },
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationSuite { WpaEnterprise, WpaPersonal, WpaNone, RsnEnterprise, RsnPersonal }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CipherSuites { pub wep40: bool, pub wep104: bool, pub tkip: bool, pub ccmp: bool }

#[derive(Clone, Copy)]
#[repr(u8)]
enum FirmwareSelector { Key = 8, InterfaceMode = 10 }

pub struct StationDriver {
    security: SecurityPublication,
    association: association::Association,
    sequence: u8,
    next_identity: u64,
    operation: StationRequestId,
    pending: Option<FirmwareCommand>,
    outcome: Option<Result<(), StationDriverError>>,
    last_physical: Option<SdioWriteOutcome>,
    stopped: bool,
}
impl StationDriver {
    pub const fn new(last_sequence: u8) -> Self {
        Self {
            security: SecurityPublication { mode: InterfaceMode::Infrastructure, authentication: Authentication::Open,
                encryption: Encryption::Disabled, transmit_key_present: false },
            association: association::Association::new(),
            sequence: last_sequence,
            next_identity: 0, pending: None, outcome: None, last_physical: None, stopped: false,
            operation: StationRequestId(0),
        }
    }
    pub const fn security(&self) -> SecurityPublication { self.security }
    pub const fn outcome(&self) -> Option<Result<(), StationDriverError>> { self.outcome }
    pub const fn last_physical_outcome(&self) -> Option<SdioWriteOutcome> { self.last_physical }
    pub const fn last_sequence(&self) -> u8 { self.sequence }
    pub fn set_last_sequence(&mut self, sequence: u8) { self.sequence = sequence; }
    pub fn association(&self) -> &association::Association { &self.association }
    pub fn association_mut(&mut self) -> &mut association::Association { &mut self.association }
    pub fn completion(&self) -> Option<StationDriverCompletion> {
        self.outcome.map(|outcome| StationDriverCompletion { operation: self.operation, outcome })
    }
    pub fn submit_identified(&mut self, request: StationDriverRequest) -> Result<StationRequestId, StationDriverError> {
        self.submit(request)?;
        Ok(self.operation)
    }

    pub fn submit(&mut self, request: StationDriverRequest) -> Result<(), StationDriverError> {
        if self.pending.is_some() { return Err(StationDriverError::Busy); }
        if self.stopped { return Err(StationDriverError::Stopped); }
        self.outcome = None;
        match request {
            StationDriverRequest::SetAuthentication(authentication) => {
                let requires_infrastructure = matches!(authentication, Authentication::Wpa | Authentication::WpaPsk | Authentication::Wpa2 | Authentication::Wpa2Psk);
                if (requires_infrastructure && self.security.mode != InterfaceMode::Infrastructure)
                    || (authentication == Authentication::WpaNone && self.security.mode != InterfaceMode::Adhoc) {
                    return Err(StationDriverError::InvalidAuthenticationMode);
                }
                self.security.authentication = authentication;
                self.association.set_security(association::SecurityPublication { authentication, encryption: self.security.encryption });
                self.outcome = Some(Ok(()));
            }
            StationDriverRequest::SetEncryption(encryption) => {
                self.security.encryption = encryption;
                self.association.set_security(association::SecurityPublication { authentication: self.security.authentication, encryption });
                self.outcome = Some(Ok(()));
            }
            StationDriverRequest::InstallKey(key) => {
                if key.transmit && key.slot.pairwise && (key.slot.index.get() != 0 || key.slot.peer.0 == [255; 6]) {
                    return Err(StationDriverError::InvalidKey);
                }
                if key.slot.pairwise && !key.transmit { return Err(StationDriverError::InvalidKey); }
                let mut bytes = [0; 72];
                let body = &mut bytes[8..];
                body[0] = 1;
                body[1] = u8::from(key.transmit);
                body[2] = u8::from(key.slot.pairwise);
                body[3] = u8::from(key.authenticator);
                body[4..10].copy_from_slice(&key.slot.peer.0);
                body[12] = key.slot.index.get();
                let material = key.material.as_bytes();
                body[13] = material.len() as u8;
                body[16..16 + material.len()].copy_from_slice(material);
                body[11] = match key.material {
                    KeyMaterial::Wep40(_) => 1,
                    KeyMaterial::Wep104(_) => 5,
                    KeyMaterial::Temporal128(_) if (self.security.authentication as u32) <= 2 => 7,
                    KeyMaterial::Temporal128(_) => 4,
                    KeyMaterial::Temporal256(_) if self.security.authentication == Authentication::WpaNone => match self.security.encryption {
                        Encryption::Tkip => 2,
                        Encryption::Ccmp => { body[13] = 16; 4 },
                        _ => 0,
                    },
                    KeyMaterial::Temporal256(_) => 2,
                };
                self.security.transmit_key_present |= key.transmit;
                if matches!(key.material, KeyMaterial::Temporal256(_)) && self.security.authentication != Authentication::WpaNone {
                    self.association.temporal_key_installed().map_err(StationDriverError::Association)?;
                }
                self.queue_command(FirmwareSelector::Key, bytes, 72);
            }
            StationDriverRequest::RemoveKey(slot) => {
                let mut bytes = [0; 72];
                let body = &mut bytes[8..];
                body[2] = u8::from(slot.pairwise);
                body[4..10].copy_from_slice(&slot.peer.0);
                body[12] = slot.index.get();
                if slot.pairwise { self.security.transmit_key_present = false; }
                self.queue_command(FirmwareSelector::Key, bytes, 72);
            }
            StationDriverRequest::SetInterfaceMode(mode) => {
                if matches!(mode, InterfaceMode::Adhoc | InterfaceMode::Dedicated) && !self.association.interface_mode_admitted() {
                    return Err(StationDriverError::InvalidAuthenticationMode);
                }
                self.association.request(association::AssociationRequest::SetInterfaceMode(mode)).map_err(StationDriverError::Association)?;
                self.security.mode = mode;
                self.security.transmit_key_present = false;
                self.queue_command(FirmwareSelector::InterfaceMode, [0; 72], 8);
            }
            StationDriverRequest::SelectSsid(ssid) => {
                self.association.request(association::AssociationRequest::SelectSsid(ssid)).map_err(StationDriverError::Association)?;
                self.outcome = Some(Ok(()));
            }
            StationDriverRequest::SelectBssid(bssid) => {
                self.association.request(association::AssociationRequest::SelectBssid(bssid)).map_err(StationDriverError::Association)?;
                self.outcome = Some(Ok(()));
            }
            StationDriverRequest::StopStation => {
                self.association.request(association::AssociationRequest::StopStation).map_err(StationDriverError::Association)?;
                self.outcome = Some(Ok(()));
            }
        }
        self.operation.0 = self.operation.0.wrapping_add(1);
        Ok(())
    }

    fn queue_command(&mut self, command: FirmwareSelector, mut bytes: [u8; 72], length: usize) {
        self.next_identity = self.next_identity.wrapping_add(1);
        bytes[..2].copy_from_slice(&(length as u16).to_le_bytes());
        bytes[3] = 0x50;
        bytes[4] = command as u8;
        bytes[5] = 1;
        self.sequence = self.sequence.wrapping_add(1);
        bytes[6] = self.sequence;
        self.pending = Some(FirmwareCommand { identity: StationCompletion(self.next_identity), bytes, length });
    }

    pub fn command(&self) -> Option<&FirmwareCommand> { self.pending.as_ref() }
    pub fn command_mut(&mut self) -> Option<&mut FirmwareCommand> { self.pending.as_mut() }
    pub fn receive(&mut self, identity: StationCompletion, written: SdioWriteOutcome) -> StationReply {
        if self.pending.as_ref().map(|v| v.identity) != Some(identity) { return StationReply::Late; }
        self.pending = None;
        self.last_physical = Some(written);
        self.outcome = Some(Ok(()));
        StationReply::Applied
    }
    pub fn cancel(&mut self) {
        self.pending = None;
        self.association.cancel();
        self.outcome = Some(Err(StationDriverError::Cancelled));
    }
    pub fn set_stopped(&mut self, stopped: bool) { self.stopped = stopped; }
}

#[cfg(test)]
#[path = "vendor_wireless_station_driver_tests.rs"]
mod tests;
