//! X-M1 1.50 protocol WLAN result and notification consumers.

use super::vendor_wireless_network::{MacAddress, Ssid};
use super::wireless_association::station::{StationRuntime, StationRequest, StationOperationId, StationOutcome, StationError, WpsConfiguration, WpsError};

#[path = "wireless_network_services_selftest.rs"]
pub mod selftest;
#[path = "wireless_network_services_remote.rs"]
pub mod remote;
#[path = "wireless_network_services_iperf.rs"]
pub mod iperf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigKey(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceSignal { Complete, InvalidParameter }
impl ServiceSignal {
    pub const fn bits(self) -> u32 { match self { Self::Complete => 0, Self::InvalidParameter => 0x8000_0000 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceText { Connected, Disconnected, ScanDone, ScanRead, WpsOk, WpsTimeout, WpsFailed, WpsError }
impl ServiceText {
    pub const fn bytes(self) -> &'static [u8] {
        match self { Self::Connected => b"CONNECTED", Self::Disconnected => b"DISCON.  ", Self::ScanDone => b"SCAN DONE ", Self::ScanRead => b"SCAN READ ", Self::WpsOk => b"WPS CB OK", Self::WpsTimeout => b"WPS TOUT ", Self::WpsFailed => b"WPS FAIL ", Self::WpsError => b"WPS CB ERR " }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayText { bytes: [u8; 32], length: u8 }
impl DisplayText { pub fn bytes(&self) -> &[u8] { &self.bytes[..usize::from(self.length)] } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceEffect { WriteByte { key: ConfigKey, value: u8 }, Signal(ServiceSignal), Display { text: DisplayText, x: u8, y: u8 } }
pub struct ServiceOutput { effects: [Option<ServiceEffect>; 256], read: usize, length: usize }
impl ServiceOutput {
    pub const fn new() -> Self { Self { effects: [None; 256], read: 0, length: 0 } }
    pub fn pop(&mut self) -> Option<ServiceEffect> {
        if self.length == 0 { return None; }
        let effect = self.effects[self.read].take();
        self.read = (self.read + 1) % self.effects.len(); self.length -= 1;
        effect
    }
    pub fn remaining(&self) -> usize { self.effects.len() - self.length }
    fn push(&mut self, effect: ServiceEffect) {
        assert!(self.remaining() != 0, "service effects must be drained");
        let index = (self.read + self.length) % self.effects.len();
        self.effects[index] = Some(effect); self.length += 1;
    }
    fn write_byte(&mut self, key: ConfigKey, value: u8) { self.push(ServiceEffect::WriteByte { key, value }); }
    fn signal(&mut self, signal: ServiceSignal) { self.push(ServiceEffect::Signal(signal)); }
    fn display(&mut self, text: &[u8], x: u8, y: u8) {
        let mut value = DisplayText { bytes: [0; 32], length: text.len() as u8 };
        value.bytes[..text.len()].copy_from_slice(text);
        self.push(ServiceEffect::Display { text: value, x, y });
    }
}
impl Default for ServiceOutput { fn default() -> Self { Self::new() } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceError { Busy, Station(StationError), ScanIndex, OutputCapacity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionKeys { pub result: ConfigKey }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanKeys { pub result: ConfigKey, pub count: ConfigKey }
#[derive(Clone, Copy)]
enum Pending { Connection { operation: StationOperationId, keys: ConnectionKeys }, Scan { operation: StationOperationId, keys: ScanKeys }, Wps { operation: StationOperationId, result: ConfigKey, connection: ConnectionKeys, credentials_published: bool } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WpsResult { Credentials, Timeout, Failure, Overlap, Error(u8) }
impl WpsResult {
    pub const fn code(self) -> u8 { match self { Self::Credentials => 0, Self::Timeout => 1, Self::Failure => 2, Self::Overlap => 3, Self::Error(code) => code } }
}

pub struct NetworkServices { pending: Option<Pending>, indicator: u8 }
impl NetworkServices {
    pub const fn new() -> Self { Self { pending: None, indicator: b'0' } }
    pub fn connect(&mut self, station: &mut StationRuntime, request: StationRequest, keys: ConnectionKeys, output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if self.pending.is_some() { return Err(ServiceError::Busy); }
        if output.remaining() < 1 { return Err(ServiceError::OutputCapacity); }
        let operation = station.submit(request).map_err(ServiceError::Station)?;
        output.write_byte(keys.result, 255);
        self.pending = Some(Pending::Connection { operation, keys });
        Ok(())
    }
    pub fn scan(&mut self, station: &mut StationRuntime, keys: ScanKeys, output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if self.pending.is_some() { return Err(ServiceError::Busy); }
        if output.remaining() < 1 { return Err(ServiceError::OutputCapacity); }
        let operation = station.submit(StationRequest::Scan).map_err(ServiceError::Station)?;
        output.write_byte(keys.result, 255);
        self.pending = Some(Pending::Scan { operation, keys });
        Ok(())
    }
    pub fn wps(&mut self, station: &mut StationRuntime, configuration: WpsConfiguration, result: ConfigKey, connection: ConnectionKeys, output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if self.pending.is_some() { return Err(ServiceError::Busy); }
        if output.remaining() < 1 { return Err(ServiceError::OutputCapacity); }
        let operation = station.submit(StationRequest::ApplyWps(configuration)).map_err(ServiceError::Station)?;
        output.write_byte(result, 255);
        self.pending = Some(Pending::Wps { operation, result, connection, credentials_published: false });
        Ok(())
    }
    pub fn poll(&mut self, station: &StationRuntime, output: &mut ServiceOutput) -> bool {
        let Some(mut pending) = self.pending else { return false; };
        let operation = match pending { Pending::Connection { operation, .. } | Pending::Scan { operation, .. } | Pending::Wps { operation, .. } => operation };
        let matches = station.current_operation() == Some(operation) || station.completion().is_some_and(|completion| completion.operation() == operation);
        if !matches { return false; }
        if let Pending::Wps { result, ref mut credentials_published, .. } = pending {
            if !*credentials_published {
                if let Some(credential) = station.wps_credential() {
                    if Self::wps_notification(WpsResult::Credentials, credential.ssid().as_bytes(), credential.key(), result, output).is_err() { return false; }
                    *credentials_published = true;
                    self.pending = Some(pending);
                }
            }
        }
        let Some(completion) = station.completion() else { return false; };
        if completion.operation() != operation { return false; }
        if output.remaining() < 6 { return false; }
        match pending {
            Pending::Connection { keys, .. } => {
                let result = match completion.outcome() { StationOutcome::Connected(_) => 0, _ => 1 };
                let _ = self.connection_notification(result, keys, output);
            }
            Pending::Scan { keys, .. } => {
                let result = if completion.outcome() == StationOutcome::ScanComplete { 0 } else { 1 };
                let count = station.scan_results().candidates().count() as u8;
                output.write_byte(keys.result, result);
                output.write_byte(keys.count, count);
                output.display(ServiceText::ScanDone.bytes(), 1, 4);
                output.display(&[count.wrapping_add(b'0')], 10, 4);
                if let Some(first) = station.scan_results().candidates().next() { output.display(first.ssid.as_bytes(), 1, 5); }
            }
            Pending::Wps { result, connection, credentials_published, .. } => {
                if !credentials_published {
                    let status = match completion.outcome() {
                        StationOutcome::Failed(StationError::Wps(WpsError::Timeout)) => WpsResult::Timeout,
                        StationOutcome::Failed(StationError::Wps(WpsError::Overlap)) => WpsResult::Overlap,
                        _ => WpsResult::Failure,
                    };
                    let _ = Self::wps_notification(status, &[], &[], result, output);
                }
                let result = if matches!(completion.outcome(), StationOutcome::Connected(_)) { 0 } else { 1 };
                let _ = self.connection_notification(result, connection, output);
            }
        }
        self.pending = None;
        output.signal(ServiceSignal::Complete);
        true
    }
    pub fn connection_notification(&mut self, result: u8, keys: ConnectionKeys, output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if output.remaining() < 3 { return Err(ServiceError::OutputCapacity); }
        output.display(&[self.indicator], 11, 3);
        self.indicator = if self.indicator == b'9' { b'0' } else { self.indicator + 1 };
        output.display(if result == 0 { ServiceText::Connected.bytes() } else { ServiceText::Disconnected.bytes() }, 1, 3);
        output.write_byte(keys.result, result);
        Ok(())
    }
    pub fn read_scan(&self, station: &StationRuntime, index: usize, keys: &[ConfigKey], output: &mut ServiceOutput) -> Result<(), ServiceError> {
        let ssid = station.scan_results().candidates().nth(index).ok_or(ServiceError::ScanIndex)?.ssid;
        if keys.len() < ssid.as_bytes().len() || output.remaining() < ssid.as_bytes().len() + 4 { return Err(ServiceError::OutputCapacity); }
        output.display(ServiceText::ScanRead.bytes(), 1, 4);
        output.display(&[(index as u8).wrapping_add(b'0'), b' ', (ssid.as_bytes().len() as u8).wrapping_add(b'0')], 10, 4);
        Self::publish_scan_ssid(ssid, keys, output)
    }
    pub fn publish_scan_ssid(ssid: Ssid, keys: &[ConfigKey], output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if keys.len() < ssid.as_bytes().len() { return Err(ServiceError::OutputCapacity); }
        if output.remaining() < ssid.as_bytes().len() + 2 { return Err(ServiceError::OutputCapacity); }
        for (&key, &byte) in keys.iter().zip(ssid.as_bytes()) { output.write_byte(key, byte); }
        output.display(ssid.as_bytes(), 1, 5);
        output.signal(ServiceSignal::Complete);
        Ok(())
    }
    pub fn query_strength(access_point: &super::vendor_wireless_access_point::AccessPoint, address: MacAddress, key: ConfigKey, output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if output.remaining() < 2 { return Err(ServiceError::OutputCapacity); }
        output.write_byte(key, access_point.peer_signal(address).unwrap_or(0));
        output.signal(ServiceSignal::Complete);
        Ok(())
    }
    pub fn wps_notification(result: WpsResult, ssid: &[u8], key: &[u8], result_key: ConfigKey, output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if ssid.len() > 32 || key.len() > 64 { return Err(ServiceError::OutputCapacity); }
        if output.remaining() < ssid.len() + key.len() + 2 { return Err(ServiceError::OutputCapacity); }
        let text = match result { WpsResult::Credentials => ServiceText::WpsOk, WpsResult::Timeout => ServiceText::WpsTimeout, WpsResult::Failure | WpsResult::Overlap => ServiceText::WpsFailed, WpsResult::Error(_) => ServiceText::WpsError };
        output.display(text.bytes(), 1, 4);
        if result == WpsResult::Credentials {
            for (index, &byte) in ssid.iter().enumerate() { output.write_byte(ConfigKey(0x3d40 + index as u16), byte); }
            for (index, &byte) in key.iter().enumerate() { output.write_byte(ConfigKey(0x3d62 + index as u16), byte); }
        }
        output.write_byte(result_key, result.code());
        Ok(())
    }
    pub fn complete_iperf(result: i32, key: ConfigKey, output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if output.remaining() < 2 { return Err(ServiceError::OutputCapacity); }
        output.write_byte(key, result as u8);
        output.signal(ServiceSignal::Complete);
        Ok(())
    }
    pub fn complete_reconfigure(address: Option<MacAddress>, keys: [ConfigKey; 6], output: &mut ServiceOutput) -> Result<(), ServiceError> {
        if output.remaining() < 7 { return Err(ServiceError::OutputCapacity); }
        if let Some(address) = address {
            for (key, byte) in keys.into_iter().zip(address.0) { output.write_byte(key, byte); }
        }
        output.signal(ServiceSignal::Complete);
        Ok(())
    }
}
impl Default for NetworkServices { fn default() -> Self { Self::new() } }

#[cfg(test)]
#[path = "wireless_network_services_tests.rs"]
mod tests;
