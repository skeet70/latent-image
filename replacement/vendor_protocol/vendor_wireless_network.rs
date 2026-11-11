//! WLAN scan conversion and owned LAN lifecycle, packet and socket consumers.
//! X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

#[path = "vendor_wireless_network_state.rs"]
pub mod state;
#[path = "vendor_wireless_network_dhcp.rs"]
pub mod dhcp;
#[path = "vendor_wireless_network_packet.rs"]
pub mod packet;
#[path = "vendor_wireless_network_client.rs"]
pub mod client;
#[path = "vendor_wireless_network_link.rs"]
pub mod link;
#[path = "vendor_wireless_network_transport.rs"]
pub mod transport;
#[path = "vendor_wireless_network_lan.rs"]
pub mod lan;
#[path = "vendor_wireless_network_multicast.rs"]
pub mod multicast;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacAddress(pub [u8; 6]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalDbm(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrequencyMhz(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BssFlags(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanError { SsidTooLong, TruncatedElements, OutputCapacity }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ssid { bytes: [u8; 32], length: u8 }
impl Ssid {
    pub fn parse(bytes: &[u8]) -> Result<Self, ScanError> {
        if bytes.len() > 32 { return Err(ScanError::SsidTooLong); }
        let mut result = Self { bytes: [0; 32], length: bytes.len() as u8 };
        result.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(result)
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length as usize] }
}

pub struct Bss<'a> {
    flags: BssFlags,
    address: MacAddress,
    ssid: Ssid,
    frequency: FrequencyMhz,
    capabilities: Capabilities,
    signal: SignalDbm,
    elements: &'a [u8],
}
impl<'a> Bss<'a> {
    pub fn new(flags: BssFlags, address: MacAddress, ssid: Ssid,
        frequency: FrequencyMhz, capabilities: Capabilities, signal: SignalDbm,
        elements: &'a [u8]) -> Result<Self, ScanError> {
        let mut rest = elements;
        while !rest.is_empty() {
            if rest.len() < 2 || rest.len() < 2 + rest[1] as usize {
                return Err(ScanError::TruncatedElements);
            }
            rest = &rest[2 + rest[1] as usize..];
        }
        Ok(Self { flags, address, ssid, frequency, capabilities, signal, elements })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyManagement { Unspecified, Enterprise, Personal }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairwiseCipher { Unspecified, Tkip, Ccmp }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkMode { Unspecified, Infrastructure, Independent }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Security { pub key_management: KeyManagement, pub cipher: PairwiseCipher }
impl Security {
    const EMPTY: Self = Self { key_management: KeyManagement::Unspecified, cipher: PairwiseCipher::Unspecified };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupCipher { Unspecified, Wep40, Wep104, Tkip, Ccmp }
struct ParsedSecurity { security: Security, group: GroupCipher }
impl ParsedSecurity {
    const EMPTY: Self = Self { security: Security::EMPTY, group: GroupCipher::Unspecified };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityElements {
    pub wpa: Security,
    pub rsn: Security,
    pub wpa_group: GroupCipher,
    pub rsn_group: GroupCipher,
    pub wps: bool,
    wpa_present: bool,
    rsn_present: bool,
}
impl SecurityElements {
    const EMPTY: Self = Self { wpa: Security::EMPTY, rsn: Security::EMPTY, wpa_group: GroupCipher::Unspecified, rsn_group: GroupCipher::Unspecified, wps: false, wpa_present: false, rsn_present: false };
    pub fn legacy_privacy(&self, capabilities: Capabilities) -> bool {
        !self.wpa_present && !self.rsn_present && capabilities.0 & 16 != 0
    }
    fn observe(&mut self, element: &[u8]) {
        let kind = element[0];
        let body = &element[2..];
        if kind == 48 && !self.rsn_present {
            self.rsn_present = true;
            let parsed = parse_security(body, SecurityProtocol::Rsn).unwrap_or(ParsedSecurity::EMPTY);
            self.rsn = parsed.security; self.rsn_group = parsed.group;
        }
        if kind == 221 && body.len() >= 4 {
            if body[..4] == [0, 80, 242, 1] && !self.wpa_present {
                self.wpa_present = true;
                let parsed = parse_security(&body[4..], SecurityProtocol::Wpa).unwrap_or(ParsedSecurity::EMPTY);
                self.wpa = parsed.security; self.wpa_group = parsed.group;
            }
            if body[..4] == [0, 80, 242, 4] { self.wps = true; }
        }
    }
}
pub fn parse_security_elements(mut elements: &[u8]) -> Result<SecurityElements, ScanError> {
    let mut security = SecurityElements::EMPTY;
    while !elements.is_empty() {
        if elements.len() < 2 || elements.len() < 2 + usize::from(elements[1]) { return Err(ScanError::TruncatedElements); }
        let end = 2 + usize::from(elements[1]);
        security.observe(&elements[..end]);
        elements = &elements[end..];
    }
    Ok(security)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanRow {
    pub flags: BssFlags,
    pub address: MacAddress,
    pub ssid: Ssid,
    pub frequency: FrequencyMhz,
    pub mode: NetworkMode,
    pub signal: SignalDbm,
    pub privacy: bool,
    pub wpa: Security,
    pub rsn: Security,
    pub wps: bool,
}
impl ScanRow {
    pub const fn empty() -> Self {
        Self { flags: BssFlags(0), address: MacAddress([0; 6]), ssid: Ssid { bytes: [0; 32], length: 0 },
            frequency: FrequencyMhz(0), mode: NetworkMode::Unspecified, signal: SignalDbm(0),
            privacy: false, wpa: Security::EMPTY, rsn: Security::EMPTY, wps: false }
    }
    pub fn from_bss(bss: &Bss<'_>) -> Self {
        let mut row = Self { flags: bss.flags, address: bss.address, ssid: bss.ssid,
            frequency: bss.frequency, mode: NetworkMode::Unspecified, signal: bss.signal,
            privacy: false, wpa: Security::EMPTY, rsn: Security::EMPTY, wps: false };
        let mut security = SecurityElements::EMPTY;
        let mut rest = bss.elements;
        while !rest.is_empty() {
            let element = &rest[..2 + rest[1] as usize];
            security.observe(element);
            rest = &rest[element.len()..];
        }
        row.wpa = security.wpa; row.rsn = security.rsn; row.wps = security.wps;
        row.privacy = security.legacy_privacy(bss.capabilities);
        if bss.capabilities.0 & 2 != 0 { row.mode = NetworkMode::Independent; }
        if bss.capabilities.0 & 1 != 0 { row.mode = NetworkMode::Infrastructure; }
        row
    }

    // The 64-byte output layout is established by 0x013d7eec's stores.
    pub fn encode(&self) -> [u8; 64] {
        let mut bytes = [0; 64];
        bytes[..4].copy_from_slice(&self.flags.0.to_le_bytes());
        bytes[4..10].copy_from_slice(&self.address.0);
        bytes[10..42].copy_from_slice(&self.ssid.bytes);
        bytes[42] = self.ssid.length;
        bytes[44..48].copy_from_slice(&self.frequency.0.to_le_bytes());
        let mode: u16 = match self.mode { NetworkMode::Unspecified => 0, NetworkMode::Infrastructure => 1, NetworkMode::Independent => 2 };
        bytes[48..50].copy_from_slice(&mode.to_le_bytes());
        bytes[52..56].copy_from_slice(&self.signal.0.to_le_bytes());
        bytes[56] = u8::from(self.privacy);
        bytes[57] = key_code(self.wpa.key_management);
        bytes[58] = key_code(self.rsn.key_management);
        bytes[59] = u8::from(self.wps);
        bytes[60] = cipher_code(self.wpa.cipher);
        bytes[61] = cipher_code(self.rsn.cipher);
        bytes
    }
}

pub fn write_scan_rows(input: &[Bss<'_>], output: &mut [ScanRow]) -> Result<usize, ScanError> {
    let count = input.len().min(64);
    if output.len() < count { return Err(ScanError::OutputCapacity); }
    for (bss, row) in input[..count].iter().zip(&mut output[..count]) { *row = ScanRow::from_bss(bss); }
    Ok(count)
}
fn key_code(key: KeyManagement) -> u8 {
    match key { KeyManagement::Unspecified => 0, KeyManagement::Enterprise => 1, KeyManagement::Personal => 2 }
}
fn cipher_code(cipher: PairwiseCipher) -> u8 {
    match cipher { PairwiseCipher::Unspecified => 0, PairwiseCipher::Tkip => 1, PairwiseCipher::Ccmp => 2 }
}
#[derive(Clone, Copy)]
enum SecurityProtocol { Wpa, Rsn }
impl SecurityProtocol {
    fn oui(self) -> [u8; 3] { match self { Self::Wpa => [0, 80, 242], Self::Rsn => [0, 15, 172] } }
}
fn take<'a>(rest: &mut &'a [u8], length: usize) -> Option<&'a [u8]> {
    let value = rest.get(..length)?;
    *rest = &rest[length..];
    Some(value)
}
fn count(rest: &mut &[u8]) -> Option<usize> {
    let bytes = take(rest, 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]) as usize)
}
fn parse_security(mut rest: &[u8], protocol: SecurityProtocol) -> Option<ParsedSecurity> {
    if count(&mut rest)? != 1 { return None; }
    let mut security = Security { key_management: KeyManagement::Enterprise,
        cipher: match protocol { SecurityProtocol::Wpa => PairwiseCipher::Tkip, SecurityProtocol::Rsn => PairwiseCipher::Ccmp } };
    let mut group = match protocol { SecurityProtocol::Wpa => GroupCipher::Tkip, SecurityProtocol::Rsn => GroupCipher::Ccmp };
    if rest.is_empty() { return Some(ParsedSecurity { security, group }); }
    let suite = take(&mut rest, 4)?;
    group = if suite[..3] == protocol.oui() {
        match suite[3] { 1 => GroupCipher::Wep40, 2 => GroupCipher::Tkip, 4 => GroupCipher::Ccmp, 5 => GroupCipher::Wep104, _ => GroupCipher::Unspecified }
    } else { GroupCipher::Unspecified };
    if rest.is_empty() { return Some(ParsedSecurity { security, group }); }
    let pairs = count(&mut rest)?;
    if pairs == 0 { return None; }
    security.cipher = PairwiseCipher::Unspecified;
    let mut tkip = false;
    let mut ccmp = false;
    for _ in 0..pairs {
        let suite = take(&mut rest, 4)?;
        if suite[..3] == protocol.oui() {
            tkip |= suite[3] == 2;
            ccmp |= suite[3] == 4;
        }
    }
    if tkip { security.cipher = PairwiseCipher::Tkip; }
    if ccmp { security.cipher = PairwiseCipher::Ccmp; }
    if rest.is_empty() { return Some(ParsedSecurity { security, group }); }
    let keys = count(&mut rest)?;
    if keys == 0 { return None; }
    security.key_management = KeyManagement::Unspecified;
    let mut enterprise = false;
    let mut personal = false;
    for _ in 0..keys {
        let suite = take(&mut rest, 4)?;
        if suite[..3] == protocol.oui() {
            enterprise |= suite[3] == 1;
            personal |= suite[3] == 2;
        }
    }
    if enterprise { security.key_management = KeyManagement::Enterprise; }
    if personal { security.key_management = KeyManagement::Personal; }
    if rest.len() >= 2 { take(&mut rest, 2)?; }
    if matches!(protocol, SecurityProtocol::Rsn) && rest.len() >= 2 {
        let pmkids = count(&mut rest)?;
        take(&mut rest, pmkids * 16)?;
    }
    Some(ParsedSecurity { security, group })
}

#[cfg(test)]
mod tests {
    use super::{Bss, BssFlags, Capabilities, FrequencyMhz, KeyManagement, MacAddress, NetworkMode, PairwiseCipher, ScanError, ScanRow, SignalDbm, Ssid};
    fn row(elements: &[u8], caps: u16) -> ScanRow {
        ScanRow::from_bss(&Bss::new(BssFlags(9), MacAddress([1, 2, 3, 4, 5, 6]),
            Ssid::parse(b"camera").unwrap(), FrequencyMhz(2412), Capabilities(caps), SignalDbm(-42), elements).unwrap())
    }
    #[test]
    fn exposes_scan_identity_and_wire_layout() {
        let row = row(&[], 3);
        assert_eq!(row.mode, NetworkMode::Infrastructure);
        let bytes = row.encode();
        assert_eq!(&bytes[4..16], &[1, 2, 3, 4, 5, 6, b'c', b'a', b'm', b'e', b'r', b'a']);
        assert_eq!(bytes[42], 6);
        assert_eq!(&bytes[44..48], &[108, 9, 0, 0]);
        assert_eq!(&bytes[52..56], &[214, 255, 255, 255]);
    }
    #[test]
    fn reports_rsn_personal_ccmp_and_wps() {
        let row = row(&[48, 18, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 221, 4, 0, 80, 242, 4], 16);
        assert_eq!(row.rsn.key_management, KeyManagement::Personal);
        assert_eq!(row.rsn.cipher, PairwiseCipher::Ccmp);
        assert!(row.wps);
        assert!(!row.privacy);
    }
    #[test]
    fn malformed_security_does_not_become_legacy_privacy() {
        let row = row(&[48, 2, 2, 0], 16);
        assert_eq!(row.rsn.cipher, PairwiseCipher::Unspecified);
        assert!(!row.privacy);
        assert!(super::tests::row(&[], 16).privacy);
    }
    #[test]
    fn rejects_invalid_boundary_lengths() {
        assert_eq!(Ssid::parse(&[0; 33]), Err(ScanError::SsidTooLong));
        assert!(matches!(Bss::new(BssFlags(0), MacAddress([0; 6]), Ssid::parse(&[]).unwrap(),
            FrequencyMhz(0), Capabilities(0), SignalDbm(0), &[48, 5, 1]), Err(ScanError::TruncatedElements)));
    }
    #[test]
    fn preserves_defaults_for_version_only_security_elements() {
        let result = row(&[48, 2, 1, 0, 221, 6, 0, 80, 242, 1, 1, 0], 0);
        assert_eq!(result.rsn.key_management, KeyManagement::Enterprise);
        assert_eq!(result.rsn.cipher, PairwiseCipher::Ccmp);
        assert_eq!(result.wpa.key_management, KeyManagement::Enterprise);
        assert_eq!(result.wpa.cipher, PairwiseCipher::Tkip);
    }
    #[test]
    fn personal_and_ccmp_win_when_multiple_suites_are_present() {
        let result = row(&[48, 26, 1, 0, 0, 15, 172, 4, 2, 0,
            0, 15, 172, 4, 0, 15, 172, 2, 2, 0,
            0, 15, 172, 2, 0, 15, 172, 1], 0);
        assert_eq!(result.rsn.key_management, KeyManagement::Personal);
        assert_eq!(result.rsn.cipher, PairwiseCipher::Ccmp);
    }
    #[test]
    fn rejects_security_with_truncated_pmkid_array() {
        let result = row(&[48, 22, 1, 0, 0, 15, 172, 4, 1, 0,
            0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0, 1, 0], 0);
        assert_eq!(result.rsn.key_management, KeyManagement::Unspecified);
        assert_eq!(result.rsn.cipher, PairwiseCipher::Unspecified);
    }
    #[test]
    fn collects_at_most_sixty_four_scan_rows() {
        let input: [Bss<'_>; 65] = core::array::from_fn(|index| Bss::new(
            BssFlags(0), MacAddress([index as u8; 6]), Ssid::parse(&[]).unwrap(),
            FrequencyMhz(2412), Capabilities(1), SignalDbm(-42), &[]).unwrap());
        let mut output = [ScanRow::empty(); 64];
        assert_eq!(super::write_scan_rows(&input, &mut output), Ok(64));
        assert_eq!(output[0].address, MacAddress([0; 6]));
        assert_eq!(output[63].address, MacAddress([63; 6]));
    }
    #[test]
    fn scan_capacity_error_and_empty_input_leave_rows_unchanged() {
        let input = [Bss::new(BssFlags(1), MacAddress([1; 6]), Ssid::parse(&[]).unwrap(),
            FrequencyMhz(2412), Capabilities(1), SignalDbm(-42), &[]).unwrap()];
        assert_eq!(super::write_scan_rows(&input, &mut []), Err(ScanError::OutputCapacity));
        let expected = row(&[], 16);
        let mut output = [expected];
        assert_eq!(super::write_scan_rows(&[], &mut output), Ok(0));
        assert_eq!(output, [expected]);
    }
    #[test]
    fn parses_station_security_without_scan_metadata() {
        let security = super::parse_security_elements(&[48, 18, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 221, 4, 0, 80, 242, 4]).unwrap();
        assert_eq!(security.rsn.key_management, KeyManagement::Personal);
        assert_eq!(security.rsn.cipher, PairwiseCipher::Ccmp);
        assert!(security.wps);
        assert!(!security.legacy_privacy(Capabilities(16)));
        assert_eq!(super::parse_security_elements(&[48, 2, 1]), Err(ScanError::TruncatedElements));
        let malformed = super::parse_security_elements(&[48, 2, 2, 0]).unwrap();
        assert_eq!(malformed.rsn.key_management, KeyManagement::Unspecified);
        assert!(!malformed.legacy_privacy(Capabilities(16)));
    }
    #[test]
    fn preserves_group_cipher_independently_from_pairwise_cipher() {
        let rsn = super::parse_security_elements(&[48, 18, 1, 0, 0, 15, 172, 1, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2]).unwrap();
        assert_eq!(rsn.rsn_group, super::GroupCipher::Wep40);
        assert_eq!(rsn.rsn.cipher, PairwiseCipher::Ccmp);
        let wpa = super::parse_security_elements(&[221, 22, 0, 80, 242, 1, 1, 0, 0, 80, 242, 5, 1, 0, 0, 80, 242, 2, 1, 0, 0, 80, 242, 2]).unwrap();
        assert_eq!(wpa.wpa_group, super::GroupCipher::Wep104);
        assert_eq!(wpa.wpa.cipher, PairwiseCipher::Tkip);
        let defaults = super::parse_security_elements(&[48, 2, 1, 0, 221, 6, 0, 80, 242, 1, 1, 0]).unwrap();
        assert_eq!(defaults.rsn_group, super::GroupCipher::Ccmp);
        assert_eq!(defaults.wpa_group, super::GroupCipher::Tkip);
    }
}
