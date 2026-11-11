//! Station service transactions above the adapter's typed driver command engine.
//! Driver acknowledgement and association indication are separate events.

use super::super::vendor_wireless_network::{MacAddress, Ssid};
use super::super::vendor_wireless_station_driver::StationDriverError;
#[path = "wireless_station_driver_bridge.rs"]
mod driver_bridge;
pub use driver_bridge::StationDrive;
#[path = "wireless_crypto.rs"]
pub mod crypto;
#[path = "wireless_station_profile.rs"]
pub mod profile;
#[path = "wireless_station_handshake.rs"]
pub mod handshake;
#[path = "wireless_station_wps.rs"]
pub mod wps;
#[path = "wireless_station_provisioning.rs"]
mod provisioning;
#[path = "wireless_station_random.rs"]
mod random_bridge;
#[path = "wireless_station_scan.rs"]
mod scan_bridge;
#[path = "wireless_station_security.rs"]
pub mod security;
pub use wps::{WpsConfiguration, WpsError, WpsMethod};
pub use handshake::{EapolFrame, HandshakeError, KeyInstall, KeyMaterial, KeySlot};
pub use profile::{Cipher, Credentials, InformationElements, KeyIndex, PersonalProtocol, ProfileError, SelectedAccessPoint, StationProfile, WepKey, WpsPin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationOperationId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRequestId { operation: StationOperationId, serial: u64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverStatus(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationState { Disabled, Disconnected, Selecting, Associating, FourWayHandshake, GroupHandshake, Completed, Scanning, Provisioning, Cancelling, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationError { Busy, Disabled, NoProfile, NoAccessPoint, StaleReply, WrongReply, ExhaustedIds, QueueCapacity, Profile(ProfileError), Scan(super::super::vendor_wireless_network::ScanError), Handshake(HandshakeError), Wps(WpsError), Random(crypto::random::RandomError), Driver(DriverStatus), DriverCpu(StationDriverError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationOutcome { Connected(MacAddress), Disconnected, ScanComplete, Cancelled, Failed(StationError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationCompletion { operation: StationOperationId, outcome: StationOutcome }
#[derive(Debug, Eq, PartialEq)]
pub struct SupplicantStopped { operation: StationOperationId }
impl SupplicantStopped { pub const fn operation(&self) -> StationOperationId { self.operation } }
impl StationCompletion {
    pub const fn operation(self) -> StationOperationId { self.operation }
    pub const fn outcome(self) -> StationOutcome { self.outcome }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StationRequest { Select(StationProfile), SelectDefault, ClearConnected, Scan, ScanPrefix(Ssid), Leave, ApplyWps(WpsConfiguration), CancelWps }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceMode { Infrastructure }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authentication { Open, WpaPersonal, RsnPersonal }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encryption { Disabled, Wep, Tkip, Ccmp }
#[derive(Debug)]
pub enum DriverAction {
    SetInterfaceMode(InterfaceMode),
    SetAuthentication(Authentication),
    SetEncryption(Encryption),
    SetSecurityElements(InformationElements),
    SelectBssid(MacAddress),
    InstallKey(KeyInstall),
    RemoveKey(KeySlot),
    Disconnect,
    Scan { ssid: Option<Ssid> },
    TransmitEapol { destination: MacAddress, frame: EapolFrame },
    RandomNonce,
    TransmitWps { destination: MacAddress, frame: wps::WpsFrame },
    ArmWpsTimer(TimerDurationMs),
    CancelWpsTimer,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerDurationMs(pub u32);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanCandidate { pub ssid: Ssid, pub privacy: bool, pub signal: super::super::vendor_wireless_network::SignalDbm, pub mode: super::super::vendor_wireless_network::NetworkMode, pub key_management: super::super::vendor_wireless_network::KeyManagement, pub access_point: SelectedAccessPoint }
pub struct ScanBatch { candidates: [Option<ScanCandidate>; 64], length: usize }
impl ScanBatch {
    pub fn new() -> Self { Self { candidates: core::array::from_fn(|_| None), length: 0 } }
    pub fn append(&mut self, candidate: ScanCandidate) -> Result<(), StationError> {
        if self.length == self.candidates.len() { return Err(StationError::QueueCapacity); }
        self.candidates[self.length] = Some(candidate); self.length += 1; Ok(())
    }
    pub fn candidates(&self) -> impl Iterator<Item = &ScanCandidate> { self.candidates[..self.length].iter().flatten() }
    fn retain_prefix(&mut self, prefix: Ssid) {
        let mut retained = 0;
        for index in 0..self.length {
            if self.candidates[index].as_ref().is_some_and(|candidate| candidate.ssid.as_bytes().starts_with(prefix.as_bytes())) {
                self.candidates[retained] = self.candidates[index].take();
                retained += 1;
            } else { self.candidates[index] = None; }
        }
        self.length = retained;
    }
}
impl Default for ScanBatch { fn default() -> Self { Self::new() } }
pub struct DriverCommand { id: DriverRequestId, action: DriverAction }
impl DriverCommand {
    pub const fn id(&self) -> DriverRequestId { self.id }
    pub fn action(&self) -> &DriverAction { &self.action }
    pub fn acknowledge(self, result: Result<(), DriverStatus>) -> Result<DriverReply, StationError> {
        if matches!(self.action, DriverAction::Scan { .. } | DriverAction::RandomNonce) && result.is_ok() { return Err(StationError::WrongReply); }
        Ok(DriverReply { id: self.id, payload: ReplyPayload::Acknowledged(result) })
    }
    pub fn scanned(self, results: ScanBatch) -> Result<DriverReply, StationError> {
        if !matches!(self.action, DriverAction::Scan { .. }) { return Err(StationError::WrongReply); }
        Ok(DriverReply { id: self.id, payload: ReplyPayload::Scanned(results) })
    }
    #[cfg(test)]
    fn nonce(self, nonce: [u8; 32]) -> Result<DriverReply, StationError> {
        if !matches!(self.action, DriverAction::RandomNonce) { return Err(StationError::WrongReply); }
        Ok(DriverReply { id: self.id, payload: ReplyPayload::Nonce(nonce) })
    }
}
pub struct DriverReply { id: DriverRequestId, payload: ReplyPayload }
enum ReplyPayload { Acknowledged(Result<(), DriverStatus>), CpuAcknowledged(Result<(), StationDriverError>), Scanned(ScanBatch), Nonce([u8; 32]), RandomFailure(crypto::random::RandomError) }
#[derive(Clone, Copy)]
enum Continuation { Next, Scan, Nonce, Association, Key(KeySlot), RemoveKey(KeySlot), CompleteHandshake, FinishDisconnect }
struct Queued { action: DriverAction, continuation: Continuation }
struct Pending { id: DriverRequestId, continuation: Continuation }
struct DeferredEapol { peer: MacAddress, bytes: [u8; 1536], length: usize }

pub struct StationRuntime {
    local: MacAddress,
    state: StationState,
    operation: Option<StationOperationId>,
    session: Option<StationOperationId>,
    scan_state: StationState,
    scan_prefix: Option<Ssid>,
    serial: u64,
    generation: u64,
    pending: Option<Pending>,
    drive_pending: Option<driver_bridge::PendingControl>,
    queue: [Option<Queued>; 8],
    queued: usize,
    profile: Option<StationProfile>,
    access_point: Option<SelectedAccessPoint>,
    supplicant: Option<handshake::RsnSupplicant>,
    installed: [Option<KeySlot>; 5],
    completion: Option<StationCompletion>,
    terminal: Option<StationOutcome>,
    scan_results: ScanBatch,
    provisioning: Option<wps::WpsEnrollee>,
    deferred_eapol: Option<DeferredEapol>,
    last_wps_configuration: Option<WpsConfiguration>,
    wps_credential: Option<wps::WpsCredential>,
}
impl StationRuntime {
    pub fn new(local: MacAddress) -> Self {
        Self { local, state: StationState::Disconnected, operation: None, session: None, scan_state: StationState::Disconnected, scan_prefix: None, serial: 0, generation: 0, pending: None, drive_pending: None, queue: core::array::from_fn(|_| None), queued: 0, profile: None, access_point: None, supplicant: None, installed: [None; 5], completion: None, terminal: None, scan_results: ScanBatch::new(), provisioning: None, deferred_eapol: None, last_wps_configuration: None, wps_credential: None }
    }
    pub const fn state(&self) -> StationState { self.state }
    pub const fn completion(&self) -> Option<StationCompletion> { self.completion }
    pub const fn current_operation(&self) -> Option<StationOperationId> { self.operation }
    pub fn scan_results(&self) -> &ScanBatch { &self.scan_results }
    pub fn profile(&self) -> Option<&StationProfile> { self.profile.as_ref() }
    pub fn finish_shutdown(&mut self) -> Result<SupplicantStopped, StationError> {
        if self.state != StationState::Disconnected || self.operation.is_some() || self.session.is_some() || self.pending.is_some() || self.drive_pending.is_some() || self.queued != 0 || self.installed.iter().any(Option::is_some) || self.supplicant.is_some() || self.provisioning.is_some() {
            return Err(StationError::Busy);
        }
        let completion = self.completion.filter(|completion| matches!(completion.outcome, StationOutcome::Disconnected | StationOutcome::Cancelled)).ok_or(StationError::WrongReply)?;
        self.profile = None; self.wps_credential = None; self.last_wps_configuration = None;
        self.access_point = None; self.deferred_eapol = None; self.scan_prefix = None;
        self.scan_results = ScanBatch::new(); self.terminal = None;
        self.state = StationState::Disabled;
        Ok(SupplicantStopped { operation: completion.operation })
    }
    pub fn submit(&mut self, request: StationRequest) -> Result<StationOperationId, StationError> {
        if matches!(request, StationRequest::CancelWps) && self.provisioning.is_some() {
            let operation = self.operation.ok_or(StationError::WrongReply)?;
            self.cancel(operation)?;
            return Ok(operation);
        }
        if self.operation.is_some() { return Err(StationError::Busy); }
        if self.state == StationState::Disabled { return Err(StationError::Disabled); }
        if matches!(request, StationRequest::SelectDefault) && self.profile.is_none() { return Err(StationError::NoProfile); }
        self.generation = self.generation.checked_add(1).ok_or(StationError::ExhaustedIds)?;
        let operation = StationOperationId(self.generation);
        self.operation = Some(operation); self.completion = None; self.terminal = None;
        match request {
            StationRequest::Select(profile) => { self.profile = Some(profile); self.begin_selection()?; }
            StationRequest::SelectDefault => self.begin_selection()?,
            StationRequest::Scan => { self.scan_prefix = None; self.scan_state = self.state; self.state = StationState::Scanning; self.enqueue(DriverAction::Scan { ssid: None }, Continuation::Scan)?; }
            StationRequest::ScanPrefix(prefix) => {
                self.scan_prefix = Some(Ssid::parse(&prefix.as_bytes()[..prefix.as_bytes().len().min(8)]).map_err(|_| StationError::Profile(ProfileError::Elements))?);
                self.scan_state = self.state; self.state = StationState::Scanning;
                self.enqueue(DriverAction::Scan { ssid: None }, Continuation::Scan)?;
            }
            StationRequest::ClearConnected if !matches!(self.state, StationState::Completed | StationState::FourWayHandshake | StationState::GroupHandshake) => { self.finish(StationOutcome::Disconnected); }
            StationRequest::ClearConnected | StationRequest::Leave => self.begin_cleanup(StationOutcome::Disconnected)?,
            StationRequest::ApplyWps(configuration) => self.start_provisioning(configuration)?,
            StationRequest::CancelWps => self.finish(StationOutcome::Cancelled),
        }
        Ok(operation)
    }
    fn begin_selection(&mut self) -> Result<(), StationError> {
        self.session = None;
        self.supplicant = None;
        self.state = StationState::Selecting;
        self.enqueue(DriverAction::Disconnect, Continuation::Next)?;
        self.enqueue_key_removal()?;
        let ssid = self.profile.as_ref().ok_or(StationError::NoProfile)?.ssid;
        self.enqueue(DriverAction::Scan { ssid: Some(ssid) }, Continuation::Scan)
    }
    pub fn poll(&mut self) -> Option<DriverCommand> {
        if self.pending.is_some() || self.queued == 0 { return None; }
        let operation = self.operation?;
        let Some(serial) = self.serial.checked_add(1) else { self.finish(StationOutcome::Failed(StationError::ExhaustedIds)); return None; };
        self.serial = serial;
        let queued = self.queue[0].take()?;
        for index in 1..self.queued { self.queue[index - 1] = self.queue[index].take(); }
        self.queued -= 1;
        let id = DriverRequestId { operation, serial };
        self.pending = Some(Pending { id, continuation: queued.continuation });
        if let Continuation::Key(slot) = queued.continuation { self.installed[key_slot_index(slot)] = Some(slot); }
        Some(DriverCommand { id, action: queued.action })
    }
    pub fn receive(&mut self, reply: DriverReply) -> Result<(), StationError> {
        let pending = self.pending.as_ref().ok_or(StationError::StaleReply)?;
        if pending.id != reply.id || self.operation != Some(reply.id.operation) { return Err(StationError::StaleReply); }
        let continuation = pending.continuation;
        if !matches!((&reply.payload, continuation), (ReplyPayload::Scanned(_), Continuation::Scan) | (ReplyPayload::Nonce(_), Continuation::Nonce) | (ReplyPayload::RandomFailure(_), Continuation::Nonce) | (ReplyPayload::Acknowledged(Err(_)) | ReplyPayload::CpuAcknowledged(Err(_)), _) | (ReplyPayload::Acknowledged(Ok(())) | ReplyPayload::CpuAcknowledged(Ok(())), Continuation::Next | Continuation::Association | Continuation::Key(_) | Continuation::RemoveKey(_) | Continuation::CompleteHandshake | Continuation::FinishDisconnect)) { return Err(StationError::WrongReply); }
        self.pending = None;
        let failure = match &reply.payload {
            ReplyPayload::Acknowledged(Err(status)) => Some(StationError::Driver(*status)),
            ReplyPayload::CpuAcknowledged(Err(error)) => Some(StationError::DriverCpu(*error)),
            ReplyPayload::RandomFailure(error) => Some(StationError::Random(*error)),
            _ => None,
        };
        if let Some(error) = failure {
            if self.state == StationState::Cancelling { self.finish(StationOutcome::Failed(error)); }
            else { self.begin_cleanup(StationOutcome::Failed(error))?; }
            return Err(error);
        }
        if self.state == StationState::Cancelling && !matches!(continuation, Continuation::RemoveKey(_) | Continuation::FinishDisconnect) { return Ok(()); }
        match reply.payload {
            ReplyPayload::Scanned(results) => {
                if self.provisioning.is_some() { self.receive_wps_scan(results)?; }
                else if self.state == StationState::Scanning {
                    let mut results = results;
                    if let Some(prefix) = self.scan_prefix.take() { results.retain_prefix(prefix); }
                    self.scan_results = results; self.finish(StationOutcome::ScanComplete);
                }
                else {
                    let profile = self.profile.as_ref().ok_or(StationError::NoProfile)?;
                    let candidate = results.candidates().filter(|candidate| candidate.mode == super::super::vendor_wireless_network::NetworkMode::Infrastructure && candidate.ssid == profile.ssid && match &profile.credentials {
                        Credentials::Open => !candidate.privacy,
                        Credentials::Wep { .. } => candidate.privacy && matches!(candidate.access_point.pairwise, Cipher::None | Cipher::Wep40 | Cipher::Wep104),
                        Credentials::Personal(_) => candidate.key_management == super::super::vendor_wireless_network::KeyManagement::Personal && candidate.privacy && matches!(candidate.access_point.pairwise, Cipher::Tkip | Cipher::Ccmp) && matches!(candidate.access_point.group, Cipher::Wep40 | Cipher::Wep104 | Cipher::Tkip | Cipher::Ccmp),
                    }).max_by_key(|candidate| candidate.signal.0).map(|candidate| candidate.access_point.clone());
                    self.scan_results = results;
                    match candidate {
                        Some(access_point) => { self.access_point = Some(access_point); if matches!(profile.credentials, Credentials::Personal(_)) { self.enqueue(DriverAction::RandomNonce, Continuation::Nonce)?; } else { self.prepare_association()?; } }
                        None => { self.finish(StationOutcome::Failed(StationError::NoAccessPoint)); return Err(StationError::NoAccessPoint); }
                    }
                }
            }
            ReplyPayload::Nonce(nonce) => {
                if let Some(provisioning) = &mut self.provisioning {
                    provisioning.supply_entropy(nonce).map_err(StationError::Wps)?;
                } else if let Some(deferred) = self.deferred_eapol.take() {
                    self.supplicant.as_mut().ok_or(StationError::WrongReply)?.begin_rekey(nonce).map_err(StationError::Handshake)?;
                    let transaction = self.supplicant.as_mut().ok_or(StationError::WrongReply)?.receive(deferred.peer, &deferred.bytes[..deferred.length]).map_err(StationError::Handshake)?;
                    self.accept_handshake_transaction(deferred.peer, transaction)?;
                } else {
                    let profile = self.profile.as_ref().ok_or(StationError::NoProfile)?;
                    let Credentials::Personal(master) = &profile.credentials else { return Err(StationError::WrongReply); };
                    let access_point = self.access_point.clone().ok_or(StationError::NoAccessPoint)?;
                    self.supplicant = Some(handshake::RsnSupplicant::new(self.local, access_point, master.clone(), nonce).map_err(StationError::Handshake)?);
                    self.prepare_association()?;
                }
            }
            ReplyPayload::Acknowledged(Ok(())) | ReplyPayload::CpuAcknowledged(Ok(())) => match continuation {
                Continuation::Association => self.state = StationState::Associating,
                Continuation::Key(_) => (),
                Continuation::RemoveKey(slot) => self.installed[key_slot_index(slot)] = None,
                Continuation::CompleteHandshake => { let peer = self.access_point.as_ref().ok_or(StationError::NoAccessPoint)?.address; self.finish(StationOutcome::Connected(peer)); }
                Continuation::FinishDisconnect => { let outcome = self.terminal.take().unwrap_or(StationOutcome::Disconnected); self.finish(outcome); }
                _ => (),
            },
            ReplyPayload::Acknowledged(Err(_)) | ReplyPayload::CpuAcknowledged(Err(_)) | ReplyPayload::RandomFailure(_) => (),
        }
        self.pump_provisioning()?;
        Ok(())
    }
    fn prepare_association(&mut self) -> Result<(), StationError> {
        let profile = self.profile.as_ref().ok_or(StationError::NoProfile)?;
        let peer = self.access_point.as_ref().ok_or(StationError::NoAccessPoint)?.address;
        let (authentication, encryption, wep) = match &profile.credentials {
            Credentials::Open => (Authentication::Open, Encryption::Disabled, None),
            Credentials::Wep { index, key } => (Authentication::Open, Encryption::Wep, Some(KeyInstall { slot: KeySlot { index: *index, peer: None }, transmit: true, material: KeyMaterial::Wep(key.clone()), receive_sequence: [0; 6] })),
            Credentials::Personal(_) => {
                let access_point = self.access_point.as_ref().ok_or(StationError::NoAccessPoint)?;
                let authentication = match access_point.protocol { PersonalProtocol::Wpa => Authentication::WpaPersonal, PersonalProtocol::Rsn => Authentication::RsnPersonal, PersonalProtocol::None => return Err(StationError::WrongReply) };
                let encryption = if access_point.pairwise == Cipher::Ccmp || access_point.group == Cipher::Ccmp { Encryption::Ccmp } else if access_point.pairwise == Cipher::Tkip || access_point.group == Cipher::Tkip { Encryption::Tkip } else { Encryption::Wep };
                (authentication, encryption, None)
            }
        };
        self.enqueue(DriverAction::SetInterfaceMode(InterfaceMode::Infrastructure), Continuation::Next)?;
        self.enqueue(DriverAction::SetAuthentication(authentication), Continuation::Next)?;
        self.enqueue(DriverAction::SetEncryption(encryption), Continuation::Next)?;
        let elements = match &self.supplicant {
            Some(supplicant) => supplicant.association_elements().clone(),
            None => InformationElements::parse(&[]).map_err(StationError::Profile)?,
        };
        self.enqueue(DriverAction::SetSecurityElements(elements), Continuation::Next)?;
        if let Some(key) = wep { let slot = key.slot; self.enqueue(DriverAction::InstallKey(key), Continuation::Key(slot))?; }
        self.enqueue(DriverAction::SelectBssid(peer), Continuation::Association)
    }
    pub fn associated(&mut self, operation: StationOperationId, peer: MacAddress) -> Result<(), StationError> {
        if self.operation != Some(operation) { return Err(StationError::StaleReply); }
        if self.state != StationState::Associating || self.pending.is_some() { return Err(StationError::WrongReply); }
        if self.access_point.as_ref().map(|ap| ap.address) != Some(peer) { return Err(StationError::Handshake(HandshakeError::Peer)); }
        if let Some(provisioning) = &mut self.provisioning {
            provisioning.associate(peer).map_err(StationError::Wps)?;
            self.state = StationState::Provisioning;
            self.pump_provisioning()?;
        } else if self.supplicant.is_some() { self.state = StationState::FourWayHandshake; }
        else { self.finish(StationOutcome::Connected(peer)); }
        Ok(())
    }
    pub fn receive_eapol(&mut self, operation: StationOperationId, peer: MacAddress, frame: &[u8]) -> Result<(), StationError> {
        if self.operation != Some(operation) && !(self.operation.is_none() && self.session == Some(operation)) { return Err(StationError::StaleReply); }
        if self.pending.is_some() || self.queued != 0 { return Err(StationError::Busy); }
        if let Some(provisioning) = &mut self.provisioning {
            let result = provisioning.receive_eapol(peer, frame).map_err(StationError::Wps);
            self.pump_provisioning()?;
            return result;
        }
        let transaction = self.supplicant.as_mut().ok_or(StationError::WrongReply)?.receive(peer, frame);
        match transaction {
            Err(HandshakeError::NonceRequired) => {
                if frame.len() > 1536 { return Err(StationError::Handshake(HandshakeError::Truncated)); }
                let mut bytes = [0; 1536]; bytes[..frame.len()].copy_from_slice(frame);
                self.deferred_eapol = Some(DeferredEapol { peer, bytes, length: frame.len() });
                self.operation = Some(operation); self.completion = None;
                self.enqueue(DriverAction::RandomNonce, Continuation::Nonce)
            }
            Err(error) => {
                let failure = StationError::Handshake(error);
                if error.disconnects() {
                    self.operation = Some(operation); self.completion = None;
                    self.begin_cleanup(StationOutcome::Failed(failure))?;
                }
                Err(failure)
            }
            Ok(transaction) => { self.operation = Some(operation); self.completion = None; self.accept_handshake_transaction(peer, transaction) }
        }
    }
    fn accept_handshake_transaction(&mut self, peer: MacAddress, transaction: handshake::HandshakeTransaction) -> Result<(), StationError> {
        self.state = match self.supplicant.as_ref().ok_or(StationError::WrongReply)?.phase() { handshake::HandshakePhase::Group => StationState::GroupHandshake, _ => StationState::FourWayHandshake };
        if let Some(pairwise) = transaction.pairwise { let slot = pairwise.slot; self.enqueue(DriverAction::InstallKey(pairwise), Continuation::Key(slot))?; }
        if let Some(group) = transaction.group { let slot = group.slot; self.enqueue(DriverAction::InstallKey(group), Continuation::Key(slot))?; }
        self.enqueue(DriverAction::TransmitEapol { destination: peer, frame: transaction.reply }, if transaction.completes { Continuation::CompleteHandshake } else { Continuation::Next })
    }
    pub fn cancel(&mut self, operation: StationOperationId) -> Result<(), StationError> {
        if self.operation.is_none() && self.session == Some(operation) { self.operation = Some(operation); self.completion = None; }
        if self.operation != Some(operation) { return Err(StationError::StaleReply); }
        self.begin_cleanup(StationOutcome::Cancelled)
    }
    fn begin_cleanup(&mut self, outcome: StationOutcome) -> Result<(), StationError> {
        for entry in &mut self.queue { *entry = None; } self.queued = 0;
        self.supplicant = None; self.session = None; self.state = StationState::Cancelling; self.terminal = Some(outcome);
        self.deferred_eapol = None;
        if self.provisioning.take().is_some() { self.enqueue(DriverAction::CancelWpsTimer, Continuation::Next)?; }
        self.enqueue_key_removal()?;
        self.enqueue(DriverAction::Disconnect, Continuation::FinishDisconnect)
    }
    fn enqueue_key_removal(&mut self) -> Result<(), StationError> {
        for index in 0..self.installed.len() { if let Some(slot) = self.installed[index] { self.enqueue(DriverAction::RemoveKey(slot), Continuation::RemoveKey(slot))?; } }
        Ok(())
    }
    fn enqueue(&mut self, action: DriverAction, continuation: Continuation) -> Result<(), StationError> {
        if self.queued == self.queue.len() { return Err(StationError::QueueCapacity); }
        self.queue[self.queued] = Some(Queued { action, continuation }); self.queued += 1; Ok(())
    }
    fn finish(&mut self, outcome: StationOutcome) {
        if let Some(operation) = self.operation.take() {
            if matches!(outcome, StationOutcome::Connected(_)) { self.session = Some(operation); }
            self.completion = Some(StationCompletion { operation, outcome });
        }
        self.pending = None;
        for entry in &mut self.queue { *entry = None; } self.queued = 0;
        self.state = match outcome { StationOutcome::Connected(_) => StationState::Completed, StationOutcome::ScanComplete => self.scan_state, StationOutcome::Failed(_) => StationState::Failed, _ => StationState::Disconnected };
    }
}

fn key_slot_index(slot: KeySlot) -> usize { if slot.peer.is_some_and(|peer| peer.0 != [255; 6]) { 4 } else { slot.index.get() as usize } }

#[cfg(test)]
#[path = "wireless_station_tests.rs"]
mod tests;
