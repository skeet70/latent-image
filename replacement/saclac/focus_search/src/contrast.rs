#![forbid(unsafe_code)]

//! Stock X-M1 1.50's contrast search, the first strategy in the #109 slot.

use xm1_saclac_controllers::search::LensPosition;

mod direction;
mod motion;
mod sample;
pub mod settings;
mod step;
mod tracks;

pub use sample::{BridgeDrive, Stage};
use settings::Setting;

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
    fn lens_position(&mut self) -> LensPosition;
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
    /// (0x74ab14 less 0x746944 and 0x746a80, rounded by 0x741fa0).
    fn focus_correction(&mut self, query: CorrectionQuery) -> i16;
    /// 0x746834.
    fn parameter_index(&mut self, parameter: u32) -> u32;
    /// 0xfa8018.
    fn fine_steps(&mut self) -> bool;
    /// The profile period 0x1005f60 reports for `class`.
    fn profile_period(&mut self, class: u32) -> u32;
    /// The record's profile class (0x757e80).
    fn record_class(&mut self) -> u32;
    /// 0x7492ac: the step and its rate the lens table gives for a base step.
    fn lens_step(&mut self, query: StepQuery) -> (u16, u32);
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
}

/// What 0xf44f58 asks the lens table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectionQuery {
    pub position: LensPosition,
    pub class: u32,
    pub lens: LensIdentity,
    pub parameter: u32,
    pub flags: u32,
}

/// What 0xfaab3c asks the lens table for a step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepQuery {
    pub lens: LensIdentity,
    pub parameter_index: u32,
    pub base: u16,
    /// The shifted profile period in hundredths.
    pub period: u32,
    pub lens_mode: u32,
    pub fine: bool,
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
    pub(crate) focus_started: bool,
    pub(crate) pacing: [u32; 3],
    pub(crate) stage: Stage,
    pub(crate) values: sample::Values,
    pub(crate) frame: sample::Frame,
    pub(crate) bridge: sample::Bridge,
    pub(crate) ring: sample::Ring,
    pub(crate) transition: sample::Transition,
    pub(crate) monitor: sample::Monitor,
    pub(crate) search: direction::Search,
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
            focus_started: false,
            pacing: [0, 1, 1],
            stage: Stage::Idle,
            values: sample::Values::new(),
            frame: sample::Frame::new(),
            bridge: sample::Bridge::new(),
            ring: sample::Ring::new(),
            transition: sample::Transition::new(),
            monitor: sample::Monitor::new(),
            search: direction::Search::new(),
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
