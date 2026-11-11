use super::crypto::{derive_pairwise_keys, equal_authenticator, hmac_sha1, unwrap_key_data, MasterKey, PairwiseKeys};
use super::crypto::legacy::{hmac_md5, rc4_skip};
use super::profile::{Cipher, InformationElements, KeyIndex, PersonalProtocol, SelectedAccessPoint};
use super::MacAddress;

const KEY_TYPE: u16 = 0x0008;
const INSTALL: u16 = 0x0040;
const ACK: u16 = 0x0080;
const MIC: u16 = 0x0100;
const SECURE: u16 = 0x0200;
const ERROR: u16 = 0x0400;
const REQUEST: u16 = 0x0800;
const ENCRYPTED: u16 = 0x1000;
const HEADER: usize = 99;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeError { Truncated, Descriptor, Flags, Peer, Replay, State, Mic, Nonce, NonceRequired, Decryption, KeyLength, KeyData, SecurityElements, Cipher }
impl HandshakeError {
    pub const fn disconnects(self) -> bool {
        matches!(self, Self::Nonce | Self::KeyLength | Self::KeyData | Self::SecurityElements)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakePhase { MessageOne, MessageThree, Group, Complete }
#[derive(Clone, Eq, PartialEq)]
pub struct EapolFrame { bytes: [u8; 512], length: u16 }
impl EapolFrame {
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length as usize] }
}
impl core::fmt::Debug for EapolFrame { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.debug_struct("EapolFrame").field("length", &self.length).finish() } }

#[derive(Clone, Eq, PartialEq)]
pub enum KeyMaterial { Wep(super::profile::WepKey), Tkip([u8; 32]), Ccmp([u8; 16]) }
impl core::fmt::Debug for KeyMaterial {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self { Self::Wep(_) => "Wep([redacted])", Self::Tkip(_) => "Tkip([redacted])", Self::Ccmp(_) => "Ccmp([redacted])" })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeySlot { pub index: KeyIndex, pub peer: Option<MacAddress> }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyInstall { pub slot: KeySlot, pub transmit: bool, pub material: KeyMaterial, pub receive_sequence: [u8; 6] }
#[derive(Debug)]
pub struct HandshakeTransaction {
    pub reply: EapolFrame,
    pub pairwise: Option<KeyInstall>,
    pub group: Option<KeyInstall>,
    pub completes: bool,
}

struct ReceivedKey<'a> { frame: &'a [u8], info: u16, replay: [u8; 8], nonce: [u8; 32], data: &'a [u8] }
impl<'a> ReceivedKey<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, HandshakeError> {
        if bytes.len() < HEADER { return Err(HandshakeError::Truncated); }
        let length = 4 + u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
        if length < HEADER || length > bytes.len() { return Err(HandshakeError::Truncated); }
        if bytes[1] != 3 || !matches!(bytes[4], 2 | 254) { return Err(HandshakeError::Descriptor); }
        let info = u16::from_be_bytes([bytes[5], bytes[6]]);
        DescriptorVersion::parse(info & 7)?;
        if info & ACK == 0 || info & (REQUEST | ERROR) != 0 { return Err(HandshakeError::Flags); }
        if info & KEY_TYPE != 0 && info & 0x30 != 0 { return Err(HandshakeError::Flags); }
        let data_length = u16::from_be_bytes([bytes[97], bytes[98]]) as usize;
        if data_length > length - HEADER { return Err(HandshakeError::Truncated); }
        let mut replay = [0; 8]; replay.copy_from_slice(&bytes[9..17]);
        let mut nonce = [0; 32]; nonce.copy_from_slice(&bytes[17..49]);
        Ok(Self { frame: &bytes[..length], info, replay, nonce, data: &bytes[HEADER..HEADER + data_length] })
    }
    fn verify(&self, keys: &PairwiseKeys) -> bool {
        let digest = self.version().authenticate(keys.confirmation(), &[&self.frame[..81], &[0; 16], &self.frame[97..]]);
        equal_authenticator(&digest, &self.frame[81..97])
    }
    fn version(&self) -> DescriptorVersion { DescriptorVersion::parse(self.info & 7).expect("parsed descriptor") }
    fn reply(&self, keys: &PairwiseKeys, info: u16, nonce: Option<&[u8; 32]>, data: &[u8]) -> EapolFrame {
        let mut reply = EapolFrame { bytes: [0; 512], length: (HEADER + data.len()) as u16 };
        let bytes = &mut reply.bytes;
        bytes[0] = 1; bytes[1] = 3;
        bytes[2..4].copy_from_slice(&((HEADER - 4 + data.len()) as u16).to_be_bytes());
        bytes[4] = self.frame[4]; bytes[5..7].copy_from_slice(&info.to_be_bytes());
        if bytes[4] == 254 { bytes[7..9].copy_from_slice(&self.frame[7..9]); }
        bytes[9..17].copy_from_slice(&self.replay);
        if let Some(nonce) = nonce { bytes[17..49].copy_from_slice(nonce); }
        bytes[97..99].copy_from_slice(&(data.len() as u16).to_be_bytes());
        bytes[HEADER..HEADER + data.len()].copy_from_slice(data);
        let digest = self.version().authenticate(keys.confirmation(), &[&bytes[..reply.length as usize]]);
        bytes[81..97].copy_from_slice(&digest);
        reply
    }
}

pub struct RsnSupplicant {
    local: MacAddress,
    access_point: SelectedAccessPoint,
    master: MasterKey,
    nonce: [u8; 32],
    authenticator_nonce: [u8; 32],
    keys: Option<PairwiseKeys>,
    replay: Option<[u8; 8]>,
    request_counter: [u8; 8],
    phase: HandshakePhase,
    association_elements: InformationElements,
}
impl RsnSupplicant {
    pub fn new(local: MacAddress, access_point: SelectedAccessPoint, master: MasterKey, nonce: [u8; 32]) -> Result<Self, HandshakeError> {
        if access_point.protocol == PersonalProtocol::None { return Err(HandshakeError::Cipher); }
        if !matches!(access_point.pairwise, Cipher::Tkip | Cipher::Ccmp) || !matches!(access_point.group, Cipher::Wep40 | Cipher::Wep104 | Cipher::Tkip | Cipher::Ccmp) { return Err(HandshakeError::Cipher); }
        let association_elements = association_elements(&access_point)?;
        if security_element(&access_point.advertised_elements, access_point.protocol).is_none() { return Err(HandshakeError::SecurityElements); }
        Ok(Self { local, access_point, master, nonce, authenticator_nonce: [0; 32], keys: None, replay: None, request_counter: [0; 8], phase: HandshakePhase::MessageOne, association_elements })
    }
    pub const fn phase(&self) -> HandshakePhase { self.phase }
    pub fn association_elements(&self) -> &InformationElements { &self.association_elements }
    pub fn begin_rekey(&mut self, nonce: [u8; 32]) -> Result<(), HandshakeError> {
        if self.phase != HandshakePhase::Complete { return Err(HandshakeError::State); }
        if equal_authenticator(&nonce, &self.nonce) { return Err(HandshakeError::Nonce); }
        self.nonce = nonce;
        self.phase = HandshakePhase::MessageOne;
        Ok(())
    }
    pub fn michael_failure_request(&mut self, event: crate::vendor_wireless_station_driver::association::MicFailure) -> Result<EapolFrame, HandshakeError> {
        use crate::vendor_wireless_station_driver::association::MicKey;
        if event.bssid != self.access_point.address { return Err(HandshakeError::Peer); }
        let cipher = match event.key { MicKey::Pairwise => self.access_point.pairwise, MicKey::Group => self.access_point.group };
        if cipher != Cipher::Tkip { return Err(HandshakeError::Cipher); }
        let version = if self.access_point.pairwise == Cipher::Ccmp { DescriptorVersion::Sha1Aes } else { DescriptorVersion::Md5Rc4 };
        let confirmed = matches!(self.phase, HandshakePhase::Group | HandshakePhase::Complete);
        let mut info = REQUEST | ERROR | match version { DescriptorVersion::Md5Rc4 => 1, DescriptorVersion::Sha1Aes => 2 };
        if event.key == MicKey::Pairwise { info |= KEY_TYPE; }
        if confirmed { info |= MIC; }
        let mut reply = EapolFrame { bytes: [0; 512], length: HEADER as u16 };
        reply.bytes[..5].copy_from_slice(&[1,3,0,95,match self.access_point.protocol { PersonalProtocol::Wpa => 254, PersonalProtocol::Rsn => 2, PersonalProtocol::None => return Err(HandshakeError::Cipher) }]);
        reply.bytes[5..7].copy_from_slice(&info.to_be_bytes());
        reply.bytes[9..17].copy_from_slice(&self.request_counter);
        if confirmed {
            let keys = self.keys.as_ref().ok_or(HandshakeError::State)?;
            let mic = version.authenticate(keys.confirmation(), &[reply.as_bytes()]);
            reply.bytes[81..97].copy_from_slice(&mic);
        }
        for byte in self.request_counter.iter_mut().rev() {
            let (next, carry) = byte.overflowing_add(1); *byte = next; if !carry { break; }
        }
        Ok(reply)
    }
    pub fn receive(&mut self, peer: MacAddress, frame: &[u8]) -> Result<HandshakeTransaction, HandshakeError> {
        if peer != self.access_point.address { return Err(HandshakeError::Peer); }
        let received = ReceivedKey::parse(frame)?;
        if received.frame[4] != match self.access_point.protocol { PersonalProtocol::Wpa => 254, PersonalProtocol::Rsn => 2, PersonalProtocol::None => return Err(HandshakeError::Cipher) } { return Err(HandshakeError::Descriptor); }
        let pairwise = received.info & KEY_TYPE != 0;
        if self.access_point.pairwise == Cipher::Ccmp && received.version() != DescriptorVersion::Sha1Aes && (pairwise || self.access_point.group == Cipher::Ccmp) { return Err(HandshakeError::Descriptor); }
        if self.replay.is_some_and(|previous| received.replay <= previous) { return Err(HandshakeError::Replay); }
        if received.info & KEY_TYPE != 0 && received.info & MIC == 0 {
            if received.info & (INSTALL | SECURE | ENCRYPTED) != 0 { return Err(HandshakeError::Flags); }
            if self.phase == HandshakePhase::Complete { return Err(HandshakeError::NonceRequired); }
            if !matches!(self.phase, HandshakePhase::MessageOne | HandshakePhase::MessageThree) { return Err(HandshakeError::State); }
            let keys = derive_pairwise_keys(&self.master, &self.local.0, &peer.0, &self.nonce, &received.nonce);
            let reply = received.reply(&keys, (received.info & 7) | KEY_TYPE | MIC, Some(&self.nonce), self.association_elements.as_bytes());
            self.authenticator_nonce = received.nonce;
            self.keys = Some(keys);
            self.phase = HandshakePhase::MessageThree;
            return Ok(HandshakeTransaction { reply, pairwise: None, group: None, completes: false });
        }
        let keys = self.keys.as_ref().ok_or(HandshakeError::State)?;
        if received.info & MIC == 0 || !received.verify(keys) { return Err(HandshakeError::Mic); }
        if pairwise && self.phase != HandshakePhase::MessageThree { return Err(HandshakeError::State); }
        if !pairwise && !matches!(self.phase, HandshakePhase::Group | HandshakePhase::Complete) { return Err(HandshakeError::State); }
        let mut plaintext = [0; 512];
        let encrypted = received.info & ENCRYPTED != 0 || (!pairwise && self.access_point.protocol == PersonalProtocol::Wpa);
        let decryption_error = if self.access_point.protocol == PersonalProtocol::Rsn { HandshakeError::Decryption } else { HandshakeError::KeyData };
        let data = if encrypted {
            let length = match received.version() {
                DescriptorVersion::Sha1Aes => unwrap_key_data(keys.encryption(), received.data, &mut plaintext).map_err(|_| decryption_error)?,
                DescriptorVersion::Md5Rc4 => {
                    if received.data.len() > plaintext.len() { return Err(decryption_error); }
                    let mut key = [0; 32]; key[..16].copy_from_slice(&received.frame[49..65]); key[16..].copy_from_slice(keys.encryption());
                    plaintext[..received.data.len()].copy_from_slice(received.data);
                    rc4_skip(&key, 256, &mut plaintext[..received.data.len()]).map_err(|_| decryption_error)?;
                    key.fill(0);
                    received.data.len()
                }
            };
            &plaintext[..length]
        } else { received.data };
        let (security, group) = if !pairwise && self.access_point.protocol == PersonalProtocol::Wpa {
            let length = u16::from_be_bytes([received.frame[7], received.frame[8]]) as usize;
            if length > data.len() || data.len() > 32 { return Err(HandshakeError::KeyLength); }
            (None, Some(group_install(self.access_point.group, (received.info >> 4) as u8 & 3, &data[..length], &received.frame[65..71])?))
        } else {
            parse_key_data(data, encrypted, &received.frame[65..71], self.access_point.group, self.access_point.protocol)?
        };
        if pairwise {
            if !equal_authenticator(&self.authenticator_nonce, &received.nonce) { return Err(HandshakeError::Nonce); }
            let length = if self.access_point.pairwise == Cipher::Tkip { 32 } else { 16 };
            if u16::from_be_bytes([received.frame[7], received.frame[8]]) != length { return Err(HandshakeError::KeyLength); }
            if security != security_element(&self.access_point.advertised_elements, self.access_point.protocol) { return Err(HandshakeError::SecurityElements); }
        } else if group.is_none() { return Err(HandshakeError::KeyData); }
        let pairwise_key = if pairwise && received.info & INSTALL != 0 {
            let material = match self.access_point.pairwise { Cipher::Tkip => KeyMaterial::Tkip(swap_mic(keys.temporal_tkip())), _ => KeyMaterial::Ccmp(*keys.temporal()) };
            let mut receive_sequence = [0; 6];
            if self.access_point.protocol == PersonalProtocol::Wpa { receive_sequence.copy_from_slice(&received.frame[65..71]); }
            Some(KeyInstall { slot: KeySlot { index: KeyIndex::parse(0).map_err(|_| HandshakeError::KeyLength)?, peer: Some(peer) }, transmit: true, material, receive_sequence })
        } else { None };
        let reply = received.reply(keys, if pairwise { (received.info & 7) | KEY_TYPE | MIC | (received.info & SECURE) } else { (received.info & 0x37) | MIC | SECURE }, None, &[]);
        plaintext.fill(0);
        self.replay = Some(received.replay);
        let completes = group.is_some();
        self.phase = if completes { HandshakePhase::Complete } else { HandshakePhase::Group };
        Ok(HandshakeTransaction { reply, pairwise: pairwise_key, group, completes })
    }
}
impl Drop for RsnSupplicant { fn drop(&mut self) { self.nonce.fill(0); self.authenticator_nonce.fill(0); } }

fn parse_key_data<'a>(mut data: &'a [u8], encrypted: bool, sequence: &[u8], cipher: Cipher, protocol: PersonalProtocol) -> Result<(Option<&'a [u8]>, Option<KeyInstall>), HandshakeError> {
    let mut rsn = None;
    let mut group = None;
    while !data.is_empty() {
        if data[0] == 221 && (data.len() == 1 || data[1] == 0) { break; }
        if data.len() < 2 || 2 + data[1] as usize > data.len() { return Err(HandshakeError::KeyData); }
        let length = 2 + data[1] as usize;
        let element = &data[..length];
        if (element[0] == 48 && protocol == PersonalProtocol::Rsn) || (element.len() >= 6 && element[0] == 221 && element[2..6] == [0,80,242,1] && protocol == PersonalProtocol::Wpa) { if rsn.replace(element).is_some() { return Err(HandshakeError::KeyData); } }
        if element[0] == 221 && element.len() >= 6 && element[2..6] == [0,15,172,1] {
            if !encrypted || element.len() < 8 || group.is_some() { return Err(HandshakeError::KeyData); }
            group = Some(group_install(cipher, element[6] & 3, &element[8..], sequence)?);
        }
        data = &data[length..];
    }
    Ok((rsn, group))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DescriptorVersion { Md5Rc4, Sha1Aes }
impl DescriptorVersion {
    fn parse(value: u16) -> Result<Self, HandshakeError> { match value { 1 => Ok(Self::Md5Rc4), 2 => Ok(Self::Sha1Aes), _ => Err(HandshakeError::Descriptor) } }
    fn authenticate(self, key: &[u8], fragments: &[&[u8]]) -> [u8; 16] {
        match self { Self::Md5Rc4 => hmac_md5(key, fragments), Self::Sha1Aes => { let hash = hmac_sha1(key, fragments); let mut mic = [0; 16]; mic.copy_from_slice(&hash[..16]); mic } }
    }
}
fn swap_mic(key: &[u8; 32]) -> [u8; 32] {
    let mut output = *key; output[16..24].copy_from_slice(&key[24..]); output[24..].copy_from_slice(&key[16..24]); output
}
fn group_install(cipher: Cipher, index: u8, key: &[u8], sequence: &[u8]) -> Result<KeyInstall, HandshakeError> {
    let material = match (cipher, key.len()) {
        (Cipher::Wep40, 5) | (Cipher::Wep104, 13) => KeyMaterial::Wep(super::profile::WepKey::parse(key).map_err(|_| HandshakeError::KeyLength)?),
        (Cipher::Ccmp, 16) => KeyMaterial::Ccmp(key.try_into().map_err(|_| HandshakeError::KeyLength)?),
        (Cipher::Tkip, 32) => KeyMaterial::Tkip(swap_mic(key.try_into().map_err(|_| HandshakeError::KeyLength)?)),
        _ => return Err(HandshakeError::KeyLength),
    };
    let mut receive_sequence = [0; 6];
    if matches!(cipher, Cipher::Tkip | Cipher::Ccmp) { receive_sequence.copy_from_slice(sequence); }
    Ok(KeyInstall { slot: KeySlot { index: KeyIndex::parse(index).map_err(|_| HandshakeError::KeyLength)?, peer: Some(MacAddress([255; 6])) }, transmit: false, material, receive_sequence })
}
fn security_element(elements: &InformationElements, protocol: PersonalProtocol) -> Option<&[u8]> {
    if protocol == PersonalProtocol::None { return None; }
    if protocol == PersonalProtocol::Rsn { return elements.find(48); }
    let mut rest = elements.as_bytes();
    while !rest.is_empty() {
        let length = rest[1] as usize + 2;
        if rest[0] == 221 && length >= 6 && rest[2..6] == [0,80,242,1] { return Some(&rest[..length]); }
        rest = &rest[length..];
    }
    None
}
fn association_elements(ap: &SelectedAccessPoint) -> Result<InformationElements, HandshakeError> {
    let selector = |cipher| match cipher { Cipher::Wep40 => 1, Cipher::Tkip => 2, Cipher::Ccmp => 4, Cipher::Wep104 => 5, Cipher::None => 0 };
    let mut bytes = [0u8; 24];
    let (offset, oui, length) = match ap.protocol {
        PersonalProtocol::Rsn => { bytes[..4].copy_from_slice(&[48,20,1,0]); (4, [0,15,172], 22) },
        PersonalProtocol::Wpa => { bytes[..8].copy_from_slice(&[221,22,0,80,242,1,1,0]); (8, [0,80,242], 24) },
        PersonalProtocol::None => return Err(HandshakeError::Cipher),
    };
    bytes[offset..offset+3].copy_from_slice(&oui); bytes[offset+3] = selector(ap.group);
    bytes[offset+4] = 1;
    bytes[offset+6..offset+9].copy_from_slice(&oui); bytes[offset+9] = selector(ap.pairwise);
    bytes[offset+10] = 1;
    bytes[offset+12..offset+15].copy_from_slice(&oui); bytes[offset+15] = 2;
    InformationElements::parse(&bytes[..length]).map_err(|_| HandshakeError::SecurityElements)
}

#[cfg(test)]
#[path = "wireless_station_handshake_tests.rs"]
mod tests;
