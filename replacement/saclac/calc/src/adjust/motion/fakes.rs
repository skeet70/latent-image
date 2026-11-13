//! A bench for the motion layer: configuration values by id, drive limits, and stock memory that
//! keeps its stores in order.

use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::drive::{DriveKind, DriveLimits};
use crate::exposure::ports::{Properties, Property, StockMemory};

#[derive(Default)]
pub struct Bench {
    pub properties: BTreeMap<u32, u32>,
    pub first_ramp_speed: u32,
    pub first_ramp_acceleration: u32,
    pub second_ramp_acceleration: u32,
    pub slowest_ramped_speed: u32,
    /// The kinds the limits were read for.
    pub kinds: Vec<DriveKind>,
    pub memory: BTreeMap<u32, u32>,
    /// Every word store, in order: address and word.
    pub stores: Vec<(u32, u32)>,
    /// Every fill, in order: address, bytes and value.
    pub fills: Vec<(u32, usize, u8)>,
}

impl Bench {
    pub fn property(&mut self, id: Property, value: u32) { self.properties.insert(id.0, value); }

    pub fn word(&self, at: SharedAddress) -> Option<u32> { self.memory.get(&at.0).copied() }
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { self.properties.get(&property.0).copied().unwrap_or(0) }
}

impl DriveLimits for Bench {
    fn first_ramp_speed(&mut self, kind: DriveKind) -> u32 {
        self.kinds.push(kind);
        self.first_ramp_speed
    }
    fn first_ramp_acceleration(&mut self, kind: DriveKind) -> u32 {
        self.kinds.push(kind);
        self.first_ramp_acceleration
    }
    fn second_ramp_acceleration(&mut self, kind: DriveKind) -> u32 {
        self.kinds.push(kind);
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
