//! X-M1 1.50 secondary BSS event CPU. Instruction evidence is in the adjacent evidence packet.

use core::borrow::Borrow;
use crate::vendor_wireless_peers::{NetIndex, PeerRecord, PeerState, PoolError};

#[path = "vendor_wireless_secondary_wire.rs"]
pub mod wire;
#[path = "vendor_wireless_secondary_runtime.rs"]
pub mod runtime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Truncated, WrongBss, TooManySchedules, Busy, WrongCompletion, Device(DeviceFailure), InvalidAbortMode, WrongChannel, BeaconTooLarge, MissingPeer, MissingProjection, NetworkNameTooLong, PeerPool(PoolError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceFailure(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleFlags(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareTime(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareDuration(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleEntry { pub flags: ScheduleFlags, pub count: u8, pub duration: FirmwareDuration, pub interval: FirmwareDuration, pub start: FirmwareTime }
const EMPTY_ENTRY: ScheduleEntry = ScheduleEntry { flags: ScheduleFlags(0), count: 0, duration: FirmwareDuration(0), interval: FirmwareDuration(0), start: FirmwareTime(0) };
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Schedule { pub flags: ScheduleFlags, pub interval: u16, pub index: u8, count: u8, entries: [ScheduleEntry; 8], pub combined_flags: ScheduleFlags }
impl Schedule {
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        if body.len() < 8 { return Err(Error::Truncated); }
        if body[0] != 1 { return Err(Error::WrongBss); }
        let count = body[7];
        if count > 8 { return Err(Error::TooManySchedules); }
        if body.len() < 8 + usize::from(count) * 16 { return Err(Error::Truncated); }
        let mut result = Self { flags: ScheduleFlags(body[3]), interval: u16::from_le_bytes([body[4], body[5]]), index: body[6], count, entries: [EMPTY_ENTRY; 8], combined_flags: ScheduleFlags(body[3]) };
        for (entry, wire) in result.entries[..usize::from(count)].iter_mut().zip(body[8..].chunks_exact(16)) {
            *entry = ScheduleEntry { flags: ScheduleFlags(wire[0]), count: wire[1], duration: FirmwareDuration(word(wire, 4)), interval: FirmwareDuration(word(wire, 8)), start: FirmwareTime(word(wire, 12)) };
            result.combined_flags.0 |= entry.flags.0;
        }
        Ok(result)
    }
    pub fn entries(&self) -> &[ScheduleEntry] { &self.entries[..usize::from(self.count)] }
}
fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset+1], bytes[offset+2], bytes[offset+3]]) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootUpdate { pub index: u8, pub first: u8, pub second: u8, pub count: u8, pub coexistence_blocked: bool }
impl RootUpdate {
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        if body.len() < 6 { return Err(Error::Truncated); }
        if body[0] != 1 { return Err(Error::WrongBss); }
        Ok(Self { index: body[1], first: body[2], second: body[3], count: body[4], coexistence_blocked: body[5] != 0 })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCapabilities { pub active: bool, pub associated: bool, pub secondary: bool, pub phy: u8, pub ht: u16, pub capability: u16 }
impl From<&PeerRecord> for PeerCapabilities {
    fn from(peer: &PeerRecord) -> Self { Self { active: !peer.retiring(), associated: peer.state() == PeerState::Associated, secondary: peer.net() == NetIndex::SECONDARY, phy: peer.phy.0, ht: peer.ht.0, capability: peer.capability.0 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protection { None, Mixed, Legacy }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerProjection { pub non_erp: bool, pub protection: Protection, pub non_greenfield: bool, pub secondary_offset: u8, pub short_preamble: bool, pub short_slot: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BssMode { Client, Independent, AccessPoint, Device }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BeaconStorage { Absent, Present }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BssContext { pub mode: BssMode, pub active: bool, pub short_preamble_allowed: bool, pub secondary_offset: u8, pub coexistence_forced: bool, pub connected: bool, pub has_peer: bool, pub beacon: BeaconStorage }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation { ApplyBss, PublishBeacon, DisconnectNotification, RemovePeer, DisconnectInterface, FlushInterface }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect { ApplyBss(PeerProjection), PublishBeacon, DisconnectNotification { reason: u16 }, RemovePeer { reason: u16 }, DisconnectInterface, FlushInterface }
impl Effect { pub const fn operation(self) -> Operation { match self { Self::ApplyBss(_) => Operation::ApplyBss, Self::PublishBeacon => Operation::PublishBeacon, Self::DisconnectNotification {..} => Operation::DisconnectNotification, Self::RemovePeer {..} => Operation::RemovePeer, Self::DisconnectInterface => Operation::DisconnectInterface, Self::FlushInterface => Operation::FlushInterface } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending { Idle, Apply, Beacon, Notify, Remove, Disconnect, Flush }
pub struct SecondaryBss { pub context: BssContext, schedule: Option<Schedule>, root: Option<RootUpdate>, projection: Option<PeerProjection>, pending: Pending, failure: Option<DeviceFailure> }
impl SecondaryBss {
    pub const fn new(context: BssContext, initial: PeerProjection) -> Self { Self { context, schedule: None, root: None, projection: Some(initial), pending: Pending::Idle, failure: None } }
    pub fn schedule(&self) -> Option<&Schedule> { self.schedule.as_ref() }
    pub fn root(&self) -> Option<&RootUpdate> { self.root.as_ref() }
    pub fn projection(&self) -> Option<PeerProjection> { self.projection }
    fn admit(&self) -> Result<(), Error> { if self.pending != Pending::Idle { Err(Error::Busy) } else { Ok(()) } }
    pub fn receive_schedule(&mut self, body: &[u8]) -> Result<(), Error> { self.admit()?; let schedule = Schedule::parse(body)?; self.schedule = Some(schedule); if self.context.beacon == BeaconStorage::Present { self.pending = Pending::Beacon; } Ok(()) }
    pub fn receive_root<I>(&mut self, body: &[u8], peers: I) -> Result<(), Error> where I: IntoIterator, I::Item: Borrow<PeerCapabilities> {
        self.admit()?;
        let update = RootUpdate::parse(body)?;
        if self.context.mode != BssMode::AccessPoint { return Ok(()); }
        self.root = Some(update);
        self.recompute(peers, true)
    }
    pub fn recompute<I>(&mut self, peers: I, force_beacon: bool) -> Result<(), Error> where I: IntoIterator, I::Item: Borrow<PeerCapabilities> {
        self.admit()?;
        if !self.context.active || self.context.mode != BssMode::AccessPoint { return Ok(()); }
        let mut next = PeerProjection { non_erp: false, protection: Protection::None, non_greenfield: false, secondary_offset: 0, short_preamble: self.context.short_preamble_allowed, short_slot: true };
        let mut intolerant = false;
        for peer in peers {
            let peer = peer.borrow();
            if !peer.active || !peer.associated || !peer.secondary { continue; }
            next.non_erp |= peer.phy & 0x1a == 0;
            if peer.phy & 0x10 == 0 { next.protection = Protection::Legacy; }
            else if peer.ht & 2 == 0 && next.protection == Protection::None { next.protection = Protection::Mixed; }
            next.non_greenfield |= peer.ht & 0x10 == 0;
            next.short_preamble &= peer.capability & 0x20 != 0;
            next.short_slot &= peer.capability & 0x400 != 0;
            intolerant |= peer.ht & 0x4000 != 0;
        }
        if self.context.secondary_offset != 0 && !intolerant && !self.context.coexistence_forced && !self.root.is_some_and(|r| r.coexistence_blocked) { next.secondary_offset = self.context.secondary_offset | 4; }
        if self.projection != Some(next) { self.projection = Some(next); self.pending = Pending::Apply; }
        else if force_beacon && self.context.beacon == BeaconStorage::Present { self.pending = Pending::Beacon; }
        Ok(())
    }
    pub fn abort(&mut self) -> Result<(), Error> {
        self.admit()?;
        if self.context.mode != BssMode::Client { return Err(Error::InvalidAbortMode); }
        if self.context.connected { self.pending = Pending::Notify; }
        Ok(())
    }
    pub fn poll(&self) -> Option<Effect> {
        if self.failure.is_some() { return None; }
        match self.pending { Pending::Idle => None, Pending::Apply => self.projection.map(Effect::ApplyBss), Pending::Beacon => Some(Effect::PublishBeacon), Pending::Notify => Some(Effect::DisconnectNotification { reason: 4 }), Pending::Remove => Some(Effect::RemovePeer { reason: 4 }), Pending::Disconnect => Some(Effect::DisconnectInterface), Pending::Flush => Some(Effect::FlushInterface) }
    }
    pub fn complete(&mut self, operation: Operation, result: Result<(), DeviceFailure>) -> Result<(), Error> {
        if self.poll().map(Effect::operation) != Some(operation) { return Err(Error::WrongCompletion); }
        if let Err(failure) = result { self.failure = Some(failure); return Err(Error::Device(failure)); }
        self.pending = match self.pending {
            Pending::Apply if self.context.beacon == BeaconStorage::Present => Pending::Beacon,
            Pending::Notify if self.context.has_peer => Pending::Remove,
            Pending::Remove => { self.context.has_peer = false; self.context.connected = false; Pending::Disconnect },
            Pending::Disconnect => Pending::Flush,
            Pending::Flush => {
                self.context.secondary_offset = 0;
                self.context.coexistence_forced = false;
                self.root = None;
                if let Some(projection) = self.projection.as_mut() { projection.non_erp = false; projection.protection = Protection::None; projection.non_greenfield = false; projection.secondary_offset = 0; }
                Pending::Idle
            },
            _ => Pending::Idle,
        };
        Ok(())
    }
    pub fn retry(&mut self) -> Option<DeviceFailure> { self.failure.take() }
    pub fn failure(&self) -> Option<DeviceFailure> { self.failure }
}

#[cfg(test)]
#[path = "vendor_wireless_secondary_tests.rs"]
mod tests;
