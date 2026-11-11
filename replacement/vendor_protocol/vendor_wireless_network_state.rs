//! Peer roster and channel lease consumers recovered from X-M1 1.50 bytes.

use super::MacAddress;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ipv4Address(pub [u8; 4]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ipv4Mask(pub [u8; 4]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lease {
    pub address: Ipv4Address,
    pub mask: Ipv4Mask,
    pub router: Ipv4Address,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeasePhase { Inactive, Provisional, Bound }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseEntry {
    pub phase: LeasePhase,
    pub lease: Lease,
    pub usable: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelSlot(u8);
impl ChannelSlot {
    pub fn parse(value: u8) -> Option<Self> { (value < 4).then_some(Self(value)) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseBank { Primary, Secondary }

pub struct ChannelLeases {
    current: [Option<LeaseEntry>; 4],
    fallback: [Option<LeaseEntry>; 4],
    provisional_enabled: bool,
}
impl ChannelLeases {
    pub const fn new() -> Self {
        Self { current: [None; 4], fallback: [None; 4], provisional_enabled: false }
    }
    pub fn enable_provisional(&mut self, enabled: bool) { self.provisional_enabled = enabled; }
    pub fn install(&mut self, bank: LeaseBank, slot: ChannelSlot, entry: LeaseEntry) {
        let table = match bank { LeaseBank::Primary => &mut self.current, LeaseBank::Secondary => &mut self.fallback };
        table[slot.0 as usize] = Some(entry);
    }
    pub fn clear(&mut self, slot: ChannelSlot) {
        self.current[slot.0 as usize] = None;
        self.fallback[slot.0 as usize] = None;
    }
    // 0x012fe7b8 tries table 0 then table 1; 0x012fe6e8 gates provisional entries.
    pub fn lookup(&self, slot: ChannelSlot) -> Option<&Lease> {
        let index = slot.0 as usize;
        if let Some(entry) = &self.current[index] {
            if entry.phase == LeasePhase::Bound ||
                (entry.phase != LeasePhase::Inactive && self.provisional_enabled && entry.usable) {
                return Some(&entry.lease);
            }
        }
        self.fallback[index].as_ref().filter(|entry| entry.phase == LeasePhase::Bound).map(|entry| &entry.lease)
    }
}
impl Default for ChannelLeases { fn default() -> Self { Self::new() } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerChange { Connected, Disconnected }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskId(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerError { Full, UnknownPeer }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerNotification { change: PeerChange, source: TaskId }
impl PeerNotification {
    pub fn encode(&self) -> [u8; 12] {
        let kind: u16 = match self.change { PeerChange::Connected => 0x6705, PeerChange::Disconnected => 0x6706 };
        let kind = kind.to_le_bytes();
        [kind[0], kind[1], self.source.0, 0x57, 0, 0, 0, 0, 0, 0, 0, 0]
    }
}

pub struct PeerRoster { peers: [Option<MacAddress>; 10], count: u16 }
impl PeerRoster {
    pub const fn new() -> Self { Self { peers: [None; 10], count: 0 } }
    pub fn count(&self) -> u16 { self.count }
    pub fn contains(&self, address: MacAddress) -> bool { self.peers.contains(&Some(address)) }
    pub fn addresses(&self) -> impl Iterator<Item = MacAddress> + '_ {
        self.peers.iter().filter_map(|peer| *peer)
    }
    pub fn change(&mut self, change: PeerChange, address: MacAddress, source: TaskId)
        -> Result<Option<PeerNotification>, PeerError> {
        match change {
            PeerChange::Connected => {
                if self.contains(address) { return Ok(None); }
                let slot = self.peers.iter_mut().find(|peer| peer.is_none()).ok_or(PeerError::Full)?;
                *slot = Some(address);
                self.count += 1;
            }
            PeerChange::Disconnected => {
                let slot = self.peers.iter_mut().find(|peer| **peer == Some(address)).ok_or(PeerError::UnknownPeer)?;
                *slot = None;
                self.count -= 1;
            }
        }
        Ok(Some(PeerNotification { change, source }))
    }
    pub fn clear(&mut self) { self.peers = [None; 10]; self.count = 0; }
}
impl Default for PeerRoster { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::{ChannelLeases, ChannelSlot, Ipv4Address, Ipv4Mask, Lease, LeaseEntry, LeasePhase, MacAddress, PeerChange, PeerError, PeerRoster, TaskId};
    fn lease(last: u8, phase: LeasePhase, usable: bool) -> LeaseEntry {
        LeaseEntry { phase, usable, lease: Lease { address: Ipv4Address([192, 168, 0, last]),
            mask: Ipv4Mask([255, 255, 0, 0]), router: Ipv4Address([192, 168, 0, 1]) } }
    }
    #[test]
    fn selects_current_bound_lease_then_fallback() {
        let mut channel = ChannelLeases::new();
        let slot = ChannelSlot::parse(0).unwrap();
        channel.install(super::LeaseBank::Secondary, slot, lease(2, LeasePhase::Bound, false));
        channel.install(super::LeaseBank::Primary, slot, lease(3, LeasePhase::Inactive, false));
        assert_eq!(channel.lookup(slot).unwrap().address, Ipv4Address([192, 168, 0, 2]));
        channel.install(super::LeaseBank::Primary, slot, lease(4, LeasePhase::Bound, false));
        assert_eq!(channel.lookup(slot).unwrap().address, Ipv4Address([192, 168, 0, 4]));
        channel.clear(slot);
        assert_eq!(channel.lookup(slot), None);
    }
    #[test]
    fn provisional_lease_requires_both_channel_and_entry_admission() {
        let mut channel = ChannelLeases::new();
        let slot = ChannelSlot::parse(3).unwrap();
        channel.install(super::LeaseBank::Primary, slot, lease(2, LeasePhase::Provisional, true));
        assert_eq!(channel.lookup(slot), None);
        channel.enable_provisional(true);
        assert_eq!(channel.lookup(slot).unwrap().address, Ipv4Address([192, 168, 0, 2]));
        assert_eq!(ChannelSlot::parse(4), None);
    }
    #[test]
    fn updates_peer_roster_and_encodes_service_notifications() {
        let mut peers = PeerRoster::new();
        let address = MacAddress([1, 2, 3, 4, 5, 6]);
        let event = peers.change(PeerChange::Connected, address, TaskId(7)).unwrap().unwrap();
        assert_eq!(event.encode(), [5, 103, 7, 87, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(peers.count(), 1);
        assert_eq!(peers.change(PeerChange::Connected, address, TaskId(7)), Ok(None));
        assert!(peers.addresses().eq([address]));
        let event = peers.change(PeerChange::Disconnected, address, TaskId(7)).unwrap().unwrap();
        assert_eq!(event.encode()[0], 6);
        assert_eq!(peers.count(), 0);
        assert!(!peers.contains(address));
    }
    #[test]
    fn rejects_late_unknown_disconnect_without_changing_roster() {
        let mut peers = PeerRoster::new();
        let address = MacAddress([1; 6]);
        peers.change(PeerChange::Connected, address, TaskId(0)).unwrap();
        peers.clear();
        assert_eq!(peers.change(PeerChange::Disconnected, address, TaskId(0)), Err(PeerError::UnknownPeer));
        assert_eq!(peers.count(), 0);
        assert_eq!(peers.addresses().count(), 0);
    }
    #[test]
    fn full_roster_rejects_insert_without_incrementing_count() {
        let mut peers = PeerRoster::new();
        for value in 0..10 { peers.change(PeerChange::Connected, MacAddress([value; 6]), TaskId(0)).unwrap(); }
        assert_eq!(peers.change(PeerChange::Connected, MacAddress([10; 6]), TaskId(0)), Err(PeerError::Full));
        assert_eq!(peers.count(), 10);
    }
}
