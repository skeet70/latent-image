//! A bench for the motion layer: configuration values by id, drive limits, lens tables, a clock,
//! and stock memory that keeps its stores in order.

use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::drive::{DriveKind, DriveLimits, DriveMode};
use crate::adjust::motion::ports::{Clock, LensTables, Ticks};
use crate::exposure::ports::{Properties, Property, StockMemory};

#[derive(Default)]
pub struct Bench {
    pub properties: BTreeMap<u32, u32>,
    pub first_ramp_speed: u32,
    pub first_ramp_acceleration: u32,
    pub second_ramp_acceleration: u32,
    pub slowest_ramped_speed: u32,
    /// What the limits were read for.
    pub kinds: Vec<(DriveKind, DriveMode)>,
    /// Returned for every lens, step and reference.
    pub channel_position: i16,
    /// Added for a change of side from 1 to 0.
    pub side_offset: i16,
    pub now: u32,
    pub memory: BTreeMap<u32, u32>,
    /// Every word store, in order: address and word.
    pub stores: Vec<(u32, u32)>,
    /// Every fill, in order: address, bytes and value.
    pub fills: Vec<(u32, usize, u8)>,
}

impl Bench {
    pub fn property(&mut self, id: Property, value: u32) { self.properties.insert(id.0, value); }

    /// A two-byte configuration value, low byte first.
    pub fn property_word(&mut self, id: Property, value: u16) {
        self.property(id, u32::from(value as u8));
        self.property(id.high(), u32::from(value >> 8));
    }

    pub fn word(&self, at: SharedAddress) -> Option<u32> { self.memory.get(&at.0).copied() }

    pub fn put(&mut self, at: SharedAddress, value: u32) { self.memory.insert(at.0, value); }
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { self.properties.get(&property.0).copied().unwrap_or(0) }
}

/// Offsets are ten per step; channel starts and period positions are 0.
impl LensTables for Bench {
    fn channel_positions(&mut self, _lens: u32, _step: u32, _reference: u32, _mode: u32) -> [i16; 2] { [self.channel_position, 0] }
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16 {
        if (from, to) == (1, 0) { value + self.side_offset } else { value }
    }
    fn table_offset(&mut self, _lens: u32, _step: u32, delta: i16) -> i32 { i32::from(delta) * 10 }
    fn channel_start(&mut self, _lens: u32, _channel: u8, _reference: u32) -> i16 { 0 }
    fn period_position(&mut self, _period: u16, _lens: u32, _step: u32, _reference: u32) -> i32 { 0 }
}

impl Clock for Bench {
    fn now(&mut self) -> Ticks { Ticks(self.now) }
    fn elapsed(&mut self, since: Ticks) -> u32 { (self.now - since.0) * 10 }
}

impl DriveLimits for Bench {
    fn first_ramp_speed(&mut self, kind: DriveKind, mode: DriveMode) -> u32 {
        self.kinds.push((kind, mode));
        self.first_ramp_speed
    }
    fn first_ramp_acceleration(&mut self, kind: DriveKind, mode: DriveMode) -> u32 {
        self.kinds.push((kind, mode));
        self.first_ramp_acceleration
    }
    fn second_ramp_acceleration(&mut self, kind: DriveKind, mode: DriveMode) -> u32 {
        self.kinds.push((kind, mode));
        self.second_ramp_acceleration
    }
    fn slowest_ramped_speed(&mut self) -> u32 { self.slowest_ramped_speed }
}

impl StockMemory for Bench {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.memory.get(&at.0).copied().unwrap_or(0) as u8 }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.memory.get(&at.0).copied().unwrap_or(0) as u16 }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.memory.get(&at.0).copied().unwrap_or(0) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.write_u32(at, u32::from(value)) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.write_u32(at, u32::from(value)) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) {
        self.stores.push((at.0, value));
        self.memory.insert(at.0, value);
    }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { self.fills.push((at.0, bytes, value)) }
}
