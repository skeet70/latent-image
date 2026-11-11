use super::{frames::Packet, AssociationError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElementError { Truncated, InvalidChannel, InvalidRates, InvalidRateBand, UnsupportedBasicRate, NoCommonPhy }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelWidth { Twenty, Forty }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Band { Ghz2, Ghz5 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateMask(u16);
impl RateMask {
    pub const NONE: Self = Self(0);
    pub const LEGACY: Self = Self(0x3fcf);
    pub const fn from_bits(bits: u16) -> Option<Self> {
        if bits & !0x7fff == 0 { Some(Self(bits)) } else { None }
    }
    pub const fn wire(self) -> u16 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhySet(u8);
impl PhySet {
    pub const STOCK: Self = Self(0x13);
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !0x1b == 0 { Some(Self(bits)) } else { None }
    }
    pub const fn wire(self) -> u8 { self.0 }
    pub const fn without_ht(self) -> Self { Self(self.0 & !0x10) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NegotiatedRates {
    pub desired_phy: PhySet,
    pub desired_rates: RateMask,
    pub basic_rates: RateMask,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateSet {
    pub operational_rates: RateMask,
    pub basic_rates: RateMask,
    pub peer_phy: PhySet,
    pub unknown_basic: bool,
}
impl RateSet {
    pub fn parse(channel: u8, mut raw_tlvs: &[u8]) -> Result<Self, ElementError> {
        let mut supported = None;
        let mut extended = None;
        let mut ht = false;
        let mut erp = false;
        while !raw_tlvs.is_empty() {
            if raw_tlvs.len() < 2 { return Err(ElementError::Truncated); }
            let length = usize::from(raw_tlvs[1]);
            if raw_tlvs.len() < length + 2 { return Err(ElementError::Truncated); }
            let body = &raw_tlvs[2..length + 2];
            match raw_tlvs[0] {
                1 if supported.is_none() && length < 16 => supported = Some(body),
                50 if extended.is_none() => extended = Some(body),
                45 => ht = true,
                42 if length == 1 => erp = true,
                _ => {}
            }
            raw_tlvs = &raw_tlvs[length + 2..];
        }
        let mut result = Self::empty();
        if let Some(rates) = supported { result.add_rates(rates); }
        if let Some(rates) = extended { result.add_rates(rates); }
        result.classify(channel, ht, erp)?;
        Ok(result)
    }
    pub fn from_rates(channel: u8, rates: &[u8]) -> Result<Self, ElementError> {
        let mut result = Self::empty();
        result.add_rates(rates);
        result.classify(channel, false, false)?;
        Ok(result)
    }
    const fn empty() -> Self {
        Self { operational_rates: RateMask::NONE, basic_rates: RateMask::NONE,
            peer_phy: PhySet(0), unknown_basic: false }
    }
    fn add_rates(&mut self, rates: &[u8]) {
        const HALF_MBPS: [u8; 15] = [2, 4, 11, 22, 44, 66, 12, 18, 24, 36, 48, 72, 96, 108, 127];
        for &rate in rates {
            if let Some(index) = HALF_MBPS.iter().position(|&known| known == rate & 0x7f) {
                let bit = 1u16 << index;
                self.operational_rates.0 |= bit;
                if rate & 0x80 != 0 { self.basic_rates.0 |= bit; }
            } else if rate & 0x80 != 0 { self.unknown_basic = true; }
        }
    }
    fn classify(&mut self, channel: u8, ht: bool, erp: bool) -> Result<(), ElementError> {
        if channel == 0 || channel > 199 { return Err(ElementError::InvalidChannel); }
        let operational = self.operational_rates.0;
        let basic = self.basic_rates.0;
        let mut phy = if ht { 0x10 } else { 0 };
        if basic & 0x4000 == 0 {
            if channel <= 14 {
                if operational & 0x3fc0 != 0 || erp { phy |= 2; }
                if basic & 0x3fc0 == 0 && operational & 15 != 0 { phy |= 1; }
            } else {
                if operational & 15 != 0 { return Err(ElementError::InvalidRateBand); }
                phy |= 8;
            }
        }
        self.peer_phy = PhySet(phy);
        Ok(())
    }
    pub fn negotiate(self, local_phy: PhySet, allowed: RateMask) -> Result<NegotiatedRates, ElementError> {
        if self.unknown_basic { return Err(ElementError::UnsupportedBasicRate); }
        let desired_phy = PhySet(self.peer_phy.0 & local_phy.0);
        if desired_phy.0 == 0 { return Err(ElementError::NoCommonPhy); }
        let desired_rates = RateMask(self.operational_rates.0 & allowed.0);
        if desired_rates.0 == 0 && desired_phy.0 & 0x10 == 0 { return Err(ElementError::InvalidRates); }
        Ok(NegotiatedRates { desired_phy, desired_rates, basic_rates: self.basic_rates })
    }
}

pub fn rate_masks(rates: &[u8]) -> (RateMask, RateMask) {
    let mut result = RateSet::empty();
    result.add_rates(rates);
    (result.operational_rates, result.basic_rates)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cipher { Wep40, Tkip, Ccmp, Wep104 }
impl Cipher {
    const fn selector(self) -> u8 { match self { Self::Wep40 => 1, Self::Tkip => 2, Self::Ccmp => 4, Self::Wep104 => 5 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyManagement { Enterprise, PreSharedKey }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecuritySuites { pub group: Cipher, pub pairwise: Cipher, pub key_management: KeyManagement }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pmkid(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityElements { None, Wpa(SecuritySuites), Rsn { suites: SecuritySuites, pmkid: Option<Pmkid> } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServicePeriod { Unlimited, Two, Four, Six }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessCategories(u8);
impl AccessCategories {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(15);
    pub const fn new(best_effort: bool, background: bool, video: bool, voice: bool) -> Self {
        Self(best_effort as u8 | ((background as u8) << 1) | ((video as u8) << 2) | ((voice as u8) << 3))
    }
    const fn wire(self) -> u8 { self.0.reverse_bits() >> 4 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElementConfiguration {
    pub ht_enabled: bool,
    pub width: ChannelWidth,
    pub disable_short_guard_interval: bool,
    pub band: Band,
    pub wmm_enabled: bool,
    pub uapsd_enabled: bool,
    pub delivery_enabled: AccessCategories,
    pub trigger_enabled: AccessCategories,
    pub service_period: ServicePeriod,
    pub coexistence_scan_enabled: bool,
    pub security: SecurityElements,
}
impl ElementConfiguration {
    pub const fn new() -> Self {
        Self { ht_enabled: true, width: ChannelWidth::Twenty, disable_short_guard_interval: false,
            band: Band::Ghz2, wmm_enabled: true, uapsd_enabled: true,
            delivery_enabled: AccessCategories::NONE, trigger_enabled: AccessCategories::NONE,
            service_period: ServicePeriod::Unlimited, coexistence_scan_enabled: false,
            security: SecurityElements::None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtCapabilities {
    pub capability_info: u16,
    pub ampdu_parameters: u8,
    pub rx_mcs: [u8; 10],
    pub extended_capabilities: u16,
    pub tx_beamforming: u32,
    pub antenna_selection: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondaryChannel { None, Above, Below }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtOperation {
    pub primary_channel: u8,
    pub information: u8,
    pub protection: u16,
    pub basic_stbc: u16,
    pub secondary_channel: SecondaryChannel,
    pub non_greenfield_present: bool,
    pub rifs_protection: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessCategoryParameters {
    pub aifsn: u8, pub admission_required: bool, pub contention_min: u16,
    pub contention_max: u16, pub txop: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmmParameters {
    pub uapsd: bool,
    pub parameter_set: u8,
    pub access_categories: Option<[AccessCategoryParameters; 4]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NegotiatedElements {
    pub ht: Option<HtCapabilities>,
    pub operation: Option<HtOperation>,
    pub wmm: Option<WmmParameters>,
    pub channel: Option<u8>,
    pub use_protection: bool,
    pub short_preamble: bool,
    pub forty_mhz: bool,
}
impl NegotiatedElements {
    const fn new() -> Self {
        Self { ht: None, operation: None, wmm: None, channel: None,
            use_protection: false, short_preamble: true, forty_mhz: false }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerChange { Unchanged, Cancel, Arm(Milliseconds) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssociationElements {
    configuration: ElementConfiguration,
    negotiated: NegotiatedElements,
    scan_interval_seconds: Option<u16>,
}
impl AssociationElements {
    pub const fn new() -> Self {
        Self { configuration: ElementConfiguration::new(), negotiated: NegotiatedElements::new(), scan_interval_seconds: None }
    }
    pub fn configure(&mut self, configuration: ElementConfiguration) { self.configuration = configuration; }
    pub const fn negotiated(&self) -> NegotiatedElements { self.negotiated }
    pub fn set_peer_capabilities(&mut self, capabilities: u16) {
        self.negotiated.short_preamble = capabilities & 0x20 != 0;
    }
    pub fn radio_parameters(&self, short_slot: bool) -> super::ibss::BssRadioParameters {
        use super::ibss::{BssRadioParameters, HtProtection};
        let Some(operation) = self.negotiated.operation else {
            return BssRadioParameters {
                erp_protection: self.negotiated.use_protection,
                ..BssRadioParameters::legacy(self.negotiated.short_preamble, short_slot)
            };
        };
        let information = if self.negotiated.forty_mhz { operation.information } else { operation.information & !7 };
        BssRadioParameters {
            secondary_channel: if self.negotiated.forty_mhz { operation.secondary_channel } else { SecondaryChannel::None },
            erp_protection: self.negotiated.use_protection,
            ht_protection: match operation.protection & 3 {
                0 => HtProtection::None,
                1 => HtProtection::NonMember,
                2 => HtProtection::TwentyMhz,
                _ => HtProtection::NonHtMixed,
            },
            non_greenfield_present: operation.non_greenfield_present,
            rifs_protection: operation.rifs_protection,
            basic_stbc: operation.basic_stbc,
            operation_protection: operation.protection,
            operation_information: information,
            short_preamble: self.negotiated.short_preamble,
            short_slot,
        }
    }
    pub fn cancel(&mut self) -> TimerChange {
        let change = if self.scan_interval_seconds.is_some() { TimerChange::Cancel } else { TimerChange::Unchanged };
        self.scan_interval_seconds = None;
        self.negotiated = NegotiatedElements::new();
        change
    }
    pub fn append_before_security(&self, packet: &mut Packet) -> Result<(), AssociationError> {
        if !self.configuration.ht_enabled || self.negotiated.ht.is_none() { return Ok(()); }
        let wide = self.configuration.width == ChannelWidth::Forty;
        let mut capability: u16 = if wide { 0x1062 } else { 0x20 };
        if self.configuration.disable_short_guard_interval { capability &= !0x60; }
        let mut body = [0; 26];
        body[..2].copy_from_slice(&capability.to_le_bytes());
        body[2] = 3;
        body[3] = 0xff;
        body[7] = u8::from(wide);
        body[15] = 1;
        body[19] = u8::from(wide);
        packet.element(45, &body)
    }
    pub fn append_after_security(&self, packet: &mut Packet) -> Result<(), AssociationError> {
        if let SecurityElements::Wpa(suites) = self.configuration.security {
            let mut body = [0; 22];
            body[..6].copy_from_slice(&[0, 0x50, 0xf2, 1, 1, 0]);
            encode_suites(&mut body[6..], suites, [0, 0x50, 0xf2]);
            packet.element(221, &body)?;
        }
        if self.configuration.ht_enabled && self.negotiated.ht.is_some() {
            packet.element(127, &[u8::from(self.configuration.width == ChannelWidth::Forty)])?;
        }
        if self.configuration.wmm_enabled {
            if let Some(peer) = self.negotiated.wmm {
                let mut qos = 0;
                if self.configuration.uapsd_enabled && peer.uapsd {
                    let categories = AccessCategories(self.configuration.delivery_enabled.0 & self.configuration.trigger_enabled.0);
                    qos = categories.wire();
                    if categories.0 != 0 {
                        qos |= match self.configuration.service_period { ServicePeriod::Unlimited => 0, ServicePeriod::Two => 0x20, ServicePeriod::Four => 0x40, ServicePeriod::Six => 0x60 };
                    }
                }
                packet.element(221, &[0, 0x50, 0xf2, 2, 0, 1, qos])?;
            }
        }
        if let SecurityElements::Rsn { suites, pmkid } = self.configuration.security {
            let mut body = [0; 38];
            body[..2].copy_from_slice(&[1, 0]);
            encode_suites(&mut body[2..18], suites, [0, 0x0f, 0xac]);
            let length = if let Some(Pmkid(id)) = pmkid {
                body[20] = 1;
                body[22..38].copy_from_slice(&id);
                38
            } else { 20 };
            packet.element(48, &body[..length])?;
        }
        Ok(())
    }
    pub fn observe_beacon(&mut self, bytes: &[u8]) -> Result<(), ElementError> {
        let mut next = *self;
        next.negotiated = NegotiatedElements::new();
        next.process(bytes, false)?;
        *self = next;
        Ok(())
    }
    pub fn observe_response(&mut self, bytes: &[u8]) -> Result<TimerChange, ElementError> {
        let mut next = *self;
        next.negotiated.operation = None;
        next.negotiated.use_protection = false;
        let interval = next.process(bytes, true)?;
        next.negotiated.forty_mhz = next.configuration.width == ChannelWidth::Forty
            && next.negotiated.ht.is_some_and(|ht| ht.capability_info & 2 != 0);
        let change = match interval {
            Some(seconds) if Some(seconds) != self.scan_interval_seconds => TimerChange::Arm(Milliseconds(u32::from(seconds) * 1000)),
            None if self.scan_interval_seconds.is_some() => TimerChange::Cancel,
            _ => TimerChange::Unchanged,
        };
        next.scan_interval_seconds = interval;
        *self = next;
        Ok(change)
    }
    pub fn observe_connected_beacon(&mut self, bytes: &[u8]) -> Result<TimerChange, ElementError> {
        let mut next = *self;
        let interval = next.process(bytes, false)?;
        let change = match interval {
            Some(seconds) if Some(seconds) != self.scan_interval_seconds => TimerChange::Arm(Milliseconds(u32::from(seconds) * 1000)),
            None if self.scan_interval_seconds.is_some() => TimerChange::Cancel,
            _ => TimerChange::Unchanged,
        };
        next.scan_interval_seconds = interval;
        *self = next;
        Ok(change)
    }
    fn process(&mut self, mut bytes: &[u8], response: bool) -> Result<Option<u16>, ElementError> {
        let mut interval = None;
        let mut channel = None;
        while !bytes.is_empty() {
            if bytes.len() < 2 { return Err(ElementError::Truncated); }
            let length = usize::from(bytes[1]);
            if bytes.len() < length + 2 { return Err(ElementError::Truncated); }
            let body = &bytes[2..length + 2];
            match (bytes[0], length) {
                (45, 26) if self.configuration.ht_enabled => {
                    let mut rx_mcs = [0; 10];
                    rx_mcs.copy_from_slice(&body[3..13]);
                    self.negotiated.ht = Some(HtCapabilities { capability_info: le16(body), ampdu_parameters: body[2], rx_mcs,
                        extended_capabilities: le16(&body[19..]), tx_beamforming: u32::from_le_bytes([body[21], body[22], body[23], body[24]]), antenna_selection: body[25] });
                }
                (61, 22) if self.configuration.ht_enabled => {
                    if channel.is_none() { channel = Some(body[0]); }
                    let mut information = body[1];
                    if self.configuration.width == ChannelWidth::Twenty { information &= !7; }
                    let previous = self.negotiated.operation.map_or(SecondaryChannel::None, |op| op.secondary_channel);
                    let secondary_channel = match information & 3 { 0 => SecondaryChannel::None, 1 => SecondaryChannel::Above, 3 => SecondaryChannel::Below, _ => previous };
                    self.negotiated.operation = Some(HtOperation { primary_channel: body[0], information, protection: le16(&body[2..]), basic_stbc: le16(&body[4..]), secondary_channel,
                        non_greenfield_present: body[2] & 4 != 0, rifs_protection: information & 8 == 0 });
                }
                (3, 1) => channel = Some(body[0]),
                (42, 1) if self.configuration.band == Band::Ghz2 => {
                    self.negotiated.use_protection = body[0] & 2 != 0;
                    if body[0] & 4 != 0 { self.negotiated.short_preamble = false; }
                }
                (74, 14) if self.configuration.ht_enabled => interval = Some(le16(&body[4..]).max(10)),
                (221, _) if self.configuration.wmm_enabled && length >= 7 && body[..4] == [0, 0x50, 0xf2, 2] => {
                    let valid = (body[4] == 0 && length == 7) || (body[4] == 1 && length == 24);
                    if valid {
                        let mut parameters = WmmParameters { uapsd: body[6] & 0x80 != 0, parameter_set: body[6] & 15, access_categories: None };
                        if response {
                            let Some(previous) = self.negotiated.wmm else {
                                bytes = &bytes[length + 2..];
                                continue;
                            };
                            parameters.uapsd = previous.uapsd;
                            if body[4] == 0 { parameters = previous; }
                        }
                        if body[4] == 1 {
                            if !response {
                                if let Some(previous) = self.negotiated.wmm {
                                    if previous.parameter_set == parameters.parameter_set && previous.access_categories.is_some() {
                                        parameters.access_categories = previous.access_categories;
                                        self.negotiated.wmm = Some(parameters);
                                        bytes = &bytes[length + 2..];
                                        continue;
                                    }
                                }
                            }
                            let mut ac = [AccessCategoryParameters { aifsn: 0, admission_required: false, contention_min: 0, contention_max: 0, txop: 0 }; 4];
                            for (index, value) in ac.iter_mut().enumerate() {
                                let record = &body[8 + index * 4..];
                                *value = AccessCategoryParameters { aifsn: record[0] & 15, admission_required: record[0] & 16 != 0,
                                    contention_min: (1u16 << (record[1] & 15)) - 1, contention_max: (1u16 << (record[1] >> 4)) - 1, txop: le16(&record[2..]) };
                            }
                            parameters.access_categories = Some(ac);
                        }
                        self.negotiated.wmm = Some(parameters);
                    }
                }
                _ => {}
            }
            bytes = &bytes[length + 2..];
        }
        if let Some(value) = channel {
            let valid = match self.configuration.band { Band::Ghz2 => (1..=14).contains(&value), Band::Ghz5 => (15..=199).contains(&value) };
            if !valid { return Err(ElementError::InvalidChannel); }
            self.negotiated.channel = Some(value);
        }
        if !self.configuration.coexistence_scan_enabled || self.configuration.band != Band::Ghz2
            || self.configuration.width != ChannelWidth::Forty || !self.negotiated.ht.is_some_and(|ht| ht.capability_info & 2 != 0) { interval = None; }
        Ok(interval)
    }
}

fn le16(bytes: &[u8]) -> u16 { u16::from_le_bytes([bytes[0], bytes[1]]) }
fn encode_suites(body: &mut [u8], suites: SecuritySuites, oui: [u8; 3]) {
    body[..3].copy_from_slice(&oui);
    body[3] = suites.group.selector();
    body[4..6].copy_from_slice(&[1, 0]);
    body[6..9].copy_from_slice(&oui);
    body[9] = suites.pairwise.selector();
    body[10..12].copy_from_slice(&[1, 0]);
    body[12..15].copy_from_slice(&oui);
    body[15] = match suites.key_management { KeyManagement::Enterprise => 1, KeyManagement::PreSharedKey => 2 };
}

#[cfg(test)]
#[path = "vendor_wireless_station_driver_association_elements_tests.rs"]
mod tests;
