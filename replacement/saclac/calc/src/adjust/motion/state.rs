//! The motion layer's state: what a run's preparation read and decided, the dump-frame section it
//! traces into, the area sampling and the planned drive.

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::drive::DriveProfile;
use crate::adjust::motion::exposure::{FrameExposure, Program};
use crate::adjust::motion::plan::Plan;
use crate::adjust::motion::ports::{SearchBounds, Ticks};
use crate::adjust::motion::preparation::PreviousFocus;
use crate::adjust::motion::readings::{AreaList, RecordReadings, StepLimits};
use crate::adjust::motion::sampling::{PRIMARY_AREAS, SECONDARY_AREAS};
use crate::adjust::motion::search::SearchSetup;
use crate::adjust::motion::statistics::Statistics;

/// When each area is exposed relative to its frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SampleSchedule {
    /// How long each area is exposed.
    pub exposure: i32,
    primary: [i32; PRIMARY_AREAS],
    pub(crate) primary_count: u8,
    secondary: [i32; SECONDARY_AREAS],
    pub(crate) secondary_rows: u8,
    pub(crate) secondary_columns: u8,
}

impl SampleSchedule {
    /// One exposure offset per primary area and per secondary row; the secondary areas form a
    /// grid of `secondary_columns` by the rows.
    pub fn new(exposure: i32, primary: &[i32], secondary: &[i32], secondary_columns: u8) -> Self {
        let mut schedule = Self { exposure, secondary_columns, ..Self::default() };
        let primary = &primary[..primary.len().min(PRIMARY_AREAS)];
        let secondary = &secondary[..secondary.len().min(SECONDARY_AREAS)];
        schedule.primary[..primary.len()].copy_from_slice(primary);
        schedule.primary_count = primary.len() as u8;
        schedule.secondary[..secondary.len()].copy_from_slice(secondary);
        schedule.secondary_rows = secondary.len() as u8;
        schedule
    }

    pub fn primary(&self) -> &[i32] { &self.primary[..usize::from(self.primary_count)] }

    pub fn secondary(&self) -> &[i32] { &self.secondary[..usize::from(self.secondary_rows)] }
}

/// How the measurement lays out its secondary areas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AreaLayout {
    #[default]
    Single,
    /// One sample per secondary row.
    Listed,
    /// Every secondary area at the centre primary area's target.
    Grid,
    GridAlternate,
}

/// The drive a measurement run follows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Drive {
    /// The run moves the lens; a still run takes no target corrections.
    pub moving: bool,
    pub profile: DriveProfile,
    pub layout: AreaLayout,
    /// The run time, from its frame 0, at which the drive starts.
    pub start_time: i32,
    /// Frames the run had taken before its recorded positions begin.
    pub first_frame: u16,
    /// The exposure level lies in the window where the run checks for the contrast peak.
    pub checks_peak: bool,
}

/// The two frame exposures a preparation computes: the first corrected to the second's lens
/// step, the second the run's frames follow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameExposures {
    pub first: FrameExposure,
    pub second: FrameExposure,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionState {
    /// The focus section of the client's dump frame.
    pub(crate) section: SharedAddress,
    /// The contrast results in the client's dump frame.
    pub(crate) results: SharedAddress,
    pub(crate) prepared_at: Ticks,
    pub(crate) readings: RecordReadings,
    pub(crate) limits: StepLimits,
    pub(crate) search: SearchSetup,
    /// The listed areas in the search profile's frame; a treated run's in its second exposure's.
    pub(crate) areas: AreaList,
    pub(crate) exposures: FrameExposures,
    pub(crate) program: Program,
    pub(crate) previous: PreviousFocus,
    pub(crate) bounds: SearchBounds,
    pub(crate) statistics: Statistics,
    /// Configuration bytes a treated run samples with; zero for the others.
    pub(crate) treated_settings: [u8; 4],
    pub(crate) plan: Plan,
    pub(crate) sampling: SampleSchedule,
    pub(crate) drive: Drive,
}

impl MotionState {
    /// Microseconds per frame.
    pub fn frame_period(&self) -> u32 { self.exposures.second.time }
}
