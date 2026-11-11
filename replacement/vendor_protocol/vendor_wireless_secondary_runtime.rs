use super::{BssContext, BssMode, DeviceFailure, Effect, Error, FirmwareTime, Operation, PeerCapabilities, PeerProjection, SecondaryBss};
use super::wire::{Band, BeaconElements, BssParameters, Capability, InterfaceParameters};
use crate::vendor_wireless_adapter::FirmwareRequest;
use crate::vendor_wireless_peers::{NetIndex, PeerLease, PeerPool, PeerState};
use crate::vendor_wireless_station_driver::association::ibss::{PeerParameters, peer_record_body};
use crate::vendor_wireless_station_driver::receive::{FirmwareEvent, FirmwareEventKind};
use crate::vendor_wireless_station_driver::receive::reorder::{DeleteMode, ReorderEngine};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisconnectPublication { pub reason: u16 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbortAdmission { Allowed, Suppressed }
pub struct SecondaryRuntime {
    pub cpu: SecondaryBss,
    parameters: BssParameters,
    capability: Capability,
    interface: InterfaceParameters,
    interface_body: [u8; 80],
    flush_body: [u8; 4],
    peer: Option<PeerLease>,
    coexistence_deadline: Option<FirmwareTime>,
    bss_body: [u8; 16],
    beacon_body: [u8; 608],
    beacon_length: usize,
    peer_body: [u8; 40],
    prepared: Option<Operation>,
    notification: Option<DisconnectPublication>,
}
impl SecondaryRuntime {
    pub const fn new(mut context: BssContext, parameters: BssParameters, capability: Capability, interface: InterfaceParameters, initial: PeerProjection, peer: Option<PeerLease>) -> Self {
        context.has_peer = peer.is_some();
        Self { cpu: SecondaryBss::new(context, initial), parameters, capability, interface, interface_body: [0; 80], flush_body: [1, 0, 0, 0], peer, coexistence_deadline: None, bss_body: [0; 16], beacon_body: [0; 608], beacon_length: 0, peer_body: [0; 40], prepared: None, notification: None }
    }
    pub fn receive(&mut self, event: &FirmwareEvent<'_>, peers: &PeerPool, abort: AbortAdmission) -> Result<bool, Error> {
        match event.kind() {
            FirmwareEventKind::Event28 => self.receive_schedule(event.body())?,
            FirmwareEventKind::Event29 => self.receive_root(event.body(), peers)?,
            FirmwareEventKind::StationAbort if event.body().first() == Some(&1) && abort == AbortAdmission::Allowed => self.abort()?,
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub fn receive_schedule(&mut self, body: &[u8]) -> Result<(), Error> { self.cpu.receive_schedule(body) }
    pub fn receive_root(&mut self, body: &[u8], peers: &PeerPool) -> Result<(), Error> { self.cpu.receive_root(body, peers.iter().map(PeerCapabilities::from)) }
    pub fn peer_changed(&mut self, peers: &PeerPool) -> Result<(), Error> { self.cpu.recompute(peers.iter().map(PeerCapabilities::from), false) }
    pub fn abort(&mut self) -> Result<(), Error> { self.cpu.abort() }
    pub fn peer(&self) -> Option<PeerLease> { self.peer }
    pub fn notification(&self) -> Option<DisconnectPublication> { self.notification }
    pub fn take_notification(&mut self) -> Option<DisconnectPublication> { self.notification.take() }
    pub fn set_coexistence_deadline(&mut self, deadline: FirmwareTime) { self.coexistence_deadline = Some(deadline); }
    pub fn coexistence_deadline(&self) -> Option<FirmwareTime> { self.coexistence_deadline }
    pub fn prepare<P, const N: usize>(&mut self, beacon: BeaconElements<'_>, peers: &mut PeerPool, reorder: &mut ReorderEngine<P, N>, peer_parameters: Option<&PeerParameters>, rates: &[u8]) -> Result<(), Error> {
        if self.prepared.is_some() { return Err(Error::Busy); }
        loop {
            match self.cpu.poll() {
                Some(Effect::ApplyBss(projection)) => {
                    self.bss_body = self.parameters.encode(projection);
                    self.capability = self.capability.project(projection);
                    self.prepared = Some(Operation::ApplyBss);
                }
                Some(Effect::PublishBeacon) => {
                    let projection = self.cpu.projection().ok_or(Error::MissingProjection)?;
                    let include_obss = self.cpu.context.mode == BssMode::AccessPoint && self.interface.phy & 0x10 != 0 && self.parameters.band == Band::Ghz2 && self.parameters.configured_secondary != 0;
                    self.beacon_length = beacon.encode_projected(self.capability, self.parameters, projection, self.cpu.root().copied(), include_obss, &mut self.beacon_body)?;
                    self.prepared = Some(Operation::PublishBeacon);
                }
                Some(Effect::DisconnectNotification { reason }) => {
                    if self.notification.is_some() { return Err(Error::Busy); }
                    self.notification = Some(DisconnectPublication { reason });
                    self.cpu.complete(Operation::DisconnectNotification, Ok(()))?;
                    continue;
                }
                Some(Effect::RemovePeer { .. }) => {
                    let lease = self.peer.ok_or(Error::MissingPeer)?;
                    let peer = peers.get(lease).map_err(Error::PeerPool)?;
                    if peer.net() != NetIndex::SECONDARY { return Err(Error::WrongBss); }
                    let local_only = peer.state() == PeerState::Authenticated;
                    let body = if local_only { None } else { Some(peer_record_body(peer.net(), peer.mac(), rates, lease.index(), PeerState::Disconnected, peer_parameters.ok_or(Error::MissingPeer)?)) };
                    let transition = peers.set_state(lease, PeerState::Disconnected).map_err(Error::PeerPool)?;
                    if transition.clear_hardware_queues { reorder.teardown_peer(lease, DeleteMode::Cancel); }
                    peers.clear_secondary_session(lease).map_err(Error::PeerPool)?;
                    if let Some(body) = body { self.peer_body = body; self.prepared = Some(Operation::RemovePeer); }
                    else { self.cpu.complete(Operation::RemovePeer, Ok(()))?; continue; }
                }
                Some(Effect::DisconnectInterface) => {
                    let projection = self.cpu.projection().ok_or(Error::MissingProjection)?;
                    self.interface_body = self.interface.encode_disconnected(self.parameters, projection, self.peer.ok_or(Error::MissingPeer)?.index());
                    self.prepared = Some(Operation::DisconnectInterface);
                }
                Some(Effect::FlushInterface) => { self.prepared = Some(Operation::FlushInterface); }
                None => {}
            }
            return Ok(());
        }
    }
    pub fn command(&self) -> Option<FirmwareRequest<'_>> {
        if self.cpu.failure().is_some() { return None; }
        match self.prepared {
            Some(Operation::ApplyBss) => Some(FirmwareRequest::BssParameters(&self.bss_body)),
            Some(Operation::PublishBeacon) => Some(FirmwareRequest::BeaconTemplate(&self.beacon_body[..self.beacon_length])),
            Some(Operation::RemovePeer) => Some(FirmwareRequest::PeerParameters(&self.peer_body)),
            Some(Operation::DisconnectInterface) => Some(FirmwareRequest::InterfaceParameters(&self.interface_body)),
            Some(Operation::FlushInterface) => Some(FirmwareRequest::FlushInterface(&self.flush_body)),
            _ => None,
        }
    }
    pub fn finish_command(&mut self, result: Result<(), DeviceFailure>, peers: &mut PeerPool) -> Result<(), Error> {
        let operation = self.prepared.ok_or(Error::WrongCompletion)?;
        if operation == Operation::FlushInterface && result.is_ok() { peers.get(self.peer.ok_or(Error::MissingPeer)?).map_err(Error::PeerPool)?; }
        self.cpu.complete(operation, result)?;
        self.prepared = None;
        if operation == Operation::FlushInterface {
            self.parameters.configured_secondary = 0;
            self.parameters.rifs = 0;
            self.parameters.ht_info2 = 0;
            self.parameters.ht_info3 = 0;
            self.coexistence_deadline = None;
            peers.release(self.peer.take().ok_or(Error::MissingPeer)?).map_err(Error::PeerPool)?;
        }
        Ok(())
    }
    pub fn retry(&mut self) -> Option<DeviceFailure> { self.cpu.retry() }
}
