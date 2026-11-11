use super::MacAddress;
use super::state::{Ipv4Address, Lease};
use super::packet::PacketError;
use super::dhcp::Milliseconds;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EtherType { Ipv4, Arp }
pub struct EthernetFrame<'a> {
    pub source: MacAddress,
    pub destination: MacAddress,
    pub kind: EtherType,
    pub payload: &'a [u8],
}
impl<'a> EthernetFrame<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, PacketError> {
        if bytes.len() < 14 { return Err(PacketError::Truncated); }
        let kind = match &bytes[12..14] { [8, 0] => EtherType::Ipv4, [8, 6] => EtherType::Arp, _ => return Err(PacketError::Protocol) };
        Ok(Self { destination: MacAddress(bytes[..6].try_into().map_err(|_| PacketError::Truncated)?),
            source: MacAddress(bytes[6..12].try_into().map_err(|_| PacketError::Truncated)?), kind, payload: &bytes[14..] })
    }
    pub fn encode(&self, output: &mut [u8]) -> Result<usize, PacketError> {
        let length = self.payload.len().checked_add(14).ok_or(PacketError::Capacity)?;
        if output.len() < length { return Err(PacketError::Capacity); }
        write_header(output, self.source, self.destination, self.kind);
        output[14..length].copy_from_slice(self.payload);
        Ok(length)
    }
}
pub fn write_header(output: &mut [u8], source: MacAddress, destination: MacAddress, kind: EtherType) {
    output[..6].copy_from_slice(&destination.0);
    output[6..12].copy_from_slice(&source.0);
    output[12..14].copy_from_slice(match kind { EtherType::Ipv4 => &[8, 0], EtherType::Arp => &[8, 6] });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError { NoAddress, NoRoute, Unresolved(Ipv4Address), NeighborTimedOut(Ipv4Address), Full }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Neighbor { address: Ipv4Address, hardware: MacAddress, expires: Milliseconds }
#[derive(Clone, Copy)]
enum ResolutionPhase { Probing { started: Milliseconds, sent: u8 }, Holding { until: Milliseconds } }
#[derive(Clone, Copy)]
struct Resolution { address: Ipv4Address, phase: ResolutionPhase }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionOutcome { Idle, Frame { length: usize }, Failed(Ipv4Address) }
pub struct LinkState { hardware: MacAddress, lease: Option<Lease>, neighbors: [Option<Neighbor>; 16], clock: Milliseconds, resolution: Option<Resolution> }
impl LinkState {
    pub const fn new(hardware: MacAddress) -> Self { Self { hardware, lease: None, neighbors: [None; 16], clock: Milliseconds(0), resolution: None } }
    pub fn hardware(&self) -> MacAddress { self.hardware }
    pub fn lease(&self) -> Option<Lease> { self.lease }
    pub fn neighbor(&self, address: Ipv4Address) -> Option<MacAddress> {
        self.neighbors.iter().flatten().find(|entry| entry.address == address).map(|entry| entry.hardware)
    }
    pub fn install(&mut self, lease: Lease) { self.clear(); self.lease = Some(lease); }
    pub fn clear(&mut self) { self.lease = None; self.neighbors.fill(None); self.resolution = None; }
    pub fn advance(&mut self, now: Milliseconds) {
        self.clock = now;
        for entry in &mut self.neighbors {
            if entry.is_some_and(|entry| reached(now, entry.expires)) { *entry = None; }
        }
    }
    pub fn begin_resolution(&mut self, address: Ipv4Address, output: &mut [u8]) -> Result<usize, PacketError> {
        let length = self.request(address, output)?;
        self.resolution = Some(Resolution { address, phase: ResolutionPhase::Probing { started: self.clock, sent: 1 } });
        Ok(length)
    }
    pub fn poll_resolution(&mut self, now: Milliseconds, output: &mut [u8]) -> Result<ResolutionOutcome, PacketError> {
        self.advance(now);
        let Some(mut resolution) = self.resolution else { return Ok(ResolutionOutcome::Idle); };
        match resolution.phase {
            ResolutionPhase::Probing { started, sent } => {
                let elapsed = now.0.wrapping_sub(started.0);
                if elapsed >= 5000 {
                    let until = Milliseconds(started.0.wrapping_add(25000));
                    if reached(now, until) { self.resolution = None; return Ok(ResolutionOutcome::Failed(resolution.address)); }
                    resolution.phase = ResolutionPhase::Holding { until };
                    self.resolution = Some(resolution);
                } else if elapsed / 1000 >= u32::from(sent) {
                    let length = self.request(resolution.address, output)?;
                    resolution.phase = ResolutionPhase::Probing { started, sent: (elapsed / 1000) as u8 + 1 };
                    self.resolution = Some(resolution);
                    return Ok(ResolutionOutcome::Frame { length });
                }
            }
            ResolutionPhase::Holding { until } => {
                if reached(now, until) { self.resolution = None; return Ok(ResolutionOutcome::Failed(resolution.address)); }
            }
        }
        Ok(ResolutionOutcome::Idle)
    }
    pub fn accepts(&self, destination: Ipv4Address) -> bool {
        self.lease.is_some_and(|lease| destination == lease.address || destination == broadcast(lease)) || destination == Ipv4Address([255; 4])
    }
    pub fn next_hop(&self, destination: Ipv4Address) -> Result<Ipv4Address, RouteError> {
        let lease = self.lease.ok_or(RouteError::NoAddress)?;
        if destination.0.iter().zip(lease.address.0).zip(lease.mask.0).all(|((&target, local), mask)| target & mask == local & mask) {
            Ok(destination)
        } else if lease.router != Ipv4Address([0; 4]) { Ok(lease.router) }
        else { Err(RouteError::NoRoute) }
    }
    pub fn resolve(&self, destination: Ipv4Address) -> Result<MacAddress, RouteError> {
        if destination == Ipv4Address([255; 4]) || self.lease.is_some_and(|lease| destination == broadcast(lease)) { return Ok(MacAddress([255; 6])); }
        if destination.0[0] & 0xf0 == 0xe0 { return Ok(MacAddress([1, 0, 94, destination.0[1] & 127, destination.0[2], destination.0[3]])); }
        let next = self.next_hop(destination)?;
        if self.resolution.is_some_and(|resolution| resolution.address == next && matches!(resolution.phase, ResolutionPhase::Holding { .. })) { return Err(RouteError::NeighborTimedOut(next)); }
        self.neighbor(next).ok_or(RouteError::Unresolved(next))
    }
    pub fn learn(&mut self, address: Ipv4Address, hardware: MacAddress) -> Result<(), RouteError> {
        if address == Ipv4Address([0; 4]) || address == Ipv4Address([255; 4]) || hardware.0[0] & 1 != 0 || hardware == MacAddress([0; 6]) { return Ok(()); }
        let expires = Milliseconds(self.clock.0.wrapping_add(600000));
        if let Some(entry) = self.neighbors.iter_mut().flatten().find(|entry| entry.address == address) {
            entry.hardware = hardware; entry.expires = expires;
        } else {
            let slot = self.neighbors.iter_mut().find(|entry| entry.is_none()).ok_or(RouteError::Full)?;
            *slot = Some(Neighbor { address, hardware, expires });
        }
        if self.resolution.is_some_and(|resolution| resolution.address == address) { self.resolution = None; }
        Ok(())
    }
    pub fn request(&self, address: Ipv4Address, output: &mut [u8]) -> Result<usize, PacketError> {
        let lease = self.lease.ok_or(PacketError::Protocol)?;
        encode_arp(self.hardware, lease.address, MacAddress([0; 6]), address, ArpOperation::Request, output)
    }
    pub fn receive_arp(&mut self, frame: &EthernetFrame<'_>, output: &mut [u8]) -> Result<Option<usize>, PacketError> {
        let arp = ArpPacket::parse(frame.payload)?;
        if arp.hardware != frame.source { return Err(PacketError::Protocol); }
        let Some(lease) = self.lease else { return Ok(None); };
        if arp.target != lease.address { return Ok(None); }
        if arp.operation == ArpOperation::Request && output.len() < 42 { return Err(PacketError::Capacity); }
        self.learn(arp.address, arp.hardware).map_err(|_| PacketError::Capacity)?;
        if arp.operation == ArpOperation::Request {
            encode_arp(self.hardware, lease.address, arp.hardware, arp.address, ArpOperation::Reply, output).map(Some)
        } else { Ok(None) }
    }
}
fn reached(now: Milliseconds, deadline: Milliseconds) -> bool { now.0.wrapping_sub(deadline.0) < 0x8000_0000 }
fn broadcast(lease: Lease) -> Ipv4Address { Ipv4Address(core::array::from_fn(|i| lease.address.0[i] | !lease.mask.0[i])) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArpOperation { Request, Reply }
pub struct ArpPacket { pub operation: ArpOperation, pub hardware: MacAddress, pub address: Ipv4Address, pub target: Ipv4Address }
impl ArpPacket {
    pub fn parse(bytes: &[u8]) -> Result<Self, PacketError> {
        if bytes.len() < 28 { return Err(PacketError::Truncated); }
        if bytes[..6] != [0, 1, 8, 0, 6, 4] { return Err(PacketError::Protocol); }
        let operation = match &bytes[6..8] { [0, 1] => ArpOperation::Request, [0, 2] => ArpOperation::Reply, _ => return Err(PacketError::Protocol) };
        Ok(Self { operation, hardware: MacAddress(bytes[8..14].try_into().map_err(|_| PacketError::Truncated)?),
            address: Ipv4Address(bytes[14..18].try_into().map_err(|_| PacketError::Truncated)?),
            target: Ipv4Address(bytes[24..28].try_into().map_err(|_| PacketError::Truncated)?) })
    }
}
fn encode_arp(source: MacAddress, address: Ipv4Address, target_hardware: MacAddress, target: Ipv4Address,
    operation: ArpOperation, output: &mut [u8]) -> Result<usize, PacketError> {
    if output.len() < 42 { return Err(PacketError::Capacity); }
    write_header(output, source, if operation == ArpOperation::Request { MacAddress([255; 6]) } else { target_hardware }, EtherType::Arp);
    output[14..20].copy_from_slice(&[0, 1, 8, 0, 6, 4]);
    output[20..22].copy_from_slice(match operation { ArpOperation::Request => &[0, 1], ArpOperation::Reply => &[0, 2] });
    output[22..28].copy_from_slice(&source.0);
    output[28..32].copy_from_slice(&address.0);
    output[32..38].copy_from_slice(&target_hardware.0);
    output[38..42].copy_from_slice(&target.0);
    Ok(42)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::Ipv4Mask;
    #[test]
    fn routes_remote_peer_via_router_and_clears_neighbors_on_address_change() {
        let mut link = LinkState::new(MacAddress([2; 6]));
        let lease = Lease { address: Ipv4Address([192, 168, 0, 1]), mask: Ipv4Mask([255, 255, 255, 0]), router: Ipv4Address([192, 168, 0, 254]) };
        link.install(lease);
        link.learn(lease.router, MacAddress([4; 6])).unwrap();
        assert_eq!(link.resolve(Ipv4Address([10, 0, 0, 1])), Ok(MacAddress([4; 6])));
        assert_eq!(link.resolve(Ipv4Address([192, 168, 0, 255])), Ok(MacAddress([255; 6])));
        link.install(lease);
        assert_eq!(link.resolve(Ipv4Address([10, 0, 0, 1])), Err(RouteError::Unresolved(lease.router)));
    }
    #[test]
    fn answers_only_arp_for_owned_address_and_learns_sender() {
        let local = MacAddress([2; 6]);
        let peer = MacAddress([4; 6]);
        let address = Ipv4Address([192, 168, 0, 1]);
        let remote = Ipv4Address([192, 168, 0, 2]);
        let mut link = LinkState::new(local);
        link.install(Lease { address, mask: super::super::state::Ipv4Mask([255, 255, 255, 0]), router: Ipv4Address([0; 4]) });
        let mut incoming = [0; 42];
        encode_arp(peer, remote, MacAddress([0; 6]), address, ArpOperation::Request, &mut incoming).unwrap();
        let mut output = [0; 42];
        assert_eq!(link.receive_arp(&EthernetFrame::parse(&incoming).unwrap(), &mut output), Ok(Some(42)));
        let reply = EthernetFrame::parse(&output).unwrap();
        assert_eq!(reply.destination, peer);
        assert_eq!(ArpPacket::parse(reply.payload).unwrap().operation, ArpOperation::Reply);
        assert_eq!(link.resolve(remote), Ok(peer));
        link.clear();
        assert_eq!(link.receive_arp(&EthernetFrame::parse(&incoming).unwrap(), &mut output), Ok(None));
    }
}
