#![forbid(unsafe_code)]

//! What the layer calls outside itself; saclac's composition root binds each trait.

use xm1_saclac_interface::pipeline::FormatPayload;

use crate::memory::Location;
use crate::profile::{PipelineGroup, ProfileHandle, QuadGroup, Rectangle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigOffset(pub u32);

pub trait Configuration {
    /// 0x69f8fc.
    fn byte(&mut self, offset: ConfigOffset) -> u8;
    /// 0x69fec8.
    fn halfword(&mut self, offset: ConfigOffset) -> u16;
    /// 0x69ff7c.
    fn signed_halfword(&mut self, offset: ConfigOffset) -> i16;
    /// 0x746834.
    fn collapse_index(&mut self, value: u32) -> u32;
    /// 0x7468b8.
    fn transform(&mut self, value: u32, scale: u32) -> u16;
}

/// An image pipeline channel the controller's requests name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipelineChannel(pub u32);

/// A set of pipeline stages, one bit each.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stages(pub u32);

pub trait Pipeline {
    /// Stops `stages` on `channel`; false when the pipeline refuses (0xd9f504).
    fn stop(&mut self, channel: PipelineChannel, stages: Stages) -> bool;
    /// 0xd9ef3c.
    fn release(&mut self, channel: PipelineChannel, stages: Stages);
    /// 0xd9f0a0.
    fn settle(&mut self);
    /// 0xd5d27c.
    fn release_capture_hardware(&mut self);
    /// Releases the configuration the controller handed the pipeline (0xd5d268).
    fn release_configuration_hardware(&mut self);
    /// Enables `stages` with the payloads at `payloads`; false when refused (0xd9f7c0).
    fn enable(&mut self, channel: PipelineChannel, stages: Stages, payloads: Location) -> bool;
    /// 0xd9f7d8.
    fn disable(&mut self, channel: PipelineChannel, stages: Stages) -> bool;
    /// Applies the payloads at `payloads` to running `stages` (0xd9f828).
    fn apply(&mut self, channel: PipelineChannel, stages: Stages, payloads: Location) -> bool;
    /// Sends the geometry records at `records` that `stages` names (0xd9fd54).
    fn configure(&mut self, channel: PipelineChannel, stages: Stages, records: Location) -> bool;
    /// Hands `stages` of the configuration at `configuration` back to the pipeline (0xd5e1f4).
    fn restore_stages(&mut self, configuration: Location, stages: Stages);
}

/// The parameter fields the pipeline's mode payload depends on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeRequest {
    pub group: PipelineGroup,
    pub index_scale: u8,
    pub index: u8,
    pub shift: i16,
    pub mode: u8,
    pub first: Location,
    pub second: Location,
    pub mode_byte: i8,
}

/// The sensor window a windowed primary payload covers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryWindow {
    pub rectangle: Rectangle,
    pub offset: u32,
    pub scale: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryPayload {
    pub entries: u32,
    /// Zero leaves the primary payload stage off.
    pub count: u16,
    pub first_offset: u16,
    pub second_offset: u16,
    pub option: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondaryWindow {
    pub rectangle: Rectangle,
    pub origin: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondaryPayload {
    pub first: u16,
    pub second: u16,
    pub value: u32,
    pub extra: u32,
}

/// The pipeline layer's payload builders.
pub trait StagePayloads {
    /// Fills the mode payloads at `into`; the mode stages it filled (0xda0ab0).
    fn mode_payload(&mut self, request: &ModeRequest, into: Location) -> Stages;
    /// Fills the tertiary payload at `into`; the stages it filled (0xda09f4).
    fn tertiary_payload(&mut self, group: PipelineGroup, index_scale: u8, index: u8, into: Location) -> Stages;
    /// False when the pipeline refuses (0xda027c on channel 2, format stage).
    fn set_format(&mut self, payload: &FormatPayload) -> bool;
    /// The configuration byte that selects the quad gains for `group`, if the group has one
    /// (0xdabb38).
    fn quad_selection_offset(&mut self, group: PipelineGroup) -> Option<ConfigOffset>;
    /// 0xdab6f8.
    fn quad_gains(&mut self, group: QuadGroup, limit: i16) -> [u16; 4];
    /// 0xda1acc.
    fn quad_area_scale(&mut self, area: u32) -> u32;
    /// 0xda1af8.
    fn quad_area_limit(&mut self) -> u16;
    /// The primary payload for `group`, over `window` when the profile's policy has one; `None`
    /// when the group has none (0xdaba08).
    fn primary_payload(&mut self, group: PipelineGroup, window: Option<PrimaryWindow>) -> Option<PrimaryPayload>;
    /// 0xdab900.
    fn secondary_payload(&mut self, group: PipelineGroup, window: Option<SecondaryWindow>) -> Option<SecondaryPayload>;
    /// Whether `group`'s primary payload takes a window under policy 0x40 (0xdabbac).
    fn has_windowed_payload(&mut self, group: PipelineGroup) -> bool;
}

/// One setting of the pipeline's scan configuration object, each taken by its own builder entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanSetting {
    /// 0xd783ec.
    ProcessingOption(u32),
    /// 0xd7910c.
    OutputMode(u32),
    /// 0xd7912c.
    ScanOption(bool),
    /// 0xd78af0.
    GeometryKind(u32),
    /// 0xd784ac.
    RecordProcessing(u32),
    /// 0xd79364.
    RecordProcessingThird(u32),
    /// 0xd784cc.
    RecordProcessingSecond(u32),
    /// 0xd785e0.
    Toggle(u32),
    /// 0xd78a6c.
    QueryKind(u32),
    /// 0xd78a80.
    Format(u32),
    /// 0xd78660.
    Code(u32),
    /// 0xd77f7c.
    QueryMode(u32),
    /// 0xd77fec.
    PrimaryRecordKind(u32),
    /// 0xd7805c.
    SecondaryRecordKind(u32),
    /// 0xd78674.
    Channel(u32),
    /// 0xd78508.
    GeometryEnabled(u32),
    /// 0xd78538.
    FormatCompanion(u32),
    /// 0xd78070.
    OffsetSum(u32),
    /// 0xd786ac with a leading zero.
    OffsetSumLow(u16),
    /// 0xd78db8.
    ScanRate(u32),
    /// 0xd787b8 with a leading zero.
    Limit(u16),
    /// 0xd789f0.
    Dimensions(u32),
    /// 0xd785a8.
    Fixed(u32),
    /// 0xd78f80.
    LimitTenths(u16),
    /// 0xd78f9c.
    Rate(u16),
    /// 0xd78400.
    RateCompanion(u32),
    /// 0xd78eec.
    Byte70(u8),
    /// 0xd790f8.
    ProcessingLevel(u32),
    /// 0xd79344.
    AreaPresent(bool),
    /// 0xd792e4.
    FirstOf(u32),
    /// 0xd79324.
    Split(u32),
    /// 0xd79304.
    Multiple(bool),
    /// 0xd7914c.
    WhiteBalance([u16; 3]),
    /// 0xd78fb8.
    OutputLevel(u32),
}

/// The pipeline layer's scan configuration object, which the controller fills, compiles and
/// attaches.
pub trait ScanConfiguration {
    fn set(&mut self, setting: ScanSetting);
    /// The compile entries 0xd5e208 to 0xd5e538 in their stock order; `output_kind` goes to
    /// 0xd5e240.
    fn compile_scan_configuration(&mut self, output_kind: u8);
    /// Copies the selection block's callbacks into the compiled configuration (0xd5e0f8,
    /// 0xd5dfd4, 0xd5e05c, 0xd5e168).
    fn apply_selection(&mut self, selection: Location, configuration: Location);
    /// 0xd5e1d4.
    fn attach_scan_configuration(&mut self, configuration: Location, stages: Stages);
    /// 0x772ff8.
    fn rate_code(&mut self, offset_sum: u32) -> u32;
    /// 0x10065fc.
    fn limit_tenths(&mut self, limit: i16) -> u16;
    /// 0x1004618.
    fn output_level(&mut self) -> u32;
    /// 0x747b50.
    fn channel_count(&mut self) -> u8;
}

/// A radial model the image's radial geometry derives from 31 coefficients; its layout is the
/// model's own.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RadialModel(pub [u32; 9]);

/// The coefficients, scales and results of one radial table, as the radial model reads and
/// writes them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RadialBuffer {
    pub points: [u32; 31],
    pub scales: [u16; 2],
    pub bounds: [u16; 2],
    pub gains: [u16; 2],
    pub shift: u16,
}

/// The image-4 radial geometry the scan stages use; every call takes the pixel clock pair.
pub trait RadialGeometry {
    /// 0x6e3b0c.
    fn radial_model(&mut self, points: &[u32; 31]) -> RadialModel;
    /// Refits the table for the compact pipeline group; false when refused (0x6e3744).
    fn fit_compact(&mut self, clocks: (u32, u32), model: &RadialModel, table: &mut RadialBuffer) -> bool;
    /// 0x6e3488.
    fn radial_bound(&mut self, clocks: (u32, u32)) -> u16;
    /// 0x6e3400.
    fn radial_centre(&mut self, model: &mut RadialModel) -> u32;
    /// 0x6e35f4.
    fn radial_extent(&mut self, clocks: (u32, u32), centre: u32, bound: u16, gain: u16) -> u16;
    /// The table's block shift (0x6e384c).
    fn radial_table(
        &mut self,
        clocks: (u32, u32),
        pair: [u32; 2],
        model: &RadialModel,
        extent: u16,
        scales: [u16; 2],
        gain: u16,
    ) -> u16;
}

/// One of the two handler units 0x7411c4 serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerUnit {
    First,
    Second,
}

/// A stock code address the layer stores or calls but does not own.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StockCode(pub u32);

impl StockCode {
    pub const fn is_set(self) -> bool { self.0 != 0 }
}

pub trait Handlers {
    /// Attaches the controller's handler to `unit`; the handle, or the unit's refusal code
    /// (0x7411c4).
    fn attach(&mut self, unit: HandlerUnit) -> Result<u32, u32>;
    /// Detaches `handle` from `unit`; zero or the unit's refusal code (0x74140c).
    fn detach(&mut self, unit: HandlerUnit, handle: u32) -> u32;
    /// Enters the completion `operation`'s starter stored, telling it the controller stopped it.
    fn notify_stopped(&mut self, operation: crate::controller::Operation, callback: StockCode);
}

/// The dump block's two regions, declared at initialization (0x75e6d8, 0x75e704, 0x75e6f0).
pub trait DumpRegions {
    fn reserve_region(&mut self, region: u32, address: u32, bytes: u32);
    fn register_region(&mut self, region: u32, address: u32, bytes: u32);
    fn release_region(&mut self, region: u32);
}

/// The image's diagnostic label tables.
pub trait DiagnosticLabels {
    /// Writes the label of `profile` and returns its length.
    fn profile_label(&mut self, profile: ProfileHandle, into: &mut [u8]) -> usize;
    /// Writes the label of profile feature `feature` and returns its length.
    fn feature_label(&mut self, feature: u8, into: &mut [u8]) -> usize;
    /// The label shown instead of the description while the description is fixed.
    fn fixed_label(&mut self, into: &mut [u8]) -> usize;
    /// The separator after the integer part and the suffix after the fraction.
    fn rate_separator(&mut self, into: &mut [u8]) -> usize;
    fn rate_suffix(&mut self, into: &mut [u8]) -> usize;
}

/// The rate the diagnostic description shows for profiles of kind 2 and 3 (0x1007ce0).
pub trait ConfiguredRate {
    fn configured_rate(&mut self) -> u16;
}

/// Band functions other tasks reach, which stay stock.
pub trait Retained {
    /// Limits an adjusted triplet (0xef4bf4).
    fn limit_triplet(&mut self, triplet: [u16; 3]) -> [u16; 3];
    /// 0xef54fc.
    fn expanded_rectangle(&mut self, profile: ProfileHandle) -> Rectangle;
    /// 0xef5190.
    fn configured_rectangle(&mut self, profile: ProfileHandle) -> Rectangle;
    /// The profile's window, made relative to its configured rectangle and divided by the class
    /// multipliers when `relative` (0xef5d44).
    fn relative_rectangle(&mut self, profile: ProfileHandle, relative: bool) -> Rectangle;
    /// The scan selection the two scan thresholds encode (0xef743c).
    fn encode_thresholds(&mut self, first: i16, second: i16) -> u32;
    /// 0xef74e4.
    fn converted_decade(&mut self, selection: u32) -> i32;
}

pub trait Ports:
    Configuration
    + Pipeline
    + StagePayloads
    + ScanConfiguration
    + Handlers
    + DumpRegions
    + DiagnosticLabels
    + ConfiguredRate
    + Retained
    + RadialGeometry
{
}

impl<T> Ports for T where
    T: Configuration
        + Pipeline
        + StagePayloads
        + ScanConfiguration
        + Handlers
        + DumpRegions
        + DiagnosticLabels
        + ConfiguredRate
        + Retained
        + RadialGeometry
{
}
