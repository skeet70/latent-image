//! What the exposure engine reaches outside the library.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::{Memory, StockEntries};

use crate::adjust::exposure_limits::{CellLevels, ChannelLevels};
use crate::adjust::lens::LensConfiguration;
use crate::adjust::profile_adjustment::LensSteps;
use crate::profile::Profile;
use crate::scene_levels::FeatureClasses;
use crate::selection::RowIndex;

/// A camera configuration value the engine reads; the ids are in `crate::stock::property`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Property(pub(crate) u32);

impl Property {
    /// The property holding the high byte of a two-byte value.
    pub(crate) const fn high(self) -> Self { Self(self.0 + 1) }
    /// The configuration byte's key, which 0x69f8fc reads.
    pub const fn key(self) -> u32 { self.0 }
}

pub trait Properties {
    fn property(&mut self, property: Property) -> u32;
}

/// The lens calibration services of image 4.
pub trait Lens {
    /// The lens's level offset at `reference` (0x746bf8).
    fn level(&mut self, lens: u32, reference: u32) -> i16;
    /// The lens's base level (0x7469a0).
    fn base_level(&mut self, lens: u32) -> i16;
    /// The level offset of the bound lens reference (0x746c2c).
    fn reference_offset(&mut self, reference: u32) -> i16;
    /// The lens's calibration index relative to the reference lens (0x746834).
    fn calibration_index(&mut self, lens: u32) -> u32;
}

/// The capture profile table of saclac's display and capture layer.
pub trait CaptureProfiles {
    /// The profile's level offset (0xef5310).
    fn level_offset(&mut self, profile: u32) -> i16;
    /// The level offset of an offset class (0xef52e4).
    fn class_offset(&mut self, class: u8) -> i16;
    /// The profile's offset class (0xef53f8).
    fn offset_class(&mut self, profile: u32) -> u8;
    /// The profile's sensitivity class (0xef5420).
    fn sensitivity_class(&mut self, profile: u32) -> u8;
    /// The sensitivity class of a sensitivity value (0xef5384).
    fn sensitivity_class_for(&mut self, value: u32) -> u8;
    /// The level the configured measurement takes (0xeea030's third output).
    fn configured_level(&mut self) -> u16;
    /// The capture profile of the configured measurement (0xeea094).
    fn configured_profile(&mut self, level: i16) -> u32;
    /// The capture profile of the record's exposure settings (0xef6790, which reads none of its
    /// arguments).
    fn record_profile(&mut self) -> u32;
    /// The selection dimensions of a record that keeps none (0xef3434 with arguments 5 and 0).
    fn default_dimensions(&mut self) -> [u16; 3];
    /// The column and row steps of selection grid layout `layout` (0xef7570 with mode 0).
    fn grid_steps(&mut self, layout: u8) -> [u8; 2];
}

/// Image 6's levels per capture profile.
pub trait ProfileLevels {
    /// The profile's level (0x1006000's first output with mode 0).
    fn profile_level(&mut self, profile: u32) -> i16;
    /// `level` converted for `profile` (0x1005df8).
    fn converted_level(&mut self, profile: u32, level: i16) -> u16;
}

/// Stock memory the engine shares with calc, the image task and the client's dump frame. Every
/// address comes from `crate::stock`.
pub trait StockMemory {
    fn read_u8(&mut self, at: SharedAddress) -> u8;
    fn read_u16(&mut self, at: SharedAddress) -> u16;
    fn read_u32(&mut self, at: SharedAddress) -> u32;
    fn write_u8(&mut self, at: SharedAddress, value: u8);
    fn write_u16(&mut self, at: SharedAddress, value: u16);
    fn write_u32(&mut self, at: SharedAddress, value: u32);
    /// Sets `bytes` bytes from `at` to `value`, as 0x742154 does.
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8);
}

/// The profile records as the library reads them.
pub trait ProfileAccess {
    type Records: Memory;
    type Entries: StockEntries;
    fn profile(&mut self) -> Profile<'_, Self::Records, Self::Entries>;
}

/// The blocks of the exposure context that calc and the image task's statistics check reach in
/// place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InPlace {
    /// A selection column's subject regions.
    ColumnRegions(RowIndex),
    /// A row's `selection::RowMeasurement`, which the statistics check fills.
    RowMeasurement(RowIndex),
    BoundSubjects,
    BoundSummary,
    SnapshotSubjects,
    SnapshotTables,
}

/// Where the composition root placed the exposure context.
pub trait Placement {
    fn address(&mut self, block: InPlace) -> SharedAddress;
}

/// Image 4's settings block 0x914db8.
pub trait ImageReport {
    /// The second of the four words 0x740888 reports (+0x38).
    fn second_report_word(&mut self) -> u32;
    /// The fourth (+0x40).
    fn fourth_report_word(&mut self) -> u32;
}

/// Everything the engine reaches.
pub trait Ports:
    Properties + Lens + LensSteps + LensConfiguration + CaptureProfiles + ProfileLevels + ChannelLevels + FeatureClasses + CellLevels + StockMemory + ProfileAccess + Placement + ImageReport
{
}

impl<T> Ports for T where
    T: Properties + Lens + LensSteps + LensConfiguration + CaptureProfiles + ProfileLevels + ChannelLevels + FeatureClasses + CellLevels + StockMemory + ProfileAccess + Placement + ImageReport
{
}
