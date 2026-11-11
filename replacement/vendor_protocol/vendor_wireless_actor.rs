//! X-M1 wireless service storage and station-result projection.
//!
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3,
//! index entries 0x012ae1ec, 0x012ae36c, 0x012ae6b8, and 0x012ae72c.
//! Except for 0x012ae36c, these definitions are NONMATCHING; behavior is taken from the
//! authenticated overlay47 instructions, not their draft C types.
//! 0x012ae1ec reads a station address at byte offset 10, not word offset 10.
//!
//! Radio mode transitions and supplicant request execution are not owned
//! by this storage component. In particular, storing a configuration does
//! not apply it or complete either of its notification endpoints.

use super::vendor_wireless_network::{Bss, BssFlags, MacAddress, ScanRow, write_scan_rows};
use core::convert::TryInto;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceMode {
    Idle,
    Station,
    Transport,
    RadioTest,
}

impl ServiceMode {
    pub const fn from_abi(value: u32) -> Result<Self, WirelessError> {
        match value {
            0 => Ok(Self::Idle),
            1 => Ok(Self::Station),
            2 => Ok(Self::Transport),
            3 => Ok(Self::RadioTest),
            _ => Err(WirelessError::InvalidMode),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WirelessError {
    InvalidMode,
    InvalidConfigurationExtent,
    WrongMode,
    Cancelled,
    StationListUnavailable,
    OutputTooSmall,
}

impl WirelessError {
    pub const fn service_status(self) -> u32 {
        match self {
            Self::WrongMode => 0xffff_fffe,
            Self::Cancelled => 0xffff_ffee,
            Self::StationListUnavailable => 0xc001_0015,
            Self::InvalidMode | Self::InvalidConfigurationExtent | Self::OutputTooSmall => 0xffff_ffff,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkProfileKind(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkNameLength(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationEndpoint(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionEndpoint(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkName(pub [u8; 8]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WirelessConfiguration {
    pub profile: NetworkProfileKind,
    pub name: NetworkName,
    pub name_length: NetworkNameLength,
    pub address: MacAddress,
    reserved: [u8; 2],
    pub notification: NotificationEndpoint,
    pub completion: CompletionEndpoint,
}

impl WirelessConfiguration {
    pub fn from_abi(bytes: &[u8]) -> Result<Self, WirelessError> {
        let bytes: &[u8; 32] = bytes.try_into().map_err(|_| WirelessError::InvalidConfigurationExtent)?;
        Ok(Self {
            profile: NetworkProfileKind(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            name: NetworkName([bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11]]),
            name_length: NetworkNameLength(u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]])),
            address: MacAddress([bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21]]),
            reserved: [bytes[22], bytes[23]],
            notification: NotificationEndpoint(u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]])),
            completion: CompletionEndpoint(u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]])),
        })
    }

    pub fn as_abi(&self) -> [u8; 32] {
        let mut bytes = [0; 32];
        bytes[0..4].copy_from_slice(&self.profile.0.to_le_bytes());
        bytes[4..12].copy_from_slice(&self.name.0);
        bytes[12..16].copy_from_slice(&self.name_length.0.to_le_bytes());
        bytes[16..22].copy_from_slice(&self.address.0);
        bytes[22..24].copy_from_slice(&self.reserved);
        bytes[24..28].copy_from_slice(&self.notification.0.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.completion.0.to_le_bytes());
        bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationSignal(u8);

impl StationSignal {
    pub const fn from_observed_byte(value: u8) -> Self { Self(value) }

    pub const fn dbm(self) -> i8 {
        if self.0 > 220 { 0 } else { ((self.0 >> 1) as i16 - 110) as i8 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Station {
    pub address: MacAddress,
    pub signal: StationSignal,
}

pub enum StationList<'a> {
    Unavailable,
    Available(&'a [Station]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanPublicationCount(u8);

impl ScanPublicationCount {
    pub const fn get(self) -> u8 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionCompletion(u8);

impl ConnectionCompletion {
    pub const fn from_supplicant_code(code: i32) -> Self {
        if code >= 0 && code <= 11 {
            Self(code as u8 + 1)
        } else if code <= i32::MIN + 7 {
            Self(code.wrapping_sub(i32::MIN) as u8 + 13)
        } else {
            Self(0)
        }
    }

    pub const fn get(self) -> u8 { self.0 }
}

pub struct WirelessActor {
    mode: ServiceMode,
    cancelled: bool,
    address: MacAddress,
    configuration: Option<WirelessConfiguration>,
    scan_count: ScanPublicationCount,
    completion: ConnectionCompletion,
}

impl WirelessActor {
    pub const fn new(mode: ServiceMode, address: MacAddress) -> Self {
        Self { mode, cancelled: false, address, configuration: None, scan_count: ScanPublicationCount(0), completion: ConnectionCompletion(0) }
    }

    pub const fn mode(&self) -> ServiceMode { self.mode }

    pub fn set_cancelled(&mut self, cancelled: bool) { self.cancelled = cancelled; }

    pub const fn copy_configuration(&self) -> MacAddress { self.address }

    pub fn apply_completion_code(&mut self, code: i32) {
        self.completion = ConnectionCompletion::from_supplicant_code(code);
    }

    pub const fn connection_completion(&self) -> ConnectionCompletion { self.completion }

    pub fn set_configuration(&mut self, configuration: WirelessConfiguration) -> Result<(), WirelessError> {
        if self.mode != ServiceMode::Station { return Err(WirelessError::WrongMode); }
        if self.cancelled { return Err(WirelessError::Cancelled); }
        self.configuration = Some(configuration);
        Ok(())
    }

    pub const fn configuration(&self) -> Option<&WirelessConfiguration> { self.configuration.as_ref() }

    pub fn export_scan_rows(&mut self, supplicant: Option<&[Bss<'_>]>, output: &mut [ScanRow; 64]) -> Result<Option<ScanPublicationCount>, WirelessError> {
        if self.mode != ServiceMode::Station { return Err(WirelessError::WrongMode); }
        if self.cancelled { return Err(WirelessError::Cancelled); }
        let Some(input) = supplicant else { return Ok(None); };
        let written = write_scan_rows(input, output).map_err(|_| WirelessError::OutputTooSmall)?;
        if written < 64 { output[written].flags = BssFlags(0); }
        if written != 0 { self.scan_count = ScanPublicationCount(written as u8); }
        Ok(Some(self.scan_count))
    }

    pub fn enumerate_stations(&self, stations: StationList<'_>, output: &mut [u8]) -> Result<usize, WirelessError> {
        let StationList::Available(stations) = stations else { return Err(WirelessError::StationListUnavailable); };
        let extent = stations.len().checked_mul(12).ok_or(WirelessError::OutputTooSmall)?;
        if output.len() < extent { return Err(WirelessError::OutputTooSmall); }
        for (station, row) in stations.iter().zip(output.chunks_exact_mut(12)) {
            row[..6].copy_from_slice(&station.address.0);
            row[6] = station.signal.dbm() as u8;
        }
        Ok(stations.len())
    }
}

#[cfg(test)]
#[path = "vendor_wireless_actor_tests.rs"]
mod tests;
