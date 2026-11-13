//! What the measurement pipeline reads of a prepared run, in place of the pointers into the motion
//! block that stock's preparation hands it.

use crate::adjust::motion::exposure::FrameExposure;
use crate::adjust::motion::plan::Plan;
use crate::adjust::motion::ports::SearchBounds;
use crate::adjust::motion::preparation::PreviousFocus;
use crate::adjust::motion::readings::{AreaList, RecordReadings, StepLimits};
use crate::adjust::motion::search::{SearchMode, SearchSetup};
use crate::adjust::motion::layout::SampleLayout;
use crate::adjust::motion::state::{FrameExposures, MotionState, SampleSchedule};
use crate::adjust::motion::statistics::Statistics;

impl MotionState {
    /// The record, lens and selection-list readings (output +0x8).
    pub fn readings(&self) -> &RecordReadings { &self.readings }

    pub fn step_limits(&self) -> StepLimits { self.limits }

    /// The search setup (output +0xc).
    pub fn search(&self) -> &SearchSetup { &self.search }

    /// 0xf9d49c.
    pub fn search_mode(&self) -> SearchMode { self.search.mode }

    /// 0xf9cdd8 (output +0x4c).
    pub fn channel_status(&self) -> bool { self.search.channel_status }

    /// The frame exposure the run's frames follow (output +0x4).
    pub fn frame_exposure(&self) -> &FrameExposure { &self.exposures.second }

    /// Both frame exposures (output +0x54 and +0x50).
    pub fn exposures(&self) -> &FrameExposures { &self.exposures }

    pub fn statistics(&self) -> &Statistics { &self.statistics }

    pub fn previous_focus(&self) -> PreviousFocus { self.previous }

    /// The lens positions the run may drive between.
    pub fn bounds(&self) -> SearchBounds { self.bounds }

    /// The listed areas the run measures.
    pub fn areas(&self) -> &AreaList { &self.areas }

    /// The plan the pipeline keeps between the plan's calls (output +0x10).
    pub fn plan(&self) -> &Plan { &self.plan }

    /// The sample layout: its grids, crop, positions, curve and exposure ratio.
    pub fn layout(&self) -> &SampleLayout { &self.layout }

    /// When each sampled area is exposed.
    pub fn schedule(&self) -> &SampleSchedule { &self.sampling }
}
