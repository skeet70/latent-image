use super::crypto::{derive_master_key, MasterKey};
use super::{MacAddress, Ssid};
use core::fmt;
use xm1_wireless_crypto::zeroize::Zeroize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileError { Security, KeyLength, KeyIndex, Hex, EmptySsid, Elements }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyIndex(u8);
impl KeyIndex {
    pub fn parse(value: u8) -> Result<Self, ProfileError> { if value < 4 { Ok(Self(value)) } else { Err(ProfileError::KeyIndex) } }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Eq, PartialEq)]
pub struct WepKey { bytes: [u8; 13], length: u8 }
impl WepKey {
    pub fn parse(input: &[u8]) -> Result<Self, ProfileError> {
        let mut bytes = [0; 13];
        let length = match input.len() {
            5 | 13 => { bytes[..input.len()].copy_from_slice(input); input.len() },
            10 | 26 => { decode_hex(input, &mut bytes[..input.len() / 2])?; input.len() / 2 },
            _ => return Err(ProfileError::KeyLength),
        };
        Ok(Self { bytes, length: length as u8 })
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length as usize] }
}
impl fmt::Debug for WepKey { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("WepKey([redacted])") } }
impl Drop for WepKey { fn drop(&mut self) { self.bytes.zeroize(); } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersonalProtocol { None, Wpa, Rsn }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cipher { None, Wep40, Wep104, Tkip, Ccmp }
impl Cipher {
    pub const fn selector(self) -> Option<u32> { match self { Self::None => None, Self::Wep40 => Some(0x000fac01), Self::Wep104 => Some(0x000fac05), Self::Tkip => Some(0x000fac02), Self::Ccmp => Some(0x000fac04) } }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Credentials { Open, Wep { index: KeyIndex, key: WepKey }, Personal(MasterKey) }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StationProfile { pub ssid: Ssid, pub credentials: Credentials }
impl StationProfile {
    pub fn parse(ssid: Ssid, security: u8, key: &[u8], index: u8) -> Result<Self, ProfileError> {
        if ssid.as_bytes().is_empty() { return Err(ProfileError::EmptySsid); }
        let credentials = match security {
            0 if key.is_empty() => Credentials::Open,
            0 | 1 => Credentials::Wep { index: KeyIndex::parse(index)?, key: WepKey::parse(key)? },
            2 | 3 => {
                let master = if key.len() == 64 { let mut bytes = [0; 32]; decode_hex(key, &mut bytes)?; MasterKey::from_bytes(bytes) }
                else if (8..64).contains(&key.len()) && !key.contains(&0) { derive_master_key(key, ssid.as_bytes()) }
                else { return Err(ProfileError::KeyLength); };
                Credentials::Personal(master)
            }
            _ => return Err(ProfileError::Security),
        };
        Ok(Self { ssid, credentials })
    }
}
fn decode_hex(input: &[u8], output: &mut [u8]) -> Result<(), ProfileError> {
    fn digit(value: u8) -> Result<u8, ProfileError> { match value { b'0'..=b'9' => Ok(value - b'0'), b'a'..=b'f' => Ok(value - b'a' + 10), b'A'..=b'F' => Ok(value - b'A' + 10), _ => Err(ProfileError::Hex) } }
    for (pair, out) in input.chunks_exact(2).zip(output) { *out = digit(pair[0])? << 4 | digit(pair[1])?; }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InformationElements { bytes: [u8; 600], length: u16 }
impl InformationElements {
    pub fn parse(input: &[u8]) -> Result<Self, ProfileError> {
        if input.len() > 600 { return Err(ProfileError::Elements); }
        let mut rest = input;
        while !rest.is_empty() {
            if rest.len() < 2 || rest.len() < 2 + rest[1] as usize { return Err(ProfileError::Elements); }
            rest = &rest[2 + rest[1] as usize..];
        }
        let mut bytes = [0; 600]; bytes[..input.len()].copy_from_slice(input);
        Ok(Self { bytes, length: input.len() as u16 })
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length as usize] }
    pub fn find(&self, identifier: u8) -> Option<&[u8]> {
        let mut rest = self.as_bytes();
        while !rest.is_empty() { let size = 2 + rest[1] as usize; if rest[0] == identifier { return Some(&rest[..size]); } rest = &rest[size..]; }
        None
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedAccessPoint {
    pub address: MacAddress,
    pub protocol: PersonalProtocol,
    pub pairwise: Cipher,
    pub group: Cipher,
    pub advertised_elements: InformationElements,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WpsPin { digits: [u8; 8], length: u8 }
impl WpsPin {
    pub fn parse(input: &[u8]) -> Result<Self, ProfileError> {
        if input.len() != 4 && input.len() != 8 || input.iter().any(|byte| !byte.is_ascii_digit()) { return Err(ProfileError::KeyLength); }
        if input.len() == 8 {
            let sum: u16 = input.iter().enumerate().map(|(index, byte)| (byte - b'0') as u16 * if index % 2 == 0 { 3 } else { 1 }).sum();
            if sum % 10 != 0 { return Err(ProfileError::KeyLength); }
        }
        let mut digits = [0; 8]; digits[..input.len()].copy_from_slice(input);
        Ok(Self { digits, length: input.len() as u8 })
    }
    pub fn generate(random: [u8; 4]) -> Self {
        let value = u32::from_le_bytes(random) % 10_000_000;
        let mut digits = [b'0'; 8]; let mut rest = value;
        for digit in digits[..7].iter_mut().rev() { *digit += (rest % 10) as u8; rest /= 10; }
        let sum: u16 = digits[..7].iter().enumerate().map(|(index, byte)| (byte - b'0') as u16 * if index % 2 == 0 { 3 } else { 1 }).sum();
        digits[7] += ((10 - sum % 10) % 10) as u8;
        Self { digits, length: 8 }
    }
    pub fn as_bytes(&self) -> &[u8] { &self.digits[..self.length as usize] }
}
