//! X-M1 1.50 station message and management-frame CPU.
//! Source pin: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::{Authentication, Encryption, InterfaceMode};
use super::super::vendor_wireless_network::{MacAddress, SignalDbm, Ssid};
use super::super::vendor_wireless_peers::{NetIndex, PeerIndex, PeerLease, PeerPool, PeerState, PoolError};
use super::super::vendor_wireless_adapter::transmit::{ManagementMetadata, PacketKind, StationIndex};
#[path = "vendor_wireless_station_driver_association_frames.rs"]
pub mod frames;
#[path = "vendor_wireless_station_driver_association_ibss.rs"]
pub mod ibss;
#[path = "vendor_wireless_station_driver_association_elements.rs"]
pub mod elements;
#[path = "vendor_wireless_station_driver_association_roaming.rs"]
pub mod roaming;
use frames::{Elements, Management, Packet, half};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanToken { generation: u64, sequence: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelToken { generation: u64, sequence: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssociationToken { generation: u64, serial: u64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerKind { Rescan, IndependentAlone, Join, Management, Blacklist, Coexistence }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerToken { generation: u64, serial: u64, kind: TimerKind }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationIoError { Rejected, Timeout, Removed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationError { Busy, Capacity, TruncatedFrame, TruncatedElements, NotManagement, InvalidChannel, InvalidRates, InvalidResponse, ModeConflict, StaleCompletion, Pool(PoolError), Physical(AssociationIoError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationState { Idle, Search, Scan, OnlineScan, LookingFor, WaitForNextScan, RequestChannel, Join, IndependentAlone, IndependentMerge, Normal, Disconnecting }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationAlgorithm { Open = 0, Shared = 1, FastTransition = 2 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationRequest { SelectSsid(Ssid), SelectBssid(MacAddress), StopStation, SetInterfaceMode(InterfaceMode) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisconnectReason { Leaving = 3, Class3 = 7, MichaelFailure = 14 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityPublication { pub authentication: Authentication, pub encryption: Encryption }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkPublication { Associated { peer: MacAddress, bssid: MacAddress, association_id: u16 }, Disconnected, ScanFinished }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MicKey { Pairwise, Group }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MicFailure { pub bssid: MacAddress, pub key: MicKey }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind { Firmware, Management { protected: bool }, Entropy, ArmTimer { token: TimerToken, delay: Milliseconds }, CancelTimer(TimerToken), Publish(LinkPublication) }
#[derive(Debug, Eq, PartialEq)]
pub struct AssociationCommand { token: AssociationToken, kind: CommandKind, packet: Packet, metadata: Option<ManagementMetadata> }
impl AssociationCommand {
    pub fn token(&self) -> AssociationToken { self.token }
    pub fn kind(&self) -> CommandKind { self.kind }
    pub fn bytes(&self) -> &[u8] { self.packet.as_bytes() }
    pub fn metadata(&self) -> Option<ManagementMetadata> { self.metadata }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RxMergeIndicator { No, LargerTsf }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReceiveMetric(u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveMetadata { signal: SignalDbm, channel: Option<u8>, independent_merge: RxMergeIndicator, metric: ReceiveMetric }
impl ReceiveMetadata {
    pub fn parse(signal: SignalDbm, descriptor: &[u8]) -> Result<Self, AssociationError> {
        if descriptor.len() < 11 { return Err(AssociationError::TruncatedFrame); }
        let channel = if descriptor[10] > 241 { descriptor[10].wrapping_add(15) } else { descriptor[10] };
        if channel == 0 { return Err(AssociationError::InvalidChannel); }
        Ok(Self { signal, channel: Some(channel), independent_merge: if descriptor[5] & 8 != 0 { RxMergeIndicator::LargerTsf } else { RxMergeIndicator::No }, metric: ReceiveMetric(descriptor[9]) })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateMode { Infrastructure, Independent }
#[derive(Debug, Eq, PartialEq)]
pub struct Candidate {
    pub peer: MacAddress,
    pub bssid: MacAddress,
    pub ssid: Ssid,
    pub mode: CandidateMode,
    pub signal: SignalDbm,
    pub channel: u8,
    pub capabilities: u16,
    pub beacon_interval: u16,
    independent_merge: RxMergeIndicator,
    metric: ReceiveMetric,
    rates: [u8; 15],
    rates_length: u8,
    failures: u8,
    rate_set: elements::RateSet,
    last_failure: u32,
    elements: [u8; 600],
    elements_length: usize,
}
impl Candidate {
    pub fn rates(&self) -> &[u8] { &self.rates[..self.rates_length as usize] }
    pub fn elements(&self) -> &[u8] { &self.elements[..self.elements_length] }
    pub fn parse_beacon(bytes: &[u8], signal: SignalDbm) -> Result<Self, AssociationError> {
        let frame = Management::parse(bytes)?;
        if !matches!(frame.control & 0xfc, 0x80 | 0x50) { return Err(AssociationError::InvalidResponse); }
        Self::from_frame(&frame, ReceiveMetadata { signal, channel: None, independent_merge: RxMergeIndicator::No, metric: ReceiveMetric(0) })
    }
    fn from_frame(frame: &Management<'_>, metadata: ReceiveMetadata) -> Result<Self, AssociationError> {
        if frame.body.len() < 12 { return Err(AssociationError::TruncatedFrame); }
        let elements = Elements::parse(&frame.body[12..])?;
        if frame.body.len() - 12 > 600 { return Err(AssociationError::Capacity); }
        let ssid = Ssid::parse(elements.get(0).unwrap_or(&[])).map_err(|_| AssociationError::InvalidResponse)?;
        let channel = elements.get(3).or_else(|| elements.get(61)).and_then(|value| value.first()).copied().or(metadata.channel).ok_or(AssociationError::InvalidChannel)?;
        if channel == 0 { return Err(AssociationError::InvalidChannel); }
        let rate_set = elements::RateSet::parse(channel, &frame.body[12..]).map_err(|_| AssociationError::InvalidRates)?;
        let capabilities = half(frame.body, 10)?;
        let mode = if capabilities & 1 != 0 { CandidateMode::Infrastructure } else if capabilities & 2 != 0 { CandidateMode::Independent } else { return Err(AssociationError::InvalidResponse); };
        let mut result = Self { peer: frame.source, bssid: frame.bssid, ssid, mode, signal: metadata.signal, channel, independent_merge: metadata.independent_merge, metric: metadata.metric,
            capabilities, beacon_interval: half(frame.body, 8)?, rates: [0; 15], rates_length: 0, failures: 0, last_failure: 0,
            rate_set, elements: [0; 600], elements_length: frame.body.len() - 12 };
        result.elements[..result.elements_length].copy_from_slice(&frame.body[12..]);
        for rates in [elements.get(1), elements.get(50)].into_iter().flatten() {
            for rate in rates {
                if result.rates_length as usize == result.rates.len() { return Err(AssociationError::InvalidRates); }
                result.rates[result.rates_length as usize] = *rate;
                result.rates_length += 1;
            }
        }
        if result.rates_length == 0 { return Err(AssociationError::InvalidRates); }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanBand { Both, Ghz2, Ghz5, Specified }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanType { Passive, Active }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanChannel { band: ScanBand, number: u8 }
impl ScanChannel {
    pub fn parse(band: ScanBand, number: u8) -> Result<Self, AssociationError> {
        if !matches!((band, number), (ScanBand::Ghz2, 1..=14) | (ScanBand::Ghz5, 15..=196)) { return Err(AssociationError::InvalidChannel); }
        Ok(Self { band, number })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanPlan {
    ssids: [Option<Ssid>; 4],
    channels: [Option<ScanChannel>; 32],
    scan_type: ScanType,
    band: ScanBand,
    dwell: Milliseconds,
    probe_delay: Milliseconds,
    multiple: bool,
}
impl ScanPlan {
    pub fn new(ssids: &[Ssid], channels: &[ScanChannel], scan_type: ScanType,
        band: ScanBand, dwell: Milliseconds, probe_delay: Milliseconds) -> Result<Self, AssociationError> {
        if ssids.len() > 4 || channels.len() > 32 || dwell.0 > u16::MAX as u32 || probe_delay.0 > 255 { return Err(AssociationError::Capacity); }
        let mut result = Self { ssids: [None; 4], channels: [None; 32], scan_type, band, dwell, probe_delay, multiple: ssids.len() > 1 };
        for (output, ssid) in result.ssids.iter_mut().zip(ssids) { *output = Some(*ssid); }
        for (output, channel) in result.channels.iter_mut().zip(channels) { *output = Some(*channel); }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManagementState { Idle, Authenticate, WaitAuthentication, Challenge, WaitChallenge, Associate, WaitAssociation }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action { Search, RemovePeer(PeerLease, MacAddress), Scan, AbortScan(u8), RequestChannel, ReleaseChannel(u8), Authenticate, Associate, Peer(PeerState, u16), ConfigureBss(u16), Timing(u16), Edca, Radio, Entropy, Roaming(roaming::RoamingCommand), Class3(MacAddress, MacAddress), Deauthenticate(usize, DisconnectReason), Arm(TimerToken, Milliseconds), Cancel(TimerToken), Publish(LinkPublication) }

pub struct Association {
    state: AssociationState,
    previous: AssociationState,
    mode: InterfaceMode,
    security: SecurityPublication,
    local: MacAddress,
    target_ssid: Option<Ssid>,
    local_phy: elements::PhySet,
    allowed_rates: elements::RateMask,
    target_bssid: Option<MacAddress>,
    enabled: bool,
    secondary_active: bool,
    connected: Option<MacAddress>,
    candidates: [Option<Candidate>; 64],
    selected: Option<usize>,
    leases: [Option<PeerLease>; 64],
    needs_peer: bool,
    begin_independent: bool,
    peer_admitted: bool,
    pending_pool_admitted: bool,
    pending_remove: Option<PeerLease>,
    release_pending: Option<PeerLease>,
    elements: elements::AssociationElements,
    element_configuration: elements::ElementConfiguration,
    ibss: ibss::Ibss,
    independent_configuration: ibss::CreationParameters,
    pending_ibss: bool,
    scanned_for_target: bool,
    restart_after_independent: bool,
    disconnect_pending: bool,
    merge_pending: bool,
    roaming: roaming::Roaming,
    management: ManagementState,
    algorithm: AuthenticationAlgorithm,
    attempts: u8,
    join_attempts: u8,
    challenge: [u8; 255],
    challenge_length: u8,
    security_elements: [u8; 600],
    security_length: usize,
    scan_extra: [u8; 600],
    scan_extra_length: usize,
    scan_plan: Option<ScanPlan>,
    generation: u64,
    serial: u64,
    device_sequence: u8,
    scan_sequence: u8,
    channel_sequence: u8,
    scan_finished_pending: Option<ScanToken>,
    scan_finished: Option<ScanToken>,
    scan_failure: Option<(ScanToken, AssociationError)>,
    scan_active: bool,
    channel_active: bool,
    timers: [Option<TimerToken>; 6],
    actions: [Option<Action>; 40],
    action_read: usize,
    action_length: usize,
    pending: Option<AssociationCommand>,
    pending_phase: Option<ManagementState>,
    stopping: bool,
    disconnect_reason: Option<DisconnectReason>,
    publication: Option<LinkPublication>,
    mic_failure: Option<MicFailure>,
    failure: Option<AssociationError>,
    now: u32,
}
impl Association {
    pub const fn new() -> Self {
        Self { state: AssociationState::Idle, previous: AssociationState::Idle, mode: InterfaceMode::Infrastructure,
            local_phy: elements::PhySet::STOCK, allowed_rates: elements::RateMask::LEGACY,
            security: SecurityPublication { authentication: Authentication::Open, encryption: Encryption::Disabled },
            local: MacAddress([0; 6]), target_ssid: None, target_bssid: None, enabled: false, secondary_active: false,
            connected: None, candidates: [const { None }; 64], selected: None, management: ManagementState::Idle,
            leases: [None; 64], needs_peer: false, begin_independent: false, peer_admitted: false,
            pending_pool_admitted: false, pending_remove: None, release_pending: None,
            elements: elements::AssociationElements::new(), element_configuration: elements::ElementConfiguration::new(),
            ibss: ibss::Ibss::new(), independent_configuration: ibss::CreationParameters::new(ibss::Band::Ghz2, 1),
            pending_ibss: false, scanned_for_target: false, restart_after_independent: false, disconnect_pending: false, merge_pending: false, roaming: roaming::Roaming::new(),
            algorithm: AuthenticationAlgorithm::Open, attempts: 0, join_attempts: 0, challenge: [0; 255], challenge_length: 0,
            security_elements: [0; 600], security_length: 0, scan_extra: [0; 600], scan_extra_length: 0, scan_plan: None,
            generation: 0, serial: 0, device_sequence: 0, scan_sequence: 0, channel_sequence: 0,
            scan_finished_pending: None, scan_finished: None, scan_failure: None,
            scan_active: false, channel_active: false, timers: [None; 6], actions: [None; 40], action_read: 0, action_length: 0,
            pending: None, pending_phase: None, stopping: false, disconnect_reason: None, publication: None, mic_failure: None, failure: None, now: 0 }
    }
    pub fn state(&self) -> AssociationState { self.state }
    pub fn previous_state(&self) -> AssociationState { self.previous }
    pub fn publication(&self) -> Option<LinkPublication> { self.publication }
    pub fn scan_completion(&self) -> Option<ScanToken> { self.scan_finished }
    pub fn scan_failure(&self) -> Option<(ScanToken, AssociationError)> { self.scan_failure }
    pub fn failure(&self) -> Option<AssociationError> { self.failure }
    pub fn command(&self) -> Option<&AssociationCommand> { self.pending.as_ref() }
    pub fn candidates(&self) -> impl Iterator<Item = &Candidate> { self.candidates.iter().flatten() }
    pub fn selected_candidate(&self) -> Option<&Candidate> { self.selected.and_then(|index| self.candidates[index].as_ref()) }
    pub fn configure_phy(&mut self, phy: elements::PhySet, rates: elements::RateMask) { self.local_phy = phy; self.allowed_rates = rates; self.refresh_elements(); }
    pub fn peer_lease(&self) -> Option<PeerLease> { self.selected.and_then(|index| self.leases[index]) }
    fn peer_index(&self) -> Result<PeerIndex, AssociationError> { self.peer_lease().map(|lease| lease.index()).ok_or(AssociationError::InvalidResponse) }
    pub fn disconnect(&mut self, reason: DisconnectReason) -> Result<(), AssociationError> {
        self.disconnect_reason = Some(reason);
        self.request(AssociationRequest::StopStation)
    }
    pub fn drive(&mut self, peers: &mut PeerPool) -> Result<(), AssociationError> {
        if let Some(lease) = self.release_pending.take() {
            peers.release(lease).map_err(AssociationError::Pool)?;
            for slot in &mut self.leases { if *slot == Some(lease) { *slot = None; } }
        }
        if self.merge_pending && self.pending.is_none() && self.action_length == 0 && self.ibss.ready_for_merge() {
            self.merge_pending = false;
            self.search()?;
        }
        if self.needs_peer {
            let index = self.selected.ok_or(AssociationError::InvalidResponse)?;
            let candidate = self.candidates[index].as_ref().ok_or(AssociationError::InvalidResponse)?;
            let lease = match peers.lookup(NetIndex::PRIMARY, candidate.peer) {
                Some(lease) => lease,
                None => peers.reserve(NetIndex::PRIMARY, candidate.peer).map_err(AssociationError::Pool)?,
            };
            self.leases[index] = Some(lease); self.needs_peer = false;
            peers.get_mut(lease).map_err(AssociationError::Pool)?.signal = Some(candidate.signal);
        }
        if self.begin_independent {
            self.begin_independent = false;
            let index = self.selected.ok_or(AssociationError::InvalidResponse)?;
            let candidate = self.candidates[index].as_ref().ok_or(AssociationError::InvalidResponse)?;
            self.ibss.set_peer(self.peer_index()?, self.peer_parameters(0)?)?;
            self.ibss.set_radio(self.elements.radio_parameters(candidate.capabilities & 0x400 != 0))?;
            self.ibss.set_elements(&self.security_elements[..self.security_length])?;
            self.ibss.merge(candidate, self.local)?;
        }
        loop {
            if self.pending.is_some() || self.action_length == 0 { break; }
            if let Some(Action::Peer(state, _)) = self.actions[self.action_read] {
                let lease = self.peer_lease().ok_or(AssociationError::InvalidResponse)?;
                let transition = peers.set_state(lease, state).map_err(AssociationError::Pool)?;
                if !transition.publish_device_state {
                    self.actions[self.action_read] = None;
                    self.action_read = (self.action_read + 1) % self.actions.len(); self.action_length -= 1;
                    continue;
                }
                self.peer_admitted = true;
            }
            break;
        }
        self.materialize()?;
        self.peer_admitted = false;
        if !self.pending_pool_admitted {
            if let Some(command) = self.pending.as_ref() {
                if command.kind == CommandKind::Firmware && command.bytes().len() >= 16 {
                    let bytes = command.bytes();
                    if bytes[4] == 0x18 {
                        if let Some(lease) = self.leases.iter().flatten().find(|lease| lease.index().wire() == bytes[8]).copied() {
                            if !peers.get(lease).map_err(AssociationError::Pool)?.retiring() { peers.begin_release(lease).map_err(AssociationError::Pool)?; }
                            self.pending_remove = Some(lease);
                        }
                    } else if self.pending_ibss && bytes[4] == 0x17 && bytes.len() >= 29 {
                        let lease = self.peer_lease().ok_or(AssociationError::InvalidResponse)?;
                        let state = PeerState::from_wire(bytes[28]).ok_or(AssociationError::InvalidResponse)?;
                        peers.set_state(lease, state).map_err(AssociationError::Pool)?;
                    }
                }
            }
            self.pending_pool_admitted = true;
        }
        if self.pending.is_none() && (self.needs_peer || self.begin_independent) { return self.drive(peers); }
        Ok(())
    }
    pub fn take_mic_failure(&mut self) -> Option<MicFailure> { self.mic_failure.take() }
    pub fn receive_mic_failure(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        if bytes.len() < 4 { return Err(AssociationError::TruncatedFrame); }
        if let Some(bssid) = self.connected { self.mic_failure = Some(MicFailure { bssid, key: if bytes[0] == 1 { MicKey::Pairwise } else { MicKey::Group } }); }
        Ok(())
    }
    pub fn reject_class3(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        if bytes.len() < 24 { return Err(AssociationError::TruncatedFrame); }
        let control = half(bytes, 0)?;
        if control & 0x300 == 0 || bytes[4..10] != self.local.0 { return Err(AssociationError::InvalidResponse); }
        let mut peer = MacAddress([0; 6]); peer.0.copy_from_slice(&bytes[10..16]);
        let bssid = if control & 0x300 == 0x100 { self.local } else { peer };
        self.enqueue(Action::Class3(peer, bssid))?; self.materialize()
    }
    pub fn scan_token(&self) -> Option<ScanToken> { self.scan_active.then_some(ScanToken { generation: self.generation, sequence: self.scan_sequence }) }
    pub fn channel_token(&self) -> Option<ChannelToken> { self.channel_active.then_some(ChannelToken { generation: self.generation, sequence: self.channel_sequence }) }
    pub fn set_local_address(&mut self, local: MacAddress) { self.local = local; }
    pub fn set_time(&mut self, now: Milliseconds) { self.now = now.0; }
    pub fn set_security(&mut self, security: SecurityPublication) { self.security = security; self.refresh_elements(); }
    pub fn configure_elements(&mut self, configuration: elements::ElementConfiguration) {
        self.element_configuration = configuration; self.refresh_elements();
    }
    fn admitted_phy(&self) -> elements::PhySet {
        if matches!(self.security.encryption, Encryption::Disabled | Encryption::Ccmp) { self.local_phy } else { self.local_phy.without_ht() }
    }
    fn refresh_elements(&mut self) {
        let mut configuration = self.element_configuration;
        configuration.ht_enabled &= self.admitted_phy().wire() & 0x10 != 0;
        self.elements.configure(configuration);
    }
    pub fn configure_independent(&mut self, configuration: ibss::CreationParameters) { self.independent_configuration = configuration; }
    pub fn set_roaming_enabled(&mut self, enabled: bool) { self.roaming.configure(enabled); }
    fn element_timer(&mut self, change: elements::TimerChange) -> Result<(), AssociationError> {
        match change { elements::TimerChange::Unchanged => Ok(()), elements::TimerChange::Cancel => self.cancel_timer(TimerKind::Coexistence),
            elements::TimerChange::Arm(delay) => self.arm(TimerKind::Coexistence, Milliseconds(delay.0)) }
    }
    fn peer_parameters(&self, association_id: u16) -> Result<ibss::PeerParameters, AssociationError> {
        let mut result = ibss::PeerParameters::legacy();
        let candidate = self.selected_candidate().ok_or(AssociationError::InvalidResponse)?;
        let rates = candidate.rate_set.negotiate(self.admitted_phy(), self.allowed_rates).map_err(|_| AssociationError::InvalidRates)?;
        result.desired_rates = rates.desired_rates.wire();
        result.basic_rates = rates.basic_rates.wire();
        result.desired_phy = rates.desired_phy.wire();
        result.kind = if self.selected.and_then(|index| self.candidates[index].as_ref()).is_some_and(|candidate| candidate.mode == CandidateMode::Infrastructure) { ibss::PeerKind::AccessPoint } else { ibss::PeerKind::Independent };
        result.association_id = association_id; result.listen_interval = 10;
        result.receive_metadata = self.selected_candidate().map_or(0, |candidate| candidate.metric.0);
        let negotiated = self.elements.negotiated();
        if let Some(ht) = negotiated.ht.filter(|_| result.desired_phy & 0x10 != 0) {
            result.mcs = ht.rx_mcs[0]; result.mcs32 = ht.rx_mcs[4] & 1 != 0;
            result.receive_ampdu = ht.ampdu_parameters; result.ht_capabilities = ht.capability_info;
            result.ht_extended = ht.extended_capabilities; result.beamforming = ht.tx_beamforming;
            result.antenna_selection = ht.antenna_selection;
        }
        result.qos = negotiated.wmm.is_some(); result.uapsd_supported = negotiated.wmm.is_some_and(|wmm| wmm.uapsd);
        Ok(result)
    }
    pub fn supply_entropy(&mut self, token: AssociationToken, entropy: ibss::Entropy) -> Result<(), AssociationError> {
        if self.pending.as_ref().map(|command| (command.token, command.kind)) != Some((token, CommandKind::Entropy)) { return Err(AssociationError::StaleCompletion); }
        let ssid = self.target_ssid.ok_or(AssociationError::InvalidResponse)?;
        self.pending = None;
        self.ibss.set_elements(&self.security_elements[..self.security_length])?;
        self.independent_configuration.authentication = self.security.authentication;
        self.independent_configuration.encryption = self.security.encryption;
        self.ibss.create(self.mode, self.local, ssid, self.independent_configuration, entropy)?;
        self.materialize()
    }
    pub fn receive_roaming_event(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        if let Some((signal, discovery)) = self.roaming.receive(bytes, Milliseconds(self.now))? {
            if let Some(candidate) = self.candidates.iter_mut().flatten().find(|candidate| Some(candidate.bssid) == self.connected) {
                candidate.signal = SignalDbm((signal as i32 >> 1) - 110);
            }
            if let Some(command) = self.roaming.roam() { self.enqueue(Action::Roaming(command))?; }
            match discovery { roaming::Discovery::Scan => { self.scan()?; }, roaming::Discovery::Cached => { self.search()?; } }
        }
        self.materialize()
    }
    pub fn cancel(&mut self) {
        self.generation += 1; self.enabled = false; self.stopping = false;
        self.restart_after_independent = false; self.merge_pending = false;
        self.pending = None; self.pending_ibss = false;
        self.actions.fill(None); self.action_read = 0; self.action_length = 0;
        if let Err(error) = self.cancel_timer(TimerKind::Blacklist).and_then(|()| self.abort()).and_then(|()| self.materialize()) { self.failure = Some(error); }
        self.transition(AssociationState::Idle);
    }
    pub fn set_secondary_interface_active(&mut self, active: bool) { self.secondary_active = active; }
    pub fn interface_mode_admitted(&self) -> bool { !self.secondary_active }
    pub fn set_security_elements(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        Elements::parse(bytes)?;
        if bytes.len() > self.security_elements.len() { return Err(AssociationError::Capacity); }
        self.security_elements[..bytes.len()].copy_from_slice(bytes); self.security_length = bytes.len(); Ok(())
    }
    pub fn set_scan_elements(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        Elements::parse(bytes)?;
        if bytes.len() > self.scan_extra.len() { return Err(AssociationError::Capacity); }
        self.scan_extra[..bytes.len()].copy_from_slice(bytes); self.scan_extra_length = bytes.len(); Ok(())
    }
    fn transition(&mut self, state: AssociationState) { self.previous = self.state; self.state = state; }
    fn enqueue(&mut self, action: Action) -> Result<(), AssociationError> {
        if self.action_length == self.actions.len() { return Err(AssociationError::Capacity); }
        self.actions[(self.action_read + self.action_length) % self.actions.len()] = Some(action);
        self.action_length += 1; Ok(())
    }
    fn timer_index(kind: TimerKind) -> usize { match kind { TimerKind::Rescan => 0, TimerKind::IndependentAlone => 1, TimerKind::Join => 2, TimerKind::Management => 3, TimerKind::Blacklist => 4, TimerKind::Coexistence => 5 } }
    fn cancel_timer(&mut self, kind: TimerKind) -> Result<(), AssociationError> {
        if let Some(token) = self.timers[Self::timer_index(kind)].take() { self.enqueue(Action::Cancel(token))?; }
        Ok(())
    }
    fn arm(&mut self, kind: TimerKind, delay: Milliseconds) -> Result<(), AssociationError> {
        self.cancel_timer(kind)?;
        self.serial += 1;
        let token = TimerToken { generation: self.generation, serial: self.serial, kind };
        self.timers[Self::timer_index(kind)] = Some(token);
        self.enqueue(Action::Arm(token, delay))
    }
    pub fn request(&mut self, request: AssociationRequest) -> Result<(), AssociationError> {
        if request == AssociationRequest::StopStation && self.pending.is_some() {
            self.enabled = false; self.stopping = true;
            return Ok(());
        }
        if let AssociationRequest::SetInterfaceMode(mode) = request {
            if matches!(mode, InterfaceMode::Adhoc | InterfaceMode::Dedicated) && !self.interface_mode_admitted() { return Err(AssociationError::ModeConflict); }
            if self.pending.is_some() || self.action_length != 0 { return Err(AssociationError::Busy); }
            self.mode = mode;
            self.security_elements.fill(0); self.security_length = 0;
            self.abort()?;
            self.transition(AssociationState::Idle);
            return self.materialize();
        }
        if self.pending.is_some() || self.action_length != 0 { return Err(AssociationError::Busy); }
        self.generation += 1;
        self.failure = None;
        match request {
            AssociationRequest::SelectSsid(ssid) => { self.target_ssid = Some(ssid); self.target_bssid = None; self.enabled = true; }
            AssociationRequest::SelectBssid(bssid) => { self.target_bssid = Some(bssid); self.target_ssid = None; self.enabled = true; }
            AssociationRequest::StopStation => self.enabled = false,
            AssociationRequest::SetInterfaceMode(_) => unreachable!(),
        }
        self.scanned_for_target = false;
        self.abort()?;
        if self.enabled {
            if self.ibss.needs_cleanup() { self.restart_after_independent = true; }
            else { self.enqueue(Action::Search)?; }
        } else { self.transition(AssociationState::Idle); }
        self.materialize()
    }
    pub fn configure_scan(&mut self, plan: ScanPlan) -> Result<(), AssociationError> {
        if self.scan_active { return Err(AssociationError::Busy); }
        self.scan_plan = Some(plan);
        Ok(())
    }
    pub fn begin_scan(&mut self, ssid: Option<Ssid>) -> Result<(), AssociationError> {
        self.configure_scan(ScanPlan::new(ssid.as_slice(), &[], ScanType::Active, ScanBand::Ghz2, Milliseconds(110), Milliseconds(0))?)?;
        self.scan()
    }
    pub fn scan(&mut self) -> Result<(), AssociationError> {
        if self.pending.is_some() || self.scan_active { return Err(AssociationError::Busy); }
        self.failure = None; self.scan_failure = None; self.scan_finished = None;
        self.transition(if self.connected.is_some() { AssociationState::OnlineScan } else { AssociationState::Scan });
        self.scan_sequence = self.scan_sequence.wrapping_add(1);
        self.scan_active = true;
        self.enqueue(Action::Scan)?; self.materialize()
    }
    fn abort(&mut self) -> Result<(), AssociationError> {
        self.merge_pending = false;
        self.needs_peer = false; self.begin_independent = false;
        let reason = self.disconnect_reason.take().unwrap_or(DisconnectReason::Leaving);
        self.ibss.cancel();
        if let Some(command) = self.roaming.abort() { self.enqueue(Action::Roaming(command))?; }
        let change = self.elements.cancel();
        self.element_timer(change)?;
        for kind in [TimerKind::Rescan, TimerKind::IndependentAlone, TimerKind::Join, TimerKind::Management] { self.cancel_timer(kind)?; }
        if self.scan_active { self.enqueue(Action::AbortScan(self.scan_sequence))?; self.scan_active = false; }
        if self.management != ManagementState::Idle { self.management = ManagementState::Idle; }
        self.challenge.fill(0); self.challenge_length = 0;
        if self.channel_active { self.enqueue(Action::ReleaseChannel(self.channel_sequence))?; self.channel_active = false; }
        if self.connected.take().is_some() {
            self.disconnect_pending = true;
            if !self.ibss.needs_cleanup() {
                if let Some(selected) = self.selected { self.enqueue(Action::Deauthenticate(selected, reason))?; }
            }
        }
        for index in 0..self.leases.len() {
            if self.ibss.needs_cleanup() && self.selected == Some(index) { continue; }
            if let (Some(lease), Some(candidate)) = (self.leases[index], self.candidates[index].as_ref()) {
                self.enqueue(Action::RemovePeer(lease, candidate.peer))?;
            }
        }
        if self.disconnect_pending && !self.ibss.needs_cleanup() {
            self.enqueue(Action::Publish(LinkPublication::Disconnected))?;
            self.disconnect_pending = false;
        }
        Ok(())
    }
    fn choose(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (index, candidate) in self.candidates.iter().enumerate() {
            let Some(candidate) = candidate else { continue; };
            if candidate.failures > 1 && self.now.wrapping_sub(candidate.last_failure) < 10000 { continue; }
            if candidate.rate_set.negotiate(self.admitted_phy(), self.allowed_rates).is_err() { continue; }
            if self.target_bssid.is_some_and(|bssid| candidate.bssid != bssid) { continue; }
            if self.target_ssid.is_some_and(|ssid| !ssid.as_bytes().is_empty() && candidate.ssid != ssid) { continue; }
            if matches!(self.mode, InterfaceMode::Infrastructure) && candidate.mode != CandidateMode::Infrastructure { continue; }
            if matches!(self.mode, InterfaceMode::Adhoc | InterfaceMode::Dedicated) && candidate.mode != CandidateMode::Independent { continue; }
            if candidate.mode == CandidateMode::Independent && self.ibss.active() && Some(candidate.bssid) != self.ibss.bssid()
                && candidate.independent_merge != RxMergeIndicator::LargerTsf { continue; }
            if let Some(previous) = best {
                if let Some(old) = self.candidates[previous].as_ref() {
                    let old_score = old.signal.0 + if Some(old.bssid) == self.connected { 5 } else { 0 };
                    let score = candidate.signal.0 + if Some(candidate.bssid) == self.connected { 5 } else { 0 };
                    if old_score > score { continue; }
                }
            }
            best = Some(index);
            if self.target_bssid.is_some() || self.target_ssid.is_some_and(|ssid| ssid.as_bytes().is_empty()) { break; }
        }
        best
    }
    fn search(&mut self) -> Result<(), AssociationError> {
        self.transition(AssociationState::Search);
        if let Some(index) = self.choose() {
            self.selected = Some(index);
            self.needs_peer = true;
            let candidate = self.candidates[index].as_ref().ok_or(AssociationError::InvalidResponse)?;
            self.elements.set_peer_capabilities(candidate.capabilities);
            self.elements.observe_beacon(candidate.elements()).map_err(|_| AssociationError::InvalidResponse)?;
            if candidate.mode == CandidateMode::Independent {
                self.begin_independent = true;
                self.transition(AssociationState::IndependentMerge);
                return Ok(());
            }
            self.join_attempts = self.join_attempts.wrapping_add(1);
            self.transition(AssociationState::RequestChannel);
            self.channel_sequence = self.channel_sequence.wrapping_add(1);
            self.channel_active = true;
            self.enqueue(Action::RequestChannel)
        } else {
            if self.scanned_for_target && self.target_ssid.is_some() && matches!(self.mode, InterfaceMode::Adhoc | InterfaceMode::Auto | InterfaceMode::Dedicated) {
                self.transition(AssociationState::IndependentAlone);
                return self.enqueue(Action::Entropy);
            }
            self.transition(AssociationState::LookingFor);
            self.scan_sequence = self.scan_sequence.wrapping_add(1);
            self.scan_active = true;
            self.enqueue(Action::Scan)
        }
    }
    pub fn receive_scan_event(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        if bytes.len() < 4 { return Err(AssociationError::TruncatedFrame); }
        let token = self.scan_token().ok_or(AssociationError::StaleCompletion)?;
        if bytes[0] != token.sequence { return Err(AssociationError::StaleCompletion); }
        self.scan_complete(token)
    }
    pub fn receive_channel_event(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        if bytes.len() < 20 { return Err(AssociationError::TruncatedFrame); }
        let token = self.channel_token().ok_or(AssociationError::StaleCompletion)?;
        if bytes[0] != 0 || bytes[1] != token.sequence || bytes[2] != 0 { return Err(AssociationError::StaleCompletion); }
        self.channel_granted(token, Milliseconds(u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]])))
    }
    pub fn scan_complete(&mut self, token: ScanToken) -> Result<(), AssociationError> {
        if self.scan_token() != Some(token) { return Err(AssociationError::StaleCompletion); }
        self.scan_active = false;
        self.scan_finished_pending = Some(token);
        self.scanned_for_target = true;
        self.roaming.scanned(Milliseconds(self.now));
        match self.state {
            AssociationState::Scan => { self.transition(AssociationState::Idle); self.enqueue(Action::Publish(LinkPublication::ScanFinished))?; }
            AssociationState::OnlineScan => {
                if self.roaming.state() == roaming::RoamingState::Roam { self.search()?; }
                else { self.transition(AssociationState::Normal); }
                self.enqueue(Action::Publish(LinkPublication::ScanFinished))?;
            }
            AssociationState::LookingFor => {
                if self.choose().is_some() || matches!(self.mode, InterfaceMode::Adhoc | InterfaceMode::Auto | InterfaceMode::Dedicated) { self.search()?; }
                else { self.transition(AssociationState::WaitForNextScan); self.arm(TimerKind::Rescan, Milliseconds(2000))?; }
            }
            _ => return Err(AssociationError::StaleCompletion),
        }
        self.materialize()
    }
    pub fn channel_granted(&mut self, token: ChannelToken, duration: Milliseconds) -> Result<(), AssociationError> {
        if self.state != AssociationState::RequestChannel || self.channel_token() != Some(token) { return Err(AssociationError::StaleCompletion); }
        self.transition(AssociationState::Join);
        self.arm(TimerKind::Join, Milliseconds(duration.0.wrapping_sub(10)))?;
        self.algorithm = match self.security.authentication { Authentication::Shared => AuthenticationAlgorithm::Shared, _ => AuthenticationAlgorithm::Open };
        self.attempts = 0;
        self.management = ManagementState::Authenticate;
        self.send_management()?;
        self.materialize()
    }
    fn send_management(&mut self) -> Result<(), AssociationError> {
        if self.attempts >= 2 {
            if self.security.authentication == Authentication::Auto && self.algorithm == AuthenticationAlgorithm::Open
                && matches!(self.management, ManagementState::Authenticate | ManagementState::WaitAuthentication) {
                self.algorithm = AuthenticationAlgorithm::Shared; self.attempts = 0;
                self.management = ManagementState::Authenticate;
            } else { return self.join_failed(); }
        }
        self.attempts += 1;
        self.enqueue(if matches!(self.management, ManagementState::Associate) { Action::Associate } else { Action::Authenticate })
    }
    fn join_failed(&mut self) -> Result<(), AssociationError> {
        if let Some(index) = self.selected { if let Some(candidate) = self.candidates[index].as_mut() { candidate.failures = candidate.failures.saturating_add(1); candidate.last_failure = self.now; } }
        self.cancel_timer(TimerKind::Management)?;
        self.cancel_timer(TimerKind::Join)?;
        self.management = ManagementState::Idle;
        if self.channel_active { self.enqueue(Action::ReleaseChannel(self.channel_sequence))?; self.channel_active = false; }
        self.transition(AssociationState::WaitForNextScan);
        self.arm(TimerKind::Rescan, Milliseconds(2000))
    }
    pub fn expire(&mut self, token: TimerToken) -> Result<(), AssociationError> {
        let index = Self::timer_index(token.kind);
        if self.timers[index] != Some(token) { return Err(AssociationError::StaleCompletion); }
        self.timers[index] = None;
        match token.kind {
            TimerKind::Rescan => { if self.enabled { self.search()?; } }
            TimerKind::Management => {
                self.management = match self.management { ManagementState::WaitAuthentication => ManagementState::Authenticate, ManagementState::WaitChallenge => ManagementState::Challenge, ManagementState::WaitAssociation => ManagementState::Associate, value => value };
                self.send_management()?;
            }
            TimerKind::Join => self.join_failed()?,
            TimerKind::IndependentAlone => { self.ibss.timer_expired()?; }
            TimerKind::Blacklist => { for candidate in self.candidates.iter_mut().flatten() { candidate.failures = 0; } }
            TimerKind::Coexistence => { self.scan()?; }
        }
        self.materialize()
    }
    pub fn abort_link(&mut self) -> Result<(), AssociationError> {
        self.abort()?;
        if self.enabled { self.search()?; } else { self.transition(AssociationState::Idle); }
        self.materialize()
    }
    pub fn temporal_key_installed(&mut self) -> Result<(), AssociationError> {
        if self.candidates.iter().flatten().any(|candidate| candidate.mode == CandidateMode::Independent) {
            self.arm(TimerKind::Blacklist, Milliseconds(6000))?;
        }
        self.materialize()
    }
    pub fn receive_management(&mut self, bytes: &[u8], signal: SignalDbm) -> Result<(), AssociationError> {
        self.receive_management_with_metadata(bytes, ReceiveMetadata { signal, channel: None, independent_merge: RxMergeIndicator::No, metric: ReceiveMetric(0) })
    }
    pub fn receive_management_with_metadata(&mut self, bytes: &[u8], metadata: ReceiveMetadata) -> Result<(), AssociationError> {
        let frame = Management::parse(bytes)?;
        match frame.control & 0xfc {
            0x80 | 0x50 => {
                let candidate = Candidate::from_frame(&frame, metadata)?;
                let merge = candidate.mode == CandidateMode::Independent && self.ibss.active()
                    && (Some(candidate.bssid) == self.ibss.bssid() || candidate.independent_merge == RxMergeIndicator::LargerTsf)
                    && self.target_ssid == Some(candidate.ssid);
                let connected = self.connected == Some(candidate.bssid);
                let previous = self.elements.negotiated();
                let previous_capabilities = self.selected_candidate().map(|old| old.capabilities);
                if connected {
                    self.elements.set_peer_capabilities(candidate.capabilities);
                    let change = self.elements.observe_connected_beacon(candidate.elements()).map_err(|_| AssociationError::InvalidResponse)?;
                    self.element_timer(change)?;
                }
                let current = self.elements.negotiated();
                let radio_changed = connected && (previous != current || previous_capabilities != Some(candidate.capabilities));
                let edca_changed = connected && previous.wmm != current.wmm && current.wmm.is_some_and(|wmm| wmm.access_categories.is_some());
                if let Some(slot) = self.candidates.iter_mut().find(|slot| slot.as_ref().is_some_and(|old| old.bssid == candidate.bssid)) {
                    let old = slot.as_ref().unwrap();
                    *slot = Some(Candidate { failures: old.failures, last_failure: old.last_failure, ..candidate });
                } else if let Some(slot) = self.candidates.iter_mut().find(|slot| slot.is_none()) { *slot = Some(candidate); }
                else { return Err(AssociationError::Capacity); }
                if radio_changed { self.enqueue(Action::Radio)?; }
                if edca_changed { self.enqueue(Action::Edca)?; }
                if merge { self.merge_pending = true; }
            }
            0xb0 => {
                let Some(candidate) = self.selected.and_then(|index| self.candidates[index].as_ref()) else { return Ok(()); };
                if frame.source != candidate.peer || frame.bssid != candidate.bssid || frame.destination != self.local { return Ok(()); }
                if self.state != AssociationState::Join { return Ok(()); }
                let algorithm = half(frame.body, 0)?;
                let transaction = half(frame.body, 2)?;
                let status = half(frame.body, 4)?;
                if algorithm != self.algorithm as u16 { return Ok(()); }
                let expected = if matches!(self.management, ManagementState::Authenticate | ManagementState::WaitAuthentication) { 2 } else if matches!(self.management, ManagementState::Challenge | ManagementState::WaitChallenge) { 4 } else { return Ok(()); };
                if transaction != expected { return Ok(()); }
                let elements = Elements::parse(&frame.body[6..])?;
                self.cancel_timer(TimerKind::Management)?;
                self.attempts = 0;
                if status != 0 {
                    if self.security.authentication == Authentication::Auto && self.algorithm == AuthenticationAlgorithm::Open {
                        self.algorithm = AuthenticationAlgorithm::Shared; self.management = ManagementState::Authenticate; self.send_management()?;
                    } else { self.join_failed()?; }
                } else if self.algorithm == AuthenticationAlgorithm::Shared && transaction == 2 {
                    let challenge = elements.get(16).ok_or(AssociationError::InvalidResponse)?;
                    self.challenge[..challenge.len()].copy_from_slice(challenge); self.challenge_length = challenge.len() as u8;
                    self.management = ManagementState::Challenge; self.send_management()?;
                } else { self.enqueue(Action::Peer(PeerState::Authenticated, 0))?; self.management = ManagementState::Associate; self.send_management()?; }
            }
            0x10 | 0x30 => {
                let Some(candidate) = self.selected.and_then(|index| self.candidates[index].as_ref()) else { return Ok(()); };
                if self.state != AssociationState::Join || !matches!(self.management, ManagementState::Associate | ManagementState::WaitAssociation)
                    || frame.source != candidate.peer || frame.bssid != candidate.bssid || frame.destination != self.local { return Ok(()); }
                let peer = candidate.peer;
                let bssid = candidate.bssid;
                let status = half(frame.body, 2)?;
                let association_id = half(frame.body, 4)? & 0x3fff;
                Elements::parse(&frame.body[6..])?;
                if status != 0 { self.join_failed()?; }
                else {
                    self.cancel_timer(TimerKind::Management)?; self.cancel_timer(TimerKind::Join)?;
                    if self.channel_active { self.enqueue(Action::ReleaseChannel(self.channel_sequence))?; self.channel_active = false; }
                    self.management = ManagementState::Idle; self.join_attempts = 0;
                    self.connected = Some(bssid); self.transition(AssociationState::Normal);
                    let change = self.elements.observe_response(&frame.body[6..]).map_err(|_| AssociationError::InvalidResponse)?;
                    self.element_timer(change)?;
                    self.enqueue(Action::Peer(PeerState::Associated, association_id))?;
                    self.enqueue(Action::ConfigureBss(association_id))?;
                    self.enqueue(Action::Timing(association_id))?;
                    if self.elements.negotiated().wmm.and_then(|wmm| wmm.access_categories).is_some() { self.enqueue(Action::Edca)?; }
                    self.enqueue(Action::Publish(LinkPublication::Associated { peer, bssid, association_id }))?;
                }
            }
            0xa0 | 0xc0 => {
                half(frame.body, 0)?;
                if self.connected == Some(frame.bssid) && frame.destination == self.local
                    && self.selected.and_then(|index| self.candidates[index].as_ref()).is_some_and(|candidate| candidate.peer == frame.source) {
                    self.connected = None;
                    self.enqueue(Action::Publish(LinkPublication::Disconnected))?;
                    if self.enabled { self.search()?; } else { self.transition(AssociationState::Idle); }
                }
            }
            _ => {}
        }
        self.materialize()
    }
    fn materialize_independent(&mut self) -> Result<(), AssociationError> {
        self.device_sequence = self.device_sequence.wrapping_add(1);
        let Some(command) = self.ibss.command(self.device_sequence)? else { return Ok(()); };
        let kind = match command.kind {
            ibss::IbssCommandKind::Firmware => CommandKind::Firmware,
            ibss::IbssCommandKind::ArmAloneTimer => {
                self.serial += 1;
                let token = TimerToken { generation: self.generation, serial: self.serial, kind: TimerKind::IndependentAlone };
                self.timers[Self::timer_index(TimerKind::IndependentAlone)] = Some(token);
                CommandKind::ArmTimer { token, delay: Milliseconds(20000) }
            }
            ibss::IbssCommandKind::CancelAloneTimer => {
                if let Some(token) = self.timers[Self::timer_index(TimerKind::IndependentAlone)].take() { CommandKind::CancelTimer(token) }
                else { self.ibss.complete(Ok(()))?; return self.materialize_independent(); }
            }
            ibss::IbssCommandKind::Publish => {
                let candidate = self.selected.and_then(|index| self.candidates[index].as_ref()).ok_or(AssociationError::InvalidResponse)?;
                self.connected = Some(candidate.bssid);
                CommandKind::Publish(LinkPublication::Associated { peer: candidate.peer, bssid: candidate.bssid, association_id: 0 })
            }
            ibss::IbssCommandKind::Rescan => {
                self.ibss.complete(Ok(()))?;
                self.transition(AssociationState::LookingFor);
                self.scan_sequence = self.scan_sequence.wrapping_add(1); self.scan_active = true;
                self.enqueue(Action::Scan)?;
                return self.materialize();
            }
            ibss::IbssCommandKind::Stopped => {
                self.ibss.complete(Ok(()))?;
                if self.disconnect_pending {
                    self.disconnect_pending = false;
                    self.enqueue(Action::Publish(LinkPublication::Disconnected))?;
                }
                self.connected = None;
                if self.restart_after_independent {
                    self.restart_after_independent = false;
                    if self.enabled { self.search()?; }
                }
                return self.materialize();
            }
        };
        self.serial += 1; self.pending_ibss = true;
        self.pending_pool_admitted = false;
        self.pending = Some(AssociationCommand { token: AssociationToken { generation: self.generation, serial: self.serial }, kind, packet: command.packet, metadata: None });
        Ok(())
    }
    fn materialize(&mut self) -> Result<(), AssociationError> {
        if self.pending.is_some() { return Ok(()); }
        if self.needs_peer || self.begin_independent || self.release_pending.is_some() { return Ok(()); }
        if matches!(self.actions[self.action_read], Some(Action::Peer(_, _))) && !self.peer_admitted { return Ok(()); }
        if self.action_length == 0 { return self.materialize_independent(); }
        let action = self.actions[self.action_read].take().unwrap();
        self.action_read = (self.action_read + 1) % self.actions.len(); self.action_length -= 1;
        self.serial += 1;
        let mut command = AssociationCommand { token: AssociationToken { generation: self.generation, serial: self.serial }, kind: CommandKind::Firmware, packet: Packet::new(), metadata: None };
        self.pending_phase = None;
        self.pending_pool_admitted = false;
        match action {
            Action::Search => { self.search()?; return self.materialize(); }
            Action::RemovePeer(lease, peer) => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                self.pending_remove = Some(lease);
                command.packet = ibss::remove_peer_packet(self.device_sequence, lease.index(), peer);
            }
            Action::Scan => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let multiple = self.scan_plan.is_some_and(|plan| plan.multiple);
                let base = if multiple { 0xdc } else { 0x6e };
                let body = command.packet.device(if multiple { 4 } else { 0x1e }, self.device_sequence, base + self.scan_extra_length);
                body[0] = self.scan_sequence; body[2] = 1;
                let ssid = self.scan_plan.and_then(|plan| plan.ssids[0]).or(self.target_ssid);
                body[3] = if ssid.is_some_and(|ssid| !ssid.as_bytes().is_empty()) { 4 } else { 1 };
                if multiple {
                    if let Some(plan) = self.scan_plan {
                        for (index, ssid) in plan.ssids.iter().enumerate() {
                            if let Some(ssid) = ssid {
                                let offset = 4 + index * 36;
                                body[offset..offset + 4].copy_from_slice(&(ssid.as_bytes().len() as u32).to_le_bytes());
                                body[offset + 4..offset + 4 + ssid.as_bytes().len()].copy_from_slice(ssid.as_bytes());
                            }
                        }
                        body[148..150].copy_from_slice(&(plan.probe_delay.0 as u16).to_le_bytes());
                    }
                } else if let Some(ssid) = ssid {
                    body[4] = ssid.as_bytes().len() as u8; body[8..8 + ssid.as_bytes().len()].copy_from_slice(ssid.as_bytes());
                }
                let dwell_offset = if multiple { 150 } else { 40 };
                let channels_offset = if multiple { 152 } else { 42 };
                body[dwell_offset..dwell_offset + 2].copy_from_slice(&110u16.to_le_bytes());
                body[channels_offset] = 1;
                if let Some(plan) = self.scan_plan {
                    body[2] = match plan.scan_type { ScanType::Passive => 0, ScanType::Active => 1 };
                    body[dwell_offset..dwell_offset + 2].copy_from_slice(&(plan.dwell.0 as u16).to_le_bytes());
                    body[channels_offset] = match plan.band { ScanBand::Both => 0, ScanBand::Ghz2 => 1, ScanBand::Ghz5 => 2, ScanBand::Specified => 4 };
                    if plan.band == ScanBand::Specified {
                        for channel in plan.channels.iter().flatten() {
                            let index = body[channels_offset + 1] as usize;
                            body[channels_offset + 2 + index * 2] = if channel.band == ScanBand::Ghz2 { 1 } else { 2 };
                            body[channels_offset + 3 + index * 2] = channel.number;
                            body[channels_offset + 1] += 1;
                        }
                    }
                }
                body[base - 2..base].copy_from_slice(&(self.scan_extra_length as u16).to_le_bytes());
                body[base..].copy_from_slice(&self.scan_extra[..self.scan_extra_length]);
            }
            Action::AbortScan(sequence) => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let body = command.packet.device(0x1f, self.device_sequence, 4); body[0] = sequence;
            }
            Action::RequestChannel | Action::ReleaseChannel(_) => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let body = command.packet.device(0x20, self.device_sequence, 20);
                body[1] = self.channel_sequence;
                if let Action::ReleaseChannel(sequence) = action { body[1] = sequence; body[2] = 1; }
                else {
                    let candidate = self.selected.and_then(|index| self.candidates[index].as_ref()).ok_or(AssociationError::InvalidResponse)?;
                    body[3] = candidate.channel; body[5] = if candidate.channel <= 14 { 1 } else { 2 };
                    body[8..12].copy_from_slice(&2000u32.to_le_bytes()); body[12..18].copy_from_slice(&candidate.bssid.0);
                }
            }
            Action::Authenticate => {
                self.pending_phase = Some(self.management);
                let candidate = self.selected.and_then(|index| self.candidates[index].as_ref()).ok_or(AssociationError::InvalidResponse)?;
                let challenge = matches!(self.management, ManagementState::Challenge);
                command.kind = CommandKind::Management { protected: challenge };
                command.packet.authenticate(self.local, &candidate, self.algorithm, if challenge { 3 } else { 1 }, if challenge { &self.challenge[..self.challenge_length as usize] } else { &[] })?;
            }
            Action::Associate => {
                self.pending_phase = Some(self.management);
                let candidate = self.selected.and_then(|index| self.candidates[index].as_ref()).ok_or(AssociationError::InvalidResponse)?;
                command.kind = CommandKind::Management { protected: false };
                command.packet.associate(self.local, &candidate, candidate.ssid, &[], self.connected)?;
                self.elements.append_before_security(&mut command.packet)?;
                command.packet.append(&self.security_elements[..self.security_length])?;
                self.elements.append_after_security(&mut command.packet)?;
            }
            Action::Deauthenticate(index, reason) => {
                let candidate = self.candidates[index].as_ref().ok_or(AssociationError::InvalidResponse)?;
                command.kind = CommandKind::Management { protected: false }; command.packet.deauthenticate(self.local, &candidate, reason as u16)?;
            }
            Action::Entropy => command.kind = CommandKind::Entropy,
            Action::Roaming(roaming) => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                roaming.encode(self.device_sequence, &mut command.packet);
            }
            Action::ConfigureBss(association_id) => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let candidate = self.selected_candidate().ok_or(AssociationError::InvalidResponse)?;
                let parameters = self.peer_parameters(association_id)?;
                command.packet = ibss::infrastructure_bss_packet(self.device_sequence, candidate, self.local,
                    self.security.authentication, self.security.encryption, self.peer_index()?, &parameters,
                    &self.elements.radio_parameters(candidate.capabilities & 0x400 != 0));
            }
            Action::Timing(association_id) => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let candidate = self.selected_candidate().ok_or(AssociationError::InvalidResponse)?;
                let dtim = Elements::parse(candidate.elements())?.get(5).and_then(|tim| tim.get(1)).copied().unwrap_or(0);
                command.packet = ibss::timing_packet(self.device_sequence, candidate, association_id, dtim,
                    self.elements.negotiated().wmm.is_some_and(|wmm| wmm.uapsd));
            }
            Action::Edca => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let categories = self.elements.negotiated().wmm.and_then(|wmm| wmm.access_categories).ok_or(AssociationError::InvalidResponse)?;
                command.packet = ibss::edca_packet(self.device_sequence, &categories, true);
            }
            Action::Radio => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let candidate = self.selected_candidate().ok_or(AssociationError::InvalidResponse)?;
                command.packet = ibss::radio_packet(self.device_sequence, candidate, &self.elements.radio_parameters(candidate.capabilities & 0x400 != 0));
            }
            Action::Peer(state, association_id) => {
                self.device_sequence = self.device_sequence.wrapping_add(1);
                let candidate = self.selected.and_then(|index| self.candidates[index].as_ref()).ok_or(AssociationError::InvalidResponse)?;
                command.packet = ibss::peer_packet(self.device_sequence, candidate, self.peer_index()?, state, &self.peer_parameters(association_id)?);
            }
            Action::Class3(peer, bssid) => {
                command.kind = CommandKind::Management { protected: false };
                command.packet.header(0xc0, peer, self.local, bssid);
                command.packet.append(&7u16.to_le_bytes())?;
            }
            Action::Arm(token, delay) => command.kind = CommandKind::ArmTimer { token, delay },
            Action::Cancel(token) => command.kind = CommandKind::CancelTimer(token),
            Action::Publish(publication) => command.kind = CommandKind::Publish(publication),
        }
        if let CommandKind::Management { protected } = command.kind {
            let mut metadata = ManagementMetadata::management();
            metadata.packet_kind = PacketKind::Three;
            metadata.station = StationIndex(match action {
                Action::Class3(_, _) => 0xfe,
                Action::Deauthenticate(index, _) => self.leases[index].map(|lease| lease.index().wire()).ok_or(AssociationError::InvalidResponse)?,
                _ => self.peer_index()?.wire(),
            });
            metadata.protected = protected; metadata.burst = true;
            metadata.report_status = !matches!(action, Action::Class3(_, _));
            command.metadata = Some(metadata);
        }
        self.pending = Some(command); Ok(())
    }
    pub fn finish_command(&mut self, token: AssociationToken, result: Result<(), AssociationIoError>) -> Result<(), AssociationError> {
        if self.pending.as_ref().map(|command| command.token) != Some(token) { return Err(AssociationError::StaleCompletion); }
        if self.pending.as_ref().is_some_and(|command| command.kind == CommandKind::Entropy) { return Err(AssociationError::InvalidResponse); }
        let completed = self.pending.take().unwrap();
        let kind = completed.kind;
        if result.is_ok() && kind == CommandKind::Firmware && completed.bytes().get(4) == Some(&0x18) { self.release_pending = self.pending_remove.take(); }
        let phase = self.pending_phase.take();
        let independent = self.pending_ibss;
        if self.pending_ibss {
            self.pending_ibss = false;
            if let Err(error) = self.ibss.complete(result) { self.failure = Some(error); }
        }
        if self.stopping {
            if let Err(error) = result { self.failure = Some(AssociationError::Physical(error)); }
            self.stopping = false;
            self.actions.fill(None); self.action_length = 0; self.action_read = 0;
            self.generation += 1;
            self.abort()?;
            self.transition(AssociationState::Idle);
            return self.materialize();
        }
        if let Err(error) = result {
            if kind == CommandKind::Firmware && matches!(completed.bytes().get(4), Some(0x1e | 4)) {
                if let Some(scan) = self.scan_token() { self.scan_failure = Some((scan, AssociationError::Physical(error))); }
            }
            self.failure = Some(AssociationError::Physical(error));
            if independent { return self.materialize(); }
            if matches!(kind, CommandKind::Management { .. }) && self.state == AssociationState::Join {
                if phase.is_some_and(|phase| phase == self.management) { self.arm(TimerKind::Management, Milliseconds(102))?; }
            } else {
                self.actions.fill(None); self.action_length = 0; self.action_read = 0;
                self.abort()?; self.transition(AssociationState::Idle);
            }
        } else {
            match kind {
                CommandKind::Publish(publication) => {
                    self.publication = Some(publication);
                    if publication == LinkPublication::ScanFinished { self.scan_finished = self.scan_finished_pending.take(); }
                    if matches!(publication, LinkPublication::Associated { .. }) {
                        if let Some(command) = self.roaming.associated(self.state == AssociationState::IndependentMerge) { self.enqueue(Action::Roaming(command))?; }
                        if self.state == AssociationState::IndependentMerge { self.transition(AssociationState::Normal); }
                    }
                }
                CommandKind::Management { .. } if self.state == AssociationState::Join && phase.is_some_and(|phase| phase == self.management) => {
                    self.management = match self.management { ManagementState::Authenticate => ManagementState::WaitAuthentication, ManagementState::Challenge => ManagementState::WaitChallenge, ManagementState::Associate => ManagementState::WaitAssociation, other => other };
                    self.arm(TimerKind::Management, Milliseconds(524))?;
                }
                _ => {}
            }
        }
        self.materialize()
    }
}

#[cfg(test)]
#[path = "vendor_wireless_station_driver_association_tests.rs"]
mod tests;
