#![forbid(unsafe_code)]

//! What the round reads from and asks of the imaging pipeline.

use crate::profile::FrameInterval;

/// The pipeline's statistics captures, each with the pipeline state saved beside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Statistics { Primary, Search, Auxiliary, History }

/// A pipeline value the round passes on or converts without interpreting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PipelineWord(pub u32);

/// A sensor timing the pipeline runs or the acquisition programs; the pipeline
/// derives its frame interval.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TimingWord(pub u32);

/// Index of the gain-offset settings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GainStep(pub u32);

/// The pipeline state saved with a statistics capture, as far as the round reads it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PipelineSnapshot {
    pub mode: PipelineWord,
    pub flags: u8,
    pub gain_step: GainStep,
    pub exposure: [PipelineWord; 2],
    pub levels: [u16; 2],
    /// One lowers the window level by 100.
    pub adjustment: PipelineWord,
}

pub trait Pipeline {
    fn capture(&mut self, statistics: Statistics);
    fn snapshot(&mut self, statistics: Statistics) -> PipelineSnapshot;
    /// The timing the pipeline currently runs.
    fn timing(&mut self) -> TimingWord;
    /// `None` when `timing` has no known interval.
    fn frame_interval(&mut self, timing: TimingWord) -> Option<FrameInterval>;
    /// Requests the mode-six statistics unless a request is outstanding.
    fn request_mode_six_statistics(&mut self);
    /// Takes the flag the pipeline raises for the next window request.
    fn take_window_refresh(&mut self) -> bool;
}

/// Exposure conversions the round applies to pipeline values.
pub trait Exposure {
    fn gain_offset(&mut self, step: GainStep) -> u32;
    fn exposure_value(&mut self, exposure: [PipelineWord; 2]) -> i16;
}
