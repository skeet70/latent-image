//! The motion layer's state: the dump-frame section it traces into, the lens's drive kind, the
//! frame timing, the area sampling and the planned drive.

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::drive::{DriveKind, DriveProfile};
use crate::adjust::motion::sampling::{PRIMARY_AREAS, SECONDARY_AREAS};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Timing {
    /// Microseconds per frame.
    pub frame_period: u32,
}

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionState {
    /// The focus section of the client's dump frame.
    pub(crate) section: SharedAddress,
    pub(crate) drive_kind: DriveKind,
    pub(crate) timing: Timing,
    pub(crate) sampling: SampleSchedule,
    pub(crate) drive: Drive,
}
