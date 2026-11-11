//! X-M1 1.50 independent-BSS creation and merge CPU.
//! Source pin: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::{AssociationError, AssociationIoError, Candidate, CandidateMode, InterfaceMode, MacAddress, Ssid};
use super::frames::{Elements, Packet};
use super::super::{Authentication, Encryption};
use super::super::super::vendor_wireless_peers::{NetIndex, PeerIndex, PeerState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entropy(pub [u16; 3]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Band { Ghz2, Ghz5 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreationParameters {
    pub band: Band,
    pub channel: u8,
    pub beacon_interval: u16,
    pub atim_window: u16,
    pub privacy: bool,
    pub short_preamble: bool,
    pub available_phy: u8,
    pub authentication: Authentication,
    pub encryption: Encryption,
}
impl CreationParameters {
    pub const fn new(band: Band, channel: u8) -> Self {
        Self { band, channel, beacon_interval: 100, atim_window: 0, privacy: false,
            short_preamble: false, available_phy: 0x1b, authentication: Authentication::Open, encryption: Encryption::Disabled }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PeerKind { Independent = 0x11, SecondaryClient = 0x22, AccessPoint = 0x41, P2pAccessPoint = 0x42 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerParameters {
    pub kind: PeerKind,
    pub association_id: u16,
    pub listen_interval: u16,
    pub desired_phy: u8,
    pub desired_rates: u16,
    pub basic_rates: u16,
    pub ht_capabilities: u16,
    pub receive_ampdu: u8,
    pub ht_extended: u16,
    pub beamforming: u32,
    pub antenna_selection: u8,
    pub mcs: u8,
    pub mcs32: bool,
    pub receive_metadata: u8,
    pub uapsd_supported: bool,
    pub qos: bool,
    pub uapsd: u8,
    pub service_period: u8,
}
impl PeerParameters {
    pub const fn legacy() -> Self {
        Self { kind: PeerKind::Independent, association_id: 0, listen_interval: 0, desired_phy: 0x13,
            desired_rates: 0x3fcf, basic_rates: 0x000f, ht_capabilities: 0, receive_ampdu: 0, ht_extended: 0,
            beamforming: 0, antenna_selection: 0, mcs: 0, mcs32: false, receive_metadata: 0,
            qos: false, uapsd_supported: false, uapsd: 0, service_period: 0 }
    }
}

pub fn peer_packet(sequence: u8, candidate: &Candidate, index: PeerIndex, state: PeerState,
    parameters: &PeerParameters) -> Packet {
    peer_record_packet(sequence, NetIndex::PRIMARY, candidate.peer, candidate.rates(), index, state, parameters)
}

pub fn peer_record_packet(sequence: u8, network: NetIndex, peer: MacAddress, rates: &[u8],
    index: PeerIndex, state: PeerState, parameters: &PeerParameters) -> Packet {
    let mut packet = Packet::new();
    encode_peer_record(packet.device(0x17, sequence, 40).try_into().unwrap(), network, peer, rates, index, state, parameters);
    packet
}

pub fn peer_record_body(network: NetIndex, peer: MacAddress, rates: &[u8], index: PeerIndex,
    state: PeerState, parameters: &PeerParameters) -> [u8; 40] {
    let mut bytes = [0; 40];
    encode_peer_record(&mut bytes, network, peer, rates, index, state, parameters);
    bytes
}

fn encode_peer_record(bytes: &mut [u8; 40], network: NetIndex, peer: MacAddress, rates: &[u8],
    index: PeerIndex, state: PeerState, parameters: &PeerParameters) {
    let (operational, _) = rate_masks(rates);
    bytes[0] = index.wire();
    bytes[1] = parameters.kind as u8;
    bytes[2..8].copy_from_slice(&peer.0);
    bytes[8..10].copy_from_slice(&parameters.association_id.to_le_bytes());
    bytes[10..12].copy_from_slice(&parameters.listen_interval.to_le_bytes());
    bytes[12] = network.wire();
    bytes[13] = parameters.desired_phy;
    bytes[14..16].copy_from_slice(&(operational & parameters.desired_rates).to_le_bytes());
    bytes[16..18].copy_from_slice(&parameters.basic_rates.to_le_bytes());
    bytes[18] = u8::from(parameters.qos);
    bytes[19] = u8::from(parameters.uapsd_supported);
    bytes[20] = state.wire();
    bytes[21] = parameters.mcs;
    bytes[22] = u8::from(parameters.mcs32);
    bytes[23] = parameters.receive_ampdu;
    bytes[24..26].copy_from_slice(&parameters.ht_capabilities.to_le_bytes());
    bytes[26..28].copy_from_slice(&parameters.ht_extended.to_le_bytes());
    bytes[28..32].copy_from_slice(&parameters.beamforming.to_le_bytes());
    bytes[32] = parameters.antenna_selection;
    bytes[33] = parameters.receive_metadata;
    bytes[34] = u8::from(state == PeerState::Associated);
    bytes[35] = parameters.uapsd;
    bytes[36] = parameters.service_period;
    bytes[37..40].fill(0);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum HtProtection { None = 0, NonMember = 1, TwentyMhz = 2, NonHtMixed = 3 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BssRadioParameters {
    pub secondary_channel: super::elements::SecondaryChannel,
    pub erp_protection: bool,
    pub ht_protection: HtProtection,
    pub non_greenfield_present: bool,
    pub rifs_protection: bool,
    pub basic_stbc: u16,
    pub operation_protection: u16,
    pub operation_information: u8,
    pub short_preamble: bool,
    pub short_slot: bool,
}
impl BssRadioParameters {
    pub const fn legacy(short_preamble: bool, short_slot: bool) -> Self {
        Self { secondary_channel: super::elements::SecondaryChannel::None, erp_protection: false,
            ht_protection: HtProtection::None, non_greenfield_present: false, rifs_protection: false,
            basic_stbc: 0, operation_protection: 0, operation_information: 0, short_preamble, short_slot }
    }
    fn encode(&self, bytes: &mut [u8], band: Band, channel: u8) {
        bytes[0] = NetIndex::PRIMARY.wire();
        bytes[1] = match band { Band::Ghz2 => 1, Band::Ghz5 => 2 };
        bytes[2] = channel;
        bytes[3] = match self.secondary_channel {
            super::elements::SecondaryChannel::None => 0,
            super::elements::SecondaryChannel::Above => 1,
            super::elements::SecondaryChannel::Below => 3,
        };
        bytes[4] = u8::from(self.erp_protection); bytes[5] = self.ht_protection as u8;
        bytes[6] = u8::from(self.non_greenfield_present); bytes[7] = u8::from(self.rifs_protection);
        bytes[8..10].copy_from_slice(&self.basic_stbc.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.operation_protection.to_le_bytes());
        bytes[12] = self.operation_information;
        bytes[13] = u8::from(self.short_preamble); bytes[14] = u8::from(self.short_slot); bytes[15] = 0x72;
    }
}

struct BssParameters<'a> {
    disconnected: bool,
    mode: CandidateMode,
    ssid: Option<&'a Ssid>,
    bssid: Option<MacAddress>,
    local: MacAddress,
    operational: u16,
    basic: u16,
    peer: Option<PeerIndex>,
    phy: u8,
    available_phy: u8,
    authentication: Authentication,
    encryption: Encryption,
    qos: bool,
    band: Band,
    channel: u8,
    radio: BssRadioParameters,
}
fn serialize_bss(packet: &mut Packet, sequence: u8, parameters: BssParameters<'_>) {
    let bytes = packet.device(0x16, sequence, 80);
    bytes[0] = NetIndex::PRIMARY.wire();
    bytes[1] = u8::from(parameters.disconnected);
    bytes[2] = u8::from(parameters.mode == CandidateMode::Independent);
    if let Some(ssid) = parameters.ssid {
        bytes[3] = ssid.as_bytes().len() as u8;
        bytes[4..4 + ssid.as_bytes().len()].copy_from_slice(ssid.as_bytes());
    }
    if let Some(bssid) = parameters.bssid { bytes[36..42].copy_from_slice(&bssid.0); }
    bytes[42] = u8::from(parameters.qos);
    bytes[44..46].copy_from_slice(&parameters.operational.to_le_bytes());
    bytes[46..48].copy_from_slice(&parameters.basic.to_le_bytes());
    bytes[48] = parameters.peer.map_or(0xfe, |index| index.wire());
    bytes[51] = parameters.phy;
    bytes[52] = parameters.authentication as u8; bytes[53] = parameters.encryption as u8;
    bytes[54] = parameters.available_phy; bytes[55..61].copy_from_slice(&parameters.local.0);
    parameters.radio.encode(&mut bytes[64..80], parameters.band, parameters.channel);
}

pub fn infrastructure_bss_packet(sequence: u8, candidate: &Candidate, local: MacAddress,
    authentication: Authentication, encryption: Encryption, peer: PeerIndex,
    parameters: &PeerParameters, radio: &BssRadioParameters) -> Packet {
    let (operational, basic) = rate_masks(candidate.rates());
    let band = if candidate.channel > 14 { Band::Ghz5 } else { Band::Ghz2 };
    let phy = if parameters.desired_phy & 2 != 0 { 1 } else if parameters.desired_phy & 8 != 0 { 3 } else { 0 };
    let mut packet = Packet::new();
    serialize_bss(&mut packet, sequence, BssParameters { disconnected: false,
        mode: CandidateMode::Infrastructure, ssid: Some(&candidate.ssid), bssid: Some(candidate.bssid),
        local, operational, basic, peer: Some(peer), phy, available_phy: parameters.desired_phy,
        authentication, encryption, qos: parameters.qos, band, channel: candidate.channel, radio: *radio });
    packet
}

pub fn remove_peer_packet(sequence: u8, index: PeerIndex, peer: MacAddress) -> Packet {
    let mut packet = Packet::new();
    let bytes = packet.device(0x18, sequence, 8);
    bytes[0] = index.wire(); bytes[2..8].copy_from_slice(&peer.0);
    packet
}

pub fn radio_packet(sequence: u8, candidate: &Candidate, radio: &BssRadioParameters) -> Packet {
    let mut packet = Packet::new();
    let bytes = packet.device(0x1d, sequence, 16);
    radio.encode(bytes, if candidate.channel > 14 { Band::Ghz5 } else { Band::Ghz2 }, candidate.channel);
    packet
}

pub fn timing_packet(sequence: u8, candidate: &Candidate, association_id: u16,
    dtim_period: u8, uapsd_supported: bool) -> Packet {
    let mut packet = Packet::new();
    let bytes = packet.device(0x1a, sequence, 12);
    bytes[0] = NetIndex::PRIMARY.wire();
    bytes[1] = dtim_period;
    bytes[2..4].copy_from_slice(&association_id.to_le_bytes());
    bytes[4..6].copy_from_slice(&candidate.beacon_interval.to_le_bytes());
    bytes[8] = u8::from(uapsd_supported);
    packet
}

pub fn edca_packet(sequence: u8, categories: &[super::elements::AccessCategoryParameters; 4], qos: bool) -> Packet {
    let mut packet = Packet::new();
    let bytes = packet.device(0x21, sequence, 44);
    for (record, category) in bytes[..40].chunks_exact_mut(10).zip(categories) {
        record[..2].copy_from_slice(&category.contention_min.to_le_bytes());
        record[2..4].copy_from_slice(&category.contention_max.to_le_bytes());
        record[4..6].copy_from_slice(&category.txop.to_le_bytes());
        record[6..8].copy_from_slice(&u16::from(category.aifsn).to_le_bytes());
        record[9] = u8::from(category.admission_required);
    }
    bytes[40] = NetIndex::PRIMARY.wire();
    bytes[41] = u8::from(qos);
    packet
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IbssCommandKind { Firmware, ArmAloneTimer, CancelAloneTimer, Publish, Rescan, Stopped }
#[derive(Debug, Eq, PartialEq)]
pub struct IbssCommand { pub kind: IbssCommandKind, pub packet: Packet }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage { Idle, CancelTimer, PeerPrepare, Configure, Beacon, Timing, PeerAssociate, Publish, Arm, Alone, Joined, StopBeacon, StopPeer, StopBss, NotifyStopped, Stopped, Rescan }

pub struct Ibss {
    stage: Stage,
    issued: bool,
    merging: bool,
    active: bool,
    timer: bool,
    local: MacAddress,
    bssid: Option<MacAddress>,
    ssid: Option<Ssid>,
    peer: Option<MacAddress>,
    configuration: CreationParameters,
    capabilities: u16,
    operational: u16,
    basic: u16,
    phy: u8,
    available_phy: u8,
    rates: [u8; 15],
    rates_length: usize,
    elements: [u8; 600],
    elements_length: usize,
    peer_index: Option<PeerIndex>,
    peer_parameters: PeerParameters,
    radio: Option<BssRadioParameters>,
    failure: Option<AssociationIoError>,
}
impl Ibss {
    pub const fn new() -> Self {
        Self { stage: Stage::Idle, issued: false, merging: false, active: false, timer: false,
            local: MacAddress([0; 6]), bssid: None, ssid: None, peer: None,
            configuration: CreationParameters::new(Band::Ghz2, 1), capabilities: 2,
            operational: 0, basic: 0, phy: 0, available_phy: 0, rates: [0; 15], rates_length: 0,
            elements: [0; 600], elements_length: 0, peer_index: None,
            peer_parameters: PeerParameters::legacy(), radio: None, failure: None }
    }
    pub fn bssid(&self) -> Option<MacAddress> { self.bssid }
    pub fn failure(&self) -> Option<AssociationIoError> { self.failure }
    pub fn active(&self) -> bool { self.active }
    pub fn ready_for_merge(&self) -> bool {
        !self.issued && matches!(self.stage, Stage::Idle | Stage::Alone | Stage::Joined | Stage::Stopped)
    }
    pub fn needs_cleanup(&self) -> bool {
        matches!(self.stage, Stage::CancelTimer | Stage::StopBeacon | Stage::StopPeer | Stage::StopBss | Stage::NotifyStopped)
    }
    pub fn set_radio(&mut self, radio: BssRadioParameters) -> Result<(), AssociationError> {
        if self.issued { return Err(AssociationError::Busy); }
        self.radio = Some(radio); Ok(())
    }
    pub fn set_peer(&mut self, index: PeerIndex, parameters: PeerParameters) -> Result<(), AssociationError> {
        if !self.ready_for_merge() { return Err(AssociationError::Busy); }
        self.peer_index = Some(index); self.peer_parameters = parameters; Ok(())
    }
    pub fn set_elements(&mut self, elements: &[u8]) -> Result<(), AssociationError> {
        Elements::parse(elements)?;
        if elements.len() > self.elements.len() { return Err(AssociationError::Capacity); }
        if self.issued { return Err(AssociationError::Busy); }
        self.elements[..elements.len()].copy_from_slice(elements);
        self.elements_length = elements.len(); Ok(())
    }
    pub fn create(&mut self, mode: InterfaceMode, local: MacAddress, ssid: Ssid,
        configuration: CreationParameters, entropy: Entropy) -> Result<(), AssociationError> {
        if !matches!(mode, InterfaceMode::Adhoc | InterfaceMode::Auto | InterfaceMode::Dedicated) {
            return Err(AssociationError::ModeConflict);
        }
        if self.active && self.stage == Stage::Alone && !self.issued { return Ok(()); }
        if self.active || self.issued || !matches!(self.stage, Stage::Idle | Stage::Stopped) { return Err(AssociationError::Busy); }
        if configuration.channel == 0 { return Err(AssociationError::InvalidChannel); }
        self.local = local; self.ssid = Some(ssid); self.configuration = configuration;
        if self.bssid.is_none() {
            let mut address = [0; 6];
            for (word, bytes) in entropy.0.iter().zip(address.chunks_exact_mut(2)) { bytes.copy_from_slice(&word.to_le_bytes()); }
            address[0] = (address[0] & !1) | 2;
            self.bssid = Some(MacAddress(address));
        }
        let rates: &[u8] = match configuration.band {
            Band::Ghz2 => { self.phy = 1; self.available_phy = configuration.available_phy & 0x13;
                self.operational = 0x3fcf; self.basic = 0x000f;
                &[0x82, 0x84, 0x8b, 0x96, 12, 18, 24, 36, 48, 72, 96, 108] }
            Band::Ghz5 => { self.phy = 3; self.available_phy = configuration.available_phy & 0x18;
                self.operational = 0x3fc0; self.basic = 0x0540;
                &[0x8c, 18, 0x98, 36, 0xb0, 72, 96, 108] }
        };
        self.rates[..rates.len()].copy_from_slice(rates); self.rates_length = rates.len();
        self.capabilities = 2 | if configuration.privacy { 0x10 } else { 0 }
            | if configuration.short_preamble && configuration.band == Band::Ghz2 { 0x20 } else { 0 };
        self.merging = false; self.peer = None; self.failure = None; self.stage = Stage::Configure;
        self.radio = None;
        Ok(())
    }
    pub fn merge(&mut self, candidate: &Candidate, local: MacAddress) -> Result<(), AssociationError> {
        if !self.ready_for_merge() { return Err(AssociationError::Busy); }
        if candidate.mode != CandidateMode::Independent { return Err(AssociationError::ModeConflict); }
        if self.peer_index.is_none() { return Err(AssociationError::InvalidResponse); }
        if candidate.channel == 0 { return Err(AssociationError::InvalidChannel); }
        if candidate.rates().is_empty() { return Err(AssociationError::InvalidRates); }
        self.local = local; self.bssid = Some(candidate.bssid); self.ssid = Some(candidate.ssid);
        self.configuration.channel = candidate.channel; self.configuration.beacon_interval = candidate.beacon_interval;
        self.configuration.band = if candidate.channel > 14 { Band::Ghz5 } else { Band::Ghz2 };
        self.configuration.atim_window = 0;
        self.capabilities = candidate.capabilities & !0x400;
        (self.operational, self.basic) = rate_masks(candidate.rates());
        self.phy = if candidate.channel > 14 { 3 } else if self.operational & 0x3fc0 != 0 { 1 } else { 0 };
        self.available_phy = if candidate.channel > 14 { 8 } else { 3 };
        self.rates_length = candidate.rates().len();
        self.rates[..self.rates_length].copy_from_slice(candidate.rates());
        self.peer = Some(candidate.peer); self.merging = true; self.failure = None;
        self.stage = if self.timer { Stage::CancelTimer } else { Stage::PeerPrepare };
        Ok(())
    }
    pub fn command(&mut self, sequence: u8) -> Result<Option<IbssCommand>, AssociationError> {
        if self.issued { return Err(AssociationError::Busy); }
        let mut packet = Packet::new();
        let kind = match self.stage {
            Stage::Idle | Stage::Alone | Stage::Joined | Stage::Stopped => return Ok(None),
            Stage::CancelTimer => IbssCommandKind::CancelAloneTimer,
            Stage::Arm => IbssCommandKind::ArmAloneTimer,
            Stage::Publish => IbssCommandKind::Publish,
            Stage::Rescan => IbssCommandKind::Rescan,
            Stage::NotifyStopped => IbssCommandKind::Stopped,
            Stage::PeerPrepare | Stage::PeerAssociate => {
                packet = peer_record_packet(sequence, NetIndex::PRIMARY, self.peer.ok_or(AssociationError::InvalidResponse)?,
                    &self.rates[..self.rates_length], self.peer_index.ok_or(AssociationError::InvalidResponse)?,
                    if self.stage == Stage::PeerPrepare { PeerState::Disconnected } else { PeerState::Associated }, &self.peer_parameters);
                IbssCommandKind::Firmware
            }
            Stage::Configure | Stage::StopBss => { self.configure(&mut packet, sequence, self.stage != Stage::StopBss); IbssCommandKind::Firmware }
            Stage::Beacon => { self.beacon(&mut packet, sequence)?; IbssCommandKind::Firmware }
            Stage::Timing => {
                let bytes = packet.device(if self.merging { 0x1a } else { 0x19 }, sequence, if self.merging { 12 } else { 8 });
                let offset = if self.merging { 4 } else { 2 };
                bytes[offset..offset + 2].copy_from_slice(&self.configuration.beacon_interval.to_le_bytes());
                bytes[offset + 2..offset + 4].copy_from_slice(&self.configuration.atim_window.to_le_bytes());
                IbssCommandKind::Firmware
            }
            Stage::StopBeacon => { packet.device(0x1c, sequence, 6)[0] = 2; IbssCommandKind::Firmware }
            Stage::StopPeer => {
                packet = remove_peer_packet(sequence, self.peer_index.ok_or(AssociationError::InvalidResponse)?, self.peer.ok_or(AssociationError::InvalidResponse)?);
                IbssCommandKind::Firmware
            }
        };
        self.issued = true;
        Ok(Some(IbssCommand { kind, packet }))
    }
    pub fn complete(&mut self, result: Result<(), AssociationIoError>) -> Result<(), AssociationError> {
        if !self.issued { return Err(AssociationError::StaleCompletion); }
        self.issued = false;
        if let Err(error) = result {
            if self.failure.is_none() { self.failure = Some(error); }
            self.stage = match self.stage {
                Stage::CancelTimer => { self.merging = false; Stage::StopBeacon }
                Stage::StopBeacon => if self.peer.is_some() { Stage::StopPeer } else { Stage::StopBss },
                Stage::StopPeer => Stage::StopBss,
                Stage::StopBss => Stage::NotifyStopped,
                _ => { self.merging = false; if self.timer { Stage::CancelTimer } else { Stage::StopBeacon } }
            };
            return Err(AssociationError::Physical(error));
        }
        self.stage = match self.stage {
            Stage::CancelTimer => { self.timer = false; if self.merging { Stage::PeerPrepare } else { Stage::StopBeacon } }
            Stage::PeerPrepare => Stage::Configure,
            Stage::Configure => { self.active = true; Stage::Beacon }
            Stage::Beacon => Stage::Timing,
            Stage::Timing => if self.merging { Stage::PeerAssociate } else { Stage::Arm },
            Stage::PeerAssociate => Stage::Publish,
            Stage::Publish => Stage::Joined,
            Stage::Arm => { self.timer = true; Stage::Alone }
            Stage::StopBeacon => if self.peer.is_some() { Stage::StopPeer } else { Stage::StopBss },
            Stage::StopPeer => { self.peer = None; self.peer_index = None; Stage::StopBss }
            Stage::StopBss => { self.active = false; Stage::NotifyStopped }
            Stage::NotifyStopped => Stage::Stopped,
            Stage::Rescan => Stage::Alone,
            _ => return Err(AssociationError::StaleCompletion),
        };
        Ok(())
    }
    pub fn cancel(&mut self) {
        if !self.active && self.peer.is_none() && !self.issued && matches!(self.stage, Stage::Idle | Stage::Stopped) { return; }
        self.issued = false; self.merging = false;
        self.stage = if self.timer { Stage::CancelTimer } else if self.active || self.peer.is_some() { Stage::StopBeacon } else { Stage::NotifyStopped };
    }
    pub fn timer_expired(&mut self) -> Result<(), AssociationError> {
        if !self.timer || self.stage != Stage::Alone || self.issued { return Err(AssociationError::StaleCompletion); }
        self.timer = false; self.stage = Stage::Rescan; Ok(())
    }
    fn configure(&self, packet: &mut Packet, sequence: u8, enabled: bool) {
        let mut radio = self.radio.unwrap_or(BssRadioParameters::legacy(self.capabilities & 0x20 != 0, false));
        radio.short_slot = false;
        serialize_bss(packet, sequence, BssParameters { disconnected: !enabled,
            mode: CandidateMode::Independent, ssid: self.ssid.as_ref(), bssid: self.bssid, local: self.local,
            operational: self.operational, basic: self.basic, peer: None, phy: self.phy, available_phy: self.available_phy,
            authentication: self.configuration.authentication, encryption: self.configuration.encryption,
            qos: self.peer_parameters.qos && self.merging, band: self.configuration.band,
            channel: self.configuration.channel, radio });
    }
    fn beacon(&self, packet: &mut Packet, sequence: u8) -> Result<(), AssociationError> {
        let ssid = self.ssid.ok_or(AssociationError::InvalidResponse)?;
        let rates = &self.rates[..self.rates_length];
        let length = 2 + ssid.as_bytes().len() + 2 + rates.len().min(8) + 4
            + if self.configuration.band == Band::Ghz2 { 3 } else { 0 }
            + if rates.len() > 8 { 2 + rates.len() - 8 } else { 0 } + self.elements_length;
        if length > 600 { return Err(AssociationError::Capacity); }
        let bytes = packet.device(0x1c, sequence, 8 + length);
        bytes[0] = 1;
        bytes[1] = NetIndex::PRIMARY.wire();
        bytes[4..6].copy_from_slice(&self.capabilities.to_le_bytes());
        bytes[6..8].copy_from_slice(&(length as u16).to_le_bytes());
        let frame = &mut bytes[8..];
        let mut offset = 0;
        let channel = [self.configuration.channel];
        let atim = self.configuration.atim_window.to_le_bytes();
        for (id, value) in [(0, ssid.as_bytes()), (1, &rates[..rates.len().min(8)]),
            (3, if self.configuration.band == Band::Ghz2 { &channel[..] } else { &[] }),
            (6, &atim[..])] {
            if value.is_empty() && id == 3 { continue; }
            frame[offset] = id; frame[offset + 1] = value.len() as u8;
            frame[offset + 2..offset + 2 + value.len()].copy_from_slice(value); offset += 2 + value.len();
        }
        if rates.len() > 8 { frame[offset] = 50; frame[offset + 1] = (rates.len() - 8) as u8;
            frame[offset + 2..offset + rates.len() - 6].copy_from_slice(&rates[8..]); offset += rates.len() - 6; }
        frame[offset..offset + self.elements_length].copy_from_slice(&self.elements[..self.elements_length]);
        Ok(())
    }
}

fn rate_masks(rates: &[u8]) -> (u16, u16) {
    let mut operational = 0; let mut basic = 0;
    for rate in rates {
        let bit = match rate & 0x7f { 2 => 0, 4 => 1, 11 => 2, 22 => 3, 44 => 4, 66 => 5,
            12 => 6, 18 => 7, 24 => 8, 36 => 9, 48 => 10, 72 => 11, 96 => 12, 108 => 13, _ => continue };
        operational |= 1 << bit;
        if rate & 0x80 != 0 { basic |= 1 << bit; }
    }
    (operational, basic)
}

#[cfg(test)]
#[path = "vendor_wireless_station_driver_association_ibss_tests.rs"]
mod tests;
