//! DHCP reply construction and lease ownership for the WLAN LAN service.
//! X-M1 1.50: 0x0131b750, 0x0131b93c and 0x0131be84.

use super::state::{Ipv4Address, Ipv4Mask};
use super::packet::{HopLimit, PacketError, PacketIdentification, UdpDatagram, UdpEndpoint};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseSeconds(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionId(pub [u8; 4]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientKey(pub [u8; 16]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DhcpError { Truncated, Cookie, Hardware, Options, Message, NoAddress, NotRunning }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkError { Packet(PacketError), Dhcp(DhcpError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestKind { Discover, Request, Decline, Release, Inform }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyKind { Offer, Ack, Nak }
impl ReplyKind {
    fn code(self) -> u8 { match self { Self::Offer => 2, Self::Ack => 5, Self::Nak => 6 } }
}

pub struct DhcpRequest<'a> {
    pub kind: RequestKind,
    pub transaction: TransactionId,
    pub client: ClientKey,
    pub client_address: Ipv4Address,
    pub relay: Ipv4Address,
    pub requested: Option<Ipv4Address>,
    pub server: Option<Ipv4Address>,
    pub requested_lease: Option<LeaseSeconds>,
    flags: [u8; 2],
    identifier: Option<&'a [u8]>,
}
impl<'a> DhcpRequest<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, DhcpError> {
        if bytes.len() < 240 { return Err(DhcpError::Truncated); }
        if bytes[236..240] != [99, 130, 83, 99] { return Err(DhcpError::Cookie); }
        if bytes[1] != 1 || bytes[2] != 6 { return Err(DhcpError::Hardware); }
        let mut kind = None;
        let mut requested = None;
        let mut server = None;
        let mut identifier = None;
        let mut overload = 0;
        let mut requested_lease = None;
        parse_options(&bytes[240..], &mut kind, &mut requested, &mut server, &mut identifier, &mut overload, &mut requested_lease)?;
        let original_kind = kind.ok_or(DhcpError::Message)?;
        if overload & 1 != 0 {
            parse_options(&bytes[108..236], &mut kind, &mut requested, &mut server, &mut identifier, &mut 0, &mut requested_lease)?;
        }
        if overload & 2 != 0 {
            parse_options(&bytes[44..108], &mut kind, &mut requested, &mut server, &mut identifier, &mut 0, &mut requested_lease)?;
        }
        if kind != Some(original_kind) { return Err(DhcpError::Message); }
        let mut key = [0; 16];
        key.copy_from_slice(&bytes[28..44]);
        Ok(Self { kind: original_kind, transaction: TransactionId(bytes[4..8].try_into().map_err(|_| DhcpError::Truncated)?),
            client: ClientKey(key), client_address: Ipv4Address(bytes[12..16].try_into().map_err(|_| DhcpError::Truncated)?),
            relay: Ipv4Address(bytes[24..28].try_into().map_err(|_| DhcpError::Truncated)?), requested, server,
            flags: [bytes[10], bytes[11]], identifier, requested_lease })
    }
}
fn parse_options<'a>(mut bytes: &'a [u8], kind: &mut Option<RequestKind>, requested: &mut Option<Ipv4Address>,
    server: &mut Option<Ipv4Address>, identifier: &mut Option<&'a [u8]>, overload: &mut u8,
    requested_lease: &mut Option<LeaseSeconds>) -> Result<(), DhcpError> {
    while let Some((&tag, rest)) = bytes.split_first() {
        bytes = rest;
        if tag == 255 { break; }
        if tag == 0 { continue; }
        let (&length, rest) = bytes.split_first().ok_or(DhcpError::Truncated)?;
        let value = rest.get(..length as usize).ok_or(DhcpError::Truncated)?;
        bytes = &rest[length as usize..];
        match tag {
            50 | 54 => {
                let address = Ipv4Address(value.try_into().map_err(|_| DhcpError::Options)?);
                if tag == 50 { *requested = Some(address); } else { *server = Some(address); }
            }
            51 => {
                let seconds = u32::from_be_bytes(value.try_into().map_err(|_| DhcpError::Options)?);
                *requested_lease = Some(LeaseSeconds(seconds));
            }
            52 => { if value.len() != 1 || value[0] > 3 { return Err(DhcpError::Options); } *overload = value[0]; }
            53 => {
                let parsed = match value {
                    [1] => RequestKind::Discover, [3] => RequestKind::Request,
                    [4] => RequestKind::Decline, [7] => RequestKind::Release, [8] => RequestKind::Inform,
                    _ => return Err(DhcpError::Message),
                };
                if kind.is_some_and(|old| old != parsed) { return Err(DhcpError::Message); }
                *kind = Some(parsed);
            }
            55 => if value.is_empty() || value.len() > 32 { return Err(DhcpError::Options); },
            56 | 61 => {
                if value.is_empty() { return Err(DhcpError::Options); }
                if tag == 61 { *identifier = Some(value); }
            }
            57 => if value.len() != 2 { return Err(DhcpError::Options); },
            66 => if value.is_empty() || value.len() > 63 { return Err(DhcpError::Options); },
            _ => {}
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerConfiguration {
    pub address: Ipv4Address,
    pub mask: Ipv4Mask,
    pub dns: [Option<Ipv4Address>; 2],
    pub lease: LeaseSeconds,
}
impl ServerConfiguration {
    pub const fn camera_default() -> Self {
        Self { address: Ipv4Address([192, 168, 0, 1]), mask: Ipv4Mask([255, 255, 0, 0]),
            dns: [Some(Ipv4Address([192, 168, 0, 1])), None], lease: LeaseSeconds(u32::MAX) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpPort(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Destination { pub address: Ipv4Address, pub port: UdpPort }

pub fn encode_reply(request: &DhcpRequest<'_>, configuration: ServerConfiguration,
    kind: ReplyKind, offered: Ipv4Address, output: &mut [u8; 548]) -> Result<Destination, DhcpError> {
    output.fill(0);
    output[..3].copy_from_slice(&[2, 1, 6]);
    output[4..8].copy_from_slice(&request.transaction.0);
    output[10..12].copy_from_slice(&request.flags);
    output[16..20].copy_from_slice(&offered.0);
    output[20..24].copy_from_slice(&configuration.address.0);
    output[24..28].copy_from_slice(&request.relay.0);
    output[28..44].copy_from_slice(&request.client.0);
    output[236..240].copy_from_slice(&[99, 130, 83, 99]);
    output[240..243].copy_from_slice(&[53, 1, kind.code()]);
    output[246..248].copy_from_slice(&[54, 4]);
    output[248..252].copy_from_slice(&configuration.address.0);
    output[254..256].copy_from_slice(&[51, 4]);
    let seconds = if configuration.lease.0 == u32::MAX || request.kind == RequestKind::Request { 10000 }
        else { request.requested_lease.filter(|lease| lease.0 != 0).unwrap_or(configuration.lease).0 };
    output[256..260].copy_from_slice(&seconds.to_be_bytes());
    output[262..264].copy_from_slice(&[1, 4]);
    output[264..268].copy_from_slice(&configuration.mask.0);
    output[270..272].copy_from_slice(&[3, 4]);
    output[272..276].copy_from_slice(&configuration.address.0);
    if let Some(first) = configuration.dns[0] {
        output[278..280].copy_from_slice(&[6, if configuration.dns[1].is_some() { 8 } else { 4 }]);
        output[280..284].copy_from_slice(&first.0);
        if let Some(second) = configuration.dns[1] { output[284..288].copy_from_slice(&second.0); }
    }
    if let Some(identifier) = request.identifier {
        if identifier.len() > 255 || 291 + identifier.len() > output.len() { return Err(DhcpError::Options); }
        output[288] = 61;
        output[289] = identifier.len() as u8;
        output[290..290 + identifier.len()].copy_from_slice(identifier);
        output[290 + identifier.len()] = 255;
    } else { output[288] = 255; }
    if request.client_address != Ipv4Address([0; 4]) {
        return Ok(Destination { address: request.client_address, port: UdpPort(68) });
    }
    if request.relay != Ipv4Address([0; 4]) {
        return Ok(Destination { address: request.relay, port: UdpPort(67) });
    }
    Ok(Destination { address: Ipv4Address([255; 4]), port: UdpPort(68) })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Binding { Free, Offered { client: ClientKey, at: Milliseconds }, Bound { client: ClientKey, at: Milliseconds }, Declined }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseSlot { pub address: Ipv4Address, binding: Binding }
impl LeaseSlot {
    pub const fn new(address: Ipv4Address) -> Self { Self { address, binding: Binding::Free } }
    pub fn owner(&self) -> Option<ClientKey> {
        match self.binding { Binding::Offered { client, .. } | Binding::Bound { client, .. } => Some(client), _ => None }
    }
}

pub struct DhcpServer<'a> { configuration: ServerConfiguration, slots: &'a mut [LeaseSlot], running: bool }
impl<'a> DhcpServer<'a> {
    pub fn new(configuration: ServerConfiguration, slots: &'a mut [LeaseSlot]) -> Self {
        Self { configuration, slots, running: true }
    }
    pub fn start(&mut self, configuration: ServerConfiguration) {
        self.stop();
        self.configuration = configuration;
        self.running = true;
    }
    pub fn stop(&mut self) {
        self.running = false;
        for slot in &mut *self.slots { slot.binding = Binding::Free; }
    }
    pub fn lease_owner(&self, address: Ipv4Address) -> Option<ClientKey> {
        self.slots.iter().find(|slot| slot.address == address).and_then(LeaseSlot::owner)
    }
    pub fn expire(&mut self, now: Milliseconds) {
        for slot in &mut *self.slots {
            let expired = match slot.binding {
                Binding::Offered { at, .. } => now.0.wrapping_sub(at.0) >= 5000,
                Binding::Bound { at, .. } => self.configuration.lease.0 != u32::MAX &&
                    now.0.wrapping_sub(at.0) / 1000 >= self.configuration.lease.0,
                _ => false,
            };
            if expired { slot.binding = Binding::Free; }
        }
    }
    pub fn receive_ipv4(&mut self, packet: &[u8], now: Milliseconds,
        identification: PacketIdentification, output: &mut [u8]) -> Result<Option<usize>, NetworkError> {
        if !self.running { return Err(NetworkError::Dhcp(DhcpError::NotRunning)); }
        let incoming = UdpDatagram::parse_ipv4(packet).map_err(NetworkError::Packet)?;
        if incoming.destination.port != UdpPort(67) ||
            (incoming.source.port != UdpPort(68) && incoming.source.port != UdpPort(67)) {
            return Ok(None);
        }
        if output.len() < 576 { return Err(NetworkError::Packet(PacketError::Capacity)); }
        let mut reply = [0; 548];
        let Some(destination) = self.receive(incoming.payload, now, &mut reply).map_err(NetworkError::Dhcp)? else { return Ok(None); };
        UdpDatagram {
            source: UdpEndpoint { address: self.configuration.address, port: UdpPort(67) },
            destination: UdpEndpoint { address: destination.address, port: destination.port },
            payload: &reply,
        }.encode_ipv4(identification, HopLimit(64), output).map(Some).map_err(NetworkError::Packet)
    }
    pub fn receive(&mut self, payload: &[u8], now: Milliseconds, output: &mut [u8; 548])
        -> Result<Option<Destination>, DhcpError> {
        if !self.running { return Err(DhcpError::NotRunning); }
        let request = DhcpRequest::parse(payload)?;
        self.expire(now);
        if request.server.is_some_and(|address| address != self.configuration.address) { return Ok(None); }
        match request.kind {
            RequestKind::Discover => {
                let existing = self.slots.iter().position(|slot| slot.owner() == Some(request.client));
                let available = existing.or_else(|| self.slots.iter().position(|slot| slot.binding == Binding::Free)).ok_or(DhcpError::NoAddress)?;
                let slot = &mut self.slots[available];
                let destination = encode_reply(&request, self.configuration, ReplyKind::Offer, slot.address, output)?;
                slot.binding = Binding::Offered { client: request.client, at: now };
                Ok(Some(destination))
            }
            RequestKind::Request => {
                let desired = request.requested.unwrap_or(request.client_address);
                let index = self.slots.iter().position(|slot| slot.address == desired &&
                    (slot.owner() == Some(request.client) || slot.binding == Binding::Free));
                let Some(index) = index else {
                    return encode_reply(&request, self.configuration, ReplyKind::Nak, Ipv4Address([0; 4]), output).map(Some);
                };
                let destination = encode_reply(&request, self.configuration, ReplyKind::Ack, desired, output)?;
                self.slots[index].binding = Binding::Bound { client: request.client, at: now };
                Ok(Some(destination))
            }
            RequestKind::Decline | RequestKind::Release => {
                let address = request.requested.unwrap_or(request.client_address);
                if let Some(slot) = self.slots.iter_mut().find(|slot| slot.address == address && slot.owner() == Some(request.client)) {
                    slot.binding = if request.kind == RequestKind::Decline { Binding::Declined } else { Binding::Free };
                }
                Ok(None)
            }
            RequestKind::Inform => encode_reply(&request, self.configuration, ReplyKind::Ack, Ipv4Address([0; 4]), output).map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DhcpError, DhcpRequest, DhcpServer, Ipv4Address, LeaseSlot, Milliseconds, ServerConfiguration, UdpPort};
    fn request(kind: u8, requested: Option<[u8; 4]>) -> [u8; 256] {
        let mut packet = [0; 256];
        packet[..3].copy_from_slice(&[1, 1, 6]);
        packet[4..8].copy_from_slice(&[1, 2, 3, 4]);
        packet[28..34].copy_from_slice(&[0, 1, 2, 3, 4, 5]);
        packet[236..240].copy_from_slice(&[99, 130, 83, 99]);
        packet[240..243].copy_from_slice(&[53, 1, kind]);
        if let Some(address) = requested {
            packet[243..245].copy_from_slice(&[50, 4]);
            packet[245..249].copy_from_slice(&address);
            packet[249] = 255;
        } else { packet[243] = 255; }
        packet
    }
    #[test]
    fn discovers_requests_and_releases_a_real_address() {
        let mut slots = [LeaseSlot::new(Ipv4Address([192, 168, 0, 2]))];
        let mut server = DhcpServer::new(ServerConfiguration::camera_default(), &mut slots);
        let mut output = [0; 548];
        let destination = server.receive(&request(1, None), Milliseconds(0), &mut output).unwrap().unwrap();
        assert_eq!(destination.address, Ipv4Address([255; 4]));
        assert_eq!(destination.port, UdpPort(68));
        assert_eq!(&output[16..20], &[192, 168, 0, 2]);
        assert_eq!(&output[240..243], &[53, 1, 2]);
        server.receive(&request(3, Some([192, 168, 0, 2])), Milliseconds(1), &mut output).unwrap();
        assert_eq!(output[242], 5);
        assert!(server.lease_owner(Ipv4Address([192, 168, 0, 2])).is_some());
        server.receive(&request(7, Some([192, 168, 0, 2])), Milliseconds(2), &mut output).unwrap();
        assert_eq!(server.lease_owner(Ipv4Address([192, 168, 0, 2])), None);
    }
    #[test]
    fn stopping_cancels_offers_and_rejects_late_requests() {
        let mut slots = [LeaseSlot::new(Ipv4Address([192, 168, 0, 2]))];
        let mut server = DhcpServer::new(ServerConfiguration::camera_default(), &mut slots);
        let mut output = [0; 548];
        server.receive(&request(1, None), Milliseconds(0), &mut output).unwrap();
        server.stop();
        assert_eq!(server.receive(&request(3, Some([192, 168, 0, 2])), Milliseconds(2), &mut output), Err(DhcpError::NotRunning));
        assert_eq!(server.lease_owner(Ipv4Address([192, 168, 0, 2])), None);
    }
    #[test]
    fn expires_unaccepted_offers_at_five_seconds() {
        let mut slots = [LeaseSlot::new(Ipv4Address([192, 168, 0, 2]))];
        let mut server = DhcpServer::new(ServerConfiguration::camera_default(), &mut slots);
        server.receive(&request(1, None), Milliseconds(u32::MAX - 1000), &mut [0; 548]).unwrap();
        server.expire(Milliseconds(3998));
        assert!(server.lease_owner(Ipv4Address([192, 168, 0, 2])).is_some());
        server.expire(Milliseconds(3999));
        assert_eq!(server.lease_owner(Ipv4Address([192, 168, 0, 2])), None);
    }
    #[test]
    fn rejects_truncated_option_without_allocating_a_lease() {
        let mut packet = request(1, None);
        packet[243..245].copy_from_slice(&[50, 40]);
        assert!(matches!(DhcpRequest::parse(&packet), Err(DhcpError::Truncated)));
    }
    #[test]
    fn consumes_ipv4_request_and_emits_checksummed_dhcp_offer() {
        use super::super::packet::{HopLimit, PacketIdentification, UdpDatagram, UdpEndpoint};
        let payload = request(1, None);
        let incoming = UdpDatagram {
            source: UdpEndpoint { address: Ipv4Address([0; 4]), port: UdpPort(68) },
            destination: UdpEndpoint { address: Ipv4Address([255; 4]), port: UdpPort(67) },
            payload: &payload,
        };
        let mut input = [0; 284];
        incoming.encode_ipv4(PacketIdentification(1), HopLimit(64), &mut input).unwrap();
        let mut slots = [LeaseSlot::new(Ipv4Address([192, 168, 0, 2]))];
        let mut server = DhcpServer::new(ServerConfiguration::camera_default(), &mut slots);
        let mut output = [0; 576];
        let length = server.receive_ipv4(&input, Milliseconds(0), PacketIdentification(2), &mut output).unwrap().unwrap();
        let reply = UdpDatagram::parse_ipv4(&output[..length]).unwrap();
        assert_eq!(reply.source.port, UdpPort(67));
        assert_eq!(reply.destination.port, UdpPort(68));
        assert_eq!(&reply.payload[4..8], &[1, 2, 3, 4]);
        assert_eq!(&reply.payload[16..20], &[192, 168, 0, 2]);
        assert_eq!(&reply.payload[240..243], &[53, 1, 2]);
        assert_eq!(&reply.payload[256..260], &[0, 0, 39, 16]);
        server.stop();
        assert!(matches!(server.receive_ipv4(&input, Milliseconds(1), PacketIdentification(3), &mut output),
            Err(super::NetworkError::Dhcp(DhcpError::NotRunning))));
    }
    #[test]
    fn restarted_server_does_not_retain_previous_lease_owner() {
        let address = Ipv4Address([192, 168, 0, 2]);
        let mut slots = [LeaseSlot::new(address)];
        let mut server = DhcpServer::new(ServerConfiguration::camera_default(), &mut slots);
        let mut output = [0; 548];
        server.receive(&request(3, Some(address.0)), Milliseconds(0), &mut output).unwrap();
        assert!(server.lease_owner(address).is_some());
        server.stop();
        server.start(ServerConfiguration::camera_default());
        assert_eq!(server.lease_owner(address), None);
        assert!(server.receive(&request(1, None), Milliseconds(1), &mut output).unwrap().is_some());
        assert_eq!(&output[16..20], &address.0);
    }
}
