//! A test world for the preparation and the exposure program: the engine's world plus the lens
//! calibration, image 6's exposure services, the capture frames and the level scale, by table.

use std::collections::BTreeMap;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::Bytes;

use crate::adjust::motion::drive::{DriveKind, DriveMode, DriveSpeed};
use crate::adjust::motion::ports::{ExposureTimes, GridArea, LensCalibration, LensTables, MeasurementReadings, ProfileGains};
use crate::adjust::motion::preparation::{CaptureFrames, FrameSize, LensStatus};
use crate::adjust::motion::readings::{AreaList, SelectionLists, AREA_POSITIONS};
use crate::adjust::timing::LevelScale;
use crate::exposure::fakes::{Headers, World};
use crate::exposure::ports::{CaptureProfiles, Lens, ProfileAccess, Properties, Property, StockMemory};
use crate::profile::Profile;

pub struct MotionWorld {
    pub world: World,
    pub steps: u32,
    pub lowest_steps: BTreeMap<u32, u32>,
    pub highest_steps: BTreeMap<u32, u32>,
    /// Positions per step; a depth spans this many times the depth in tenths.
    pub depth_positions: BTreeMap<u32, i16>,
    pub lens_state: u32,
    pub drive_kind: DriveKind,
    pub top_speed: u32,
    /// Exposure times by profile, level and gain; others take `default_time`.
    pub times: BTreeMap<(u32, i16, u32), u32>,
    pub default_time: u32,
    pub shortest_times: BTreeMap<u32, u32>,
    pub gain_levels: BTreeMap<(u32, u32), i16>,
    pub selected_area: GridArea,
    pub selection_value: u32,
    pub control_state: u32,
    pub frames: BTreeMap<u32, FrameSize>,
}

impl Default for MotionWorld {
    fn default() -> Self {
        Self {
            world: World::default(),
            steps: 64,
            lowest_steps: BTreeMap::new(),
            highest_steps: BTreeMap::new(),
            depth_positions: BTreeMap::new(),
            lens_state: 0,
            drive_kind: DriveKind(0),
            top_speed: 0,
            times: BTreeMap::new(),
            default_time: 0,
            shortest_times: BTreeMap::new(),
            gain_levels: BTreeMap::new(),
            selected_area: GridArea::default(),
            selection_value: 0,
            control_state: 0,
            frames: BTreeMap::new(),
        }
    }
}

impl MotionWorld {
    pub fn property(&mut self, id: Property, value: u32) { self.world.property(id, value) }

    pub fn property_word(&mut self, id: Property, value: u16) { self.world.property_word(id, value) }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Lists {
    pub subjects: u8,
    pub single_subject: u8,
    pub areas: AreaList,
    pub area_positions: [i16; AREA_POSITIONS],
    pub reference: u32,
    pub area_words: [u32; 2],
}

impl SelectionLists for Lists {
    fn subjects(&mut self) -> u8 { self.subjects }
    fn single_subject(&mut self) -> u8 { self.single_subject }
    fn areas(&mut self) -> AreaList { self.areas }
    fn area_positions(&mut self) -> [i16; AREA_POSITIONS] { self.area_positions }
    fn reference(&mut self) -> u32 { self.reference }
    fn area_words(&mut self) -> [u32; 2] { self.area_words }
}

/// The linear factor of a level is the level plus 1000; a difference is `value - reference`.
impl LevelScale for MotionWorld {
    fn linear(&mut self, level: i16) -> u32 { (i32::from(level) + 1000) as u32 }
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16 { (value as i32 - reference as i32) as i16 }
}

impl Properties for MotionWorld {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl Lens for MotionWorld {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.world.level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.world.base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.world.reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.world.calibration_index(lens) }
}

impl CaptureProfiles for MotionWorld {
    fn level_offset(&mut self, profile: u32) -> i16 { self.world.level_offset(profile) }
    fn class_offset(&mut self, class: u8) -> i16 { self.world.class_offset(class) }
    fn offset_class(&mut self, profile: u32) -> u8 { self.world.offset_class(profile) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { self.world.sensitivity_class(profile) }
    fn configured_level(&mut self) -> u16 { self.world.configured_level() }
    fn configured_profile(&mut self, level: i16) -> u32 { self.world.configured_profile(level) }
    fn record_profile(&mut self) -> u32 { self.world.record_profile() }
    fn default_dimensions(&mut self) -> [u16; 3] { self.world.default_dimensions() }
}

impl StockMemory for MotionWorld {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.world.read_u8(at) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.world.read_u16(at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.world.read_u32(at) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.world.write_u8(at, value) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.world.write_u16(at, value) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.world.write_u32(at, value) }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { self.world.fill(at, bytes, value) }
}

impl ProfileAccess for MotionWorld {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

/// Positions are ten per step of difference.
impl LensTables for MotionWorld {
    fn channel_position(&mut self, _lens: u32, _step: u32, _reference: u32, _mode: u32) -> i16 { 0 }
    fn side_adjusted(&mut self, value: i16, _from: u32, _to: u32) -> i16 { value }
    fn table_offset(&mut self, _lens: u32, _step: u32, delta: i16) -> i32 { i32::from(delta) * 10 }
}

impl LensCalibration for MotionWorld {
    fn steps(&mut self) -> u32 { self.steps }
    fn lowest_step(&mut self, lens: u32) -> u32 { self.lowest_steps.get(&lens).copied().unwrap_or(0) }
    fn highest_step(&mut self, lens: u32) -> u32 { self.highest_steps.get(&lens).copied().unwrap_or(self.steps - 1) }
    fn depth_positions(&mut self, _lens: u32, step: u32, depth: u16) -> i16 { self.depth_positions.get(&step).copied().unwrap_or(1).wrapping_mul(depth as i16) }
    fn table_entry(&mut self, _lens: u32, _table: Property, _entries: u32) -> u32 { 0 }
    fn lens_state(&mut self) -> u32 { self.lens_state }
    fn drive_kind(&mut self) -> DriveKind { self.drive_kind }
    fn margin_position(&mut self, _channel: u8, _lens: u32, _step: u32, _reference: u32, _lens_state: u32) -> i16 { 0 }
}

impl DriveSpeed for MotionWorld {
    fn top_speed(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 { self.top_speed }
}

impl ExposureTimes for MotionWorld {
    fn exposure_time(&mut self, profile: u32, level: i16, gain: u32) -> u32 { self.times.get(&(profile, level, gain)).copied().unwrap_or(self.default_time) }
}

impl ProfileGains for MotionWorld {
    fn gain_level(&mut self, profile: u32, gain: u32) -> i16 { self.gain_levels.get(&(profile, gain)).copied().unwrap_or(0) }
    fn shortest_time(&mut self, profile: u32) -> u32 { self.shortest_times.get(&profile).copied().unwrap_or(0) }
}

impl LensStatus for MotionWorld {
    fn control_state(&mut self) -> u32 { self.control_state }
}

impl CaptureFrames for MotionWorld {
    fn frame_size(&mut self, mode: u32) -> FrameSize { self.frames.get(&mode).copied().unwrap_or_default() }
}

impl MeasurementReadings for MotionWorld {
    fn selected_area(&mut self, _record: RecordHandle) -> GridArea { self.selected_area }
    fn selection_value(&mut self, _record: RecordHandle, _lens: u32) -> u32 { self.selection_value }
}
