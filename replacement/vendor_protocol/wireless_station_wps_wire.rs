use super::WpsError;

pub const CAPACITY: usize = 2048;
#[derive(Clone, Eq, PartialEq)]
pub struct WpsFrame { bytes: [u8; CAPACITY], length: usize }
impl core::fmt::Debug for WpsFrame { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.debug_struct("WpsFrame").field("length", &self.length).finish() } }
impl Default for WpsFrame { fn default() -> Self { Self::new() } }
impl WpsFrame {
    pub const fn new() -> Self { Self { bytes: [0; CAPACITY], length: 0 } }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length] }
    pub fn append(&mut self, input: &[u8]) -> Result<(), WpsError> {
        let end = self.length.checked_add(input.len()).filter(|end| *end <= CAPACITY).ok_or(WpsError::Capacity)?;
        self.bytes[self.length..end].copy_from_slice(input); self.length = end; Ok(())
    }
    pub fn attribute(&mut self, kind: Attribute, value: &[u8]) -> Result<(), WpsError> {
        self.append(&(kind as u16).to_be_bytes())?; self.append(&(value.len() as u16).to_be_bytes())?; self.append(value)
    }
    pub fn message(kind: Message) -> Result<Self, WpsError> {
        let mut result = Self::new(); result.attribute(Attribute::Version, &[0x10])?; result.attribute(Attribute::MessageType, &[kind as u8])?; Ok(result)
    }
    pub fn clear(&mut self) { self.bytes.fill(0); self.length = 0; }
}
impl Drop for WpsFrame { fn drop(&mut self) { self.clear(); } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum Attribute {
    Authenticator=0x1005, AuthType=0x1003, AuthFlags=0x1004, ConfigMethods=0x1008, ConfigError=0x1009,
    ConnectionFlags=0x100d, Credential=0x100e, Encryption=0x100f, EncryptionFlags=0x1010,
    DeviceName=0x1011, PasswordId=0x1012, EHash1=0x1014, EHash2=0x1015, ESnonce1=0x1016, ESnonce2=0x1017,
    EncryptedSettings=0x1018, EnrolleeNonce=0x101a, Mac=0x1020, Manufacturer=0x1021, MessageType=0x1022,
    ModelName=0x1023, ModelNumber=0x1024, NetworkIndex=0x1026, NetworkKey=0x1027, NetworkKeyIndex=0x1028,
    OsVersion=0x102d, PublicKey=0x1032, RegistrarNonce=0x1039, RHash1=0x103d, RHash2=0x103e,
    RSnonce1=0x103f, RSnonce2=0x1040, RfBands=0x103c, Serial=0x1042, WpsState=0x1044, Ssid=0x1045,
    UuidE=0x1047, UuidR=0x1048, Version=0x104a, PrimaryDeviceType=0x1054, AssociationState=0x1002,
    SelectedRegistrar=0x1041,SelectedRegistrarConfigMethods=0x1053,
    ApSetupLocked=0x1057,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Message { M1=4, M2=5, M2d=6, M3=7, M4=8, M5=9, M6=10, M7=11, M8=12, Ack=13, Nack=14, Done=15 }
impl Message {
    pub fn parse(value: &[u8]) -> Result<Self, WpsError> { match value { [4]=>Ok(Self::M1),[5]=>Ok(Self::M2),[6]=>Ok(Self::M2d),[7]=>Ok(Self::M3),[8]=>Ok(Self::M4),[9]=>Ok(Self::M5),[10]=>Ok(Self::M6),[11]=>Ok(Self::M7),[12]=>Ok(Self::M8),[13]=>Ok(Self::Ack),[14]=>Ok(Self::Nack),[15]=>Ok(Self::Done),_=>Err(WpsError::Message) } }
}
pub struct Attributes<'a> { input: &'a [u8] }
impl<'a> Attributes<'a> {
    pub fn parse(input: &'a [u8]) -> Result<Self, WpsError> {
        let mut remaining = input;
        while !remaining.is_empty() { if remaining.len()<4 { return Err(WpsError::Truncated); } let length=u16::from_be_bytes([remaining[2],remaining[3]]) as usize; if length>remaining.len()-4 { return Err(WpsError::Truncated); } remaining=&remaining[4+length..]; }
        Ok(Self { input })
    }
    pub fn all(&self) -> impl Iterator<Item=(u16,&'a [u8])> { let mut remaining=self.input; core::iter::from_fn(move || { if remaining.is_empty() { return None; } let kind=u16::from_be_bytes([remaining[0],remaining[1]]); let length=u16::from_be_bytes([remaining[2],remaining[3]]) as usize; let value=&remaining[4..4+length]; remaining=&remaining[4+length..]; Some((kind,value)) }) }
    pub fn optional(&self, kind: Attribute) -> Result<Option<&'a [u8]>,WpsError> { let mut matches=self.all().filter(|(tag,_)| *tag==kind as u16); let value=matches.next().map(|(_,value)|value); if matches.next().is_some() { return Err(WpsError::Duplicate); } Ok(value) }
    pub fn get(&self, kind: Attribute) -> Result<&'a [u8],WpsError> { self.optional(kind)?.ok_or(WpsError::MissingAttribute) }
    pub fn fixed<const N:usize>(&self, kind: Attribute) -> Result<[u8;N],WpsError> { self.get(kind)?.try_into().map_err(|_|WpsError::AttributeLength) }
    pub fn word(&self,kind:Attribute)->Result<u16,WpsError> { Ok(u16::from_be_bytes(self.fixed(kind)?)) }
    pub fn authenticated(&self)->Result<(&'a [u8],[u8;8]),WpsError> {
        let authenticator=self.fixed(Attribute::Authenticator)?;
        if self.input.len()<12 || self.input[self.input.len()-12..self.input.len()-10]!=(Attribute::Authenticator as u16).to_be_bytes() { return Err(WpsError::Authentication); }
        Ok((&self.input[..self.input.len()-12],authenticator))
    }
}

pub fn eap_response(identifier:u8,opcode:u8,flags:u8,payload:&[u8],total:Option<usize>)->Result<WpsFrame,WpsError> {eap_packet(2,identifier,opcode,flags,payload,total)}
pub fn eap_packet(code:u8,identifier:u8,opcode:u8,flags:u8,payload:&[u8],total:Option<usize>)->Result<WpsFrame,WpsError> {
    let length=14+payload.len()+if total.is_some(){2}else{0}; let mut frame=WpsFrame::new();
    frame.append(&[1,0])?; frame.append(&(length as u16).to_be_bytes())?; frame.append(&[code,identifier])?; frame.append(&(length as u16).to_be_bytes())?;
    frame.append(&[254,0,0x37,0x2a,0,0,0,1,opcode,flags])?;
    if let Some(total)=total { frame.append(&(total as u16).to_be_bytes())?; } frame.append(payload)?; Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn truncated_and_duplicate_fields_are_rejected() {
        assert!(matches!(Attributes::parse(&[0x10,0x1a,0,16,1]),Err(WpsError::Truncated)));
        let mut message=WpsFrame::message(Message::M2).unwrap(); message.attribute(Attribute::EnrolleeNonce,&[7;16]).unwrap(); message.attribute(Attribute::EnrolleeNonce,&[8;16]).unwrap();
        assert_eq!(Attributes::parse(message.as_bytes()).unwrap().get(Attribute::EnrolleeNonce),Err(WpsError::Duplicate));
    }
    #[test] fn eap_response_lengths_include_vendor_and_fragment_headers() {
        let packet=eap_response(9,4,3,&[1,2,3],Some(1000)).unwrap(); let bytes=packet.as_bytes();
        assert_eq!(u16::from_be_bytes([bytes[2],bytes[3]]) as usize,bytes.len()-4); assert_eq!(&bytes[4..8],&[2,9,0,19]); assert_eq!(&bytes[18..],&[3,0xe8,1,2,3]);
    }
}
