#![forbid(unsafe_code)]

//! What the round reads from and asks of the imaging pipeline.

use crate::profile::{FrameInterval, GridCell};

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
    pub status: SnapshotStatus,
    pub timing: TimingWord,
    pub flags: u8,
    pub gain_step: GainStep,
    pub exposure: [PipelineWord; 2],
    pub levels: [u16; 2],
    pub offset_level: u16,
    /// One lowers the window level by 100.
    pub adjustment: PipelineWord,
    /// Dimensions of the statistics image.
    pub size: [PipelineWord; 2],
    pub origin: [PipelineWord; 2],
    pub extent: [PipelineWord; 2],
    /// Words the measurement history keeps for its readers.
    pub retained: [PipelineWord; 6],
}

impl PipelineSnapshot {
    pub const EMPTY: Self = Self {
        status: SnapshotStatus(0),
        timing: TimingWord(0),
        flags: 0,
        gain_step: GainStep(0),
        exposure: [PipelineWord(0); 2],
        levels: [0; 2],
        offset_level: 0,
        adjustment: PipelineWord(0),
        size: [PipelineWord(0); 2],
        origin: [PipelineWord(0); 2],
        extent: [PipelineWord(0); 2],
        retained: [PipelineWord(0); 6],
    };
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotStatus(pub u32);

impl SnapshotStatus {
    /// Bits 0 and 2 both set.
    pub const fn complete(self) -> bool { self.0 & 5 == 5 }
}

/// The history capture's statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryStatistics {
    /// Per grid cell, in cell order.
    pub banks: [[u32; 64]; 2],
    pub word: PipelineWord,
    /// The two half-word banks at the cell the capture was asked about.
    pub cell_levels: [u16; 2],
}

impl HistoryStatistics {
    pub const EMPTY: Self = Self { banks: [[0; 64]; 2], word: PipelineWord(0), cell_levels: [0; 2] };
}

impl Default for HistoryStatistics {
    fn default() -> Self { Self::EMPTY }
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
    /// The statistics of the latest history capture, with the levels at `cell`.
    fn history_statistics(&mut self, cell: GridCell) -> HistoryStatistics;
}

/// Exposure conversions the round applies to pipeline values.
pub trait Exposure {
    fn gain_offset(&mut self, step: GainStep) -> u32;
    fn exposure_value(&mut self, exposure: [PipelineWord; 2]) -> i16;
    fn timing_offset(&mut self, timing: TimingWord) -> u16;
}
