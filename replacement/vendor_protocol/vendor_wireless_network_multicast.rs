use super::packet::{InternetChecksum, PacketError, PacketIdentification};
use super::state::Ipv4Address;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MulticastGroup(Ipv4Address);
impl MulticastGroup {
    pub fn parse(address: Ipv4Address) -> Option<Self> { (address.0[0] & 0xf0 == 0xe0).then_some(Self(address)) }
    pub fn address(self) -> Ipv4Address { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Report { Membership, Leave }
#[derive(Clone, Copy)]
struct Membership { group: MulticastGroup, subscribers: u8, pending: Option<Report> }
pub struct MulticastState { groups: [Option<Membership>; 8] }
impl MulticastState {
    pub const fn new() -> Self { Self { groups: [None; 8] } }
    pub fn clear(&mut self) { self.groups.fill(None); }
    pub fn contains(&self, group: MulticastGroup) -> bool { self.groups.iter().flatten().any(|entry| entry.group == group && entry.subscribers != 0) }
    pub fn join(&mut self, group: MulticastGroup) -> Result<(), PacketError> {
        if let Some(entry) = self.groups.iter_mut().flatten().find(|entry| entry.group == group) {
            entry.subscribers = entry.subscribers.checked_add(1).ok_or(PacketError::Capacity)?;
            entry.pending = report_for(group); return Ok(());
        }
        let slot = self.groups.iter_mut().find(|entry| entry.is_none()).ok_or(PacketError::Capacity)?;
        *slot = Some(Membership { group, subscribers: 1, pending: report_for(group) }); Ok(())
    }
    pub fn leave(&mut self, group: MulticastGroup) {
        if let Some(slot) = self.groups.iter_mut().find(|slot| slot.is_some_and(|entry| entry.group == group)) {
            if let Some(entry) = slot {
                entry.subscribers = entry.subscribers.saturating_sub(1);
                if entry.subscribers == 0 {
                    if report_for(group).is_none() { *slot = None; } else { entry.pending = Some(Report::Leave); }
                }
            }
        }
    }
    pub fn receive_ipv4(&mut self, packet: &[u8]) -> Result<(), PacketError> {
        if packet.len() < 20 { return Err(PacketError::Truncated); }
        if packet[0] >> 4 != 4 || packet[9] != 2 { return Err(PacketError::Protocol); }
        let header = usize::from(packet[0] & 15) * 4;
        let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
        if header < 20 || total < header + 8 { return Err(PacketError::Length); }
        if packet.len() < total { return Err(PacketError::Truncated); }
        let mut checksum = InternetChecksum::new(); checksum.update(&packet[..header]);
        if checksum.finish() != 0 { return Err(PacketError::Checksum); }
        let igmp = &packet[header..total];
        let mut checksum = InternetChecksum::new(); checksum.update(igmp);
        if checksum.finish() != 0 { return Err(PacketError::Checksum); }
        let group = Ipv4Address(igmp[4..8].try_into().map_err(|_| PacketError::Truncated)?);
        match igmp[0] {
            0x11 => {
                if group != Ipv4Address([0; 4]) && MulticastGroup::parse(group).is_none() { return Err(PacketError::Protocol); }
                for entry in self.groups.iter_mut().flatten().filter(|entry| entry.subscribers != 0 && (group == Ipv4Address([0; 4]) || entry.group.address() == group)) {
                    entry.pending = report_for(entry.group);
                }
            }
            0x12 | 0x16 => {
                if let Some(entry) = self.groups.iter_mut().flatten().find(|entry| entry.group.address() == group && entry.subscribers != 0) { entry.pending = None; }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn poll_ipv4(&mut self, source: Ipv4Address, identification: PacketIdentification, output: &mut [u8]) -> Result<Option<usize>, PacketError> {
        let Some(slot) = self.groups.iter_mut().find(|slot| slot.is_some_and(|entry| entry.pending.is_some())) else { return Ok(None); };
        let Some(entry) = slot else { return Ok(None); };
        let Some(report) = entry.pending else { return Ok(None); };
        if output.len() < 32 { return Err(PacketError::Capacity); }
        output[..32].fill(0); output[0] = 0x46; output[2..4].copy_from_slice(&32u16.to_be_bytes());
        output[4..6].copy_from_slice(&identification.0.to_be_bytes()); output[8] = 1; output[9] = 2;
        output[12..16].copy_from_slice(&source.0);
        output[16..20].copy_from_slice(&if report == Report::Membership { entry.group.address().0 } else { [224, 0, 0, 2] });
        output[20..24].copy_from_slice(&[148, 4, 0, 0]);
        output[24] = if report == Report::Membership { 0x16 } else { 0x17 };
        output[28..32].copy_from_slice(&entry.group.address().0);
        let mut checksum = InternetChecksum::new(); checksum.update(&output[24..32]); output[26..28].copy_from_slice(&checksum.finish().to_be_bytes());
        let mut checksum = InternetChecksum::new(); checksum.update(&output[..24]); output[10..12].copy_from_slice(&checksum.finish().to_be_bytes());
        entry.pending = None;
        if entry.subscribers == 0 { *slot = None; }
        Ok(Some(32))
    }
}
fn report_for(group: MulticastGroup) -> Option<Report> { (group.address() != Ipv4Address([224, 0, 0, 1])).then_some(Report::Membership) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_membership_leaves_only_after_last_socket_and_emits_checked_igmp() {
        let group = MulticastGroup::parse(Ipv4Address([239, 1, 2, 3])).unwrap();
        let mut state = MulticastState::new(); state.join(group).unwrap(); state.join(group).unwrap();
        let mut packet = [0; 32];
        assert_eq!(state.poll_ipv4(Ipv4Address([192, 168, 0, 1]), PacketIdentification(1), &mut packet), Ok(Some(32)));
        assert_eq!(packet[24], 0x16); assert_eq!(&packet[16..20], &[239, 1, 2, 3]);
        let mut checksum = InternetChecksum::new(); checksum.update(&packet[..24]); assert_eq!(checksum.finish(), 0);
        let mut checksum = InternetChecksum::new(); checksum.update(&packet[24..]); assert_eq!(checksum.finish(), 0);
        state.leave(group); assert!(state.contains(group));
        assert_eq!(state.poll_ipv4(Ipv4Address([192, 168, 0, 1]), PacketIdentification(2), &mut packet), Ok(None));
        state.leave(group); assert!(!state.contains(group));
        assert_eq!(state.poll_ipv4(Ipv4Address([192, 168, 0, 1]), PacketIdentification(3), &mut packet), Ok(Some(32)));
        assert_eq!(packet[24], 0x17); assert_eq!(&packet[16..20], &[224, 0, 0, 2]);
    }
}
