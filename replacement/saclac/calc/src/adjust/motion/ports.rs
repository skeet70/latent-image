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
    /// The two positions 0x7498e0 returns for `lens` at `step` and `reference`.
    fn channel_positions(&mut self, lens: u32, step: u32, reference: u32, mode: u32) -> [i16; 2];
    /// `value` shifted by the configured offset for a change of side from `from` to `to`
    /// (0x747aac).
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16;
    /// The table offset of `delta` steps for `lens` at `step` (0x747e70).
    fn table_offset(&mut self, lens: u32, step: u32, delta: i16) -> i32;
    /// The configured start position of `channel` for `lens` at `reference` (0x748730 with its
    /// second, fourth and sixth arguments 0, 0 and 1).
    fn channel_start(&mut self, lens: u32, channel: u8, reference: u32) -> i16;
    /// The lens position 0x7499a0 gives `lens` at `step` for a drive `period`, its fifth argument
    /// 1.
    fn period_position(&mut self, period: u16, lens: u32, step: u32, reference: u32) -> i32;
}

/// Image 4's lens words a channel search's drive figures read.
pub trait LensWords {
    /// The rate at lens position `position` for `lens` at `step` and `reference` (0x749be4 with
    /// its fifth argument 1).
    fn position_rate(&mut self, position: i16, lens: u32, step: u32, reference: u32) -> u32;
    /// 0x747d8c.
    fn curve_word(&mut self, lens: u32) -> u32;
    /// The step's mapped word for `lens` (0x74695c).
    fn mapped_step_word(&mut self, step: u32, lens: u32) -> u32;
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
    /// The low and high lens limits 0x74b05c exports for `query`.
    fn lens_limits(&mut self, query: &LimitQuery) -> LensLimits;
}

/// What 0x74b05c's limits are read for; its first and fourth arguments are 0 and its ninth 1.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LimitQuery {
    pub channel: u8,
    pub lens: u32,
    pub step: u32,
    pub reference: u32,
    /// The calibration margins widen the limits.
    pub with_margins: bool,
    pub lens_state: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensLimits {
    pub low: i16,
    pub high: i16,
}

/// Image 6's exposure services per capture profile.
pub trait ExposureTimes {
    /// The exposure time of `profile` at `level` with gain step `gain`, in microseconds
    /// (0x10061ec).
    fn exposure_time(&mut self, profile: u32, level: i16, gain: u32) -> u32;
}

/// Image 6's level of a capture profile at a gain step (0x1006000) and its shortest exposure
/// (0x1005f60).
pub trait ProfileGains {
    fn gain_level(&mut self, profile: u32, gain: u32) -> i16;
    fn shortest_time(&mut self, profile: u32) -> u32;
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

/// What image 4's lens bounds are read for (0x74c568 with its fixed arguments 0, 1, 0 and 1).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BoundsQuery {
    pub lens: u32,
    pub scene: u8,
    pub channel: u8,
    pub class: u8,
    pub step: u32,
    pub reference: u32,
    pub held: u8,
    pub lens_state: u32,
}

/// The lens positions a search may drive between.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchBounds {
    pub low: i16,
    pub high: i16,
}

pub trait LensBounds {
    fn search_bounds(&mut self, query: &BoundsQuery) -> SearchBounds;
}

/// The library's common logarithm (0xee996c).
pub trait Logarithms10 {
    fn log10(&mut self, value: f32) -> f32;
}

/// A grid of equal cells the geometry layer lays over a capture frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaGrid {
    pub cell_height: u32,
    pub cell_width: u32,
    pub left: u32,
    pub top: u32,
    pub gap_horizontal: u16,
    pub gap_vertical: u16,
    pub columns: u8,
    pub rows: u8,
    pub at_16: [u8; 2],
}

/// The search profile's frame grids: the configured column and row counts and two configured
/// sizes, and the first listed area.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchFrameRequest {
    pub grid: [u16; 4],
    pub area: Option<Tracking>,
}

/// The capture parameters a frame's statistics area is laid out for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsFrameRequest {
    pub profile: u32,
    pub lens: u32,
    pub row_scale: [u16; 2],
    pub selection_value: u32,
    pub at_0164: u32,
    pub at_0168: u32,
}

/// The capture a frame's window and crop are computed for: the profile, the record's three
/// area words and its row scale.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureFraming {
    pub profile: u32,
    pub area: [u32; 3],
    pub row_scale: [u16; 2],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rectangle {
    pub left: u32,
    pub top: u32,
    pub height: u32,
    pub width: u32,
}

/// A capture's frame window and the crop centred in it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CropGeometry {
    pub window: Rectangle,
    pub crop: Rectangle,
}

/// The grid of equal bands a frame's samples take, in percent of its crop.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridRequest {
    pub height_percent: u8,
    pub width_percent: u8,
    pub bands: u8,
    pub row_percent: u8,
    pub column_percent: u8,
}

/// Which way a listed area's secondary samples run: rows from the bottom or the top, or along
/// one row (0xf95df0's classes 0 to 3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AreaClass {
    RowsUpward,
    #[default]
    RowsDownward,
    Columns,
    ColumnsAlternate,
}

impl AreaClass {
    pub const fn code(self) -> u32 {
        match self {
            Self::RowsUpward => 0,
            Self::RowsDownward => 1,
            Self::Columns => 2,
            Self::ColumnsAlternate => 3,
        }
    }
}

/// The listed area a grid's secondary samples follow, and their direction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tracking {
    pub area: crate::adjust::motion::readings::ListedArea,
    pub class: AreaClass,
}

/// The second exposure's capture, its sample grid and the area a treated run follows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SampleFrameRequest {
    pub framing: CaptureFraming,
    pub lens: u32,
    pub exposure_level: i16,
    pub at_0c: i16,
    pub lens_step: u32,
    pub program_step: u8,
    pub grid: GridRequest,
    pub tracking: Option<Tracking>,
    /// The run neither searches by channel nor by area.
    pub whole_frame: bool,
}

/// ProfileGeometry's frame layouts (0xeda668) and profile tables.
pub trait FrameGeometry {
    /// The two grids of the search profile's frame (0xeda668 with flags 0x81).
    fn search_frames(&mut self, request: &SearchFrameRequest) -> [AreaGrid; 2];
    /// The height and width of the statistics area at 100 percent with 16-pixel cells
    /// (0xeda668 with flags 2).
    fn statistics_frame(&mut self, request: &StatisticsFrameRequest) -> [u32; 2];
    /// The two grids of the second exposure's frame (0xeda668 with flags 0x81).
    fn sample_frames(&mut self, request: &SampleFrameRequest) -> [AreaGrid; 2];
    /// The window and crop of `framing`'s frame (0xef6030 with its sixth word 1).
    fn crop_geometry(&mut self, framing: &CaptureFraming) -> CropGeometry;
    /// The steps a cell's height and width are floored to (0xef5b5c).
    fn cell_steps(&mut self, profile: u32) -> [u32; 2];
    /// `value` scaled by the profile template's expansion ratio (0xef5658).
    fn expanded_ratio(&mut self, value: u32) -> u32;
    /// The profile's feature class (0xef53d0).
    fn feature_class(&mut self, profile: u32) -> u8;
}

/// Image 4's rate of a lens channel, in events per second (0x749e40).
pub trait ChannelRates {
    fn channel_rate(&mut self, lens: u32, channel: u8) -> u32;
}

/// The profile calibration's factors (0xef43b8).
pub trait AreaCurves {
    /// `points` with the profile factors `selector` and the two deltas applied.
    fn with_profile_factors(&mut self, points: [u16; 3], deltas: [u8; 2], selector: u8) -> [u16; 3];
}
