//! What the motion layer reaches in image 4 and image 6 and in the rest of saclac besides
//! configuration values and stock memory.

use xm1_saclac_interface::RecordHandle;

use crate::adjust::motion::drive::DriveKind;
use crate::exposure::ports::Property;

/// A dispatch tick count (0x7257a8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Ticks(pub u32);

pub trait Clock {
    fn now(&mut self) -> Ticks;
    /// Ten times the ticks since `since` (0x7257e0).
    fn elapsed(&mut self, since: Ticks) -> u32;
}

/// The lens position tables image 4 keeps in its configuration.
pub trait LensTables {
    /// The first position 0x7498e0 returns for `lens` at `step` and `reference`, with no second
    /// output.
    fn channel_position(&mut self, lens: u32, step: u32, reference: u32, mode: u32) -> i16;
    /// `value` shifted by the configured offset for a change of side from `from` to `to`
    /// (0x747aac).
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16;
    /// The table offset of `delta` steps for `lens` at `step` (0x747e70).
    fn table_offset(&mut self, lens: u32, step: u32, delta: i16) -> i32;
}

/// The step range image 4's lens calibration covers and the positions it allows.
pub trait LensCalibration {
    /// How many steps the calibration has (0x74677c).
    fn steps(&mut self) -> u32;
    /// The lowest step `lens` may take (0x746cc8 with no channel).
    fn lowest_step(&mut self, lens: u32) -> u32;
    /// The highest step `lens` may take (0x746d6c with no channel).
    fn highest_step(&mut self, lens: u32) -> u32;
    /// The lens positions a depth of `depth` spans at `step` (0x749254).
    fn depth_positions(&mut self, lens: u32, step: u32, depth: u16) -> i16;
    /// The entry for `lens` among `entries` configuration values from `table` (0x747304).
    fn table_entry(&mut self, lens: u32, table: Property, entries: u32) -> u32;
    /// The attached lens's state, 0 to 3 (0x746054).
    fn lens_state(&mut self) -> u32;
    /// The lens's drive kind (0x76f360).
    fn drive_kind(&mut self) -> DriveKind;
    /// The position the calibration margins of `channel` leave `lens` at `step` (0x74b05c's
    /// output with the preparation's arguments).
    fn margin_position(&mut self, channel: u8, lens: u32, step: u32, reference: u32, lens_state: u32) -> i16;
}

/// Image 6's exposure services per capture profile.
pub trait ExposureTimes {
    /// The exposure time of `profile` at `level` with gain step `gain`, in microseconds
    /// (0x10061ec).
    fn exposure_time(&mut self, profile: u32, level: i16, gain: u32) -> u32;
}

/// The selected area of the record's measurement, as column and row of the 7-column grid
/// (0xf5206c).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridArea {
    pub column: u32,
    pub row: u32,
}

/// The measurement layer's readings of the bound record the preparation keeps.
pub trait MeasurementReadings {
    /// 0xf5206c.
    fn selected_area(&mut self, record: RecordHandle) -> GridArea;
    /// The indexed selection value for `lens` (0xf51b50).
    fn selection_value(&mut self, record: RecordHandle, lens: u32) -> u32;
}
