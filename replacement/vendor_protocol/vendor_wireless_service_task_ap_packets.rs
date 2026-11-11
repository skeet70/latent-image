use crate::vendor_wireless_network::state::{Ipv4Address, Ipv4Mask};
#[cfg(test)]
#[path = "vendor_wireless_service_task_ap_packets_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError { Name, Model, Capacity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceName { bytes: [u8; 24], length: u8 }
impl DeviceName {
    pub fn parse(input: &[u8]) -> Result<Self, PacketError> {
        if input.is_empty() || input.len() > 24 || !input[0].is_ascii_alphanumeric() { return Err(PacketError::Name); }
        let end = input.iter().rposition(u8::is_ascii_alphanumeric).ok_or(PacketError::Name)? + 1;
        let input = &input[..end];
        if input.iter().any(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-' && *byte != b'.') { return Err(PacketError::Name); }
        let mut bytes = [0; 24]; bytes[..input.len()].copy_from_slice(input);
        Ok(Self { bytes, length: input.len() as u8 })
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length as usize] }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceModel { bytes: [u8; 24], length: u8 }
impl DeviceModel {
    pub fn parse(input: &[u8]) -> Result<Self, PacketError> {
        if input.len() > 24 || input.contains(&0) { return Err(PacketError::Model); }
        let mut bytes = [0; 24]; bytes[..input.len()].copy_from_slice(input); Ok(Self { bytes, length: input.len() as u8 })
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length as usize] }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryPort { Import, Register }
impl DiscoveryPort { pub const fn number(self) -> u16 { match self { Self::Import => 51541, Self::Register => 51542 } } }
pub struct DiscoveryPacket { bytes: [u8; 160], length: usize }
impl DiscoveryPacket {
    fn new() -> Self { Self { bytes: [0; 160], length: 0 } }
    fn append(&mut self, bytes: &[u8]) -> Result<(), PacketError> {
        let end = self.length.checked_add(bytes.len()).ok_or(PacketError::Capacity)?;
        self.bytes.get_mut(self.length..end).ok_or(PacketError::Capacity)?.copy_from_slice(bytes); self.length = end; Ok(())
    }
    fn decimal(&mut self, mut value: u32) -> Result<(), PacketError> {
        let mut digits = [0; 10]; let mut start = digits.len();
        loop { start -= 1; digits[start] = b'0' + (value % 10) as u8; value /= 10; if value == 0 { break; } }
        self.append(&digits[start..])
    }
    fn address(&mut self, address: Ipv4Address) -> Result<(), PacketError> {
        for (index, byte) in address.0.iter().enumerate() { if index != 0 { self.append(b".")?; } self.decimal(*byte as u32)?; } Ok(())
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length] }
    pub fn import(address: Ipv4Address, name: DeviceName, port: DiscoveryPort) -> Result<Self, PacketError> {
        let mut packet = Self::new(); packet.append(b"IMPORT /guest HTTP/1.1\r\nHOST:")?; packet.address(address)?;
        packet.append(b":51541\r\nDSCNAME:")?; packet.append(name.as_bytes())?; packet.append(b"\r\nDSCPORT:")?;
        packet.decimal(port.number() as u32)?; packet.append(b"\r\n")?; Ok(packet)
    }
    pub fn register(address: Ipv4Address, name: DeviceName, model: DeviceModel) -> Result<Self, PacketError> {
        let mut packet = Self::new(); packet.append(b"REGISTER * HTTP/1.1\r\nHOST:")?; packet.address(address)?;
        packet.append(b":51542\r\nDSCNAME:")?; packet.append(name.as_bytes())?; packet.append(b"\r\nDSCMODEL:")?;
        let model = model.as_bytes(); let quoted = model.get(..model.len().saturating_sub(1)).is_some_and(|bytes| bytes.contains(&b' '));
        if quoted { packet.append(b"\"")?; } packet.append(model)?; if quoted { packet.append(b"\"")?; }
        packet.append(b"\r\n")?; Ok(packet)
    }
    pub fn discover(address: Ipv4Address, mask: Ipv4Mask, name: Option<DeviceName>, port: DiscoveryPort) -> Result<Self, PacketError> {
        if name.is_some() != matches!(port, DiscoveryPort::Import) { return Err(PacketError::Name); }
        let mut packet = Self::new(); packet.append(b"DISCOVER ")?;
        match name { Some(name) => packet.append(name.as_bytes())?, None => packet.append(b"*")? }
        packet.append(b" HTTP/1.1\r\nMX:3\r\nHOST:")?;
        let mut broadcast = address.0; for (byte, mask) in broadcast.iter_mut().zip(mask.0) { *byte |= !mask; }
        packet.address(Ipv4Address(broadcast))?; packet.append(b":")?; packet.decimal(port.number() as u32)?;
        packet.append(b"\r\nDSCADDR:")?; packet.address(address)?; packet.append(b"\r\nSERVICE:PCAUTOSAVE/1.0\r\n")?; Ok(packet)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceId(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceName([u8; 32]);
impl ServiceName {
    pub fn parse(bytes: &[u8]) -> Result<Self, PacketError> {
        if bytes.len() > 32 { return Err(PacketError::Name); }
        let mut result = [0; 32]; result[..bytes.len()].copy_from_slice(bytes); Ok(Self(result))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceEntry { pub id: ServiceId, pub name: ServiceName, pub address: crate::vendor_wireless_network::MacAddress }
pub struct ServiceRegistry { entries: [Option<ServiceEntry>; 6] }
impl ServiceRegistry {
    pub const fn new() -> Self { Self { entries: [None; 6] } }
    pub fn replace(&mut self, entries: &[ServiceEntry]) -> Result<(), PacketError> {
        if entries.len() > 6 { return Err(PacketError::Capacity); }
        self.entries.fill(None); for (out, entry) in self.entries.iter_mut().zip(entries) { *out = Some(*entry); } Ok(())
    }
    pub fn find(&self, name: ServiceName) -> Option<ServiceId> { self.entries.iter().flatten().find(|entry| entry.name == name).map(|entry| entry.id) }
    pub fn entries(&self) -> impl Iterator<Item = &ServiceEntry> { self.entries.iter().flatten() }
}
impl Default for ServiceRegistry { fn default() -> Self { Self::new() } }
