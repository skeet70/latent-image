use super::{Authentication, DriverAction, DriverCommand, DriverReply, Encryption, InterfaceMode, KeyMaterial, KeySlot, ReplyPayload, StationError, StationRuntime};
use super::super::super::vendor_wireless_network::MacAddress;
use super::super::super::vendor_wireless_station_driver::{self as driver, StationDriver, StationDriverError, StationDriverRequest, StationRequestId};
use driver::association::{ScanToken, AssociationError};

pub enum StationDrive { Idle, Advanced, Waiting, Clock(DriverCommand), Physical(DriverCommand) }
enum ControlOperation { Driver(StationRequestId), Scan(ScanToken) }
pub(super) struct PendingControl { command: DriverCommand, operation: Option<ControlOperation> }

impl StationRuntime {
    pub fn drive(&mut self, driver: &mut StationDriver) -> Result<StationDrive, StationError> {
        let pending = match self.drive_pending.take() {
            Some(pending) => pending,
            None => match self.poll() {
                Some(command) => PendingControl { command, operation: None },
                None => return Ok(StationDrive::Idle),
            },
        };
        if let Some(operation) = &pending.operation {
            match operation {
                ControlOperation::Driver(operation) => {
                    let Some(completion) = driver.completion().filter(|completion| completion.operation() == *operation) else {
                        self.drive_pending = Some(pending);
                        return Ok(StationDrive::Waiting);
                    };
                    self.receive(DriverReply { id: pending.command.id(), payload: ReplyPayload::CpuAcknowledged(completion.outcome()) })?;
                }
                ControlOperation::Scan(token) => {
                    if let Some((failed, error)) = driver.association().scan_failure() {
                        if failed == *token {
                            self.receive(DriverReply { id: pending.command.id(), payload: ReplyPayload::CpuAcknowledged(Err(StationDriverError::Association(error))) })?;
                            return Ok(StationDrive::Advanced);
                        }
                    }
                    if driver.association().scan_completion() != Some(*token) {
                        self.drive_pending = Some(pending);
                        return Ok(StationDrive::Waiting);
                    }
                    let results = match super::scan_bridge::completed_scan(driver.association()) {
                        Ok(results) => results,
                        Err(error) => {
                            self.pending = None;
                            self.begin_cleanup(super::StationOutcome::Failed(error))?;
                            return Err(error);
                        }
                    };
                    self.receive(DriverReply { id: pending.command.id(), payload: ReplyPayload::Scanned(results) })?;
                }
            }
            return Ok(StationDrive::Advanced);
        }
        let request = match pending.command.action() {
            DriverAction::SetInterfaceMode(InterfaceMode::Infrastructure) => StationDriverRequest::SetInterfaceMode(driver::InterfaceMode::Infrastructure),
            DriverAction::SetAuthentication(authentication) => StationDriverRequest::SetAuthentication(match authentication {
                Authentication::Open => driver::Authentication::Open,
                Authentication::WpaPersonal => driver::Authentication::WpaPsk,
                Authentication::RsnPersonal => driver::Authentication::Wpa2Psk,
            }),
            DriverAction::SetEncryption(encryption) => StationDriverRequest::SetEncryption(match encryption {
                Encryption::Disabled => driver::Encryption::Disabled,
                Encryption::Wep => driver::Encryption::Wep,
                Encryption::Tkip => driver::Encryption::Tkip,
                Encryption::Ccmp => driver::Encryption::Ccmp,
            }),
            DriverAction::SetSecurityElements(elements) => {
                let result = driver.association_mut().set_security_elements(elements.as_bytes()).map_err(StationDriverError::Association);
                self.receive(DriverReply { id: pending.command.id(), payload: ReplyPayload::CpuAcknowledged(result) })?;
                return Ok(StationDrive::Advanced);
            }
            DriverAction::SelectBssid(peer) => StationDriverRequest::SelectBssid(*peer),
            DriverAction::InstallKey(key) => {
                let material = match &key.material {
                    KeyMaterial::Wep(key) => match key.as_bytes() {
                        bytes if bytes.len() == 5 => { let mut array = [0; 5]; array.copy_from_slice(bytes); driver::KeyMaterial::Wep40(array) }
                        bytes => { let mut array = [0; 13]; array.copy_from_slice(bytes); driver::KeyMaterial::Wep104(array) }
                    },
                    KeyMaterial::Ccmp(key) => driver::KeyMaterial::Temporal128(*key),
                    KeyMaterial::Tkip(key) => {
                        let mut encoded = *key;
                        encoded[16..24].copy_from_slice(&key[24..32]);
                        encoded[24..32].copy_from_slice(&key[16..24]);
                        driver::KeyMaterial::Temporal256(encoded)
                    }
                };
                StationDriverRequest::InstallKey(driver::StationKey { slot: key_slot(key.slot)?, transmit: key.transmit, authenticator: false, material })
            }
            DriverAction::RemoveKey(slot) => StationDriverRequest::RemoveKey(key_slot(*slot)?),
            DriverAction::Disconnect => StationDriverRequest::StopStation,
            DriverAction::Scan { ssid } => {
                match driver.association_mut().begin_scan(*ssid) {
                    Ok(()) => {
                        let token = driver.association().scan_token().ok_or(StationError::WrongReply)?;
                        self.drive_pending = Some(PendingControl { command: pending.command, operation: Some(ControlOperation::Scan(token)) });
                    }
                    Err(AssociationError::Busy) => self.drive_pending = Some(pending),
                    Err(error) => {
                        self.receive(DriverReply { id: pending.command.id(), payload: ReplyPayload::CpuAcknowledged(Err(StationDriverError::Association(error))) })?;
                    }
                }
                return Ok(StationDrive::Waiting);
            }
            DriverAction::RandomNonce => return Ok(StationDrive::Clock(pending.command)),
            DriverAction::TransmitEapol { .. } | DriverAction::TransmitWps { .. } | DriverAction::ArmWpsTimer(_) | DriverAction::CancelWpsTimer => return Ok(StationDrive::Physical(pending.command)),
        };
        match driver.submit_identified(request) {
            Ok(operation) => { self.drive_pending = Some(PendingControl { command: pending.command, operation: Some(ControlOperation::Driver(operation)) }); Ok(StationDrive::Waiting) }
            Err(StationDriverError::Busy) => { self.drive_pending = Some(pending); Ok(StationDrive::Waiting) }
            Err(error) => {
                self.receive(DriverReply { id: pending.command.id(), payload: ReplyPayload::CpuAcknowledged(Err(error)) })?;
                Ok(StationDrive::Advanced)
            }
        }
    }
}

fn key_slot(slot: KeySlot) -> Result<driver::StationKeySlot, StationError> {
    let peer = slot.peer.unwrap_or(MacAddress([255; 6]));
    Ok(driver::StationKeySlot { index: driver::KeyIndex::new(slot.index.get()).map_err(StationError::DriverCpu)?, peer, pairwise: peer.0 != [255; 6] && peer.0 != [0; 6] })
}
