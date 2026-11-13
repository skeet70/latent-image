//! Test doubles for the image-4 services the adjustment layer reads.

use std::collections::BTreeMap;
use std::vec::Vec;

use crate::adjust::exposure_limits::{CellLevels, ChannelLevels};
use crate::adjust::focus_profile::LensResources;
use crate::adjust::lens::{BiasTable, LensConfiguration, LensControl, ReferenceScale};
use crate::adjust::profile_adjustment::{LensSteps, StepTable};
use crate::exposure::fakes::World;
use crate::exposure::ports::{CaptureProfiles, Properties, Property};
use crate::scene_levels::FeatureClasses;
use crate::stock::calculation::Cell;

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

/// The exposure limits' services over a `World`. A lens limit of `l` tenths reads step `l / 10`;
/// no limit reads the near or far step given here. The other layers' services are formulas: a
/// channel's level is 100 per channel less 30 with its class offset, every feature class is 0, a
/// compensated level is seven per step plus the reference, and a cell row's level is 100 plus ten
/// per row.
pub struct LimitWorld {
    pub world: World,
    pub unlimited_near: u32,
    pub unlimited_far: u32,
}

impl Default for LimitWorld {
    fn default() -> Self { Self { world: World::default(), unlimited_near: 0, unlimited_far: 20 } }
}

impl LimitWorld {
    pub fn set(&mut self, id: u32, value: u32) { self.world.property(Property(id), value); }

    pub fn set_word(&mut self, id: u32, value: u16) { self.world.property_word(Property(id), value); }

    /// Base level `100 * step` for every step up to `steps`.
    pub fn even_base_levels(&mut self, steps: u32) {
        for step in 0..=steps {
            self.world.lens_base_levels.insert(step, 100 * step as i16);
        }
    }
}

impl Properties for LimitWorld {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl crate::exposure::ports::Lens for LimitWorld {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.world.level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.world.base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.world.reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.world.calibration_index(lens) }
}

impl CaptureProfiles for LimitWorld {
    fn level_offset(&mut self, profile: u32) -> i16 { self.world.level_offset(profile) }
    fn class_offset(&mut self, class: u8) -> i16 { self.world.class_offset(class) }
    fn offset_class(&mut self, profile: u32) -> u8 { self.world.offset_class(profile) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { self.world.sensitivity_class(profile) }
    fn sensitivity_class_for(&mut self, value: u32) -> u8 { self.world.sensitivity_class_for(value) }
    fn configured_level(&mut self) -> u16 { self.world.configured_level() }
    fn configured_profile(&mut self, level: i16) -> u32 { self.world.configured_profile(level) }
    fn record_profile(&mut self) -> u32 { self.world.record_profile() }
    fn default_dimensions(&mut self) -> [u16; 3] { self.world.default_dimensions() }
    fn grid_steps(&mut self, layout: u8) -> [u8; 2] { self.world.grid_steps(layout) }
}

impl LensSteps for LimitWorld {
    fn near_step(&mut self, _reference: u32, limit: u16, _table: StepTable) -> u32 { if limit == u16::MAX { self.unlimited_near } else { u32::from(limit) / 10 } }
    fn far_step(&mut self, _reference: u32, limit: u16, _table: StepTable) -> u32 { if limit == u16::MAX { self.unlimited_far } else { u32::from(limit) / 10 } }
    fn lens_step(&mut self, step: u32) -> u32 { step }
}

impl ChannelLevels for LimitWorld {
    fn channel_level(&mut self, channel: u32, offset: bool) -> i16 { 100 * channel as i16 - if offset { 30 } else { 0 } }
    fn compensated_level(&mut self, step: u32, reference: u32) -> i16 { (7 * step + reference) as i16 }
}

impl FeatureClasses for LimitWorld {
    fn feature_class(&mut self, _profile: u32) -> u8 { 0 }
}

impl CellLevels for LimitWorld {
    fn level_class(&mut self, cell: Cell) -> u32 {
        match cell.column + cell.row {
            sum @ 1..=4 => u32::from(sum) + 1,
            _ => 1,
        }
    }
    fn row_level(&mut self, cell: Cell) -> i16 { 100 + 10 * i16::from(cell.row) }
}
