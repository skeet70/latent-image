
use super::dhcp::UdpPort;
use super::state::Ipv4Address;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError { Truncated, Version, HeaderLength, Length, Checksum, Fragment, Protocol, Capacity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketIdentification(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HopLimit(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrafficClass(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpEndpoint { pub address: Ipv4Address, pub port: UdpPort }

pub struct UdpDatagram<'a> {
    pub source: UdpEndpoint,
    pub destination: UdpEndpoint,
    pub payload: &'a [u8],
}

pub struct InternetChecksum { sum: u32, pending: Option<u8> }
impl InternetChecksum {
    pub const fn new() -> Self { Self { sum: 0, pending: None } }
    pub fn update(&mut self, mut bytes: &[u8]) {
        if let Some(high) = self.pending.take() {
            if let Some((&low, rest)) = bytes.split_first() {
                self.add(u16::from_be_bytes([high, low]));
                bytes = rest;
            } else { self.pending = Some(high); return; }
        }
        let mut pairs = bytes.chunks_exact(2);
        for pair in &mut pairs { self.add(u16::from_be_bytes([pair[0], pair[1]])); }
        self.pending = pairs.remainder().first().copied();
    }
    fn add(&mut self, word: u16) {
        self.sum += u32::from(word);
        self.sum = (self.sum & 65535) + (self.sum >> 16);
    }
    pub fn finish(mut self) -> u16 {
        if let Some(high) = self.pending { self.add(u16::from(high) << 8); }
        while self.sum > 65535 { self.sum = (self.sum & 65535) + (self.sum >> 16); }
        !(self.sum as u16)
    }
}
impl Default for InternetChecksum { fn default() -> Self { Self::new() } }

fn udp_checksum(source: Ipv4Address, destination: Ipv4Address, packet: &[u8]) -> u16 {
    let mut checksum = InternetChecksum::new();
    checksum.update(&source.0);
    checksum.update(&destination.0);
    checksum.update(&[0, 17]);
    checksum.update(&(packet.len() as u16).to_be_bytes());
    checksum.update(packet);
    checksum.finish()
}
impl<'a> UdpDatagram<'a> {
    pub fn parse_ipv4(bytes: &'a [u8]) -> Result<Self, PacketError> {
        if bytes.len() < 20 { return Err(PacketError::Truncated); }
        if bytes[0] >> 4 != 4 { return Err(PacketError::Version); }
        let header_length = usize::from(bytes[0] & 15) * 4;
        if header_length < 20 { return Err(PacketError::HeaderLength); }
        let total = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
        if total < header_length + 8 { return Err(PacketError::Length); }
        if total > bytes.len() { return Err(PacketError::Truncated); }
        let mut checksum = InternetChecksum::new();
        checksum.update(&bytes[..header_length]);
        if checksum.finish() != 0 { return Err(PacketError::Checksum); }
        if u16::from_be_bytes([bytes[6], bytes[7]]) & 0x3fff != 0 { return Err(PacketError::Fragment); }
        if bytes[9] != 17 { return Err(PacketError::Protocol); }
        let source_address = Ipv4Address([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let destination_address = Ipv4Address([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let udp = &bytes[header_length..total];
        let udp_length = usize::from(u16::from_be_bytes([udp[4], udp[5]]));
        if udp_length < 8 || udp_length > udp.len() { return Err(PacketError::Length); }
        let udp = &udp[..udp_length];
        if (udp[6] != 0 || udp[7] != 0) && udp_checksum(source_address, destination_address, udp) != 0 {
            return Err(PacketError::Checksum);
        }
        Ok(Self { source: UdpEndpoint { address: source_address, port: UdpPort(u16::from_be_bytes([udp[0], udp[1]])) },
            destination: UdpEndpoint { address: destination_address, port: UdpPort(u16::from_be_bytes([udp[2], udp[3]])) }, payload: &udp[8..] })
    }
    pub fn encode_ipv4(&self, identification: PacketIdentification, hops: HopLimit, output: &mut [u8]) -> Result<usize, PacketError> {
        self.encode_ipv4_with_traffic(identification, hops, TrafficClass(0), output)
    }
    pub fn encode_ipv4_with_traffic(&self, identification: PacketIdentification, hops: HopLimit, traffic: TrafficClass, output: &mut [u8]) -> Result<usize, PacketError> {
        let total = self.payload.len().checked_add(28).ok_or(PacketError::Capacity)?;
        if total > usize::from(u16::MAX) || total > output.len() { return Err(PacketError::Capacity); }
        output[..28].fill(0);
        output[0] = 0x45;
        output[1] = traffic.0;
        output[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        output[4..6].copy_from_slice(&identification.0.to_be_bytes());
        output[8] = hops.0;
        output[9] = 17;
        output[12..16].copy_from_slice(&self.source.address.0);
        output[16..20].copy_from_slice(&self.destination.address.0);
        let mut checksum = InternetChecksum::new();
        checksum.update(&output[..20]);
        output[10..12].copy_from_slice(&checksum.finish().to_be_bytes());
        output[20..22].copy_from_slice(&self.source.port.0.to_be_bytes());
        output[22..24].copy_from_slice(&self.destination.port.0.to_be_bytes());
        output[24..26].copy_from_slice(&((total - 20) as u16).to_be_bytes());
        output[28..total].copy_from_slice(self.payload);
        let checksum = udp_checksum(self.source.address, self.destination.address, &output[20..total]);
        output[26..28].copy_from_slice(&(if checksum == 0 { 65535 } else { checksum }).to_be_bytes());
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::{HopLimit, InternetChecksum, Ipv4Address, PacketError, PacketIdentification, UdpDatagram, UdpEndpoint, UdpPort};
    #[test]
    fn checksum_handles_odd_chunks_without_padding_between_them() {
        let mut checksum = InternetChecksum::new();
        checksum.update(&[0, 1, 242]);
        checksum.update(&[]);
        checksum.update(&[3, 244, 245, 246]);
        checksum.update(&[247]);
        assert_eq!(checksum.finish(), 0x220d);
    }
    #[test]
    fn decodes_transmitted_udp_payload_and_rejects_corruption() {
        let datagram = UdpDatagram {
            source: UdpEndpoint { address: Ipv4Address([192, 168, 0, 1]), port: UdpPort(67) },
            destination: UdpEndpoint { address: Ipv4Address([255; 4]), port: UdpPort(68) },
            payload: &[1, 2, 3],
        };
        let mut bytes = [0; 31];
        assert_eq!(datagram.encode_ipv4(PacketIdentification(4), HopLimit(64), &mut bytes), Ok(31));
        let decoded = UdpDatagram::parse_ipv4(&bytes).unwrap();
        assert_eq!(decoded.source, datagram.source);
        assert_eq!(decoded.destination, datagram.destination);
        assert_eq!(decoded.payload, &[1, 2, 3]);
        bytes[30] ^= 1;
        assert!(matches!(UdpDatagram::parse_ipv4(&bytes), Err(PacketError::Checksum)));
    }
    #[test]
    fn packet_capacity_error_does_not_modify_output() {
        let datagram = UdpDatagram {
            source: UdpEndpoint { address: Ipv4Address([0; 4]), port: UdpPort(67) },
            destination: UdpEndpoint { address: Ipv4Address([255; 4]), port: UdpPort(68) }, payload: &[1],
        };
        let mut output = [99; 28];
        assert_eq!(datagram.encode_ipv4(PacketIdentification(0), HopLimit(64), &mut output), Err(PacketError::Capacity));
        assert_eq!(output, [99; 28]);
    }
}
