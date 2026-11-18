#![forbid(unsafe_code)]

//! What every evaluation does around the phase step (machine 23's evaluation actions), and
//! what the search tells the lens bridge.

use super::settings::Setting;
use super::{Context, FocusStatus, LensMove, Phase, PinnedStatus, Platform, ScanFrame, TransitionTrace};
use xm1_saclac_controllers::search::{Direction, LensPosition, SearchStep, Travel};
use xm1_saclac_interface::search::{BridgePhase, BridgeStatus};

/// Machine 23's searching states. `PeakMoved` and `FocusDriveMoved` are the peak and focus
/// drive phases after their first move.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Idle,
    Direction,
    PeakSearch,
    Peak,
    PeakMoved,
    Return,
    Focus,
    Approach,
    FocusDrive,
    FocusDriveMoved,
    Monitor,
}

/// What the search tells the lens bridge beside the travel (0xf3e9cc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeDrive {
    pub phase: BridgePhase,
    pub scanning: bool,
    pub window: [u16; 2],
    pub rate: u32,
    /// False for a request that keeps the lens where it is; its travel is then zero.
    pub moves: bool,
    pub speed: u32,
    pub lens_mode: u32,
}

/// The evaluation as the search keeps it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Values {
    /// Ten times the first pair's second sum over its first, or a configured level.
    pub level: u32,
    pub previous_level: u32,
    /// Each pair's two sums added when the evaluation is paired, else its first sum.
    pub combined: [u32; 3],
    pub seconds: [u32; 3],
    pub levels: [u16; 2],
    pub reference: u32,
    /// The evaluation's settled word is 0.
    pub unsettled: bool,
    pub flag: u32,
    pub scan_pairs: [[u32; 2]; 3],
    pub scan_reference: u32,
    pub scan_levels: [u16; 2],
    pub focus_raw: u16,
    pub focus_value: u16,
    pub focus_correction: i16,
    /// The channel the phase steps read, and its partner.
    pub contrast: u32,
    pub contrast_other: u32,
}

impl Values {
    pub(crate) const fn new() -> Self {
        Self {
            level: 0,
            previous_level: 0,
            combined: [0; 3],
            seconds: [0; 3],
            levels: [0; 2],
            reference: 0,
            unsettled: false,
            flag: 0,
            scan_pairs: [[0; 2]; 3],
            scan_reference: 0,
            scan_levels: [0; 2],
            focus_raw: 0,
            focus_value: 0,
            focus_correction: 0,
            contrast: 0,
            contrast_other: 0,
        }
    }

    fn forget(&mut self) {
        *self = Self {
            combined: self.combined,
            seconds: self.seconds,
            levels: self.levels,
            reference: self.reference,
            unsettled: self.unsettled,
            contrast: self.contrast,
            contrast_other: self.contrast_other,
            ..Self::new()
        };
    }
}

/// What the intake took in with the frame the evaluation is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    pub selected: u32,
    pub previous_selected: u32,
    pub selected_samples: u8,
    pub pinned: u32,
    pub parameter: u32,
    pub status_flag: u32,
    pub profile_class: u32,
    /// The frame's lens position is taken from the scan drive while the refresh stands.
    pub corrects_position: bool,
    pub correction_source: i16,
    pub correction_class: u32,
    /// The record word the debug display checks (0x74fa2c).
    pub record_mode: u32,
    /// The drive amount of a selected record.
    pub pinned_setting: u16,
    pub mark: u16,
    /// The intake record's word +0xc (0xfaab18).
    pub step_class: u32,
    /// The settled near word; a peak search restarts when it moves.
    pub watch: i16,
    /// A scanning peak search may run.
    pub scan_allowed: bool,
    pub scan: ScanFrame,
    /// The measurement snapshot's two level halfwords.
    pub levels: [u16; 2],
    /// The intake's copy of the recent-peak word.
    pub scan_flag: u8,
}

impl Frame {
    pub(crate) const fn new() -> Self {
        Self {
            selected: 0,
            previous_selected: 0,
            selected_samples: 0,
            pinned: 0,
            parameter: 0,
            status_flag: 0,
            profile_class: 0,
            corrects_position: false,
            correction_source: 0,
            correction_class: 0,
            record_mode: 0,
            pinned_setting: 0,
            mark: 0,
            step_class: 0,
            watch: 0,
            scan_allowed: false,
            scan: ScanFrame { parameter_index: 0, stage_halves: [0; 2], current_lens: 0, stage_last: 0 },
            levels: [0; 2],
            scan_flag: 0,
        }
    }
}

/// The positions the scan drive gave the evaluated frame (0xf3edcc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectedPosition {
    /// Where the frame was taken.
    pub frame: LensPosition,
    /// Where the drive was then.
    pub drive: LensPosition,
}

/// What the next bridge request carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Bridge {
    pub speed: u32,
    pub window: [u16; 2],
    /// How far a scan runs on after a frame.
    pub lead: u16,
    pub lens_mode: u32,
    pub rate: u32,
    /// A peak search that evaluates while the lens moves.
    pub scanning: bool,
    pub scan: ScanState,
    /// Set by each scanning sample after the one that planned the drive; while clear, a
    /// scanning peak search asks nothing of the bridge.
    pub scan_sampled: bool,
    /// The status of the bridge's newest report, until a direction search clears it.
    pub last_report: BridgeStatus,
}

impl Bridge {
    pub(crate) const fn new() -> Self {
        Self {
            speed: 0,
            window: [0; 2],
            lead: 0,
            lens_mode: 0,
            rate: 0,
            scanning: false,
            scan: ScanState::Running,
            scan_sampled: false,
            last_report: BridgeStatus::Moved,
        }
    }
}

/// Why a scanning peak search last kept or stopped its drive; the trace reports the code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanState {
    Running,
    PassedWithoutRise,
    Turned,
    SelectionChanged,
    FrameLate,
    Peaked,
    Bounded,
    LevelJumped,
    RecentPeak,
    LensChanged,
    Stopped,
    Flagged,
    SecondChannel,
}

impl ScanState {
    pub(crate) const fn code(self) -> u32 {
        match self {
            Self::Running => 0,
            Self::PassedWithoutRise => 1,
            Self::Turned => 2,
            Self::SelectionChanged => 3,
            Self::FrameLate => 4,
            Self::Peaked => 5,
            Self::Bounded => 6,
            Self::LevelJumped => 7,
            Self::RecentPeak => 8,
            Self::LensChanged => 9,
            Self::Stopped => 10,
            Self::Flagged => 11,
            Self::SecondChannel => 12,
        }
    }
}

/// One evaluation as the channel ring keeps it (0xf43c98).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RingEntry {
    pub contrast: u32,
    pub area_levels: [u32; 2],
    pub focus_value: u16,
    pub position: LensPosition,
    pub class: u32,
}

const RING_ENTRY: RingEntry = RingEntry { contrast: 0, area_levels: [0; 2], focus_value: 0, position: LensPosition(0), class: 0 };

/// The last seven evaluations, oldest first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ring {
    pub entries: [RingEntry; Ring::CAPACITY],
    pub newest: Option<usize>,
}

impl Ring {
    pub(crate) const CAPACITY: usize = 7;

    pub(crate) const fn new() -> Self { Self { entries: [RING_ENTRY; Self::CAPACITY], newest: None } }

    /// Appends `entry`; a full ring drops its oldest.
    pub(crate) fn push(&mut self, entry: RingEntry) {
        let slot = match self.newest {
            None => 0,
            Some(newest) if newest + 1 < Self::CAPACITY => newest + 1,
            Some(_) => {
                self.entries.copy_within(1.., 0);
                Self::CAPACITY - 1
            }
        };
        self.entries[slot] = entry;
        self.newest = Some(slot);
    }

    /// The newest entry, or an empty one before the first sample.
    pub(crate) fn newest_entry(&self) -> RingEntry { self.newest.map_or(RING_ENTRY, |index| self.entries[index]) }
}

/// What the phase steps leave for the transition's status report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Transition {
    pub restarted: u32,
    pub focus_ready: u32,
    pub return_ready: u32,
    /// 1 holds in focus, 2 drives to focus; a direction step clears it.
    pub hold_mode: u8,
    pub focus_reported: u32,
    pub return_stage: u32,
    pub peak_stage: u32,
    pub cleared: u32,
}

impl Transition {
    pub(crate) const fn new() -> Self {
        Self { restarted: 0, focus_ready: 0, return_ready: 0, hold_mode: 0, focus_reported: 0, return_stage: 0, peak_stage: 0, cleared: 0 }
    }
}

/// What the monitor runs to follow the lens (0xf467cc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Watch {
    /// The lens has not moved since the monitor began.
    Scene,
    PeakSearch,
    Return,
}

/// The scene watch after a suspension (0xf3e32c, 0xf467cc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Monitor {
    pub active: bool,
    pub watch: Watch,
    /// A peak search the monitor ran reached its peak; it waits for the lens to move again.
    pub settled: bool,
    /// Set by a resumption: the next direction decision returns when the monitor last
    /// followed the lens with a return.
    pub restart: bool,
    pub moved: Option<LensMove>,
    /// The lens bias at the lens position after the move (0xfaa7f4).
    pub mark: i16,
}

impl Monitor {
    pub(crate) const fn new() -> Self { Self { active: false, watch: Watch::Scene, settled: false, restart: false, moved: None, mark: 0 } }

    pub(crate) const fn following_lens(&self) -> bool { matches!(self.watch, Watch::Scene | Watch::Return) }
}

/// What a range check found (0xf3e280).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Range {
    NoLens,
    Ready,
}

/// The stage kinds a transition reports and the record stores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StageKind {
    Direction,
    PeakSearch,
    Peak,
    Return,
    Focus,
    FocusDrive,
    Approach,
    Other,
}

impl StageKind {
    const fn code(self) -> u32 {
        match self {
            Self::Direction => 0,
            Self::PeakSearch => 1,
            Self::Peak => 2,
            Self::Return => 3,
            Self::Focus => 4,
            Self::FocusDrive => 5,
            Self::Approach => 6,
            Self::Other => 7,
        }
    }
}

impl Context {
    /// The sample-path state 0xf3c17c clears.
    pub(crate) fn forget_samples(&mut self) {
        self.values.forget();
        self.ring = Ring::new();
        self.transition.restarted = 0;
        self.transition.cleared = 0;
        self.transition.focus_ready = 0;
        self.transition.return_ready = 0;
        self.transition.focus_reported = 0;
        self.frame.selected = 0;
        self.frame.previous_selected = 0;
        self.frame.selected_samples = 0;
        self.frame.pinned_setting = 0;
        self.frame.scan_flag = 0;
        self.peak.directed = false;
        self.focus.contrast = 0;
        self.forget_direction();
    }

    /// 0xf3e280 and 0xf3e1f8: refresh the travel limits, count the sample, and find whether
    /// a lens is there. A scanning peak search keeps its limits.
    pub(crate) fn check_range(&mut self, platform: &mut impl Platform) -> Range {
        if !(self.phase == Phase::PeakSearch && self.bridge.scanning) {
            match super::travel_limits(platform, self.retained_beyond_high) {
                Some(limits) => self.limits = limits,
                None => return Range::NoLens,
            }
        }
        self.samples = match self.samples.wrapping_add(1) {
            0xff => 0xfe,
            count => count,
        };
        Range::Ready
    }

    /// 0xf43d94: choose the channel the phase steps read, then ring the sample.
    pub(crate) fn choose_channel(&mut self, platform: &mut impl Platform) {
        let [first, second] = if self.recent_peak && self.phase == Phase::PeakSearch {
            [self.values.scan_pairs[0], self.values.scan_pairs[1]]
        } else {
            [[self.values.combined[0], self.values.seconds[0]], [self.values.combined[1], self.values.seconds[1]]]
        };
        let column = if self.search.second_channel {
            1
        } else if self.frame.selected != 0 {
            0
        } else {
            let setting = if self.phase == Phase::Direction { Setting::DirectionSecondChannel } else { Setting::SecondChannel };
            usize::from(platform.setting(setting) == 1)
        };
        self.values.contrast = first[column];
        self.values.contrast_other = second[column];
        let levels = platform.area_levels();
        self.ring.push(RingEntry {
            contrast: self.values.contrast,
            area_levels: [u32::from(levels[0]), u32::from(levels[1])],
            focus_value: self.values.focus_value,
            position: self.positions[2],
            class: self.classes[2],
        });
    }

    /// 0xf3d490: consume a pending restart, which the transition turns into a direction search.
    pub(crate) fn take_restart(&mut self) -> bool {
        if !self.search.restart_pending && !self.search.second_channel {
            return false;
        }
        self.search.restart_pending = false;
        self.search.second_channel = false;
        self.samples = 0;
        self.direction = self.previous_direction;
        self.step = 0;
        true
    }

    /// 0xf40300: enter the phase a step chose, report the stage, and remember what the next
    /// transition compares against.
    pub(crate) fn enter(&mut self, platform: &mut impl Platform, restart: bool, chosen: Phase) -> Phase {
        let (next, kind, mut mode) = if restart {
            (Phase::Direction, StageKind::Direction, 4)
        } else {
            match chosen {
                Phase::Direction => (Phase::Direction, StageKind::Direction, 4),
                Phase::PeakSearch => (Phase::PeakSearch, StageKind::PeakSearch, 4),
                Phase::Peak => (Phase::Peak, StageKind::Peak, 4),
                Phase::Return => (Phase::Return, StageKind::Return, 3),
                Phase::Focus => (Phase::Focus, StageKind::Focus, 0),
                Phase::FocusDrive => (Phase::FocusDrive, StageKind::FocusDrive, 2),
                Phase::Approach => (Phase::Approach, StageKind::Approach, 3),
                Phase::Monitor => (Phase::Monitor, self.monitor_kind(), 4),
            }
        };
        if self.frame.status_flag == 1 && self.transition.restarted == 1 {
            if matches!(next, Phase::Approach | Phase::Return) && self.transition.return_ready == 0 {
                mode = 4;
            }
            if matches!(next, Phase::Focus | Phase::FocusDrive) && self.transition.focus_ready == 0 {
                mode = 4;
            }
        }
        match (self.transition.hold_mode, next) {
            (1, Phase::Focus) => mode = 1,
            (2, Phase::Focus) => mode = 4,
            _ => {}
        }
        let snapshot = platform.status_snapshot();
        let stage = self.stage_report(platform, kind);
        platform.publish_status(FocusStatus { active: 1, mode, position: self.focus_position, snapshot, flag: self.transition.restarted, stage });
        if platform.pinned_table() {
            platform.publish_pinned_status(PinnedStatus {
                level: self.values.combined[0],
                mode,
                position: self.focus_position,
                reported: self.reported,
            });
        }
        self.phase = next;
        platform.request_scan_refresh(kind == StageKind::PeakSearch);
        let mut record = platform.stage();
        record.stage = kind.code();
        if kind != StageKind::PeakSearch {
            record.first_pass = 0;
        }
        platform.store_stage(record);
        self.frame.previous_selected = self.frame.selected;
        self.previous_lens = self.lens;
        self.pacing[2] = self.pacing[1];
        if platform.tracing() {
            let lens = platform.lens_position();
            platform.trace_transition(TransitionTrace {
                phase: next,
                limits: [self.limits.low, self.limits.high],
                marks: [self.frame.mark, self.bridge.lead],
                lens,
                position: self.positions[2],
                fit: self.search.fit,
                level: self.values.combined[0],
                direction: self.direction,
                speed: self.bridge.speed,
                focus: self.focus_position,
            });
        }
        next
    }

    fn monitor_kind(&self) -> StageKind {
        match self.monitor.watch {
            Watch::Return => StageKind::Return,
            Watch::PeakSearch => StageKind::PeakSearch,
            Watch::Scene => StageKind::Other,
        }
    }

    /// 0xf40158: the stage the status reports for `kind`.
    fn stage_report(&mut self, platform: &mut impl Platform, kind: StageKind) -> u32 {
        if self.frame.status_flag == 1 {
            return 1;
        }
        let report = platform.setting(Setting::StageReport);
        if report == 0 {
            return 2;
        }
        let stage = match kind {
            StageKind::Direction if report & 2 == 0 => 2,
            StageKind::Direction => match (self.search.rising, self.search.falling) {
                (0, 0) => 2,
                (rising, falling) if rising == falling => 2,
                (rising, falling) => u32::from(rising <= falling),
            },
            StageKind::PeakSearch if report & 8 == 0 => 2,
            StageKind::PeakSearch => self.transition.peak_stage,
            StageKind::Return | StageKind::Approach if report & 4 == 0 => 2,
            StageKind::Return | StageKind::Approach => self.transition.return_stage,
            StageKind::Peak | StageKind::Focus | StageKind::FocusDrive => {
                if self.focus_started {
                    2
                } else if self.transition.focus_reported == 1 {
                    match (report & 16 != 0, self.direction) {
                        (false, _) => 2,
                        (true, 1) => 0,
                        (true, -1) => 1,
                        (true, _) => 2,
                    }
                } else if self.transition.hold_mode != 0 || report & 1 == 0 {
                    2
                } else {
                    u32::from(self.direction != 1)
                }
            }
            StageKind::Other => 2,
        };
        if kind != StageKind::Focus {
            self.focus_started = false;
        }
        stage
    }

    /// 0xf3c428: keep the next step inside the travel limits.
    pub(crate) fn clamp_step(&mut self, platform: &mut impl Platform) {
        if (self.phase == Phase::PeakSearch && self.bridge.scanning) || (self.monitor.watch == Watch::PeakSearch && self.monitor.active) {
            return;
        }
        if self.phase == Phase::Focus && platform.lens_state() != 0 {
            return;
        }
        let position = self.positions[0].0;
        let (low, high) = (self.limits.low.0, self.limits.high.0);
        let outside = if position < low {
            self.direction = 1;
            Some(low.wrapping_sub(position) as u16)
        } else if position > high {
            self.direction = -1;
            Some(position.wrapping_sub(high) as u16)
        } else {
            None
        };
        match outside {
            Some(distance) => {
                let limit = platform.setting(Setting::ReturnStepLimit);
                let distance = if u32::from(distance) > limit { limit as u16 } else { distance };
                if (self.step as i16) < distance as i16 {
                    self.step = distance;
                }
            }
            None => match self.direction {
                -1 if i32::from(position) - i32::from(self.step as i16) < i32::from(low) => {
                    self.step = position.wrapping_sub(low) as u16;
                }
                1 if i32::from(self.step as i16) + i32::from(position) > i32::from(high) => {
                    self.step = high.wrapping_sub(position) as u16;
                }
                _ => {}
            },
        }
    }

    /// 0xf3e8e0.
    pub(crate) fn bridge_phase(&self) -> BridgePhase {
        match self.phase {
            Phase::Direction => BridgePhase::Direction,
            Phase::PeakSearch => BridgePhase::PeakSearch,
            Phase::Peak => BridgePhase::Peak,
            Phase::Return => BridgePhase::Return,
            Phase::Focus => BridgePhase::Focus,
            Phase::FocusDrive => BridgePhase::FocusDrive,
            Phase::Approach => BridgePhase::Approach,
            Phase::Monitor => match self.monitor.watch {
                Watch::Return => BridgePhase::Return,
                Watch::PeakSearch => BridgePhase::PeakSearch,
                Watch::Scene => BridgePhase::Monitor,
            },
        }
    }

    /// 0xf3e9cc mode 1 (`travel` false) or 2: the signed travel and what the bridge is told.
    pub(crate) fn drive(&self, platform: &mut impl Platform, travel: bool) -> (i16, BridgeDrive) {
        let moves = travel && !self.hold;
        let (steps, speed) = if moves { ((self.step as i16).wrapping_mul(i16::from(self.direction)), self.bridge.speed) } else { (0, 2) };
        let phase = self.bridge_phase();
        let slow = platform.setting(Setting::SlowBridge) == 1 || platform.setting(Setting::SlowBridgeAlternate) == 1;
        let drive = BridgeDrive {
            phase,
            scanning: phase == BridgePhase::PeakSearch && self.bridge.scanning,
            window: self.bridge.window,
            rate: self.bridge.rate,
            moves,
            speed: if slow { 2 } else { speed },
            lens_mode: self.bridge.lens_mode,
        };
        (steps, drive)
    }

    /// 0xf3c428 then 0xf3e9cc(2).
    pub(crate) fn travel(&mut self, platform: &mut impl Platform) -> SearchStep<BridgeDrive> {
        self.clamp_step(platform);
        let (steps, drive) = self.drive(platform, true);
        let direction = if steps < 0 { Direction::Decreasing } else { Direction::Increasing };
        SearchStep::Travel { travel: Travel { direction, steps: steps.unsigned_abs() }, drive }
    }

    /// 0xf3eaf0: a scanning peak search whose drive has not settled.
    pub(crate) fn scan_in_motion(&self) -> bool { self.bridge.scanning && !matches!(self.bridge.scan, ScanState::Running | ScanState::Turned) }

    /// 0xf3eb3c: a scanning peak search (or the monitor's) that needs no further request.
    pub(crate) fn scan_continues(&self) -> bool {
        if self.phase != Phase::PeakSearch && !self.monitor.active {
            return false;
        }
        if self.monitor.watch != Watch::PeakSearch && self.monitor.active {
            return false;
        }
        self.bridge.scanning && !self.bridge.scan_sampled
    }

    /// 0xf3ec34: stop the scan drive.
    pub(crate) fn stop_scan(&mut self, platform: &mut impl Platform) {
        platform.request_scan_refresh(false);
        self.bridge.scan = ScanState::Stopped;
    }

    /// 0xf3ebd4.
    pub(crate) fn scan_or_idle(&self, bridge_busy: bool) -> bool { self.bridge.scanning || !bridge_busy }

    /// Machine 23's work actions: whether a frame may be evaluated now.
    pub(crate) fn ready(&self, platform: &mut impl Platform, bridge_busy: bool) -> bool {
        match self.stage {
            Stage::Idle => false,
            Stage::PeakSearch => self.scan_or_idle(bridge_busy),
            Stage::PeakMoved | Stage::FocusDriveMoved => !bridge_busy && !platform.lens_busy(),
            Stage::Monitor if self.monitor.following_lens() => !bridge_busy,
            Stage::Monitor => self.scan_or_idle(bridge_busy),
            _ => !bridge_busy,
        }
    }

    /// 0xf3e32c: start watching the scene.
    pub(crate) fn begin_monitor(&mut self) {
        self.samples = 0;
        self.monitor = Monitor { active: true, ..Monitor::new() };
    }

    /// 0xf3e380: search again from where the lens is.
    pub(crate) fn resume(&mut self, platform: &mut impl Platform) {
        self.monitor.active = false;
        self.monitor.restart = true;
        self.samples = 0;
        let position = platform.lens_position();
        self.positions[0] = position;
        self.positions[1] = position;
        self.reported = position;
        let lens = platform.lens_identity();
        self.lens = lens;
        self.previous_lens = lens;
    }

    /// 0xf3c5d0: hold where the lens is, in focus or driving to it.
    pub(crate) fn hold_lens(&mut self, platform: &mut impl Platform) -> Phase {
        let drives = platform.setting(Setting::HoldDrives) == 1;
        self.step = 0;
        self.samples = 0;
        self.direction = self.previous_direction;
        if drives {
            self.transition.hold_mode = 2;
            self.focus_position = LensPosition(self.limits.offset as i16);
            Phase::FocusDrive
        } else {
            self.hold = true;
            Phase::Focus
        }
    }

    /// The pinned hold every phase step checks first.
    pub(crate) fn holding(&self) -> bool { self.frame.pinned == 1 && self.pacing[0] == 1 && self.pacing[1] != 0 }
}
