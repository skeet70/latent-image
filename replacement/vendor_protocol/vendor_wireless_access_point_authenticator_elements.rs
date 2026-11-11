use super::{Cipher, Error, Protocol};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InformationElement { bytes: [u8; 26], length: usize }
impl InformationElement { pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length] } }

pub fn advertisement(protocol: Protocol, cipher: Cipher) -> InformationElement {
    let mut ie = InformationElement { bytes: [0; 26], length: 0 };
    let selector = match cipher { Cipher::Tkip => 2, Cipher::Ccmp => 4 };
    match protocol {
        Protocol::Wpa2 => {
            ie.bytes[..22].copy_from_slice(&[48,20,1,0,0,15,172,selector,1,0,0,15,172,selector,1,0,0,15,172,2,0,0]); ie.length = 22;
        }
        Protocol::Wpa => {
            ie.bytes.copy_from_slice(&[221,24,0,80,242,1,1,0,0,80,242,selector,1,0,0,80,242,selector,1,0,0,80,242,2,0,0]); ie.length = 26;
        }
    }
    ie
}

pub fn validate_association(protocol: Protocol, cipher: Cipher, ies: &[u8]) -> Result<&[u8], Error> {
    let mut rest = ies;
    let mut selected = None;
    while !rest.is_empty() {
        if rest.len() < 2 || rest.len() < rest[1] as usize + 2 { return Err(Error::InvalidElement); }
        let (ie, next) = rest.split_at(rest[1] as usize + 2); rest = next;
        let matches = match protocol {
            Protocol::Wpa2 => ie[0] == 48,
            Protocol::Wpa => ie[0] == 221 && ie.len() >= 6 && ie[2..6] == [0,80,242,1],
        };
        if !matches { continue; }
        if selected.is_some() { return Err(Error::InvalidElement); }
        let (mut body, oui): (&[u8], [u8; 3]) = match protocol {
            Protocol::Wpa2 => (&ie[2..], [0,15,172]),
            Protocol::Wpa => (&ie[6..], [0,80,242]),
        };
        if take_u16(&mut body)? != 1 { return Err(Error::UnsupportedSecurity); }
        let selector = match cipher { Cipher::Tkip => 2, Cipher::Ccmp => 4 };
        if take_selector(&mut body)? != [oui[0],oui[1],oui[2],selector] { return Err(Error::UnsupportedSecurity); }
        let count = take_u16(&mut body)? as usize;
        if count == 0 || body.len() < count * 4 { return Err(Error::InvalidElement); }
        let mut pairwise = false;
        for _ in 0..count { pairwise |= take_selector(&mut body)? == [oui[0],oui[1],oui[2],selector]; }
        if !pairwise { return Err(Error::UnsupportedSecurity); }
        let count = take_u16(&mut body)? as usize;
        if count == 0 || body.len() < count * 4 { return Err(Error::InvalidElement); }
        let mut personal = false;
        for _ in 0..count { personal |= take_selector(&mut body)? == [oui[0],oui[1],oui[2],2]; }
        if !personal { return Err(Error::UnsupportedSecurity); }
        if !body.is_empty() {
            let capabilities = take_u16(&mut body)?;
            if capabilities & 0x40 != 0 { return Err(Error::UnsupportedSecurity); }
        }
        if protocol == Protocol::Wpa2 && !body.is_empty() {
            let count = take_u16(&mut body)? as usize;
            if body.len() < count * 16 { return Err(Error::InvalidElement); }
            body = &body[count*16..];
            if body.len() == 4 { take_selector(&mut body)?; }
        }
        if !body.is_empty() { return Err(Error::InvalidElement); }
        selected = Some(ie);
    }
    selected.ok_or(Error::UnsupportedSecurity)
}
fn take_u16(input: &mut &[u8]) -> Result<u16, Error> {
    if input.len() < 2 { return Err(Error::InvalidElement); }
    let result = u16::from_le_bytes([input[0],input[1]]); *input = &input[2..]; Ok(result)
}
fn take_selector(input: &mut &[u8]) -> Result<[u8; 4], Error> {
    if input.len() < 4 { return Err(Error::InvalidElement); }
    let result = [input[0],input[1],input[2],input[3]]; *input = &input[4..]; Ok(result)
}
