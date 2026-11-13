#![forbid(unsafe_code)]

//! The peak search: the lens runs through its travel range, stepped or in one scanning drive,
//! until the contrast has risen and fallen again (0xf438c4 with 0xf411b8 and 0xf42abc).

use super::sample::ScanState;
use super::settings::{Setting, StepKind};
use super::{travel_limits, Context, FitPoint, MonitorStepQuery, Phase, Platform, ScanQuery};
use xm1_saclac_controllers::search::LensPosition;

/// Samples a series keeps; a full series overwrites its last.
pub(crate) const SERIES: usize = 256;

/// The channel sums a peak search follows, as bits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Channels(pub u8);

impl Channels {
    pub(crate) const NONE: Self = Self(0);
    /// Each pair's second sum; also the mark a paired evaluation sets on its own.
    pub(crate) const SECONDS: Self = Self(1);
    pub(crate) const COMBINED: Self = Self(2);
    pub(crate) const BOTH: Self = Self(3);

    const fn meets(self, other: Self) -> bool { self.0 & other.0 != 0 }

    const fn has(self, channel: Self) -> bool { self.0 & channel.0 == channel.0 }

    fn add(&mut self, channel: Self) { self.0 |= channel.0; }
}

/// The channel whose extremes decide where to hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PeakChannel {
    Seconds,
    Combined,
}

impl PeakChannel {
    const fn index(self) -> usize {
        match self {
            Self::Seconds => 0,
            Self::Combined => 1,
        }
    }
}

/// The first pair's sums and the reference, one sample per evaluation from the third.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Series {
    pub len: u16,
    pub seconds: [u32; SERIES],
    pub combined: [u32; SERIES],
    pub reference: [u32; SERIES],
}

impl Series {
    const fn new() -> Self { Self { len: 0, seconds: [0; SERIES], combined: [0; SERIES], reference: [0; SERIES] } }

    fn push(&mut self, seconds: u32, combined: u32, reference: u32) {
        let index = usize::from(self.len).min(SERIES - 1);
        self.seconds[index] = seconds;
        self.combined[index] = combined;
        self.reference[index] = reference;
        self.len = index as u16 + 1;
    }

    fn taken(&self, series: &'static fn(&Self) -> &[u32; SERIES]) -> &[u32] { &series(self)[..usize::from(self.len)] }

    fn newest(&self, series: &'static fn(&Self) -> &[u32; SERIES]) -> u32 { series(self)[usize::from(self.len.max(1)) - 1] }
}

const SECONDS: fn(&Series) -> &[u32; SERIES] = |series| &series.seconds;
const COMBINED: fn(&Series) -> &[u32; SERIES] = |series| &series.combined;
const REFERENCE: fn(&Series) -> &[u32; SERIES] = |series| &series.reference;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PeakSearch {
    pub series: Series,
    pub channel: PeakChannel,
    pub channels: Channels,
    /// Each channel's highest and lowest first-pair sum, seconds first.
    pub highest: [u32; 2],
    pub lowest: [u32; 2],
    /// The peaks the fall tests found.
    pub peaks: [u32; 2],
    pub fall_percent: u32,
    pub rise_percent: u32,
    pub best: LensPosition,
    pub best_class: u32,
    pub rose: Channels,
    pub passed: Channels,
    /// Turns at the high and at the low travel limit.
    pub turns: [u8; 2],
    /// The record's focus word now and at the previous sample.
    pub focus: [i16; 2],
    /// Peaks passed without a rise since the last rise.
    pub stalls: u8,
    /// The peak phase ends at once and turns to `direction`.
    pub directed: bool,
    pub direction: i8,
}

impl PeakSearch {
    pub(crate) const fn new() -> Self {
        Self {
            series: Series::new(),
            channel: PeakChannel::Seconds,
            channels: Channels::NONE,
            highest: [0; 2],
            lowest: [0; 2],
            peaks: [0; 2],
            fall_percent: 0,
            rise_percent: 0,
            best: LensPosition(0),
            best_class: 0,
            rose: Channels::NONE,
            passed: Channels::NONE,
            turns: [0; 2],
            focus: [0; 2],
            stalls: 0,
            directed: false,
            direction: 1,
        }
    }
}

/// What the peak search traces with property 0xe8 bits 4 and 5.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeakSearchTrace {
    pub samples: u8,
    pub recent_peak: bool,
    pub scan: u32,
    pub peak_contrast: u32,
    pub focus: LensPosition,
    pub focus_speed: u32,
    pub hold_mode: u8,
    pub limits: [LensPosition; 2],
    pub frame_mark: u16,
    pub position: LensPosition,
    pub correction_source: i16,
    pub corrected_position: LensPosition,
    pub correction_class: u32,
    pub best: LensPosition,
    pub best_class: u32,
    pub contrast: u32,
    pub second: u32,
    pub combined: u32,
    pub reference: u32,
    pub series_len: u16,
    pub channel: u32,
    pub channels: u8,
    pub rose: u8,
    pub rise_percent: u32,
    pub highest: [u32; 2],
    pub lowest: [u32; 2],
    pub passed: u8,
    pub fall_percent: u32,
    pub peaks: [u32; 2],
    pub limit_marks: [u8; 2],
}

/// A percentage that is `high` at and below the logarithm `from` (in hundreds), `low` from
/// `to`, interpolated between, and `low` while the newest reference reaches 10000.
struct Thresholds {
    high: Setting,
    low: Setting,
    from: Setting,
    to: Setting,
}

const RISE: Thresholds = Thresholds { high: Setting::RiseHigh, low: Setting::RiseLow, from: Setting::RiseFrom, to: Setting::RiseTo };
const FALL: Thresholds = Thresholds { high: Setting::FallHigh, low: Setting::FallLow, from: Setting::FallFrom, to: Setting::FallTo };

impl Context {
    /// 0xf438c4.
    pub(crate) fn peak_search_step(&mut self, platform: &mut impl Platform) -> Phase {
        self.bridge.speed = 2;
        let next = if self.holding() {
            self.hold_lens(platform)
        } else if platform.setting(Setting::ScanDriveDisabled) == 1 {
            self.stepped_peak_search(platform)
        } else {
            self.scanning_peak_search(platform)
        };
        if platform.tracing() && platform.detailed_tracing() {
            let trace = self.peak_search_trace();
            platform.trace_peak_search(trace);
        }
        next
    }

    fn peak_search_trace(&self) -> PeakSearchTrace {
        let peak = &self.peak;
        PeakSearchTrace {
            samples: self.samples,
            recent_peak: self.recent_peak,
            scan: self.bridge.scan.code(),
            peak_contrast: self.search.peak_contrast,
            focus: self.focus_position,
            focus_speed: self.focus_speed,
            hold_mode: self.transition.hold_mode,
            limits: [self.limits.low, self.limits.high],
            frame_mark: self.frame.mark,
            position: self.positions[0],
            correction_source: self.frame.correction_source,
            corrected_position: self.corrected.frame,
            correction_class: self.frame.correction_class,
            best: peak.best,
            best_class: peak.best_class,
            contrast: self.values.contrast,
            second: self.values.seconds[0],
            combined: self.values.combined[0],
            reference: self.values.reference,
            series_len: peak.series.len,
            channel: peak.channel.index() as u32,
            channels: peak.channels.0,
            rose: peak.rose.0,
            rise_percent: peak.rise_percent,
            highest: peak.highest,
            lowest: peak.lowest,
            passed: peak.passed.0,
            fall_percent: peak.fall_percent,
            peaks: peak.peaks,
            limit_marks: self.search.peak_marks,
        }
    }

    /// 0xf411b8: one step per evaluation.
    fn stepped_peak_search(&mut self, platform: &mut impl Platform) -> Phase {
        self.bridge.scanning = false;
        if (!self.monitor.active && self.previous_lens != self.lens) || self.flag_restarts(platform, 8) {
            self.halt();
            self.search.probed = 0;
            return Phase::Direction;
        }
        if self.frame.previous_selected != self.frame.selected {
            self.direction = self.previous_direction;
            self.halt();
            self.values.previous_level = self.values.level;
            return Phase::PeakSearch;
        }
        if self.samples == 1 {
            self.begin_peak_search(platform);
        }
        if self.recent_peak_reached(platform) {
            self.direction = self.previous_direction;
            self.halt();
            return Phase::PeakSearch;
        }
        self.record_peak_sample(platform);
        self.bridge.lens_mode = platform.lens_mode();
        let step = self.step_size(platform, StepKind::Scan);
        self.bridge.rate = step.rate;
        self.step = if self.monitor.active {
            platform.monitor_step(MonitorStepQuery { lens: self.lens, moved: self.monitor.moved, step: step.size, mark: self.monitor.mark })
        } else {
            step.size
        };
        let next = if self.samples < 4 {
            self.direction = self.previous_direction;
            Phase::PeakSearch
        } else if !self.monitor.active && platform.setting(Setting::LevelJump) == 1 && self.level_jumped(platform) {
            self.halt();
            self.search.probed = 0;
            self.direction = self.previous_direction.wrapping_neg();
            self.limit_step();
            return Phase::Direction;
        } else {
            if self.peak.rose != self.peak.channels {
                self.test_rise(platform);
            }
            if self.peak.passed != self.peak.channels {
                self.test_fall(platform);
            }
            if platform.setting(Setting::PairedEvaluation) == 1 {
                self.classify_paired_steps(platform);
            }
            if self.peak.channels.meets(self.peak.passed) {
                self.after_peak(platform, false)
            } else {
                self.step_within_limits(platform)
            }
        };
        self.limit_step();
        self.values.previous_level = self.values.level;
        next
    }

    fn classify_paired_steps(&mut self, platform: &mut impl Platform) {
        if self.values.level < platform.setting(Setting::LevelUnits) {
            self.peak.passed = Channels::SECONDS;
            self.peak.rose = Channels::SECONDS;
            self.peak.channels = Channels::SECONDS;
            self.peak.directed = true;
        } else if self.paired_rise(platform) && self.level_fell(platform) {
            self.peak.rose = Channels::SECONDS;
            self.peak.channels = Channels::SECONDS;
        } else if self.paired_rise(platform) && self.level_rose(platform) {
            self.peak.passed = Channels::SECONDS;
            self.peak.channels = Channels::SECONDS;
            self.peak.directed = true;
        }
    }

    /// A low paired level whose first sum clears its second by the paired margin.
    fn paired_rise(&self, platform: &mut impl Platform) -> bool {
        self.values.level < platform.setting(Setting::LevelHundreds).wrapping_mul(100)
            && self.values.combined[0] > self.values.seconds[0].wrapping_add(platform.setting(Setting::PairedMargin).wrapping_mul(10))
    }

    /// The level fell below the previous one by more than `LevelDrop` percent of it.
    fn level_fell(&self, platform: &mut impl Platform) -> bool {
        let previous = self.values.previous_level;
        previous > percent_of(previous, platform.setting(Setting::LevelDrop)).wrapping_add(self.values.level)
    }

    fn level_rose(&self, platform: &mut impl Platform) -> bool {
        let previous = self.values.previous_level;
        self.values.level > percent_of(previous, platform.setting(Setting::LevelDrop)).wrapping_add(previous)
    }

    /// A stepped search before its peak turns at a travel limit and holds once it has met
    /// both.
    fn step_within_limits(&mut self, platform: &mut impl Platform) -> Phase {
        self.direction = self.previous_direction;
        let position = self.positions[0];
        if position <= self.limits.low {
            self.direction = 1;
            self.step = platform.setting(Setting::EndStep) as u16;
        } else if position >= self.limits.high {
            self.direction = -1;
            self.step = platform.setting(Setting::EndStep) as u16;
        }
        let position = self.positions[2];
        if position <= self.limits.low {
            self.search.peak_marks[0] = 1;
        } else if position >= self.limits.high {
            self.search.peak_marks[1] = 1;
        }
        if self.search.second_channel || self.search.peak_marks != [1, 1] {
            return Phase::PeakSearch;
        }
        self.hold_at_best(platform);
        self.halt();
        Phase::Peak
    }

    /// A search that passed a peak holds at its best sample when it also rose; without a
    /// rise it searches on, and a moved watch word or a second channel restarts it.
    fn after_peak(&mut self, platform: &mut impl Platform, scanning: bool) -> Phase {
        let next = if !self.peak.channels.meets(self.peak.rose) {
            if scanning && !self.monitor.active {
                self.bridge.scan = ScanState::PassedWithoutRise;
            }
            Phase::PeakSearch
        } else {
            if scanning {
                self.peak.stalls = 0;
                if self.search.contrast < self.search.previous_contrast {
                    self.peak.direction = self.peak.direction.wrapping_neg();
                }
            }
            let (next, scan) = if platform.setting(Setting::LevelJump) != 1 && !self.monitor.active && self.level_jumped(platform) {
                self.search.probed = 0;
                (Phase::Direction, ScanState::LevelJumped)
            } else if self.search.second_channel {
                self.search.second_channel = false;
                self.search.probed = 0;
                (Phase::Direction, ScanState::SecondChannel)
            } else {
                self.search.peak_contrast = self.peak.highest[self.peak.channel.index()];
                self.focus_position = self.peak.best;
                self.focus_speed = self.peak.best_class;
                (Phase::Peak, ScanState::Peaked)
            };
            if scanning {
                self.bridge.scan = scan;
            }
            next
        };
        if !self.monitor.active {
            self.direction = self.previous_direction.wrapping_neg();
            self.halt();
        }
        next
    }

    /// Where to hold once a search met both travel limits: a peak that barely clears the
    /// channel's lowest sum drives to the lens's default focus (or a selected record's
    /// drive target), any other holds at the best sample.
    fn hold_at_best(&mut self, platform: &mut impl Platform) {
        let channel = self.peak.channel.index();
        let peak = self.peak.highest[channel];
        self.search.peak_contrast = peak;
        self.watch_reference = self.frame.watch;
        if peak < self.peak.lowest[channel].wrapping_add(platform.setting(Setting::HoldMargin).wrapping_mul(10)) {
            let target = if self.frame.selected == 0 || self.frame.pinned == 1 {
                LensPosition(self.limits.offset as i16)
            } else {
                let target = platform.drive_target(self.lens, self.frame.pinned_setting, self.limits.low, self.limits.high);
                self.search.focus_marks = [target.0 as u16; 2];
                target
            };
            self.transition.hold_mode = 2;
            self.focus_position = target;
            self.focus_speed = 1;
        } else {
            self.transition.hold_mode = 1;
            self.focus_position = self.peak.best;
            self.focus_speed = self.peak.best_class;
        }
    }

    /// 0xf42abc: one scanning drive, evaluated as the lens moves.
    fn scanning_peak_search(&mut self, platform: &mut impl Platform) -> Phase {
        self.bridge.scanning = true;
        self.bridge.scan_sampled = false;
        self.bridge.scan = ScanState::Running;
        if !self.monitor.active && self.previous_lens != self.lens {
            return self.restart_scan(ScanState::LensChanged);
        }
        if self.flag_restarts(platform, 8) {
            return self.restart_scan(ScanState::Flagged);
        }
        if self.frame.mark > 0xf7 {
            self.direction = self.previous_direction;
            self.bridge.scan = ScanState::FrameLate;
            self.halt();
            return Phase::PeakSearch;
        }
        if !platform.lens_ready() {
            self.bridge.scanning = false;
            self.halt();
            return Phase::PeakSearch;
        }
        if !self.frame.scan_allowed {
            self.halt();
            return Phase::PeakSearch;
        }
        if self.samples == 1 {
            self.begin_peak_search(platform);
            self.plan_scan(platform);
            return Phase::PeakSearch;
        }
        self.bridge.scan_sampled = true;
        if !self.frame.corrects_position {
            return Phase::PeakSearch;
        }
        if self.frame.previous_selected != self.frame.selected {
            self.direction = self.previous_direction;
            self.bridge.scan = ScanState::SelectionChanged;
            self.halt();
            self.values.previous_level = self.values.level;
            return Phase::PeakSearch;
        }
        self.peak.focus = [platform.profile_focus(), self.peak.focus[0]];
        let search = &mut self.search;
        search.previous_contrast = search.contrast;
        search.contrast = self.values.combined[0];
        search.previous_change = search.change;
        search.change = search.contrast.wrapping_sub(search.previous_contrast) as i32;
        if self.recent_peak_reached(platform) {
            self.direction = self.previous_direction;
            self.bridge.scan = ScanState::RecentPeak;
            self.halt();
            return Phase::PeakSearch;
        }
        self.record_peak_sample(platform);
        let next = if self.samples < 4 {
            self.direction = self.previous_direction;
            Phase::PeakSearch
        } else if !self.monitor.active && platform.setting(Setting::LevelJump) == 1 && self.level_jumped(platform) {
            self.search.probed = 0;
            self.direction = self.previous_direction.wrapping_neg();
            self.bridge.scan = ScanState::LevelJumped;
            self.halt();
            self.limit_step();
            return Phase::Direction;
        } else {
            if self.values.level < platform.setting(Setting::LevelHundreds).wrapping_mul(100) {
                if self.peak.rose != self.peak.channels {
                    self.test_rise(platform);
                }
                if self.peak.passed != self.peak.channels {
                    self.test_fall(platform);
                }
            }
            if platform.setting(Setting::PairedEvaluation) == 1 {
                self.classify_paired_scan(platform);
            }
            self.peak.directed = true;
            if self.peak.channels.meets(self.peak.passed) {
                self.peak.direction = self.direction;
                self.after_peak(platform, true)
            } else {
                self.scan_within_limits(platform)
            }
        };
        self.limit_step();
        self.values.previous_level = self.values.level;
        next
    }

    fn restart_scan(&mut self, why: ScanState) -> Phase {
        self.search.probed = 0;
        self.bridge.scan = why;
        self.halt();
        Phase::Direction
    }

    fn classify_paired_scan(&mut self, platform: &mut impl Platform) {
        let rising = self.search.contrast > self.search.previous_contrast;
        let falling = self.search.contrast < self.search.previous_contrast;
        if self.values.level < platform.setting(Setting::LevelUnits) && self.values.combined[0] > self.values.seconds[0] {
            self.peak.passed = Channels::SECONDS;
            self.peak.rose = Channels::SECONDS;
            self.peak.channels = Channels::SECONDS;
            self.peak.stalls = 0;
        }
        if rising && self.paired_rise(platform) && self.level_fell(platform) {
            self.peak.rose = Channels::SECONDS;
            self.peak.channels = Channels::SECONDS;
            self.peak.stalls = 0;
        } else if falling && self.paired_rise(platform) && self.level_rose(platform) {
            self.peak.passed = Channels::SECONDS;
            self.peak.channels = Channels::SECONDS;
        }
        self.count_focus_moves();
        if self.peak.rose == Channels::SECONDS {
            self.peak.stalls = 0;
            let turns = platform.setting(Setting::ScanTurns);
            if (u32::from(self.search.rising) >= turns || u32::from(self.search.falling) >= turns)
                && (falling || self.search.previous_change > self.search.change)
            {
                self.peak.passed = Channels::SECONDS;
                self.peak.rose = Channels::SECONDS;
                self.peak.channels = Channels::SECONDS;
            }
        } else if self.search.previous_change <= 0 || self.search.change <= 0 {
            let stalls = platform.setting(Setting::ScanStalls);
            if u32::from(self.search.rising) >= stalls || u32::from(self.search.falling) >= stalls {
                self.peak.passed = Channels::SECONDS;
                self.peak.channels = Channels::SECONDS;
                self.search.rising = 0;
                self.search.falling = 0;
                self.peak.stalls = self.peak.stalls.wrapping_add(1);
            }
        }
    }

    /// Counts samples whose focus value rose while the record's focus word fell: as falls
    /// moving up, as rises moving down.
    fn count_focus_moves(&mut self) {
        let Some(newest) = self.ring.newest else {
            return;
        };
        let value = |index: usize| i32::from(self.ring.entries[index].focus_value as i16);
        let previous = newest.checked_sub(1).unwrap_or(newest);
        let value_rose = value(newest) - value(previous) > 0;
        let word_fell = i32::from(self.peak.focus[1]) - i32::from(self.peak.focus[0]) > 0;
        if !(value_rose && word_fell) {
            return;
        }
        let search = &mut self.search;
        if self.direction == 1 {
            search.falling = search.falling.wrapping_add(1);
            search.rising = 0;
        } else {
            search.rising = search.rising.wrapping_add(1);
            search.falling = 0;
        }
    }

    /// A scanning search before its peak turns where the drive meets a travel limit less its
    /// lead, and ends once it met both limits or turned too often.
    fn scan_within_limits(&mut self, platform: &mut impl Platform) -> Phase {
        self.direction = self.previous_direction;
        let lead = if platform.measurement_selection() == 1 { i32::from(self.bridge.lead as i16) } else { 0 };
        let position = i32::from(self.corrected.drive.0);
        if position <= i32::from(self.limits.low.0) + lead {
            self.turn_at_limit(0, 1);
        } else if position >= i32::from(self.limits.high.0) - lead {
            self.turn_at_limit(1, -1);
        }
        let marks = self.search.peak_marks;
        if !self.search.second_channel && (marks == [1, 1] || (marks != [0, 0] && self.peak.stalls != 0)) {
            self.peak.stalls = 0;
            self.hold_at_best(platform);
            self.bridge.scan = ScanState::Bounded;
            self.halt();
            return Phase::Peak;
        }
        let [at_high, at_low] = self.peak.turns;
        if at_high <= 1 && at_low <= 1 {
            return Phase::PeakSearch;
        }
        let channel = self.peak.channel.index();
        let peak = self.peak.highest[channel];
        self.search.peak_contrast = peak;
        self.watch_reference = self.frame.watch;
        if peak < self.peak.lowest[channel].wrapping_add(platform.setting(Setting::HoldMargin).wrapping_mul(10)) {
            self.direction = self.previous_direction.wrapping_neg();
            self.halt();
            self.peak.turns = [0; 2];
            return Phase::PeakSearch;
        }
        if at_high > 1 {
            self.focus_position = self.limits.high;
        } else if at_low > 1 {
            self.focus_position = self.limits.low;
        }
        self.halt();
        self.peak.turns = [0; 2];
        self.transition.hold_mode = 1;
        self.search.peak_contrast = self.values.contrast;
        self.focus_speed = 2;
        Phase::Peak
    }

    /// Marks the limit (`end` 0 low, 1 high), heads back with `direction` and counts the
    /// turn, twice after a rise.
    fn turn_at_limit(&mut self, end: usize, direction: i8) {
        self.search.peak_marks[end] = 1;
        self.direction = direction;
        self.bridge.scan = ScanState::Turned;
        self.halt();
        if self.direction != self.previous_direction {
            let turns = &mut self.peak.turns[1 - end];
            *turns = turns.wrapping_add(if self.peak.rose == Channels::SECONDS { 2 } else { 1 });
            if self.peak.rose == Channels::SECONDS {
                self.peak.stalls = 0;
            }
        }
    }

    /// 0xf41094.
    fn begin_peak_search(&mut self, platform: &mut impl Platform) {
        self.transition.hold_mode = 0;
        self.peak.passed = Channels::NONE;
        self.peak.rose = Channels::NONE;
        self.peak.channel = if platform.setting(Setting::SecondChannel) == 1 && self.frame.selected == 0 { PeakChannel::Seconds } else { PeakChannel::Combined };
        self.peak.highest = [0; 2];
        self.peak.lowest = [u32::MAX; 2];
        self.watch_reference = self.frame.watch;
        self.forget_fit_points();
        self.forget_tracks();
        self.peak.channels = if self.frame.selected == 1 {
            Channels::COMBINED
        } else if platform.setting(Setting::PeakBothChannels) == 1 {
            Channels::BOTH
        } else if self.peak.channel == PeakChannel::Seconds {
            Channels::SECONDS
        } else {
            Channels::COMBINED
        };
        self.peak.series.len = 0;
        self.peak.peaks = [0; 2];
        self.peak.best = LensPosition(0);
        self.peak.best_class = 2;
        self.peak.focus = [0; 2];
    }

    /// 0xf3ec50.
    fn plan_scan(&mut self, platform: &mut impl Platform) {
        let lens_mode = platform.lens_mode();
        self.bridge.lens_mode = lens_mode;
        if let Some(limits) = travel_limits(platform, self.retained_beyond_high) {
            self.limits = limits;
        }
        let plan = platform.plan_scan(ScanQuery {
            watching: self.monitor.active,
            contrast: self.values.contrast,
            lens_mode,
            direction: self.direction,
            rate: self.bridge.rate,
            limits: self.limits,
            frame: self.frame.scan,
            moved: self.monitor.moved,
            mark: self.monitor.mark,
        });
        [self.limits.low, self.limits.high] = plan.limits;
        self.bridge.window = plan.window;
        self.bridge.lead = plan.lead;
        self.bridge.rate = plan.rate;
    }

    /// The intake's watched word moved from its reference by more than `LevelJumpTens`.
    fn level_jumped(&self, platform: &mut impl Platform) -> bool {
        let watch = i32::from(self.frame.watch);
        let reference = i32::from(self.watch_reference);
        let tens = platform.setting(Setting::LevelJumpTens).wrapping_mul(10) as i32;
        watch > reference + tens || reference > watch + tens
    }

    /// 0xf3db9c; the first time it holds it sets the record's recent-peak flag.
    fn recent_peak_reached(&mut self, platform: &mut impl Platform) -> bool {
        if self.recent_peak || self.frame.selected != 0 || !recent_levels_high(platform, self.samples) {
            return false;
        }
        self.recent_peak = true;
        true
    }

    /// The series, a new best with the samples around it, and each channel's extremes.
    fn record_peak_sample(&mut self, platform: &mut impl Platform) {
        if self.samples <= 2 {
            return;
        }
        self.peak.series.push(self.values.seconds[0], self.values.combined[0], self.values.reference);
        let contrast = self.values.contrast;
        let position = self.positions[2].0;
        if self.peak.highest[self.peak.channel.index()] < contrast {
            self.peak.best = self.positions[2];
            self.peak.best_class = self.classes[2];
            self.transition.restarted = u32::from(self.frame.selected != 0);
            self.note_best(contrast, position);
        } else if self.fit_window_complete(platform, contrast, position) {
            self.peak.best = self.fit(platform);
        }
        self.note_newest(contrast, position);
        self.note_extremes();
    }

    /// 0xf3e0d0. A selected record keeps no second-sum extremes.
    fn note_extremes(&mut self) {
        let (second, combined) = (self.values.seconds[0], self.values.combined[0]);
        let peak = &mut self.peak;
        if self.frame.selected == 0 {
            peak.highest[0] = peak.highest[0].max(second);
        }
        peak.highest[1] = peak.highest[1].max(combined);
        if self.frame.selected == 0 {
            peak.lowest[0] = peak.lowest[0].min(second);
        }
        peak.lowest[1] = peak.lowest[1].min(combined);
    }

    /// 0xf3dca8: a channel rose once its sum clears its lowest by the rise percentage of its
    /// highest.
    fn test_rise(&mut self, platform: &mut impl Platform) {
        let channels = self.peak.channels;
        self.peak.rise_percent = self.threshold(platform, &RISE);
        let percent = self.peak.rise_percent;
        if channels.has(Channels::SECONDS)
            && !self.peak.rose.has(Channels::SECONDS)
            && platform.below_threshold(self.values.seconds[0], self.peak.highest[0], self.peak.lowest[0], percent)
        {
            self.peak.rose.add(Channels::SECONDS);
        }
        if channels.has(Channels::COMBINED)
            && !self.peak.rose.has(Channels::COMBINED)
            && platform.below_threshold(self.values.combined[0], self.peak.highest[1], self.peak.lowest[1], percent)
        {
            self.peak.rose.add(Channels::COMBINED);
        }
    }

    /// 0xf3de28: a channel passed its peak once its series fell far enough; with neither,
    /// a low combined sum lets the reference series decide.
    fn test_fall(&mut self, platform: &mut impl Platform) {
        let channels = self.peak.channels;
        self.peak.fall_percent = self.threshold(platform, &FALL);
        for (channel, index, series) in [(Channels::SECONDS, 0, &SECONDS), (Channels::COMBINED, 1, &COMBINED)] {
            if channels.has(channel) && !self.peak.passed.has(channel) {
                let (passed, peak) = platform.passed_peak(self.peak.series.taken(series), self.peak.fall_percent, self.recent_peak);
                if let Some(peak) = peak {
                    self.peak.peaks[index] = peak;
                }
                if passed {
                    self.peak.passed.add(channel);
                }
            }
        }
        if self.peak.passed == Channels::NONE
            && self.peak.series.newest(&COMBINED) < platform.setting(Setting::FallLevel).wrapping_mul(10)
            && platform.fell_from_peak(self.peak.series.taken(&REFERENCE))
        {
            self.peak.passed.add(Channels::COMBINED);
        }
    }

    fn threshold(&self, platform: &mut impl Platform, thresholds: &Thresholds) -> u32 {
        let log = platform.contrast_log(self.values.combined[0]);
        let high = platform.setting(thresholds.high);
        let low = platform.setting(thresholds.low);
        let from = platform.setting(thresholds.from).wrapping_mul(100);
        let to = platform.setting(thresholds.to).wrapping_mul(100);
        if self.peak.series.newest(&REFERENCE) > 9999 {
            low
        } else if from > log {
            high
        } else if to < log {
            low
        } else {
            let drop = high.wrapping_sub(low).wrapping_mul(log.wrapping_sub(from)).checked_div(to.wrapping_sub(from)).unwrap_or(0);
            high.wrapping_sub(drop)
        }
    }

    /// 0xf3e3ec.
    pub(crate) fn forget_fit_points(&mut self) { self.search.fit_points = [FitPoint::EMPTY; 4]; }

    /// 0xf3e440.
    pub(crate) fn note_newest(&mut self, contrast: u32, position: i16) { self.search.fit_points[3] = FitPoint { contrast, position }; }

    /// 0xf3e468: a new best, with the sample before it, waiting for the one after.
    pub(crate) fn note_best(&mut self, contrast: u32, position: i16) {
        let points = &mut self.search.fit_points;
        points[0] = points[3];
        points[1] = FitPoint { contrast, position };
        points[2] = FitPoint::EMPTY;
    }

    /// 0xf3e4bc: takes the sample after a best; true when the three bracket a peak along a
    /// monotone run of positions.
    pub(crate) fn fit_window_complete(&mut self, platform: &mut impl Platform, contrast: u32, position: i16) -> bool {
        let points = &mut self.search.fit_points;
        if points[2].contrast != u32::MAX || points[1].contrast == u32::MAX {
            return false;
        }
        points[2] = FitPoint { contrast, position };
        if platform.setting(Setting::FitPeak) == 0 {
            return false;
        }
        let [before, best, after, _] = self.search.fit_points;
        if before.contrast == u32::MAX || after.contrast == u32::MAX {
            return false;
        }
        if before.contrast as i32 >= best.contrast as i32 || after.contrast as i32 >= best.contrast as i32 {
            return false;
        }
        if [before, best, after].iter().any(|point| point.position == FitPoint::EMPTY.position) {
            return false;
        }
        (before.position < best.position && best.position < after.position) || (before.position > best.position && best.position > after.position)
    }

    /// 0xf3e63c.
    pub(crate) fn fit(&mut self, platform: &mut impl Platform) -> LensPosition {
        let [before, best, after, _] = self.search.fit_points;
        let peak = platform.fit_peak([before, best, after]);
        self.search.fit = peak.0 as u16;
        peak
    }

    /// 0xf3e1cc.
    pub(crate) fn halt(&mut self) {
        self.step = 0;
        self.samples = 0;
        self.bridge.scan_sampled = false;
    }

    /// 0xf3e054: halves a step that would carry the lens past the limit ahead.
    pub(crate) fn limit_step(&mut self) {
        let position = self.positions[0].0;
        let room = if self.direction == 1 { self.limits.high.0.wrapping_sub(position) } else { position.wrapping_sub(self.limits.low.0) } as u16;
        let step = i32::from(self.step as i16);
        if i32::from(room) < step * 2 {
            let half = ((u32::from(room) + 1) >> 1) as u16 as i16;
            if step > i32::from(half) {
                self.step = half as u16;
            }
        }
    }
}

pub(crate) fn percent_of(value: u32, percent: u32) -> u32 { value.wrapping_mul(percent) / 100 }

/// Whether `RecentPeakSamples` of the five newest measurements (an empty one skipped) had a
/// level at `RecentPeakLevel` or above without one below between them.
fn recent_levels_high(platform: &mut impl Platform, samples: u8) -> bool {
    if platform.setting(Setting::RecentPeak) & 2 == 0 {
        return false;
    }
    let Some(history) = platform.measurement_history() else {
        return false;
    };
    let needed = platform.setting(Setting::RecentPeakSamples);
    if u32::from(samples) < needed {
        return false;
    }
    let level = platform.setting(Setting::RecentPeakLevel);
    let mut run = 0u8;
    for levels in history.into_iter().flatten() {
        if levels.iter().all(|value| u32::from(*value) < level) {
            run = 0;
        } else {
            run = run.wrapping_add(1);
            if needed <= u32::from(run) {
                return true;
            }
        }
    }
    false
}
