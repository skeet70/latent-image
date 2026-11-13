use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::Bytes;

use super::ports::{LensServices, Overlay, Steadiness, SubjectQuery, SubjectRequest};
use crate::adjust::lens::{BiasTable, LensConfiguration};
use crate::adjust::motion::ports::LensTables;
use crate::adjust::profile_adjustment::{LensSteps, StepTable};
use crate::exposure::fakes::{Headers, World};
use crate::exposure::ports::{ProfileAccess, Properties, Property, StockMemory};
use crate::profile::Profile;

/// The exposure world plus the subject query, the lens's steadiness and services, and the overlay
/// lines drawn.
#[derive(Default)]
pub struct Scenery {
    pub world: World,
    pub subjects: Option<u32>,
    pub requests: Vec<SubjectRequest>,
    pub steady: bool,
    pub lines: Vec<(Vec<u8>, [u8; 4])>,
    pub lens: LensFake,
}

impl Properties for Scenery {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl StockMemory for Scenery {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.world.read_u8(at) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.world.read_u16(at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.world.read_u32(at) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.world.write_u8(at, value) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.world.write_u16(at, value) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.world.write_u32(at, value) }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { self.world.fill(at, bytes, value) }
}

impl ProfileAccess for Scenery {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

impl SubjectQuery for Scenery {
    fn subjects(&mut self, request: &SubjectRequest) -> Option<u32> {
        self.requests.push(*request);
        self.subjects
    }
}

impl Steadiness for Scenery {
    fn steady(&mut self) -> bool { self.steady }
}

impl Overlay for Scenery {
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8) { self.lines.push((text.to_vec(), [column, row, foreground, background])); }
}

impl crate::exposure::ports::Lens for Scenery {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.world.level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.world.base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.world.reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.world.calibration_index(lens) }
}

impl LensSteps for Scenery {
    fn near_step(&mut self, reference: u32, limit: u16, table: StepTable) -> u32 { self.world.near_step(reference, limit, table) }
    fn far_step(&mut self, reference: u32, limit: u16, table: StepTable) -> u32 { self.world.far_step(reference, limit, table) }
    fn lens_step(&mut self, step: u32) -> u32 { self.world.lens_step(step) }
}

impl LensConfiguration for Scenery {
    fn channel_value(&mut self, channel: i32) -> u32 { self.world.channel_value(channel) }
    fn channel_curve(&mut self, channel: i32) -> u32 { self.world.channel_curve(channel) }
    fn bias(&mut self, resource: i32, table: BiasTable) -> i16 { self.world.bias(resource, table) }
    fn biased_position(&mut self, resource: i32, table: BiasTable) -> i16 { self.world.biased_position(resource, table) }
    fn resource_count(&mut self) -> u8 { self.world.resource_count() }
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32 { self.world.selection_word(channel, flags, offset) }
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32 { self.world.remap(word, channel, mode) }
    fn position_limits(&mut self, key: u32) -> [i16; 2] { self.world.position_limits(key) }
    fn mapped_value(&mut self, key: u32) -> u32 { self.world.mapped_value(key) }
}

/// Lens positions by lens and step; the reference word must be the scenery's.
impl LensTables for Scenery {
    fn channel_position(&mut self, lens: u32, step: u32, reference: u32, _mode: u32) -> i16 {
        assert_eq!(reference, self.lens.reference);
        self.lens.targets.get(&(lens, step)).copied().unwrap_or(0)
    }
    fn side_adjusted(&mut self, value: i16, _from: u32, _to: u32) -> i16 { value }
    fn table_offset(&mut self, _lens: u32, _step: u32, _delta: i16) -> i32 { 0 }
}

/// The response is the curve point plus the step.
impl LensServices for Scenery {
    fn lens_reference(&mut self) -> u32 { self.lens.reference }
    fn primary_tenths(&mut self, lens: u32) -> u32 { self.lens.primary.get(&lens).copied().unwrap_or(0) }
    fn response(&mut self, value: u16, _lens: u32, step: u32, reference: u32) -> i32 {
        assert_eq!(reference, self.lens.reference);
        i32::from(value) + step as i32
    }
    fn converted_difference(&mut self, first: u32, second: u32) -> i16 { (10 * first + second) as i16 }
    fn level_class(&mut self, _lens: u32) -> u32 { self.lens.class }
    fn class_level(&mut self, _lens: u32, setting: u32) -> i32 { self.lens.class_level + setting as i32 }
}

#[derive(Default)]
pub struct LensFake {
    pub reference: u32,
    pub primary: BTreeMap<u32, u32>,
    pub targets: BTreeMap<(u32, u32), i16>,
    pub class: u32,
    pub class_level: i32,
}
