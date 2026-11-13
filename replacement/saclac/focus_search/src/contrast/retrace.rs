#![forbid(unsafe_code)]

//! The return and the approach: the lens steps along the contrast it follows, turning on a
//! fall, until the trend says to drive to the best position, search the peak again or start
//! over (0xf46754 and 0xf46074 with 0xf460ec, 0xf45a80, 0xf45460, 0xf44afc and 0xf44150).

use super::direction::{level_strikes, model_scaled};
use super::peak_search::percent_of;
use super::sample::RingEntry;
use super::settings::{Setting, StepKind};
use super::tracks::ReturnPath;
use super::{Context, MonitorStepQuery, Phase, Platform};

/// What a return or approach counts of the contrast it follows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Trend {
    pub rises: u8,
    /// Falls after a rise: the peaks passed.
    pub peaks: u8,
    /// Evidence for leaving, weighted by the level band.
    pub strikes: u8,
    pub rising: bool,
}

impl Trend {
    pub(crate) const fn new() -> Self { Self { rises: 0, peaks: 0, strikes: 0, rising: false } }
}

impl Context {
    /// 0xf46754.
    pub(crate) fn return_step(&mut self, platform: &mut impl Platform) -> Phase { self.retrace_step(platform, Phase::Return, Setting::PeakReturns) }

    /// 0xf46074.
    pub(crate) fn approach_step(&mut self, platform: &mut impl Platform) -> Phase { self.retrace_step(platform, Phase::Approach, Setting::FocusDriveTurn) }

    fn retrace_step(&mut self, platform: &mut impl Platform, phase: Phase, peaks: Setting) -> Phase {
        self.bridge.speed = 2;
        let next = if self.holding() { self.hold_lens(platform) } else { self.retrace(platform, phase, peaks) };
        if next != phase {
            self.transition.return_ready = 0;
        }
        next
    }

    /// 0xf460ec and 0xf45a80. `peaks` is the setting that counts the peaks passed before a
    /// decision.
    fn retrace(&mut self, platform: &mut impl Platform, phase: Phase, peaks: Setting) -> Phase {
        if self.previous_lens != self.lens || self.flag_restarts(platform, 4) {
            self.samples = 0;
            self.search.probed = 0;
            return Phase::Direction;
        }
        if self.frame.previous_selected != self.frame.selected {
            self.samples = 0;
            self.step = 0;
            self.keep_as_best();
            return phase;
        }
        self.note_direction_sample();
        self.bridge.lens_mode = platform.lens_mode();
        let step = self.step_size(platform, StepKind::Return);
        self.bridge.rate = step.rate;
        self.step = if phase == Phase::Return && self.monitor.active {
            platform.monitor_step(MonitorStepQuery { lens: self.lens, moved: self.monitor.moved, step: step.size, mark: self.monitor.mark })
        } else {
            step.size
        };
        self.scale_step_by_level(platform);
        self.bound_step_by_range(platform);
        self.follow_trend(platform);
        self.limit_direction();
        self.add_reversal_step(platform);
        let next = if self.monitor.active {
            None
        } else {
            let peaks = platform.setting(peaks);
            self.decide_retrace(platform, peaks)
        };
        self.shift_direction_history();
        next.unwrap_or(phase)
    }

    /// A flagged evaluation restarts the search when `bit` of `FlagRestart` is set (for a
    /// selected record only with `SelectedFlagRestart`).
    pub(crate) fn flag_restarts(&self, platform: &mut impl Platform, bit: u32) -> bool {
        self.values.flag != 0
            && platform.setting(Setting::FlagRestart) & bit != 0
            && (self.frame.selected == 0 || (self.frame.selected == 1 && platform.setting(Setting::SelectedFlagRestart) == 1))
    }

    /// The sample becomes the best the drive to focus aims at.
    fn keep_as_best(&mut self) {
        self.search.peak_contrast = self.search.contrast;
        self.focus_position = self.search.position;
        self.focus_speed = self.search.class;
        self.transition.restarted = u32::from(self.frame.selected != 0);
    }

    /// 0xf45460: the direction the contrast trend gives, after updating the record's scan flag.
    fn follow_trend(&mut self, platform: &mut impl Platform) {
        self.recent_peak = self.track_levels(platform, self.recent_peak);
        if u32::from(self.samples) < platform.setting(Setting::ReturnTrendSamples) || self.search.position == self.search.previous_position {
            self.direction = self.previous_direction;
            return;
        }
        if platform.setting(Setting::FocusValueTracking) != 0 && self.recent_peak {
            self.follow_focus_values(platform);
            return;
        }
        let up = self.search.position > self.search.previous_position;
        let (contrast, previous) = (self.search.contrast, self.search.previous_contrast);
        let mode = platform.setting(Setting::SelectedReturnMode);
        if (mode == 1 || mode > 2)
            && self.frame.selected == 1
            && contrast < platform.setting(Setting::SelectedReturnContrast).wrapping_mul(100)
        {
            if previous > contrast.wrapping_add(percent_of(previous, platform.setting(Setting::SelectedReturnFall))) {
                self.trend_fell(up);
            } else {
                if contrast >= previous {
                    self.count_rise();
                }
                self.direction = if up { 1 } else { -1 };
            }
            return;
        }
        if platform.setting(Setting::PairedEvaluation) == 1 {
            self.peak.directed = false;
            if contrast < previous {
                self.trend_fell(up);
                self.peak.directed = true;
            } else if contrast > previous {
                self.trend_rose(up);
            } else {
                self.search.retrace.rising = false;
                self.search.retrace.rises = 0;
                self.direction = if up { -1 } else { 1 };
            }
        } else if previous > contrast.wrapping_add(percent_of(previous, platform.setting(Setting::ReturnFall))) {
            self.trend_fell(up);
        } else {
            self.trend_rose(up);
        }
    }

    /// A fall turns the lens back; after a rise it counts a peak passed.
    fn trend_fell(&mut self, up: bool) {
        let trend = &mut self.search.retrace;
        if trend.rising {
            trend.rising = false;
            trend.peaks = trend.peaks.wrapping_add(1);
        }
        trend.rises = 0;
        self.direction = if up { -1 } else { 1 };
    }

    fn trend_rose(&mut self, up: bool) {
        self.count_rise();
        self.direction = if up { 1 } else { -1 };
    }

    fn count_rise(&mut self) {
        let trend = &mut self.search.retrace;
        trend.rising = true;
        trend.rises = trend.rises.wrapping_add(1);
    }

    /// The scan flag's half of 0xf45460: while the record's scan flag stands, a return turns on
    /// the focus values instead of the contrast.
    fn follow_focus_values(&mut self, platform: &mut impl Platform) {
        let Some(newest) = self.ring.newest.filter(|newest| *newest > 0) else {
            return;
        };
        let value = |index: usize| self.ring.entries[index].focus_value;
        let (latest, previous) = (value(newest), value(newest - 1));
        if (previous.wrapping_sub(latest) as i16 as i32) < platform.setting(Setting::FocusValueTurn) as i32 {
            self.tracks.turn = true;
            self.direction = self.previous_direction.wrapping_neg();
            self.track_extent(platform);
            self.track_scan();
            self.tracks.crossings = 0;
            return;
        }
        if newest <= 1 {
            return;
        }
        let earlier = value(newest - 2);
        self.tracks.turn = false;
        self.direction = self.previous_direction;
        let restarted = self.track_extent(platform);
        self.track_scan();
        let steep = platform.setting(Setting::FocusValueSteep) as i32;
        if latest.wrapping_sub(previous) as i16 as i32 > steep && earlier.wrapping_sub(previous) as i16 as i32 > steep {
            self.tracks.steep_rises = self.tracks.steep_rises.wrapping_add(1);
        }
        if restarted {
            self.tracks.crossings = 0;
            return;
        }
        if self.passed_extent_mark() {
            self.tracks.crossings = self.tracks.crossings.wrapping_add(1);
        }
        if u32::from(self.tracks.crossings) > platform.setting(Setting::ExtentCrossings) {
            self.tracks.turn = true;
            self.direction = self.previous_direction.wrapping_neg();
            self.tracks.crossings = 0;
        }
    }

    /// 0xf44afc: counts the runs of samples whose area levels reached `RecentPeakLevel` while
    /// the raw focus value stayed within its band, and returns the scan flag they leave.
    fn track_levels(&mut self, platform: &mut impl Platform, flag: bool) -> bool {
        let level = platform.setting(Setting::RecentPeakLevel);
        let on = platform.setting(Setting::ScanOnSamples);
        let off = platform.setting(Setting::ScanOffSamples);
        let low = platform.setting(Setting::FocusRawLow) as i32 * 10;
        let high = platform.setting(Setting::FocusRawHigh) as i32 * 10;
        let runs = usize::from(platform.setting(Setting::TrackMarks) != 0);
        let [first, second] = self.ring.newest_entry().area_levels.map(|value| value >= level);
        let raw = i32::from(self.values.focus_raw as i16);
        let counts = self.frame.selected == 0 && (low..=high).contains(&raw);
        let both = counts && first && second;
        let either = counts && (first || second);
        let tracks = &mut self.tracks;
        let count = |run: u16, counted: bool| if counted { run.wrapping_add(1) } else { 0 };
        tracks.runs[0].on = count(tracks.runs[0].on, both);
        tracks.runs[1].on = count(tracks.runs[1].on, either);
        tracks.runs[0].off = count(tracks.runs[0].off, !either);
        tracks.runs[1].off = count(tracks.runs[1].off, !both);
        let raised = flag || u32::from(tracks.runs[runs].on) >= on;
        raised && u32::from(tracks.runs[runs].off) < off
    }

    /// 0xf44150: whether the return has seen enough to leave, and for which phase.
    fn decide_retrace(&mut self, platform: &mut impl Platform, peaks: u32) -> Option<Phase> {
        let limit = if model_scaled(platform) { Setting::SelectedSamplesScaled } else { Setting::SelectedSamples };
        let selected_limit = platform.setting(limit);
        if self.frame.selected_samples == 0 {
            self.frame.selected_samples = 1;
        }
        if u32::from(self.samples) < platform.setting(Setting::ReturnSamples) {
            return None;
        }
        if platform.setting(Setting::FocusValueTracking) != 0 && self.recent_peak {
            let next = self.decide_from_tracks(platform);
            self.tracks.path = ReturnPath::FocusValues;
            return next;
        }
        self.keep_best_sample(platform);
        let next = if platform.setting(Setting::PairedEvaluation) == 1 {
            self.decide_paired(platform, peaks, selected_limit)
        } else {
            self.decide_unpaired(platform, peaks)
        };
        self.tracks.path = ReturnPath::Contrast;
        next
    }

    /// A higher contrast becomes the best; while the lens moves, three samples around the best
    /// fit its peak.
    fn keep_best_sample(&mut self, platform: &mut impl Platform) {
        let (contrast, position) = (self.search.contrast, self.search.position.0);
        let moved = self.search.position != self.search.previous_position;
        if contrast > self.search.peak_contrast {
            self.keep_as_best();
            if moved {
                self.note_best(contrast, position);
                self.note_newest(contrast, position);
            }
        } else if moved {
            if self.fit_window_complete(platform, contrast, position) {
                self.focus_position = self.fit(platform);
            }
            self.note_newest(contrast, position);
        }
    }

    fn decide_paired(&mut self, platform: &mut impl Platform, peaks: u32, selected_limit: u32) -> Option<Phase> {
        let level = self.values.level;
        let tens = platform.setting(Setting::LevelTens).wrapping_mul(10);
        let next = if level < tens && self.search.contrast >= self.search.previous_contrast {
            self.search.retrace.strikes = 0;
            None
        } else {
            let strikes = level_strikes(platform, level) as u8;
            let trend = &mut self.search.retrace;
            trend.strikes = trend.strikes.wrapping_add(strikes);
            if u32::from(trend.strikes) < platform.setting(Setting::ReturnStrikes) {
                None
            } else {
                self.frame.selected_samples = 0;
                self.restart_peak_search();
                Some(Phase::PeakSearch)
            }
        };
        if u32::from(self.search.retrace.rises) >= platform.setting(Setting::ReturnRises) {
            return Some(if level < platform.setting(Setting::HandOffLevel) {
                self.drive_to_best(50)
            } else {
                self.frame.selected_samples = 0;
                self.restart_peak_search();
                Phase::PeakSearch
            });
        }
        if u32::from(self.search.retrace.peaks) < peaks {
            return next;
        }
        if level >= tens && self.frame.selected == 0 {
            self.search.level_holds = self.search.level_holds.wrapping_add(1);
            if u32::from(self.search.level_holds) > platform.setting(Setting::LevelHolds) {
                self.frame.selected_samples = 0;
                self.restart_peak_search();
                return Some(Phase::PeakSearch);
            }
            return Some(self.drive_to_best(1));
        }
        self.search.level_holds = 0;
        let selected_samples = u32::from(self.frame.selected_samples);
        Some(if selected_samples != 0 && selected_limit > selected_samples {
            let unselected_low = level < platform.setting(Setting::HandOffLevel) && self.frame.selected == 0;
            self.drive_to_best(if unselected_low { 50 } else { 1 })
        } else {
            self.frame.selected_samples = 0;
            self.start_over()
        })
    }

    fn decide_unpaired(&mut self, platform: &mut impl Platform, peaks: u32) -> Option<Phase> {
        let (contrast, peak) = (self.search.contrast, self.search.peak_contrast);
        let near_peak = contrast >= platform.setting(Setting::MotionReturn).wrapping_mul(10) && {
            let mut band = |from: u32| {
                let base = platform.setting(Setting::FocusStepLow);
                let step = platform.minimum_step(base);
                from.wrapping_add(percent_of(peak, step)).wrapping_add(platform.setting(Setting::FocusReturnTens).wrapping_mul(10))
            };
            peak <= band(contrast) && contrast <= band(peak)
        };
        let next = if near_peak {
            self.search.retrace.strikes = 0;
            None
        } else {
            let trend = &mut self.search.retrace;
            trend.strikes = trend.strikes.wrapping_add(1);
            if u32::from(trend.strikes) < platform.setting(Setting::ReturnStrikes) { None } else { Some(self.start_over()) }
        };
        if u32::from(self.search.retrace.rises) >= platform.setting(Setting::ReturnRises) {
            self.restart_peak_search();
            return Some(Phase::PeakSearch);
        }
        if u32::from(self.search.retrace.peaks) < peaks {
            return next;
        }
        let fell = self.search.previous_contrast > contrast.wrapping_add(platform.setting(Setting::ReturnFallTens).wrapping_mul(10));
        Some(if fell || platform.setting(Setting::TrendOverride) == 0 { self.drive_to_best(50) } else { self.start_over() })
    }

    /// The scan flag's half of 0xf44150: after enough focus-value turns, drive to the scan mark
    /// (after many steep rises or without an extent ring), else to the fitted extent peak or a
    /// mark.
    fn decide_from_tracks(&mut self, platform: &mut impl Platform) -> Option<Phase> {
        if !self.tracks.turn {
            return None;
        }
        self.tracks.turns = self.tracks.turns.wrapping_add(1);
        self.tracks.turn = false;
        if u32::from(self.tracks.turns) <= platform.setting(Setting::TrackedTurns) {
            self.tracks.steep_rises = 0;
            return None;
        }
        let scan_mark = u32::from(self.tracks.steep_rises) > platform.setting(Setting::SteepRises) || self.tracks.extent_newest.is_none();
        let best = if scan_mark {
            self.tracks.scan_mark_entry()
        } else if self.fit_extent_peak() {
            let peak = self.fit(platform);
            self.tracks.flag = 1;
            self.tracks.mark = peak.0 as u16;
            self.forget_fit_points();
            self.tracks.extent_mark_entry().map(|entry| RingEntry { position: peak, ..entry })
        } else if platform.setting(Setting::ScanMarkFallback) == 0 {
            self.tracks.flag = 1;
            self.tracks.extent_mark_entry()
        } else {
            self.tracks.flag = 0;
            self.tracks.scan_mark_entry()
        };
        if let Some(best) = best {
            self.search.peak_contrast = best.contrast;
            self.focus_position = best.position;
            self.focus_speed = best.class;
        }
        self.tracks.steep_rises = 0;
        Some(self.drive_to_best(50))
    }

    /// Searches the peak again from where the lens is.
    fn restart_peak_search(&mut self) {
        self.search.peak_marks = [0; 2];
        self.search.peak_start = self.positions[0];
        self.samples = 0;
        self.step = 0;
    }

    /// Drives to the best position, counting `selected` toward the selected samples.
    fn drive_to_best(&mut self, selected: u8) -> Phase {
        self.frame.selected_samples = self.frame.selected_samples.wrapping_add(selected);
        self.samples = 0;
        self.step = 0;
        Phase::FocusDrive
    }

    fn start_over(&mut self) -> Phase {
        self.samples = 0;
        self.step = 0;
        self.search.probed = 0;
        Phase::Direction
    }
}
