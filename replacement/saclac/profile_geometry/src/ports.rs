#![forbid(unsafe_code)]

//! What the layer calls outside itself; saclac's composition root binds each trait.

use crate::memory::Location;
use crate::profile::ProfileHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigOffset(pub u32);

pub trait Configuration {
    /// 0x69f8fc.
    fn byte(&mut self, offset: ConfigOffset) -> u8;
    /// 0x69fec8.
    fn halfword(&mut self, offset: ConfigOffset) -> u16;
    /// The configured kind of `profile` (0x69ff7c).
    fn profile_kind(&mut self, profile: ProfileHandle) -> u8;
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
}

pub trait Ports: Configuration + Pipeline + Handlers + DumpRegions + DiagnosticLabels + ConfiguredRate + Retained {}

impl<T: Configuration + Pipeline + Handlers + DumpRegions + DiagnosticLabels + ConfiguredRate + Retained> Ports for T {}
