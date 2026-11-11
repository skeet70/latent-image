use super::{AccessPoint, Error, MacAddress, Peer, PeerEntry, Security, State};
use super::beacon::capabilities;
use crate::vendor_wireless_station_driver::association::frames::{Elements, Management, Packet, half};
use crate::vendor_wireless_access_point_authenticator::{self as auth, Milliseconds};
use crate::vendor_wireless_peers::{PeerPool, PeerLease, PeerState, SignalStrength};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagementId(u64);
#[derive(Debug)]
pub struct ManagementTransmission { pub id: ManagementId, pub peer: MacAddress, pub packet: Packet }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationParameters {
    pub aid: u16, pub capabilities: u16, pub listen_interval: u16,
    pub rates: [u8; 32], pub rate_count: u8, pub ht: Option<[u8; 26]>, pub wmm: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerInstallation { pub id: ManagementId, pub peer: MacAddress, pub lease: PeerLease, pub parameters: StationParameters }
pub(super) struct PendingInstallation { pending: PendingManagement, lease: PeerLease, parameters: StationParameters, wps: bool }
impl PendingInstallation { pub(super) fn lease(&self) -> PeerLease { self.lease } }
#[derive(Clone, Copy)]
pub(super) enum Completion { Association { aid: u16, wps: bool, capabilities: u16, listen_interval: u16 }, None }
pub(super) struct PendingManagement {
    id: ManagementId, peer: MacAddress, signal: u8, completion: Completion, elements: Packet, now: Milliseconds,
}
impl AccessPoint {
    pub fn receive_management(&mut self, bytes: &[u8], signal: u8, now: Milliseconds, pool: &mut PeerPool) -> Result<Option<ManagementTransmission>, Error> {
        if self.state != State::Running { return Err(Error::NotRunning); }
        if self.management_pending.is_some() { return Err(Error::Busy); }
        if self.installation_pending.is_some() { return Err(Error::Busy); }
        if self.write - self.read > 2 { return Err(Error::Busy); }
        let frame = Management::parse(bytes).map_err(|_| Error::Management)?;
        if frame.source.0[0] & 1 != 0 || frame.source == self.local { return Err(Error::Management); }
        let broadcast = MacAddress([255; 6]);
        if frame.destination != self.local && frame.destination != broadcast { return Ok(None); }
        if frame.bssid != self.local && frame.bssid != broadcast { return Ok(None); }
        if frame.control & 0xfc != 0x40 && (frame.destination != self.local || frame.bssid != self.local) { return Ok(None); }
        match frame.control & 0xfc {
            0x40 => {
                let elements = Elements::parse(frame.body).map_err(|_| Error::Management)?;
                let requested = elements.get(0).ok_or(Error::Management)?;
                let configuration = self.configuration.as_ref().ok_or(Error::NotRunning)?;
                if !requested.is_empty() && requested != configuration.ssid.as_bytes() { return Ok(None); }
                let security = self.authenticator.as_ref().map(|authenticator| authenticator.information_element());
                let security_bytes = security.as_ref().map_or(&[][..], |element| element.as_bytes());
                let template = super::BeaconTemplate::build(self.local, configuration, self.hardware.as_ref().ok_or(Error::HardwareMissing)?,
                    self.protection, self.interval, security_bytes, self.wps_elements.as_bytes())?;
                let mut packet = Packet::new(); packet.header(0x50, frame.source, self.local, self.local);
                packet.append(&template.head.as_bytes()[24..]).map_err(|_| Error::Capacity)?;
                packet.append(template.tail.as_bytes()).map_err(|_| Error::Capacity)?;
                self.management_reply(frame.source, signal, Completion::None, &[], now, packet).map(Some)
            }
            0xb0 => {
                let algorithm = half(frame.body, 0).map_err(|_| Error::Truncated)?;
                let transaction = half(frame.body, 2).map_err(|_| Error::Truncated)?;
                half(frame.body, 4).map_err(|_| Error::Truncated)?;
                let mut status = if algorithm != 0 { 13u16 } else if transaction != 1 { 14 } else { 0 };
                if status == 0 {
                    if let Some(entry) = self.peers.iter_mut().flatten().find(|entry| entry.peer.address == frame.source) { entry.authenticated = true; }
                    else if let Some(slot) = self.peers.iter_mut().find(|slot| slot.is_none()) {
                        *slot = Some(PeerEntry { peer: Peer { address: frame.source, signal }, published: false, authenticated: true, associated: false,
                            aid: 0, lease: None, wps: false, deadline: now.0.checked_add(300_000).map(Milliseconds), expiry: super::PeerExpiry::FirstDisassociation });
                    } else { status = 17; }
                }
                let mut packet = Packet::new(); packet.header(0xb0, frame.source, self.local, self.local);
                packet.append(&algorithm.to_le_bytes()).map_err(|_| Error::Capacity)?;
                packet.append(&transaction.wrapping_add(1).to_le_bytes()).map_err(|_| Error::Capacity)?;
                packet.append(&status.to_le_bytes()).map_err(|_| Error::Capacity)?;
                self.management_reply(frame.source, signal, Completion::None, &[], now, packet).map(Some)
            }
            0 | 0x20 => {
                let reassociate = frame.control & 0xfc == 0x20;
                let offset = if reassociate { 10 } else { 4 };
                let peer_capabilities = half(frame.body, 0).map_err(|_| Error::Truncated)?;
                let listen_interval = half(frame.body, 2).map_err(|_| Error::Truncated)?;
                let ies = frame.body.get(offset..).ok_or(Error::Truncated)?;
                let elements = Elements::parse(ies).map_err(|_| Error::Management)?;
                let configuration = self.configuration.as_ref().ok_or(Error::NotRunning)?;
                let index = self.peers.iter().position(|entry| entry.as_ref().is_some_and(|entry| entry.peer.address == frame.source && entry.authenticated));
                let mut status = if index.is_some() { 0u16 } else { 9u16 };
                if elements.get(0) != Some(configuration.ssid.as_bytes()) { status = 1; }
                let supported = elements.get(1).filter(|rates| !rates.is_empty() && rates.len() <= 8);
                if supported.is_none() { status = 18; }
                let hardware = self.hardware.as_ref().ok_or(Error::HardwareMissing)?;
                if hardware.rates().iter().filter(|rate| rate.0 & 128 != 0).any(|rate| {
                    !supported.into_iter().flatten().chain(elements.get(50).into_iter().flatten()).any(|peer| peer & 127 == rate.0 & 127)
                }) { status = 18; }
                let wps = self.registrar.as_ref().is_some_and(|registrar| !matches!(registrar.state(),
                    crate::wireless_association::station::wps::registrar::RegistrarState::Idle |
                    crate::wireless_association::station::wps::registrar::RegistrarState::Finished |
                    crate::wireless_association::station::wps::registrar::RegistrarState::Failed))
                    && (elements.vendor([0,80,242,4]).is_some() || elements.get(48).is_none() && elements.vendor([0,80,242,1]).is_none());
                if !wps {
                    let negotiated = match configuration.security {
                        Security::WpaTkip => auth::validate_association(auth::Protocol::Wpa, auth::Cipher::Tkip, ies).map(|_| ()),
                        Security::Wpa2Ccmp => auth::validate_association(auth::Protocol::Wpa2, auth::Cipher::Ccmp, ies).map(|_| ()),
                        _ => Ok(()),
                    };
                    if negotiated.is_err() { status = 40; }
                }
                let aid = index.map_or(0, |index| index as u16 + 1);
                let mut packet = Packet::new(); packet.header(if reassociate { 0x30 } else { 0x10 }, frame.source, self.local, self.local);
                packet.append(&capabilities(configuration, hardware, self.protection).to_le_bytes()).map_err(|_| Error::Capacity)?;
                packet.append(&status.to_le_bytes()).map_err(|_| Error::Capacity)?;
                packet.append(&(aid | 0xc000).to_le_bytes()).map_err(|_| Error::Capacity)?;
                let mut rates = [0; 32]; for (out, rate) in rates.iter_mut().zip(hardware.rates()) { *out = rate.0; }
                packet.element(1, &rates[..hardware.rates().len().min(8)]).map_err(|_| Error::Capacity)?;
                if hardware.rates().len() > 8 { packet.element(50, &rates[8..hardware.rates().len()]).map_err(|_| Error::Capacity)?; }
                self.management_reply(frame.source, signal, if status == 0 { Completion::Association { aid, wps, capabilities: peer_capabilities, listen_interval } } else { Completion::None }, ies, now, packet).map(Some)
            }
            0xa0 | 0xc0 => { half(frame.body, 0).map_err(|_| Error::Truncated)?; self.peer_disconnected(frame.source, pool)?; Ok(None) }
            _ => Ok(None),
        }
    }
    pub(super) fn management_reply(&mut self, peer: MacAddress, signal: u8, completion: Completion, ies: &[u8], now: Milliseconds, packet: Packet) -> Result<ManagementTransmission, Error> {
        self.management_serial = self.management_serial.checked_add(1).ok_or(Error::Capacity)?;
        let id = ManagementId(self.management_serial);
        let mut elements = Packet::new(); elements.append(ies).map_err(|_| Error::Capacity)?;
        self.management_pending = Some(PendingManagement { id, peer, signal, completion, elements, now });
        Ok(ManagementTransmission { id, peer, packet })
    }
    pub fn complete_management(&mut self, id: ManagementId, acknowledged: bool, pool: &mut PeerPool) -> Result<Option<PeerInstallation>, Error> {
        if self.management_pending.as_ref().map(|pending| pending.id) != Some(id) { return Err(Error::UnexpectedCompletion); }
        let pending = self.management_pending.take().ok_or(Error::UnexpectedCompletion)?;
        if self.state != State::Running || !acknowledged { return Ok(None); }
        match pending.completion {
            Completion::Association { aid, wps, capabilities, listen_interval } => {
                let elements = Elements::parse(pending.elements.as_bytes()).map_err(|_| Error::Management)?;
                let mut parameters = StationParameters { aid, capabilities, listen_interval, rates: [0; 32], rate_count: 0,
                    ht: elements.get(45).map(|bytes| bytes.try_into().map_err(|_| Error::Management)).transpose()?,
                    wmm: elements.vendor([0,80,242,2]).is_some() };
                for rate in elements.get(1).into_iter().flatten().chain(elements.get(50).into_iter().flatten()) {
                    if parameters.rate_count == 32 { return Err(Error::Capacity); }
                    parameters.rates[usize::from(parameters.rate_count)] = *rate; parameters.rate_count += 1;
                }
                let lease = match pool.lookup(self.net, pending.peer) {
                    Some(lease) => { pool.get_mut(lease).map_err(|_| Error::PeerLease)?; lease }
                    None => pool.reserve(self.net, pending.peer).map_err(|_| Error::PeerCapacity)?,
                };
                pool.set_state(lease, PeerState::Associated).map_err(|_| Error::PeerLease)?;
                let installation = PeerInstallation { id, peer: pending.peer, lease, parameters };
                self.installation_pending = Some(PendingInstallation { pending, lease, parameters, wps });
                return Ok(Some(installation));
            }
            Completion::None => {}
        }
        Ok(None)
    }
    pub fn receive_peer_frame(&mut self, bytes: &[u8], signal: u8, pool: &mut PeerPool) -> Result<(), Error> {
        if self.state != State::Running { return Err(Error::NotRunning); }
        if bytes.len() < 24 { return Err(Error::Truncated); }
        let control = u16::from_le_bytes([bytes[0], bytes[1]]);
        if control & 0x030c != 0x0108 || bytes[4..10] != self.local.0 { return Err(Error::Management); }
        let address = MacAddress(bytes[10..16].try_into().map_err(|_| Error::Truncated)?);
        let entry = self.peers.iter_mut().flatten().find(|entry| entry.peer.address == address && entry.associated).ok_or(Error::UnknownPeer)?;
        pool.get_mut(entry.lease.ok_or(Error::PeerLease)?).map_err(|_| Error::PeerLease)?.signal_strength = Some(SignalStrength(signal));
        entry.peer.signal = signal; Ok(())
    }
    pub fn complete_peer_install(&mut self, id: ManagementId, success: bool, pool: &mut PeerPool) -> Result<(), Error> {
        if self.installation_pending.as_ref().map(|installation| installation.pending.id) != Some(id) { return Err(Error::UnexpectedCompletion); }
        let installation = self.installation_pending.take().ok_or(Error::UnexpectedCompletion)?;
        let pending = installation.pending;
        if !success || self.state != State::Running {
            pool.release(installation.lease).map_err(|_| Error::PeerLease)?;
            return Err(Error::Driver(super::DriverError(-1)));
        }
        pool.complete_association(installation.lease, pending.peer).map_err(|_| Error::PeerLease)?;
        pool.get_mut(installation.lease).map_err(|_| Error::PeerLease)?.signal_strength = Some(SignalStrength(pending.signal));
        if !installation.wps {
            if let Some(authenticator) = self.authenticator.as_mut() {
                authenticator.associated(pending.peer, pending.elements.as_bytes(), pending.now).map_err(|_| Error::Security)?;
            }
        }
        let entry = self.peers.iter_mut().flatten().find(|entry| entry.peer.address == pending.peer).ok_or(Error::UnknownPeer)?;
        entry.associated = true; entry.aid = installation.parameters.aid; entry.peer.signal = pending.signal;
        entry.lease = Some(installation.lease); entry.wps = installation.wps;
        entry.deadline = pending.now.0.checked_add(300_000).map(Milliseconds);
        self.protection.long_preamble |= installation.parameters.capabilities & 32 == 0;
        self.protection.no_short_slot |= installation.parameters.capabilities & 1024 == 0;
        self.beacon_dirty = true;
        if !installation.wps && self.authenticator.is_none() { self.authorized(pending.peer, pool)?; }
        Ok(())
    }
}
