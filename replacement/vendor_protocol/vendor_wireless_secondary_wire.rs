use super::{Error, PeerProjection, Protection, RootUpdate};
use crate::vendor_wireless_network::MacAddress;
use crate::vendor_wireless_peers::PeerIndex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Band { Ghz2, Ghz5 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel(u8);
impl Channel {
    pub fn new(value: u8) -> Result<Self, Error> { if value == 0 { Err(Error::WrongChannel) } else { Ok(Self(value)) } }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BssParameters { pub band: Band, pub channel: Channel, pub configured_secondary: u8, pub rifs: u8, pub ht_info3: u16, pub ht_info2: u16 }
impl BssParameters {
    pub fn encode(self, projection: PeerProjection) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[0] = 1;
        bytes[1] = match self.band { Band::Ghz2 => 1, Band::Ghz5 => 2 };
        bytes[2] = self.channel.get();
        bytes[3] = self.configured_secondary;
        bytes[4] = u8::from(projection.non_erp);
        bytes[5] = match projection.protection { Protection::None => 0, Protection::Mixed => 2, Protection::Legacy => 3 };
        bytes[6] = u8::from(projection.non_greenfield);
        bytes[7] = self.rifs;
        bytes[8..10].copy_from_slice(&self.ht_info3.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.ht_info2.to_le_bytes());
        bytes[12] = projection.secondary_offset;
        bytes[13] = u8::from(projection.short_preamble);
        bytes[14] = u8::from(projection.short_slot);
        bytes[15] = 0x72;
        bytes
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capability(pub u16);
impl Capability {
    pub fn project(self, projection: PeerProjection) -> Self { Self((self.0 & !0x400) | if projection.short_slot { 0x400 } else { 0 }) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BeaconElements<'a>(&'a [u8]);
impl<'a> BeaconElements<'a> {
    pub fn new(elements: &'a [u8]) -> Result<Self, Error> { if elements.len() > 600 { Err(Error::BeaconTooLarge) } else { Ok(Self(elements)) } }
    pub fn encode(self, capability: Capability, output: &mut [u8; 608]) -> usize {
        output[..8].fill(0);
        output[0] = 1;
        output[1] = 1;
        output[4..6].copy_from_slice(&capability.0.to_le_bytes());
        output[6..8].copy_from_slice(&(self.0.len() as u16).to_le_bytes());
        output[8..8+self.0.len()].copy_from_slice(self.0);
        8+self.0.len()
    }
    pub fn encode_projected(self, capability: Capability, parameters: BssParameters, projection: PeerProjection, root: Option<RootUpdate>, include_obss: bool, output: &mut [u8; 608]) -> Result<usize, Error> {
        let mut cursor = 8;
        let mut length = self.encode(capability, output);
        let mut has_obss = false;
        while cursor < length {
            if cursor + 2 > length { return Err(Error::Truncated); }
            let extent = usize::from(output[cursor + 1]) + 2;
            if cursor + extent > length { return Err(Error::Truncated); }
            let element = &mut output[cursor..cursor + extent];
            match element[0] {
                42 if extent == 3 => {
                    element[2] = if root.is_some_and(|r| r.index != 0) { 2 } else { 0 };
                    if projection.non_erp { element[2] |= 3; }
                    if !projection.short_preamble { element[2] |= 4; }
                }
                61 if extent == 24 => {
                    element[2] = parameters.channel.get();
                    element[3] = projection.secondary_offset;
                    let protection = match projection.protection { Protection::Legacy => 3, _ if root.is_some_and(|r| r.first == 1) => 1, Protection::Mixed => 2, Protection::None => 0 };
                    element[4] = protection | if projection.non_greenfield { 4 } else { 0 };
                    element[5] = 0;
                    element[6..8].copy_from_slice(&parameters.ht_info3.to_le_bytes());
                    element[8..].fill(0);
                }
                74 if include_obss && extent == 16 => { element.copy_from_slice(&[74, 14, 20, 0, 10, 0, 44, 1, 200, 0, 20, 0, 5, 0, 25, 0]); has_obss = true; }
                74 => { has_obss = true; }
                _ => {}
            }
            cursor += extent;
        }
        if include_obss && !has_obss {
            if length + 16 > output.len() { return Err(Error::BeaconTooLarge); }
            output[length..length + 16].copy_from_slice(&[74, 14, 20, 0, 10, 0, 44, 1, 200, 0, 20, 0, 5, 0, 25, 0]);
            length += 16;
        }
        output[6..8].copy_from_slice(&((length - 8) as u16).to_le_bytes());
        Ok(length)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkName { bytes: [u8; 32], length: u8 }
impl NetworkName {
    pub fn new(value: &[u8]) -> Result<Self, Error> {
        if value.len() > 32 { return Err(Error::NetworkNameTooLong); }
        let mut bytes = [0; 32];
        bytes[..value.len()].copy_from_slice(value);
        Ok(Self { bytes, length: value.len() as u8 })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceParameters {
    pub name: NetworkName, pub bssid: MacAddress, pub local: MacAddress, pub qos: u8,
    pub phy_index: u8, pub operational_rates: u16, pub basic_rates: u16, pub phy: u8,
    pub security_enabled: bool, pub secondary_enabled: bool,
}
impl InterfaceParameters {
    pub fn encode_disconnected(self, bss: BssParameters, projection: PeerProjection, peer: PeerIndex) -> [u8; 80] {
        let mut bytes = [0; 80];
        bytes[0] = 1;
        bytes[1] = 1;
        bytes[2] = 0;
        bytes[3] = self.name.length;
        bytes[4..36].copy_from_slice(&self.name.bytes);
        bytes[36..42].copy_from_slice(&self.bssid.0);
        bytes[42] = self.qos;
        bytes[44..46].copy_from_slice(&self.operational_rates.to_le_bytes());
        bytes[46..48].copy_from_slice(&self.basic_rates.to_le_bytes());
        bytes[48] = peer.wire();
        bytes[51] = self.phy_index;
        bytes[52] = if self.security_enabled { 7 } else { 0 };
        bytes[53] = if self.security_enabled { 7 } else { 1 };
        bytes[54] = self.phy;
        bytes[55..61].copy_from_slice(&self.local.0);
        bytes[62] = u8::from(self.secondary_enabled);
        bytes[64..80].copy_from_slice(&bss.encode(projection));
        bytes
    }
}
