use super::{AssociationError, AuthenticationAlgorithm, Candidate, MacAddress, Ssid};

pub const PACKET_CAPACITY: usize = 1024;

#[derive(Debug, Eq, PartialEq)]
pub struct Packet { bytes: [u8; PACKET_CAPACITY], length: usize }
impl Packet {
    pub const fn new() -> Self { Self { bytes: [0; PACKET_CAPACITY], length: 0 } }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length] }
    pub(crate) fn clear(&mut self) { self.bytes.fill(0); self.length = 0; }
    pub(crate) fn device(&mut self, code: u8, sequence: u8, length: usize) -> &mut [u8] {
        self.clear();
        self.length = length + 8;
        self.bytes[..2].copy_from_slice(&(self.length as u16).to_le_bytes());
        self.bytes[4] = code;
        self.bytes[5] = 1;
        self.bytes[6] = sequence;
        &mut self.bytes[8..self.length]
    }
    pub(crate) fn header(&mut self, control: u16, peer: MacAddress, local: MacAddress, bssid: MacAddress) {
        self.clear();
        self.length = 24;
        self.bytes[..2].copy_from_slice(&control.to_le_bytes());
        self.bytes[4..10].copy_from_slice(&peer.0);
        self.bytes[10..16].copy_from_slice(&local.0);
        self.bytes[16..22].copy_from_slice(&bssid.0);
    }
    pub(crate) fn append(&mut self, bytes: &[u8]) -> Result<(), AssociationError> {
        let end = self.length + bytes.len();
        if end > self.bytes.len() { return Err(AssociationError::Capacity); }
        self.bytes[self.length..end].copy_from_slice(bytes);
        self.length = end;
        Ok(())
    }
    pub(crate) fn element(&mut self, id: u8, bytes: &[u8]) -> Result<(), AssociationError> {
        if bytes.len() > 255 { return Err(AssociationError::Capacity); }
        self.append(&[id, bytes.len() as u8])?;
        self.append(bytes)
    }
    pub(crate) fn authenticate(&mut self, local: MacAddress, candidate: &Candidate,
        algorithm: AuthenticationAlgorithm, transaction: u16, challenge: &[u8]) -> Result<(), AssociationError> {
        self.header(if transaction == 3 && algorithm == AuthenticationAlgorithm::Shared { 0x40b0 } else { 0xb0 },
            candidate.peer, local, candidate.bssid);
        self.append(&(algorithm as u16).to_le_bytes())?;
        self.append(&transaction.to_le_bytes())?;
        self.append(&0u16.to_le_bytes())?;
        if !challenge.is_empty() { self.element(16, challenge)?; }
        Ok(())
    }
    pub(crate) fn associate(&mut self, local: MacAddress, candidate: &Candidate,
        ssid: Ssid, security_elements: &[u8], current: Option<MacAddress>) -> Result<(), AssociationError> {
        self.header(if current.is_some() { 0x20 } else { 0 }, candidate.peer, local, candidate.bssid);
        self.append(&candidate.capabilities.to_le_bytes())?;
        self.append(&10u16.to_le_bytes())?;
        if let Some(current) = current { self.append(&current.0)?; }
        self.element(0, ssid.as_bytes())?;
        self.element(1, &candidate.rates()[..candidate.rates().len().min(8)])?;
        if candidate.rates().len() > 8 { self.element(50, &candidate.rates()[8..])?; }
        self.append(security_elements)
    }
    pub(crate) fn deauthenticate(&mut self, local: MacAddress, candidate: &Candidate, reason: u16) -> Result<(), AssociationError> {
        self.header(0xc0, candidate.peer, local, candidate.bssid);
        self.append(&reason.to_le_bytes())
    }
}

pub(crate) fn half(bytes: &[u8], offset: usize) -> Result<u16, AssociationError> {
    let value = bytes.get(offset..offset + 2).ok_or(AssociationError::TruncatedFrame)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

pub(crate) struct Management<'a> {
    pub control: u16,
    pub destination: MacAddress,
    pub source: MacAddress,
    pub bssid: MacAddress,
    pub body: &'a [u8],
}
impl<'a> Management<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AssociationError> {
        if bytes.len() < 24 { return Err(AssociationError::TruncatedFrame); }
        let control = half(bytes, 0)?;
        if control & 0x000c != 0 { return Err(AssociationError::NotManagement); }
        let mut destination = MacAddress([0; 6]);
        let mut source = MacAddress([0; 6]);
        let mut bssid = MacAddress([0; 6]);
        destination.0.copy_from_slice(&bytes[4..10]);
        source.0.copy_from_slice(&bytes[10..16]);
        bssid.0.copy_from_slice(&bytes[16..22]);
        Ok(Self { control, destination, source, bssid, body: &bytes[24..] })
    }
}

pub(crate) struct Elements<'a>(&'a [u8]);
impl<'a> Elements<'a> {
    pub fn parse(mut bytes: &'a [u8]) -> Result<Self, AssociationError> {
        let original = bytes;
        while !bytes.is_empty() {
            if bytes.len() < 2 || bytes.len() < bytes[1] as usize + 2 { return Err(AssociationError::TruncatedElements); }
            bytes = &bytes[bytes[1] as usize + 2..];
        }
        Ok(Self(original))
    }
    pub fn get(&self, id: u8) -> Option<&'a [u8]> {
        let mut bytes = self.0;
        while !bytes.is_empty() {
            let length = bytes[1] as usize;
            if bytes[0] == id { return Some(&bytes[2..2 + length]); }
            bytes = &bytes[2 + length..];
        }
        None
    }
    pub fn vendor(&self, oui_type: [u8; 4]) -> Option<&'a [u8]> {
        let mut bytes = self.0;
        while !bytes.is_empty() {
            let length = bytes[1] as usize;
            let body = &bytes[2..2 + length];
            if bytes[0] == 221 && body.get(..4) == Some(oui_type.as_slice()) { return Some(body); }
            bytes = &bytes[2 + length..];
        }
        None
    }
}
