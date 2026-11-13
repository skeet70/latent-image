//! Test doubles for the image-4 services the adjustment layer reads.

use std::collections::BTreeMap;
use std::vec::Vec;

use crate::adjust::focus_profile::LensResources;
use crate::adjust::lens::{BiasTable, LensConfiguration, LensControl, ReferenceScale};

/// Lens services by table; the selection word records its arguments and returns the offset, and
/// `remap` adds the mode to the word.
#[derive(Default)]
pub struct Lens {
    pub values: BTreeMap<i32, u32>,
    pub curves: BTreeMap<i32, u32>,
    pub biases: BTreeMap<(i32, u8), i16>,
    pub positions: BTreeMap<(i32, u8), i16>,
    pub count: u8,
    pub limits: [i16; 2],
    pub mapped: u32,
    pub flags: u32,
    pub reference: u32,
    pub position: u16,
    pub margin: u16,
    pub selections: Vec<(i32, u32, i16)>,
}

fn table(table: BiasTable) -> u8 {
    match table {
        BiasTable::Area => 0,
        BiasTable::Descending => 1,
        BiasTable::Ascending => 2,
    }
}

impl LensConfiguration for Lens {
    fn channel_value(&mut self, channel: i32) -> u32 { self.values.get(&channel).copied().unwrap_or(0) }
    fn channel_curve(&mut self, channel: i32) -> u32 { self.curves.get(&channel).copied().unwrap_or(0) }
    fn bias(&mut self, resource: i32, kind: BiasTable) -> i16 { self.biases.get(&(resource, table(kind))).copied().unwrap_or(0) }
    fn biased_position(&mut self, resource: i32, kind: BiasTable) -> i16 { self.positions.get(&(resource, table(kind))).copied().unwrap_or(0) }
    fn resource_count(&mut self) -> u8 { self.count }
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32 {
        self.selections.push((channel, flags, offset));
        offset as u16 as u32
    }
    fn remap(&mut self, word: u32, _channel: i32, mode: u32) -> i32 { word as i32 + mode as i32 }
    fn position_limits(&mut self, _key: u32) -> [i16; 2] { self.limits }
    fn mapped_value(&mut self, _key: u32) -> u32 { self.mapped }
}

impl LensControl for Lens {
    fn position(&mut self) -> u16 { self.position }
    fn travel_margin(&mut self) -> u16 { self.margin }
}

impl LensResources for Lens {
    fn resource(&mut self) -> Option<u32> { None }
    fn reference(&mut self) -> u32 { self.reference }
    fn flags(&mut self) -> u32 { self.flags }
    fn channel_bias(&mut self, _resource: u32) -> i32 { 0 }
}

impl ReferenceScale for Lens {
    fn reference_scale(&mut self, group: u16, entry: u8) -> u32 { u32::from(group) * u32::from(entry) }
}

