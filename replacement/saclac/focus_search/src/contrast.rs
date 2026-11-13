#![forbid(unsafe_code)]

//! Stock X-M1 1.50's contrast search, the first strategy in the #109 slot.

use xm1_saclac_controllers::search::LensPosition;

mod accept;
mod direction;
mod dispatch;
mod focus;
mod focus_drive;
mod log;
mod monitor;
mod motion;
mod peak;
mod peak_search;
mod retrace;
mod sample;
pub mod settings;
mod step;
mod tracks;

pub use dispatch::ContrastSearch;
pub use focus::FocusTrace;
pub use log::{FocusLog, FocusLogEntry, LoggedSample};
pub use peak_search::PeakSearchTrace;
pub(crate) use peak_search::{percent_of, recent_levels_high};
pub use sample::{BridgeDrive, CorrectedPosition, Stage};
use settings::{Setting, StepKind};

/// How the lens layer reports a lens move the monitor follows (0x76d9ec returns 1 for
/// `Ascending`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensMove {
    Ascending,
    Descending,
}

/// The phase the search is in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Direction,
    PeakSearch,
    Peak,
    Return,
    Focus,
    FocusDrive,
    Approach,
    Monitor,
}

/// The lens the lens layer reports (0x76d9c8): its table index, or 0 when the index is
/// outside the table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensIdentity(pub i32);

/// Which lens-table row the travel limits come from (record field read by 0x74f9a4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitKind(pub u32);

/// The travel range the search keeps the lens in, and the inner range the lens table gives
/// with it (0xfa767c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TravelLimits {
    pub low: LensPosition,
    pub high: LensPosition,
    pub inner_low: LensPosition,
    pub inner_high: LensPosition,
    /// The lens table's focus offset plus the lens bias (0x747374).
    pub offset: u16,
}

/// What the lens table is asked for one range (the inputs 0x74bd64 and 0x74c568 share).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitQuery {
    pub lens: LensIdentity,
    pub position: u16,
    /// `None` takes the record's own table (stock passes kind 0 and selector 0).
    pub table: Option<LimitKind>,
    pub flags: u32,
    pub auxiliary: u32,
    pub state: u32,
}

/// The record's offer of a retained focus (shared +0x140); the search claims it with 2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusOffer(pub u32);

/// The focus a previous search left in the record (shared +0x120).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetainedFocus {
    pub valid: u32,
    pub spread: u32,
    pub position: LensPosition,
    pub profile: u32,
}

/// The search's status in the record (shared +0x84), which the shot reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusStatus {
    pub active: u32,
    pub mode: u32,
    pub position: LensPosition,
    pub snapshot: [u32; 2],
    pub flag: u32,
    pub stage: u32,
}

/// The search stage the record holds (shared +0x74c).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StageRecord {
    pub stage: u32,
    pub first_pass: u8,
    pub words: [u32; 2],
    pub halves: [u16; 2],
    pub last: u8,
}

pub trait Platform {
    /// The lens position word (0x76da84).
    fn lens_position(&mut self) -> LensPosition;
    /// 0x76d9c8.
    fn lens_identity(&mut self) -> LensIdentity;
    /// The lens-table range count (adjustment 0x3e1214); identities at or above it fall back
    /// to table 1.
    fn table_count(&mut self) -> u32;
    /// The record's lens-table selector (0x74f9a4).
    fn limit_kind(&mut self) -> LimitKind;
    /// Whether the record pins the default table (0x7503ec returns 1).
    fn pinned_table(&mut self) -> bool;
    /// The record's reference position for the table (0xfa7634).
    fn table_position(&mut self) -> u16;
    fn lens_flags(&mut self) -> u32;
    fn record_auxiliary(&mut self) -> u32;
    fn lens_state(&mut self) -> u32;
    fn lens_bias(&mut self, lens: LensIdentity) -> u16;
    /// 0x74bd64: the outer range.
    fn outer_limits(&mut self, query: LimitQuery) -> (LensPosition, LensPosition);
    /// 0x74c568, which also reads the lens reference 0x76dbb4: the inner range.
    fn inner_limits(&mut self, query: LimitQuery) -> (LensPosition, LensPosition);
    /// The lens table's focus offset for `kind` (adjustment 0x3e07f7, 0x3e08c0 or 0x3e0989
    /// plus twice the lens).
    fn table_offset(&mut self, lens: LensIdentity, kind: TableOffset) -> u16;

    fn focus_offer(&mut self) -> FocusOffer;
    fn claim_focus_offer(&mut self);
    fn retained_focus(&mut self) -> RetainedFocus;
    fn measurement_selection(&mut self) -> u8;
    /// Record field read by 0x75060c.
    fn record_selected(&mut self) -> u32;
    /// The record profile 0x757b78 reads.
    fn record_profile(&mut self) -> u32;
    fn publish_status(&mut self, status: FocusStatus);
    fn store_stage(&mut self, stage: StageRecord);
    /// Sets bit 1 of the record's statistics-in-use word.
    fn claim_image_statistics(&mut self);
    /// The focus log reset shared with the shot (0xf47468).
    fn reset_focus_log(&mut self);

    /// Property 0xe8 bit 4.
    fn tracing(&mut self) -> bool;
    fn trace_begin(&mut self, retained: LensPosition, focus: LensPosition);

    /// A camera adjustment (0x69f8fc, or 0x69fec8 for the halfword ones).
    fn setting(&mut self, setting: Setting) -> u32;
    /// 0x76f360.
    fn lens_mode(&mut self) -> u32;
    /// 0x76da38 returns 1.
    fn lens_busy(&mut self) -> bool;
    /// 0x76dac4 returns 1.
    fn lens_ready(&mut self) -> bool;
    /// The record's two levels for its focus area (0x7567ec's area table, +0x204 and +0x206).
    fn area_levels(&mut self) -> [u16; 2];
    /// The lens table's correction of a focus value for a lens position, in tenths
    /// (0x74ab14 with the lens word 0x76f340, less 0x746944 and 0x746a80, rounded by
    /// 0x741fa0).
    fn focus_correction(&mut self, query: CorrectionQuery) -> i16;
    /// The profile period 0x1005f60 reports for `class`.
    fn profile_period(&mut self, class: u32) -> u32;
    /// The record's profile class (0x757e80).
    fn record_class(&mut self) -> u32;
    /// The lens step a phase moves by and the rate the bridge is told with it (0xfaab3c with
    /// its base step 0xfab390).
    fn step(&mut self, query: StepQuery) -> Step;
    /// A monitor's step scaled by how far the scene moved (0xfaa928).
    fn monitor_step(&mut self, query: MonitorStepQuery) -> u16;
    /// Asks the calc planner to refresh the scan drive, or withdraws the request (0xfa9554).
    fn request_scan_refresh(&mut self, requested: bool);
    /// Whether the scan-drive refresh stands (0xfa9588).
    fn scan_refresh(&mut self) -> bool;
    /// Plans a scanning peak search's drive (0xf3ec50's calls into the calc planner, whose
    /// plan stays with the planner).
    fn plan_scan(&mut self, query: ScanQuery) -> ScanPlan;
    /// Where the planned scan drive had the frame and the lens when the evaluated frame was
    /// taken (0xf3edcc).
    fn corrected_position(&mut self, frame_mark: u16) -> CorrectedPosition;
    /// The drive target for `amount` within `low..=high` (0xfa7ce0).
    fn drive_target(&mut self, lens: LensIdentity, amount: u16, low: LensPosition, high: LensPosition) -> LensPosition;
    /// Whether `low` plus `percent` of `high` and the configured margin stays below `value`
    /// (0xfa9320).
    fn below_threshold(&mut self, value: u32, high: u32, low: u32, percent: u32) -> bool;
    /// Whether `series` has passed its peak, and the peak it fell from (0xfa936c).
    fn passed_peak(&mut self, series: &[u32], percent: u32, recent_peak: bool) -> (bool, Option<u32>);
    /// Whether `series` fell from its peak far enough (0xfa9488).
    fn fell_from_peak(&mut self, series: &[u32]) -> bool;
    /// The library's scaled logarithm of a contrast (0xee9928).
    fn contrast_log(&mut self, value: u32) -> u32;
    /// The position where the parabola through three samples peaks (0xf8e9e4 over the curve
    /// 0xf3e63c builds).
    fn fit_peak(&mut self, points: [FitPoint; 3]) -> LensPosition;
    /// The five newest measurements' area levels, newest first, `None` for an empty entry;
    /// `None` while the measurement history is not kept (0x7567ec).
    fn measurement_history(&mut self) -> Option<[Option<[u16; 2]>; 5]>;
    /// The record's focus word (0x757470).
    fn profile_focus(&mut self) -> i16;
    /// Property 0xe8 bit 5, which with bit 4 traces each peak search step.
    fn detailed_tracing(&mut self) -> bool;
    fn trace_peak_search(&mut self, trace: PeakSearchTrace);
    /// Property 0xe8 bit 6, which with bit 4 traces each focus step.
    fn focus_tracing(&mut self) -> bool;
    fn trace_focus(&mut self, trace: FocusTrace);
    /// The configured minimum step, at least `minimum` (0xfa7614).
    fn minimum_step(&mut self, minimum: u32) -> u32;
    /// The record's mode word (profile shared +0x14c, 0x756ac4).
    fn profile_mode(&mut self) -> u32;
    /// The lens table's label of the intake's parameter (0x7469a0 of the parameter modulo
    /// 0x74677c).
    fn parameter_label(&mut self, parameter: u32) -> u16;
    /// The dispatch tick count (0x7257a8).
    fn ticks(&mut self) -> u32;
    /// Writes `log` at the focus log's next slot and advances it (0xf47598's stores).
    fn append_focus_log(&mut self, log: &FocusLog);
    /// The entry in the slot before the focus log's next one.
    fn newest_focus_log(&mut self) -> FocusLogEntry;
    /// Shows `text` on the overlay's debug view at `column` and `row` (0x1110918).
    fn show_debug_text(&mut self, text: &[u8], column: u8, row: u8);
    /// The angular-rate sample the lens reports (0x6b74e4), two axes centred on 0x8000.
    fn angular_rate(&mut self) -> u32;
    /// Asks the lens for the next angular-rate sample (0x6b7660).
    fn request_angular_rate(&mut self);
    /// The record's stage (shared +0x74c, 0x758068).
    fn stage(&mut self) -> StageRecord;
    /// Words 0xf8 and 0x110 of the measurement snapshot 0xfea7ac(1) copies.
    fn status_snapshot(&mut self) -> [u32; 2];
    /// The pinned record's status fields (0x7588e8, 0x75890c) and the shot's copy of the
    /// reported position (0xf4dd98).
    fn publish_pinned_status(&mut self, status: PinnedStatus);
    fn trace_transition(&mut self, trace: TransitionTrace);
    /// The lens move the monitor follows once the lens identity changed: `None` unless
    /// 0x76d9a8 returns 1.
    fn lens_move(&mut self) -> Option<LensMove>;
    /// The lens bias at `position` for a lens that moved as `moved` (0xfaa7f4).
    fn interpolated_bias(&mut self, lens: LensIdentity, moved: LensMove, position: LensPosition) -> i16;
}

/// What 0xf44f58 asks the lens table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectionQuery {
    pub position: LensPosition,
    pub class: u32,
    pub lens: LensIdentity,
    pub parameter: u32,
}

/// What a step is asked for (0xfaab3c's arguments after the record).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepQuery {
    pub kind: StepKind,
    /// The intake's parameter word.
    pub parameter: u32,
    /// The channel contrast the phase reads; a low one takes the low-contrast base step.
    pub contrast: u32,
    /// The profile class the intake took from the capture pipeline.
    pub profile_class: u32,
    pub lens_mode: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Step {
    pub size: u16,
    pub rate: u32,
}

/// What 0xfaa928 scales a monitor's step by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorStepQuery {
    pub lens: LensIdentity,
    pub moved: Option<LensMove>,
    pub step: u16,
    pub mark: i16,
}

/// What a scanning peak search plans its drive from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanQuery {
    /// The monitor runs the scan.
    pub watching: bool,
    pub contrast: u32,
    pub lens_mode: u32,
    pub direction: i8,
    pub rate: u32,
    pub limits: TravelLimits,
    pub frame: ScanFrame,
    pub moved: Option<LensMove>,
    pub mark: i16,
}

/// The intake's words a scan plan reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScanFrame {
    pub parameter_index: u32,
    pub stage_halves: [u16; 2],
    pub current_lens: u32,
    pub stage_last: u8,
}

/// What a scan plan gives the search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanPlan {
    /// The travel limits narrowed by the scan's run-up.
    pub limits: [LensPosition; 2],
    /// The drive's start and end the bridge is told.
    pub window: [u16; 2],
    /// How far the lens runs on after a frame.
    pub lead: u16,
    /// The scan step's rate, which the next bridge request carries.
    pub rate: u32,
}

/// A contrast sample and the lens position it was taken at, as the peak fit keeps them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FitPoint {
    pub contrast: u32,
    pub position: i16,
}

impl FitPoint {
    pub(crate) const EMPTY: Self = Self { contrast: u32::MAX, position: 0x7fff };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinnedStatus {
    pub level: u32,
    pub mode: u32,
    pub position: LensPosition,
    pub reported: LensPosition,
}

/// The values a transition traces when tracing is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionTrace {
    pub phase: Phase,
    pub limits: [LensPosition; 2],
    pub marks: [u16; 2],
    pub lens: LensPosition,
    pub position: LensPosition,
    pub fit: u16,
    pub level: u32,
    pub direction: i8,
    pub speed: u32,
    pub focus: LensPosition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableOffset {
    Default,
    First,
    Second,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Context {
    pub(crate) phase: Phase,
    pub(crate) initial: Phase,
    pub(crate) accepted: bool,
    pub(crate) samples: u8,
    pub(crate) retained_beyond_high: bool,
    pub(crate) hold: bool,
    pub(crate) limits: TravelLimits,
    pub(crate) step: u16,
    pub(crate) direction: i8,
    pub(crate) previous_direction: i8,
    /// Newest first: the lens position at this sample and the two before.
    pub(crate) positions: [LensPosition; 3],
    pub(crate) reported: LensPosition,
    pub(crate) lens: LensIdentity,
    pub(crate) previous_lens: LensIdentity,
    /// 0xff when initialization finds the previous search's lens, else 0; the direction
    /// search counts it.
    pub(crate) lens_count: u16,
    /// Set when initialization finds another lens than the previous search's.
    pub(crate) lens_changed: bool,
    /// Newest first: the reported lens classes.
    pub(crate) classes: [u32; 3],
    pub(crate) reported_class: u32,
    pub(crate) focus_entered: bool,
    pub(crate) focus_position: LensPosition,
    /// The bridge speed of the drive to `focus_position`.
    pub(crate) focus_speed: u32,
    pub(crate) focus_started: bool,
    /// The evaluation record's scan flag (+0x48), which the intake carries into the next
    /// record: while it stands a peak search reads the scan pairs and a return follows the
    /// focus values.
    pub(crate) recent_peak: bool,
    pub(crate) corrected: CorrectedPosition,
    /// The settled near word when the peak search last looked.
    pub(crate) watch_reference: i16,
    pub(crate) focus: focus::Focus,
    pub(crate) display: log::DebugDisplay,
    pub(crate) pacing: [u32; 3],
    pub(crate) stage: Stage,
    pub(crate) values: sample::Values,
    pub(crate) frame: sample::Frame,
    pub(crate) bridge: sample::Bridge,
    pub(crate) ring: sample::Ring,
    pub(crate) transition: sample::Transition,
    pub(crate) monitor: sample::Monitor,
    pub(crate) search: direction::Search,
    pub(crate) peak: peak_search::PeakSearch,
    pub(crate) motion: motion::Motion,
    pub(crate) tracks: tracks::Tracks,
}

const NO_LIMITS: TravelLimits = TravelLimits {
    low: LensPosition(0),
    high: LensPosition(0),
    inner_low: LensPosition(0),
    inner_high: LensPosition(0),
    offset: 0,
};

impl Context {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Direction,
            initial: Phase::Direction,
            accepted: false,
            samples: 0,
            retained_beyond_high: false,
            hold: false,
            limits: NO_LIMITS,
            step: 0,
            direction: 1,
            previous_direction: 1,
            positions: [LensPosition(0); 3],
            reported: LensPosition(0),
            lens: LensIdentity(0),
            previous_lens: LensIdentity(0),
            lens_count: 0,
            lens_changed: false,
            classes: [2; 3],
            reported_class: 2,
            focus_entered: false,
            focus_position: LensPosition(0),
            focus_speed: 0,
            focus_started: false,
            recent_peak: false,
            corrected: CorrectedPosition { frame: LensPosition(0), drive: LensPosition(0) },
            watch_reference: 0,
            focus: focus::Focus::new(),
            display: log::DebugDisplay::new(),
            pacing: [0, 1, 1],
            stage: Stage::Idle,
            values: sample::Values::new(),
            frame: sample::Frame::new(),
            bridge: sample::Bridge::new(),
            ring: sample::Ring::new(),
            transition: sample::Transition::new(),
            monitor: sample::Monitor::new(),
            search: direction::Search::new(),
            peak: peak_search::PeakSearch::new(),
            motion: motion::Motion::new(),
            tracks: tracks::Tracks::new(),
        }
    }

    /// 0xf3c17c: forget the previous search, keep the lens it was on, and publish an
    /// inactive status.
    pub fn initialize(&mut self, platform: &mut impl Platform) {
        self.accepted = false;
        self.samples = 0;
        self.hold = false;
        self.step = 0;
        self.direction = 1;
        self.previous_direction = 1;
        let position = platform.lens_position();
        self.positions = [position, position, self.positions[2]];
        self.reported = position;
        self.classes = [2; 3];
        self.reported_class = 2;
        let same_lens = self.lens == platform.lens_identity();
        self.lens_changed = !(same_lens && !self.lens_changed);
        self.lens_count = if self.lens_changed { 0 } else { 0xff };
        self.forget_samples();
        if self.initial == Phase::Focus {
            self.phase = Phase::Focus;
            self.focus_entered = true;
            self.focus_position = platform.lens_position();
            self.focus_started = true;
        } else {
            self.phase = Phase::Direction;
        }
        self.lens = platform.lens_identity();
        self.previous_lens = self.lens;
        self.pacing = [0, 1, 1];
        platform.store_stage(StageRecord::default());
        platform.publish_status(FocusStatus {
            active: 0,
            mode: 4,
            position: LensPosition(0),
            snapshot: [0; 2],
            flag: 0,
            stage: 2,
        });
        platform.claim_image_statistics();
        if platform.measurement_selection() != 1 {
            platform.reset_focus_log();
        }
    }

    /// 0xf3e6f0: start in focus when the record retains a focus taken on this lens and
    /// profile, otherwise in direction search.
    pub fn choose_start(&mut self, platform: &mut impl Platform) -> Phase {
        let start = self.start_phase(platform);
        self.initial = start;
        start
    }

    fn start_phase(&mut self, platform: &mut impl Platform) -> Phase {
        let offer = platform.focus_offer();
        if offer.0 != 0 {
            if offer.0 == 1 {
                self.retained_beyond_high = false;
            }
            return Phase::Direction;
        }
        platform.claim_focus_offer();
        let retained = platform.retained_focus();
        if retained.valid == 0 {
            return Phase::Direction;
        }
        self.retained_beyond_high = false;
        let selection = platform.measurement_selection();
        let selected = platform.record_selected();
        let profile = platform.record_profile();
        if let Some(limits) = travel_limits(platform, false) {
            self.limits = limits;
        }
        self.retained_beyond_high = retained.position > self.limits.high;
        let lens = platform.lens_identity();
        let start = if self.lens != lens || self.previous_lens != platform.lens_identity() || self.lens_changed || retained.spread > 1 {
            Phase::Direction
        } else if selection == 1 || selected != 1 || retained.profile == profile {
            Phase::Focus
        } else {
            Phase::Direction
        };
        if platform.tracing() {
            platform.trace_begin(retained.position, self.focus_position);
        }
        start
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// 0xfa767c: the travel range for the lens on the record, or `None` without a lens.
/// `force` takes the record's own table as a pinned record does.
pub fn travel_limits(platform: &mut impl Platform, force: bool) -> Option<TravelLimits> {
    let mut kind = platform.limit_kind();
    let position = platform.table_position();
    let flags = platform.lens_flags();
    let lens = platform.lens_identity();
    if lens.0 < 0 {
        return None;
    }
    if kind.0 == 2 && lens.0 > platform.table_count() as i32 - 1 {
        kind = LimitKind(1);
    }
    let bias = platform.lens_bias(lens);
    let auxiliary = platform.record_auxiliary();
    let state = platform.lens_state();
    let table = if platform.pinned_table() || force { None } else { Some(kind) };
    let query = LimitQuery { lens, position, table, flags, auxiliary, state };
    let (low, high) = platform.outer_limits(query);
    let (inner_low, inner_high) = platform.inner_limits(query);
    let offset_kind = if platform.pinned_table() {
        TableOffset::Default
    } else {
        match kind.0 {
            1 => TableOffset::First,
            2 => TableOffset::Second,
            _ => TableOffset::Default,
        }
    };
    let offset = platform.table_offset(lens, offset_kind).wrapping_add(bias);
    Some(TravelLimits { low, high, inner_low, inner_high, offset })
}
