//! X-M1 1.50 firmware-event state consumers. Evidence is in the adjacent JSON file.

use super::{FirmwareEvent, FirmwareEventKind};
use super::reorder::{BlockAckWindow, DeleteMode, ReorderEngine, ReorderError, SequenceNumber, Tid};
use crate::vendor_wireless_peers::{PeerIndex, PeerPool};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockAckError { Packet(EventError), InvalidTid, Reorder(ReorderError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockAckDisposition { Added, Deleted, UnknownPeer }

pub fn receive_block_ack<P, const N: usize>(
    event: &FirmwareEvent<'_>,
    peers: &PeerPool,
    reorder: &mut ReorderEngine<P, N>,
) -> Result<BlockAckDisposition, BlockAckError> {
    if !matches!(event.kind(), FirmwareEventKind::AddBlockAck | FirmwareEventKind::DeleteBlockAck) {
        return Err(BlockAckError::Packet(EventError::WrongKind));
    }
    let body = event.body();
    require(body, 1).map_err(BlockAckError::Packet)?;
    let Some(peer) = PeerIndex::new(body[0]).and_then(|index| peers.lease_for_index(index))
        else { return Ok(BlockAckDisposition::UnknownPeer); };
    match event.kind() {
        FirmwareEventKind::AddBlockAck => {
            require(body, 8).map_err(BlockAckError::Packet)?;
            let parameter = u16::from_le_bytes([body[2], body[3]]);
            let tid = Tid::new(((parameter & 0x3c) >> 2) as u8).ok_or(BlockAckError::InvalidTid)?;
            let sequence = SequenceNumber::new(u16::from_le_bytes([body[6], body[7]]) >> 4).unwrap();
            let window = BlockAckWindow::new(parameter >> 6).unwrap();
            reorder.add(peer, tid, sequence, window).map_err(BlockAckError::Reorder)?;
            Ok(BlockAckDisposition::Added)
        }
        FirmwareEventKind::DeleteBlockAck => {
            require(body, 2).map_err(BlockAckError::Packet)?;
            let tid = Tid::new(body[1]).ok_or(BlockAckError::InvalidTid)?;
            reorder.delete(peer, tid, DeleteMode::Publish);
            Ok(BlockAckDisposition::Deleted)
        }
        _ => Err(BlockAckError::Packet(EventError::WrongKind)),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventTick(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalDbm(pub i8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkSpeed(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalTrigger { Below(SignalDbm), Above(SignalDbm) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalSample { pub signal: SignalDbm, pub noise: SignalDbm, pub tick: EventTick }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Statistics { pub bytes: [u8; 96], pub tick: EventTick }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventError { Truncated, WrongKind, InvalidQuotaOperation, OutputTooSmall, NotLocalEvent }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventOutcome {
    Signal { refreshed: bool, threshold_crossed: Option<SignalDbm> },
    Statistics,
    Network,
    Sleep,
    Service,
}

pub struct EventCpu {
    signal: Option<SignalSample>,
    speed: Option<(LinkSpeed, EventTick)>,
    statistics: Option<Statistics>,
    threshold: Option<SignalTrigger>,
    sleepy: u8,
    network: [u8; 0x930],
    network_len: usize,
}
impl EventCpu {
    pub const fn new() -> Self {
        Self { signal: None, speed: None, statistics: None, threshold: None, sleepy: 0,
            network: [0; 0x930], network_len: 0 }
    }
    pub fn signal(&self) -> Option<SignalSample> { self.signal }
    pub fn speed(&self) -> Option<LinkSpeed> { self.speed.map(|entry| entry.0) }
    pub fn statistics(&self) -> Option<&Statistics> { self.statistics.as_ref() }
    pub fn sleepy(&self) -> u8 { self.sleepy }
    pub fn arm_signal_trigger(&mut self, trigger: SignalTrigger) { self.threshold = Some(trigger); }
    pub fn cancel_signal_trigger(&mut self) { self.threshold = None; }
    pub fn network_notification(&self) -> Option<&[u8]> {
        (self.network_len != 0).then_some(&self.network[..self.network_len])
    }
    pub fn reset(&mut self) { *self = Self::new(); }

    pub fn receive(&mut self, event: &FirmwareEvent<'_>, now: EventTick) -> Result<EventOutcome, EventError> {
        let body = event.body();
        match event.kind() {
            FirmwareEventKind::Signal => {
                require(body, 4)?;
                let refreshed = self.signal.is_none_or(|sample| now.0.wrapping_sub(sample.tick.0) > 5000);
                if refreshed {
                    self.signal = Some(SignalSample { signal: SignalDbm(body[0] as i8), noise: SignalDbm(body[1] as i8), tick: now });
                }
                if self.speed.is_none_or(|sample| now.0.wrapping_sub(sample.1.0) > 5000) {
                    self.speed = Some((LinkSpeed(u16::from_le_bytes([body[2], body[3]])), now));
                }
                let signal = self.signal.map(|sample| sample.signal);
                let crossed = match (self.threshold, signal) {
                    (Some(SignalTrigger::Below(limit)), Some(value)) if value.0 <= limit.0 => Some(limit),
                    (Some(SignalTrigger::Above(limit)), Some(value)) if value.0 >= limit.0 => Some(limit),
                    _ => None,
                };
                if crossed.is_some() { self.threshold = None; }
                Ok(EventOutcome::Signal { refreshed, threshold_crossed: crossed })
            }
            FirmwareEventKind::Statistics | FirmwareEventKind::StatisticsExtended => {
                require(body, 96)?;
                let mut bytes = [0; 96];
                bytes.copy_from_slice(&body[..96]);
                self.statistics = Some(Statistics { bytes, tick: now });
                Ok(EventOutcome::Statistics)
            }
            FirmwareEventKind::Network => {
                if body.len() > self.network.len() - 4 { return Err(EventError::OutputTooSmall); }
                self.network[..4].copy_from_slice(&2u32.to_le_bytes());
                self.network[4..4 + body.len()].copy_from_slice(body);
                self.network_len = 4 + body.len();
                Ok(EventOutcome::Network)
            }
            FirmwareEventKind::Sleepy => {
                require(body, 1)?;
                self.sleepy = body[0];
                Ok(EventOutcome::Sleep)
            }
            // 0x012b2508 returns without reading its arguments.
            FirmwareEventKind::Service => Ok(EventOutcome::Service),
            _ => Err(EventError::NotLocalEvent),
        }
    }
}
impl Default for EventCpu { fn default() -> Self { Self::new() } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaOperation { Initialize, Overwrite, Increase, Decrease }
impl QuotaOperation {
    pub fn parse(value: u8) -> Result<Self, EventError> {
        match value { 0 => Ok(Self::Initialize), 1 => Ok(Self::Overwrite), 2 => Ok(Self::Increase),
            3 => Ok(Self::Decrease), _ => Err(EventError::InvalidQuotaOperation) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HardwarePeerPower {
    asleep: u8,
    total: u8,
    delivery: u8,
    nondelivery: u8,
    qos_enabled: bool,
    uapsd_enabled: bool,
}
impl HardwarePeerPower {
    pub const fn new(qos_enabled: bool, uapsd_enabled: bool) -> Self {
        Self { asleep: 0, total: 0, delivery: 0, nondelivery: 0, qos_enabled, uapsd_enabled }
    }
    pub fn set_capabilities(&mut self, qos_enabled: bool, uapsd_enabled: bool) {
        self.qos_enabled = qos_enabled;
        self.uapsd_enabled = uapsd_enabled;
    }
    pub fn asleep(&self) -> bool { self.asleep != 0 }
    pub fn quotas(&self) -> (u8, u8, u8) { (self.total, self.delivery, self.nondelivery) }
    pub fn receive(&mut self, event: &FirmwareEvent<'_>) -> Result<bool, EventError> {
        let body = event.body();
        match event.kind() {
            FirmwareEventKind::Event26 => {
                require(body, 4)?;
                let operation = QuotaOperation::parse(body[2])?;
                let was_asleep = self.asleep != 0;
                self.asleep = body[1];
                self.update(operation, body[3]);
                Ok(was_asleep && self.asleep == 0)
            }
            FirmwareEventKind::Event30 => {
                require(body, 3)?;
                let operation = QuotaOperation::parse(body[1])?;
                if self.asleep == 0 { return Ok(false); }
                self.update(operation, body[2]);
                Ok(true)
            }
            _ => Err(EventError::WrongKind),
        }
    }
    fn update(&mut self, operation: QuotaOperation, value: u8) {
        if self.asleep == 0 { return; }
        self.total = match operation {
            QuotaOperation::Initialize | QuotaOperation::Overwrite => value,
            QuotaOperation::Increase => self.total.wrapping_add(value),
            QuotaOperation::Decrease => self.total.wrapping_sub(value),
        };
        let total = self.total;
        let (delivery, nondelivery) = if total == 0 { (0, 0) }
            else if !self.qos_enabled || !self.uapsd_enabled { (0, total) }
            else if (self.delivery == 0) == (self.nondelivery == 0) { (total - total / 2, total / 2) }
            else if self.nondelivery == 0 { (total.saturating_sub(2), total.min(2)) }
            else { (total.min(2), total.saturating_sub(2)) };
        self.delivery = delivery;
        self.nondelivery = nondelivery;
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HardwareBssPower { absent: u8, free_quota: u8 }
impl HardwareBssPower {
    pub const fn new() -> Self { Self { absent: 0, free_quota: 0 } }
    pub fn absent(&self) -> bool { self.absent != 0 }
    pub fn free_quota(&self) -> u8 { self.free_quota }
    pub fn receive(&mut self, event: &FirmwareEvent<'_>) -> Result<bool, EventError> {
        if event.kind() != FirmwareEventKind::Event25 { return Err(EventError::WrongKind); }
        let body = event.body();
        require(body, 3)?;
        let was_absent = self.absent != 0;
        self.absent = body[1];
        self.free_quota = body[2];
        Ok(was_absent && self.absent == 0)
    }
}
impl Default for HardwareBssPower { fn default() -> Self { Self::new() } }
fn require(body: &[u8], length: usize) -> Result<(), EventError> {
    if body.len() < length { Err(EventError::Truncated) } else { Ok(()) }
}

#[cfg(test)]
#[path = "vendor_wireless_station_driver_receive_events_tests.rs"]
mod tests;
