use super::{AccessPoint, Error, ManagementTransmission, PeerExpiry, State};
use super::management::Completion;
use crate::vendor_wireless_access_point_authenticator::Milliseconds;
use crate::vendor_wireless_peers::PeerPool;
use crate::vendor_wireless_station_driver::association::frames::Packet;

impl AccessPoint {
    pub fn poll_expired_management(&mut self, now: Milliseconds, pool: &mut PeerPool) -> Result<Option<ManagementTransmission>, Error> {
        if self.state != State::Running { return Ok(None); }
        if self.management_pending.is_some() || self.installation_pending.is_some() { return Ok(None); }
        let mut expired = None;
        for (index, entry) in self.peers.iter_mut().enumerate() {
            if let Some(entry) = entry {
                if entry.deadline.is_some_and(|deadline| now.0 >= deadline.0) {
                    if entry.associated {
                        entry.deadline = Some(Milliseconds(now.0.checked_add(300_000).ok_or(Error::Capacity)?));
                    } else { expired = Some(index); break; }
                }
            }
        }
        let Some(index) = expired else { return Ok(None); };
        let entry = self.peers[index].ok_or(Error::UnknownPeer)?;
        let mut packet = Packet::new();
        match entry.expiry {
            PeerExpiry::FirstDisassociation | PeerExpiry::SecondDisassociation => {
                packet.header(0xa0, entry.peer.address, self.local, self.local);
                packet.append(&4u16.to_le_bytes()).map_err(|_| Error::Capacity)?;
                let current = self.peers[index].as_mut().ok_or(Error::UnknownPeer)?;
                current.expiry = match entry.expiry { PeerExpiry::FirstDisassociation => PeerExpiry::SecondDisassociation, _ => PeerExpiry::Deauthentication };
                current.deadline = Some(Milliseconds(now.0.checked_add(1000).ok_or(Error::Capacity)?));
            }
            PeerExpiry::Deauthentication => {
                packet.header(0xc0, entry.peer.address, self.local, self.local);
                packet.append(&2u16.to_le_bytes()).map_err(|_| Error::Capacity)?;
                self.peer_disconnected(entry.peer.address, pool)?;
            }
        }
        self.management_reply(entry.peer.address, entry.peer.signal, Completion::None, &[], now, packet).map(Some)
    }
}
