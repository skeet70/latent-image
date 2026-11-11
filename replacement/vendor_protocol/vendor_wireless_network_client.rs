use super::MacAddress;
use super::dhcp::{DhcpError, LeaseSeconds, Milliseconds, TransactionId, UdpPort};
use super::state::{Ipv4Address, Ipv4Mask, Lease};
use super::packet::{HopLimit, PacketError, PacketIdentification, UdpDatagram, UdpEndpoint};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientPhase { Stopped, Selecting, Requesting, Bound, Renewing, Rebinding }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientEvent { Ignored, Request, Bound(Lease), Lost }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientBinding { pub lease: Lease, pub dns: [Option<Ipv4Address>; 2], pub duration: LeaseSeconds }
#[derive(Clone, Copy)]
struct Offer { binding: ClientBinding, server: Ipv4Address, renewal: u32, rebinding: u32 }
pub struct DhcpClient {
    hardware: MacAddress,
    transaction: TransactionId,
    phase: ClientPhase,
    offer: Option<Offer>,
    age_ms: u64,
    next_send_ms: u64,
    last: Milliseconds,
}
impl DhcpClient {
    pub const fn new(hardware: MacAddress) -> Self {
        Self { hardware, transaction: TransactionId([0; 4]), phase: ClientPhase::Stopped, offer: None, age_ms: 0, next_send_ms: 1000, last: Milliseconds(0) }
    }
    pub fn phase(&self) -> ClientPhase { self.phase }
    pub fn binding(&self) -> Option<ClientBinding> {
        if matches!(self.phase, ClientPhase::Bound | ClientPhase::Renewing | ClientPhase::Rebinding) { self.offer.map(|offer| offer.binding) } else { None }
    }
    pub fn start(&mut self, transaction: TransactionId, now: Milliseconds) {
        self.stop(); self.transaction = transaction; self.last = now; self.phase = ClientPhase::Selecting;
    }
    pub fn stop(&mut self) { self.phase = ClientPhase::Stopped; self.offer = None; self.age_ms = 0; self.next_send_ms = 1000; }
    pub fn poll(&mut self, now: Milliseconds) -> ClientEvent {
        self.age_ms = self.age_ms.saturating_add(u64::from(now.0.wrapping_sub(self.last.0)));
        self.last = now;
        if matches!(self.phase, ClientPhase::Stopped) { return ClientEvent::Ignored; }
        if let Some(offer) = self.offer {
            if matches!(self.phase, ClientPhase::Bound | ClientPhase::Renewing | ClientPhase::Rebinding) && offer.binding.duration.0 != u32::MAX {
                let age = self.age_ms / 1000;
                if age >= u64::from(offer.binding.duration.0) {
                    self.offer = None; self.phase = ClientPhase::Selecting; self.age_ms = 0; self.next_send_ms = 1000; return ClientEvent::Lost;
                }
                if age >= u64::from(offer.rebinding) && self.phase != ClientPhase::Rebinding {
                    self.phase = ClientPhase::Rebinding; self.next_send_ms = self.age_ms + 1000; return ClientEvent::Request;
                }
                if age >= u64::from(offer.renewal) && self.phase == ClientPhase::Bound {
                    self.phase = ClientPhase::Renewing; self.next_send_ms = self.age_ms + 1000; return ClientEvent::Request;
                }
            }
        }
        if self.phase != ClientPhase::Bound && self.age_ms >= self.next_send_ms {
            self.next_send_ms = self.age_ms + 1000; return ClientEvent::Request;
        }
        ClientEvent::Ignored
    }
    pub fn receive(&mut self, bytes: &[u8], now: Milliseconds) -> Result<ClientEvent, DhcpError> {
        if self.phase == ClientPhase::Stopped { return Err(DhcpError::NotRunning); }
        if self.poll(now) == ClientEvent::Lost { return Ok(ClientEvent::Lost); }
        if bytes.len() < 240 { return Err(DhcpError::Truncated); }
        if bytes[..3] != [2, 1, 6] { return Err(DhcpError::Hardware); }
        if bytes[236..240] != [99, 130, 83, 99] { return Err(DhcpError::Cookie); }
        if bytes[4..8] != self.transaction.0 || bytes[28..34] != self.hardware.0 { return Ok(ClientEvent::Ignored); }
        let mut options = ClientOptions::new();
        options.parse(&bytes[240..])?;
        let overload = options.overload;
        if overload & 1 != 0 { options.parse(&bytes[108..236])?; }
        if overload & 2 != 0 { options.parse(&bytes[44..108])?; }
        let server = options.server.ok_or(DhcpError::Options)?;
        let kind = options.kind.ok_or(DhcpError::Message)?;
        if matches!(self.phase, ClientPhase::Requesting | ClientPhase::Renewing | ClientPhase::Bound) && self.offer.is_some_and(|offer| offer.server != server) { return Ok(ClientEvent::Ignored); }
        if kind == ClientMessage::Nak {
            if !matches!(self.phase, ClientPhase::Requesting | ClientPhase::Renewing | ClientPhase::Rebinding) { return Ok(ClientEvent::Ignored); }
            self.offer = None; self.phase = ClientPhase::Selecting; self.age_ms = 0; self.next_send_ms = 1000; self.last = now; return Ok(ClientEvent::Lost);
        }
        if (kind == ClientMessage::Offer && self.phase != ClientPhase::Selecting) ||
            (kind == ClientMessage::Ack && !matches!(self.phase, ClientPhase::Requesting | ClientPhase::Renewing | ClientPhase::Rebinding)) { return Ok(ClientEvent::Ignored); }
        let mut address = Ipv4Address(bytes[16..20].try_into().map_err(|_| DhcpError::Truncated)?);
        if address == Ipv4Address([0; 4]) { address = self.binding().ok_or(DhcpError::Options)?.lease.address; }
        let duration = options.duration.ok_or(DhcpError::Options)?;
        let mask = options.mask.ok_or(DhcpError::Options)?;
        if duration.0 == 0 || address.0[0] == 0 || address.0[0] >= 224 { return Err(DhcpError::Options); }
        let renewal = options.renewal.unwrap_or(duration.0 / 2);
        let rebinding = options.rebinding.unwrap_or(duration.0 - duration.0 / 8);
        if duration.0 != u32::MAX && (renewal > rebinding || rebinding > duration.0) { return Err(DhcpError::Options); }
        let offer = Offer { binding: ClientBinding { lease: Lease { address, mask, router: options.router.unwrap_or(Ipv4Address([0; 4])) }, dns: options.dns, duration }, server, renewal, rebinding };
        self.offer = Some(offer); self.age_ms = 0; self.next_send_ms = 1000; self.last = now;
        if kind == ClientMessage::Offer { self.phase = ClientPhase::Requesting; Ok(ClientEvent::Request) }
        else { self.phase = ClientPhase::Bound; Ok(ClientEvent::Bound(offer.binding.lease)) }
    }
    pub fn encode_ipv4(&self, identification: PacketIdentification, output: &mut [u8]) -> Result<usize, PacketError> {
        if matches!(self.phase, ClientPhase::Stopped | ClientPhase::Bound) { return Err(PacketError::Protocol); }
        let mut body = [0; 300];
        body[..3].copy_from_slice(&[1, 1, 6]); body[4..8].copy_from_slice(&self.transaction.0);
        body[28..34].copy_from_slice(&self.hardware.0); body[236..240].copy_from_slice(&[99, 130, 83, 99]);
        let mut source = Ipv4Address([0; 4]);
        let mut destination = Ipv4Address([255; 4]);
        body[240..243].copy_from_slice(&[53, 1, if self.phase == ClientPhase::Selecting { 1 } else { 3 }]);
        let mut cursor = 243;
        if let Some(offer) = self.offer {
            if self.phase == ClientPhase::Requesting {
                body[cursor..cursor + 2].copy_from_slice(&[50, 4]); body[cursor + 2..cursor + 6].copy_from_slice(&offer.binding.lease.address.0); cursor += 6;
                body[cursor..cursor + 2].copy_from_slice(&[54, 4]); body[cursor + 2..cursor + 6].copy_from_slice(&offer.server.0); cursor += 6;
            } else {
                source = offer.binding.lease.address;
                body[12..16].copy_from_slice(&source.0);
                if self.phase == ClientPhase::Renewing { destination = offer.server; }
            }
        }
        if source == Ipv4Address([0; 4]) { body[10] = 128; }
        body[cursor..cursor + 7].copy_from_slice(&[55, 4, 1, 3, 6, 51, 255]);
        UdpDatagram { source: UdpEndpoint { address: source, port: UdpPort(68) }, destination: UdpEndpoint { address: destination, port: UdpPort(67) }, payload: &body }
            .encode_ipv4(identification, HopLimit(64), output)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClientMessage { Offer, Ack, Nak }
struct ClientOptions {
    kind: Option<ClientMessage>, server: Option<Ipv4Address>, mask: Option<Ipv4Mask>, router: Option<Ipv4Address>,
    dns: [Option<Ipv4Address>; 2], duration: Option<LeaseSeconds>, renewal: Option<u32>, rebinding: Option<u32>, overload: u8,
}
impl ClientOptions {
    fn new() -> Self { Self { kind: None, server: None, mask: None, router: None, dns: [None; 2], duration: None, renewal: None, rebinding: None, overload: 0 } }
    fn parse(&mut self, mut rest: &[u8]) -> Result<(), DhcpError> {
        while let Some((&tag, remaining)) = rest.split_first() {
            rest = remaining;
            if tag == 255 { return Ok(()); }
            if tag == 0 { continue; }
            let (&length, remaining) = rest.split_first().ok_or(DhcpError::Truncated)?;
            let value = remaining.get(..usize::from(length)).ok_or(DhcpError::Truncated)?;
            rest = &remaining[usize::from(length)..];
            match tag {
                53 => { let kind = match value { [2] => ClientMessage::Offer, [5] => ClientMessage::Ack, [6] => ClientMessage::Nak, _ => return Err(DhcpError::Message) }; if self.kind.is_some_and(|old| old != kind) { return Err(DhcpError::Message); } self.kind = Some(kind); }
                1 => self.mask = Some(Ipv4Mask(value.try_into().map_err(|_| DhcpError::Options)?)),
                54 => self.server = Some(Ipv4Address(value.try_into().map_err(|_| DhcpError::Options)?)),
                3 | 6 => {
                    if value.is_empty() || value.len() % 4 != 0 { return Err(DhcpError::Options); }
                    if tag == 3 { self.router = Some(Ipv4Address(value[..4].try_into().map_err(|_| DhcpError::Options)?)); }
                    else { for (slot, address) in self.dns.iter_mut().zip(value.chunks_exact(4)) { *slot = Some(Ipv4Address(address.try_into().map_err(|_| DhcpError::Options)?)); } }
                }
                51 | 58 | 59 => { let seconds = u32::from_be_bytes(value.try_into().map_err(|_| DhcpError::Options)?); match tag { 51 => self.duration = Some(LeaseSeconds(seconds)), 58 => self.renewal = Some(seconds), _ => self.rebinding = Some(seconds) } }
                52 => match value { [overload @ 1..=3] => self.overload = *overload, _ => return Err(DhcpError::Options) },
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::dhcp::{DhcpRequest, ReplyKind, ServerConfiguration, encode_reply};
    #[test]
    fn negotiates_server_reply_and_expires_before_accepting_late_packets() {
        let mac = MacAddress([2, 3, 4, 5, 6, 7]);
        let mut client = DhcpClient::new(mac);
        client.start(TransactionId([1, 2, 3, 4]), Milliseconds(0));
        let mut packet = [0; 400];
        let len = client.encode_ipv4(PacketIdentification(0), &mut packet).unwrap();
        let discover = UdpDatagram::parse_ipv4(&packet[..len]).unwrap();
        let request = DhcpRequest::parse(discover.payload).unwrap();
        let mut configuration = ServerConfiguration::camera_default(); configuration.lease = LeaseSeconds(8);
        let mut reply = [0; 548];
        let address = Ipv4Address([192, 168, 0, 2]);
        encode_reply(&request, configuration, ReplyKind::Offer, address, &mut reply).unwrap();
        assert_eq!(client.receive(&reply, Milliseconds(1)), Ok(ClientEvent::Request));
        reply[242] = 5;
        assert!(matches!(client.receive(&reply, Milliseconds(2)), Ok(ClientEvent::Bound(Lease { address: bound, .. })) if bound == address));
        assert_eq!(client.poll(Milliseconds(4002)), ClientEvent::Request);
        assert_eq!(client.phase(), ClientPhase::Renewing);
        assert_eq!(client.poll(Milliseconds(7002)), ClientEvent::Request);
        assert_eq!(client.phase(), ClientPhase::Rebinding);
        assert_eq!(client.poll(Milliseconds(8002)), ClientEvent::Lost);
        assert_eq!(client.binding(), None);
        client.stop();
        assert_eq!(client.receive(&reply, Milliseconds(9000)), Err(DhcpError::NotRunning));
    }
    #[test]
    fn ignores_reply_to_another_transaction() {
        let mut client = DhcpClient::new(MacAddress([2; 6]));
        client.start(TransactionId([1; 4]), Milliseconds(0));
        let mut reply = [0; 240]; reply[..3].copy_from_slice(&[2, 1, 6]); reply[236..].copy_from_slice(&[99, 130, 83, 99]);
        assert_eq!(client.receive(&reply, Milliseconds(1)), Ok(ClientEvent::Ignored));
        assert_eq!(client.phase(), ClientPhase::Selecting);
    }
    #[test]
    fn discover_timer_wraps_and_stop_disarms_transmission() {
        let mut client = DhcpClient::new(MacAddress([2; 6]));
        client.start(TransactionId([1; 4]), Milliseconds(u32::MAX - 499));
        assert_eq!(client.poll(Milliseconds(499)), ClientEvent::Ignored);
        assert_eq!(client.poll(Milliseconds(500)), ClientEvent::Request);
        client.stop();
        assert_eq!(client.poll(Milliseconds(1500)), ClientEvent::Ignored);
    }
}
