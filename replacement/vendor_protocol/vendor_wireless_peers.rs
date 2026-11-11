//! Canonical X-M1 station records, overlay47 0x012e5038..0x012e5858.
use super::vendor_wireless_network::{MacAddress, SignalDbm};
use super::vendor_wireless_station_driver::receive::events::HardwarePeerPower;

pub const PEER_CAPACITY: usize = 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetIndex(u8);
impl NetIndex {
    pub const PRIMARY: Self = Self(0);
    pub const SECONDARY: Self = Self(1);
    pub const P2P: Self = Self(2);
    pub const fn new(value: u8) -> Option<Self> { if value < 3 { Some(Self(value)) } else { None } }
    pub const fn wire(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerIndex(u8);
impl PeerIndex {
    pub const fn new(value: u8) -> Option<Self> { if value < PEER_CAPACITY as u8 { Some(Self(value)) } else { None } }
    pub const fn wire(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseGeneration(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerLease { index: PeerIndex, generation: LeaseGeneration }
impl PeerLease {
    pub const fn index(self) -> PeerIndex { self.index }
    pub const fn generation(self) -> LeaseGeneration { self.generation }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PeerState { Disconnected = 0, Authenticated = 1, Associated = 2 }
impl PeerState {
    pub const fn wire(self) -> u8 { self as u8 }
    pub const fn from_wire(value: u8) -> Option<Self> {
        match value { 0 => Some(Self::Disconnected), 1 => Some(Self::Authenticated), 2 => Some(Self::Associated), _ => None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolError { Exhausted, DuplicatePeer, StaleLease, Retiring, GenerationExhausted, CompletionMismatch }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerTransition {
    pub publish_device_state: bool,
    pub request_activation: bool,
    pub clear_hardware_queues: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhyFlags(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtCapabilities(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityBits(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalStrength(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssociationStatus(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeauthenticationReason(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerControlFlags(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerSessionState {
    pub association_status: AssociationStatus,
    pub deauthentication_reason: DeauthenticationReason,
    pub control_74: PeerControlFlags,
    pub control_15d: PeerControlFlags,
    pub control_64: PeerControlFlags,
}
impl PeerSessionState {
    pub const fn cleared() -> Self {
        Self { association_status: AssociationStatus(0), deauthentication_reason: DeauthenticationReason(0), control_74: PeerControlFlags(0), control_15d: PeerControlFlags(0), control_64: PeerControlFlags(0) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerRecord {
    lease: PeerLease,
    net: NetIndex,
    mac: MacAddress,
    state: PeerState,
    hardware_active: bool,
    retiring: bool,
    pub signal: Option<SignalDbm>,
    pub signal_strength: Option<SignalStrength>,
    pub security_authorized: bool,
    pub power: HardwarePeerPower,
    pub phy: PhyFlags,
    pub ht: HtCapabilities,
    pub capability: CapabilityBits,
    pub session: PeerSessionState,
}
impl PeerRecord {
    pub const fn lease(&self) -> PeerLease { self.lease }
    pub const fn net(&self) -> NetIndex { self.net }
    pub const fn mac(&self) -> MacAddress { self.mac }
    pub const fn state(&self) -> PeerState { self.state }
    pub const fn hardware_active(&self) -> bool { self.hardware_active }
    pub const fn retiring(&self) -> bool { self.retiring }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleasedPeer { pub record: PeerRecord }

pub struct PeerPool { slots: [Option<PeerRecord>; PEER_CAPACITY], generations: [LeaseGeneration; PEER_CAPACITY] }
impl PeerPool {
    pub const fn new() -> Self { Self { slots: [None; PEER_CAPACITY], generations: [LeaseGeneration(0); PEER_CAPACITY] } }
    pub fn reserve(&mut self, net: NetIndex, mac: MacAddress) -> Result<PeerLease, PoolError> {
        if self.lookup(net, mac).is_some() { return Err(PoolError::DuplicatePeer); }
        let index = self.slots.iter().position(Option::is_none).ok_or(PoolError::Exhausted)?;
        let generation = LeaseGeneration(self.generations[index].0.checked_add(1).ok_or(PoolError::GenerationExhausted)?);
        let lease = PeerLease { index: PeerIndex(index as u8), generation };
        self.generations[index] = generation;
        self.slots[index] = Some(PeerRecord { lease, net, mac, state: PeerState::Disconnected, hardware_active: false, retiring: false, signal: None, signal_strength: None, security_authorized: false, power: HardwarePeerPower::new(false, false), phy: PhyFlags(0), ht: HtCapabilities(0), capability: CapabilityBits(0), session: PeerSessionState::cleared() });
        Ok(lease)
    }
    pub fn lookup(&self, net: NetIndex, mac: MacAddress) -> Option<PeerLease> {
        self.slots.iter().flatten().find(|peer| peer.net == net && peer.mac == mac).map(|peer| peer.lease)
    }
    pub fn lease_for_index(&self, index: PeerIndex) -> Option<PeerLease> { self.slots[index.0 as usize].as_ref().filter(|p| !p.retiring).map(|p| p.lease) }
    pub fn get(&self, lease: PeerLease) -> Result<&PeerRecord, PoolError> {
        self.slots[lease.index.0 as usize].as_ref().filter(|p| p.lease == lease).ok_or(PoolError::StaleLease)
    }
    pub fn get_mut(&mut self, lease: PeerLease) -> Result<&mut PeerRecord, PoolError> {
        let peer = self.slots[lease.index.0 as usize].as_mut().filter(|p| p.lease == lease).ok_or(PoolError::StaleLease)?;
        if peer.retiring { return Err(PoolError::Retiring); }
        Ok(peer)
    }
    pub fn iter(&self) -> impl Iterator<Item = &PeerRecord> { self.slots.iter().flatten() }
    pub fn set_state(&mut self, lease: PeerLease, state: PeerState) -> Result<PeerTransition, PoolError> {
        let peer = self.get_mut(lease)?;
        let previous = peer.state;
        let local_only = (state == PeerState::Authenticated && previous != PeerState::Associated) || (state == PeerState::Disconnected && previous == PeerState::Authenticated);
        peer.state = state;
        let clear_hardware_queues = previous == PeerState::Associated && state != previous;
        if clear_hardware_queues { peer.hardware_active = false; peer.security_authorized = false; }
        Ok(PeerTransition { publish_device_state: !local_only, request_activation: state == PeerState::Associated && previous != state, clear_hardware_queues })
    }
    pub fn complete_association(&mut self, lease: PeerLease, mac: MacAddress) -> Result<(), PoolError> {
        let peer = self.get_mut(lease)?;
        if peer.state != PeerState::Associated || peer.mac != mac { return Err(PoolError::CompletionMismatch); }
        peer.hardware_active = true;
        Ok(())
    }
    pub fn clear_secondary_session(&mut self, lease: PeerLease) -> Result<(), PoolError> {
        self.get_mut(lease)?.session = PeerSessionState::cleared();
        Ok(())
    }
    pub fn begin_release(&mut self, lease: PeerLease) -> Result<(), PoolError> {
        let peer = self.get_mut(lease)?;
        peer.retiring = true;
        peer.hardware_active = false;
        peer.security_authorized = false;
        Ok(())
    }
    pub fn release(&mut self, lease: PeerLease) -> Result<ReleasedPeer, PoolError> {
        self.get(lease)?;
        Ok(ReleasedPeer { record: self.slots[lease.index.0 as usize].take().ok_or(PoolError::StaleLease)? })
    }
}
impl Default for PeerPool { fn default() -> Self { Self::new() } }

#[cfg(test)]
#[path = "vendor_wireless_peers_tests.rs"]
mod tests;
