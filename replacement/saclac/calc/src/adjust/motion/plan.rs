//! The run's plan, which the pipeline installs before each pass (block +0xf0c): the drive and
//! sampling the plan decides, the pass results the pipeline records into it, and the two records
//! the plan writes beside it.

use crate::adjust::motion::drive::DriveProfile;

/// What a pass does, as the dump frame and the pipeline see it; it also indexes the stock kind
/// table (`stock::plan::kind_row`).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlanKind(pub u8);

impl PlanKind {
    /// No pass has been planned since the preparation.
    pub const FIRST: Self = Self(0);
    /// The run keeps its plan: the pipeline installs no new one.
    pub const SETTLED: Self = Self(3);
}

/// How the lens moves while a pass takes its frames.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Motion {
    /// Before the first plan.
    #[default]
    Unplanned,
    /// The lens drives through the pass while frames are taken; sample targets are corrected
    /// for the motion.
    Continuous,
    /// The lens moves one step per frame.
    Stepped,
}

impl Motion {
    pub const fn code(self) -> u32 {
        match self {
            Self::Unplanned => 0,
            Self::Continuous => 1,
            Self::Stepped => 2,
        }
    }
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

impl AreaLayout {
    pub const fn code(self) -> u32 {
        match self {
            Self::Single => 0,
            Self::Listed => 1,
            Self::Grid => 2,
            Self::GridAlternate => 3,
        }
    }
}

/// Calc 0x3c04's result as the pipeline records it into the plan (+0x9c, 27 words).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToneResult {
    pub words: [u32; 27],
}

/// The words 0xf9c8e0 sets, by index.
const TONE_RESET: [(usize, u32); 9] = [(0, 2), (1, 0), (4, 0x61), (5, 4), (6, 3), (8, 2), (9, 2), (11, 2), (12, 2)];

/// One area of a focus result: its lens position and its index in the 7-column grid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResultArea {
    pub position: i16,
    pub index: u16,
}

pub const RESULT_AREAS: usize = 3;

/// The focus result the plan carries (+0x108): calc 0x3c05's result as the pipeline records it,
/// or what the S1 result (0xf9c96c), its fallback (0xf9cd00) or a settled plan writes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusResult {
    /// 0 when a result stands, 2 when the run searches, 3 before either.
    pub kind: u32,
    pub position: u16,
    /// Row times seven plus column.
    pub area: u32,
    /// The two status words included in the focus diagnostic.
    pub diagnostic_status: [u32; 2],
    /// 9 marks a settled plan's result found.
    pub class: u32,
    pub pending: u32,
    pub treated_pending: u32,
    pub decision: u32,
    /// 1 once the pipeline's selected sample word reaches `Plan::speeds[2]` (0xf203f8).
    pub fast: u32,
    pub area_status: u32,
    pub area_count: u8,
    pub areas: [ResultArea; RESULT_AREAS],
    /// The 20 bytes the pipeline copies from +0x48.
    pub source: [u8; 20],
    pub retry_requested: u32,
}

/// What the plan tells the pipeline about the result (+0x168).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResultFlags {
    pub found: bool,
    pub searching: bool,
    /// 0 or 1 identifies a drive side; 2 means neither side was selected.
    pub side: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StartTarget {
    pub source: u32,
    pub position: i16,
    pub direction: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToneWindow {
    pub peak: bool,
    pub channel: bool,
    pub enabled: bool,
    pub below_level: bool,
    pub untreated: bool,
    pub single_cell: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    /// The kind decided before a scan kind replaced it.
    pub decided: PlanKind,
    pub motion: Motion,
    pub kind: PlanKind,
    /// The drive's ends, direction and phases.
    pub profile: DriveProfile,
    /// The index of the pass's last frame.
    pub last_frame: u16,
    /// Lens steps per frame, negative on a reverse drive.
    pub step: i16,
    /// Steps per second.
    pub speed: u32,
    /// Tenths of a step the drive covers in one frame period.
    pub frame_travel: u16,
    /// That travel as the lens's depth (0x749430).
    pub frame_depth: u16,
    /// Which of `speeds` the pass starts at.
    pub speed_choice: u8,
    /// The pass's slow, middle and fast speeds.
    pub speeds: [u32; 3],
    /// The pipeline compares its selected sample word with the fast speed.
    pub checks_speed: bool,
    /// The exposure level lies in the window where the run checks for the contrast peak.
    pub checks_peak: bool,
    /// Calc 0x3c04's window words +0x38 and +0x40.
    pub channel_window: bool,
    pub below_level: bool,
    pub window_enabled: bool,
    pub untreated_window: bool,
    pub single_cell_window: bool,
    pub layout: AreaLayout,
    /// A primary and a secondary sample cell's area in ten-thousandths of a search frame cell.
    pub primary_share: u32,
    pub secondary_share: u32,
    /// The three statistics weights the pipeline's scan request carries.
    pub weights: [u8; 3],
    pub target: StartTarget,
    /// The run time, from its frame 0, at which the drive starts.
    pub start_time: i32,
    /// Frames the run had taken before its recorded positions begin.
    pub first_frame: u16,
    pub decision: u32,
    /// The near bound of a forward pass over the wide channel class.
    pub near_bound: i16,
    pub tone: ToneResult,
    pub result: FocusResult,
    pub flags: ResultFlags,
    /// Words the pipeline keeps from calc 0x3c04's result once they are 1.
    pub tone_limits: [u32; 2],
    pub scan_requested: u32,
    /// Which of `speeds` the pass runs at, as the pipeline last chose.
    pub selection: u32,
}

impl Plan {
    /// Sets the pass results a new plan starts from (0xf9c8e0).
    pub fn reset_results(&mut self) {
        for (index, value) in TONE_RESET {
            self.tone.words[index] = value;
        }
        let result = &mut self.result;
        (result.kind, result.position, result.area) = (3, 0, 0x18);
        result.diagnostic_status = [4, 3];
        (result.pending, result.treated_pending) = (0, 0);
        self.flags = ResultFlags { found: false, searching: false, side: 2 };
        self.tone_limits = [2, 2];
        self.scan_requested = 0;
    }

    pub fn selected_speed(&self) -> Option<u32> {
        self.speeds.get(self.selection as usize).copied()
    }

    pub fn tone_window(&self) -> ToneWindow {
        ToneWindow {
            peak: self.checks_peak,
            channel: self.channel_window,
            enabled: self.window_enabled,
            below_level: self.below_level,
            untreated: self.untreated_window,
            single_cell: self.single_cell_window,
        }
    }
}

/// The record 0xfa0130 writes at block +0x498: configured contrast thresholds scaled by the
/// second exposure's scales and the plan's area shares, at one, two and three frame depths.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContrastThresholds {
    pub rows: [[u32; 3]; 5],
}

impl ContrastThresholds {
    /// The record's 18 words; the sixth row repeats the fifth.
    pub fn words(&self) -> [u32; 18] {
        let mut words = [0; 18];
        for (slot, value) in words.iter_mut().zip(self.rows.iter().chain(core::iter::once(&self.rows[4])).flatten()) {
            *slot = *value;
        }
        words
    }
}

/// The record 0xfa0130 writes at block +0x96c: configured values chosen by the plan's frame
/// depth. `halves` are +0, +6 and +8 (0x942d, 0x9437, 0x9439 below a depth of 151, else 0x943b,
/// 0x9440, 0x9442); `bytes` are +2, +3 and +4 (0x942c, 0x942b, 0x9436, else 0x943d, 0x943e,
/// 0x943f).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DepthSettings {
    pub halves: [u16; 3],
    pub bytes: [u8; 3],
}
