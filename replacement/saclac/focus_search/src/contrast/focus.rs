#![forbid(unsafe_code)]

//! The focus phase: keep the lens where it is and watch a filtered contrast for a reason to
//! search again (0xf400fc with 0xf3fc04, 0xf3f230, 0xf3ee60, 0xf3efbc, 0xf3f7a8, 0xf3c64c and
//! 0xf400ac).

use super::direction::{level_strikes, model_scaled};
use super::peak_search::percent_of;
use super::settings::Setting;
use super::{Context, LensPosition, Phase, Platform};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Focus {
    /// The channel contrast at the newest focus sample.
    pub contrast: u32,
    /// Focus samples taken, saturating below 0xff.
    pub count: u8,
    /// Evidence for leaving focus, weighted by the level band.
    pub strikes: u8,
    pub falls: u8,
    pub rises: u8,
    /// The filter's last two inputs and outputs, newest first.
    pub inputs: [u32; 2],
    pub outputs: [i32; 2],
    /// Percent of the peak a filtered contrast must fall below or rise above, and the base of
    /// the minimum step a return checks.
    pub fall_percent: u32,
    pub rise_percent: u32,
    pub step_base: u8,
    /// The settled near word's allowed drift (in tens) and the strikes that end focus.
    pub watch_tens: u8,
    pub strike_limit: u8,
    pub pinned_setting: u16,
}

impl Focus {
    pub(crate) const fn new() -> Self {
        Self {
            contrast: 0,
            count: 0,
            strikes: 0,
            falls: 0,
            rises: 0,
            inputs: [0; 2],
            outputs: [0; 2],
            fall_percent: 0,
            rise_percent: 0,
            step_base: 0,
            watch_tens: 0,
            strike_limit: 0,
            pinned_setting: 0,
        }
    }
}

/// What the focus step traces with property 0xe8 bits 4 and 6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusTrace {
    pub samples: u8,
    pub movement: [u32; 2],
    pub motion: [u16; 2],
    pub focus_reported: u32,
    pub hold_mode: u8,
    pub restarted: u32,
    pub selected: u32,
    pub count: u8,
    pub contrast: u32,
    pub peak_contrast: u32,
    pub fall_percent: u32,
    pub rise_percent: u32,
    pub step_base: u8,
    pub watch: i16,
    pub watch_reference: i16,
    pub watch_tens: u8,
    pub rises: u8,
    pub falls: u8,
    pub strikes: u8,
    pub strike_limit: u8,
    pub probed: u8,
}

impl Context {
    /// 0xf400fc.
    pub(crate) fn focus_step(&mut self, platform: &mut impl Platform) -> Phase {
        if self.frame.pinned != 1 {
            return self.watch_focus(platform);
        }
        let paced = self.pacing[2] == self.pacing[1];
        if self.holding() {
            if platform.setting(Setting::HoldDrives) != 1 || paced {
                return Phase::Focus;
            }
            self.step = 0;
            self.samples = 0;
            self.direction = self.previous_direction;
            self.transition.hold_mode = 2;
            self.focus_position = LensPosition(self.limits.offset as i16);
            return Phase::FocusDrive;
        }
        if !paced {
            self.step = 0;
            self.samples = 0;
            self.search.probed = 0;
            self.direction = self.previous_direction;
            return Phase::Direction;
        }
        self.watch_focus(platform)
    }

    /// 0xf3fc04.
    fn watch_focus(&mut self, platform: &mut impl Platform) -> Phase {
        let selected_limit = if model_scaled(platform) {
            platform.setting(Setting::SelectedSamplesScaled)
        } else {
            platform.setting(Setting::SelectedSamples)
        };
        self.focus_position = platform.lens_position();
        if self.previous_lens != self.lens || self.flag_restarts(platform, 1) {
            self.samples = 0;
            self.search.probed = 0;
            return Phase::Direction;
        }
        if self.samples == 1 {
            self.begin_focus(platform);
        }
        let moving = self.camera_moving(platform);
        let selected = self.frame.selected_samples;
        let approach = selected != 0 && selected_limit > u32::from(selected);
        if selected != 0 && !approach {
            self.frame.selected_samples = 0;
        }
        let next = if moving {
            self.transition.focus_reported = 1;
            Phase::Focus
        } else if approach {
            self.transition.focus_reported = 0;
            self.search.peak_contrast = self.focus.contrast;
            self.samples = 0;
            self.transition.return_ready = 1;
            Phase::Approach
        } else {
            self.transition.focus_reported = 0;
            self.focus.contrast = self.values.contrast;
            self.focus.count = match self.focus.count.wrapping_add(1) {
                0xff => 0xfe,
                count => count,
            };
            if self.samples <= 4 {
                Phase::Focus
            } else {
                if self.focus_entered {
                    self.search.peak_contrast = self.values.contrast;
                    self.focus_entered = false;
                }
                self.judge_focus(platform)
            }
        };
        self.direction = self.previous_direction;
        self.step = 0;
        if platform.tracing() && platform.focus_tracing() {
            let trace = self.focus_trace();
            platform.trace_focus(trace);
        }
        next
    }

    /// The focus phase's first sample.
    fn begin_focus(&mut self, platform: &mut impl Platform) {
        let focus = &mut self.focus;
        focus.count = 0;
        focus.strikes = 0;
        focus.falls = 0;
        focus.rises = 0;
        focus.inputs = [0; 2];
        focus.outputs = [0; 2];
        focus.pinned_setting = self.frame.pinned_setting;
        if platform.setting(Setting::PeakAtFocus) == 1 {
            self.search.peak_contrast = self.values.contrast;
            self.transition.restarted = u32::from(self.frame.selected != 0);
        }
        let mode = platform.profile_mode();
        self.choose_focus_limits(platform, mode);
        self.motion.level = 0;
        self.motion.interval = 0;
        self.motion.forget();
        if platform.setting(Setting::MotionCheck) == 1 {
            platform.request_angular_rate();
        }
    }

    /// 0xf3f7a8: the drift allowance and strike limit for the record's mode, a selected
    /// pinned record taking its own, the limit lower while the peak is below
    /// `FocusLimitContrast`.
    fn choose_focus_limits(&mut self, platform: &mut impl Platform, mode: u32) {
        const MOVING_MODES: u32 = 0x3c1_8071;
        let moving = mode <= 25 && MOVING_MODES & (1 << mode) != 0;
        let pinned_selected = self.frame.status_flag == 1 && self.frame.selected == 1;
        let (tens, low, high) = match (moving, pinned_selected) {
            (true, true) => (Setting::FocusDriftMovingSelected, Setting::StrikesMovingSelectedLow, Setting::StrikesMovingSelected),
            (true, false) => (Setting::FocusDriftMoving, Setting::StrikesMovingLow, Setting::StrikesMoving),
            (false, true) => (Setting::FocusDriftSelected, Setting::StrikesSelectedLow, Setting::StrikesSelected),
            (false, false) => (Setting::FocusLevelTens, Setting::StrikesLow, Setting::Strikes),
        };
        self.focus.watch_tens = platform.setting(tens) as u8;
        let threshold = platform.setting(Setting::FocusLimitContrast);
        let limit = if self.search.peak_contrast < threshold { low } else { high };
        self.focus.strike_limit = platform.setting(limit) as u8;
    }

    /// 0xf3f230: after enough samples, a selection that changed hands moves on; otherwise
    /// strikes gather from a filtered contrast that left the peak's band, a low contrast or
    /// second pair, or a drifting near word, and enough of them end focus.
    fn judge_focus(&mut self, platform: &mut impl Platform) -> Phase {
        let handed_over = matches!((self.transition.restarted, self.frame.selected), (0, 1) | (1, 0));
        if u32::from(self.focus.count) >= platform.setting(Setting::FocusSamples) && handed_over {
            self.samples = 0;
            if platform.setting(Setting::FocusApproaches) == 1 {
                self.transition.return_ready = 1;
                return Phase::Approach;
            }
            self.search.probed = 0;
            self.focus.count = 0;
            return Phase::Direction;
        }
        let filtered = self.filter_focus_contrast(platform);
        self.set_focus_thresholds(platform, filtered);
        self.strike_for_level(platform);
        if self.focus.strikes < self.focus.strike_limit {
            let peak = self.search.peak_contrast;
            let free = self.transition.hold_mode == 0;
            let watch = i32::from(self.frame.watch);
            let reference = i32::from(self.watch_reference);
            let drift = i32::from(self.focus.watch_tens) * 10;
            if filtered > percent_of(peak, self.focus.rise_percent).wrapping_add(peak).wrapping_add(platform.setting(Setting::FocusChangeTens).wrapping_mul(10)) {
                self.focus.rises = self.focus.rises.wrapping_add(1);
                self.focus.strikes = self.focus.strikes.wrapping_add(1);
            } else if (self.focus.contrast < platform.setting(Setting::FocusLowTens).wrapping_mul(10) && free)
                || (self.values.combined[1] < platform.setting(Setting::FocusSecondLowTens).wrapping_mul(10) && free)
                || ((watch > reference + drift || reference > watch + drift) && !free)
                || peak > percent_of(peak, self.focus.fall_percent).wrapping_add(filtered).wrapping_add(platform.setting(Setting::FocusChangeTens).wrapping_mul(10))
            {
                self.focus.falls = self.focus.falls.wrapping_add(1);
                self.focus.strikes = self.focus.strikes.wrapping_add(1);
            } else if self.values.level < platform.setting(Setting::LevelUnits) && !self.values.unsettled {
                self.focus.strikes = 0;
                return Phase::Focus;
            }
            if self.focus.strikes < self.focus.strike_limit {
                return Phase::Focus;
            }
        }
        self.leave_focus(platform, filtered)
    }

    /// Enough strikes: a free focus whose filtered contrast stayed within the minimum-step
    /// band of the peak approaches (or restarts above the units level) when `FocusApproaches`
    /// allows; any other restarts, as a probe when falls outnumbered rises.
    fn leave_focus(&mut self, platform: &mut impl Platform, filtered: u32) -> Phase {
        if self.transition.hold_mode == 0 && self.focus.contrast > platform.setting(Setting::MotionReturn).wrapping_mul(10) {
            let peak = self.search.peak_contrast;
            let band = |platform: &mut _, from: u32| {
                let step = Platform::minimum_step(platform, u32::from(self.focus.step_base));
                percent_of(peak, step).wrapping_add(from).wrapping_add(Platform::setting(platform, Setting::FocusReturnTens).wrapping_mul(10))
            };
            if peak <= band(platform, filtered) && filtered <= band(platform, peak) && platform.setting(Setting::FocusApproaches) == 1 {
                self.samples = 0;
                if self.values.level < platform.setting(Setting::LevelUnits) {
                    self.search.peak_contrast = self.focus.contrast;
                    self.transition.return_ready = 1;
                    return Phase::Approach;
                }
                self.search.probed = 0;
                return Phase::Direction;
            }
        }
        self.samples = 0;
        self.search.probed = u8::from(self.focus.falls < self.focus.rises);
        Phase::Direction
    }

    /// Adds the strikes the level band carries, unless a low level rose by no more than
    /// `LevelDrop` percent.
    fn strike_for_level(&mut self, platform: &mut impl Platform) {
        let level = self.values.level;
        let tens = platform.setting(Setting::LevelTens).wrapping_mul(10);
        if level < tens {
            let previous = self.values.previous_level;
            if level <= percent_of(previous, platform.setting(Setting::LevelDrop)).wrapping_add(previous) {
                return;
            }
        }
        let weight = level_strikes(platform, level);
        self.focus.strikes = self.focus.strikes.wrapping_add(weight as u8);
    }

    /// 0xf3ee60: a second-order filter over the focus contrast, scaled for a selection that
    /// changed hands; the output never falls below the input.
    fn filter_focus_contrast(&mut self, platform: &mut impl Platform) -> u32 {
        let gain = platform.setting(Setting::FilterHandover);
        let feedback = u64::from(platform.setting(Setting::FilterFeedback) as u8);
        let damping = u64::from(platform.setting(Setting::FilterDamping) as u8);
        let input_weight = u64::from(platform.setting(Setting::FilterInput) as u8);
        let focus = &mut self.focus;
        let input = focus.contrast;
        let [previous, earlier] = focus.inputs;
        let sum = input.wrapping_add(earlier).wrapping_add(previous.wrapping_mul(2));
        let feedforward = (u64::from(sum) * input_weight / 100) as u32;
        let [last, before] = focus.outputs;
        let recursion = (i64::from(last) * feedback as i64 - i64::from(before) * damping as i64) / 100;
        let output = (input as i32).max(0).max(feedforward.wrapping_add(recursion as u32) as i32);
        focus.inputs = [input, previous];
        focus.outputs = [output, last];
        let scale = match (self.transition.restarted, self.frame.selected) {
            (0, 1) => gain.wrapping_mul(100) as u16 as i32,
            (1, 0) => 100u32.checked_div(gain).unwrap_or(u32::MAX) as u8 as i32,
            _ => 100,
        };
        (scale.wrapping_mul(output) / 100) as u32
    }

    /// 0xf3efbc: the fall and rise percentages and the minimum-step base for the logarithm
    /// of the filtered contrast.
    fn set_focus_thresholds(&mut self, platform: &mut impl Platform, filtered: u32) {
        let log = platform.contrast_log(filtered);
        let fall_from = platform.setting(Setting::FocusFallFrom).wrapping_mul(100);
        let fall_to = platform.setting(Setting::FocusFallTo);
        let rise_from = platform.setting(Setting::FocusRiseFrom).wrapping_mul(100);
        let rise_to = platform.setting(Setting::FocusRiseTo);
        let step_from = platform.setting(Setting::FocusStepFrom).wrapping_mul(100);
        let step_to = platform.setting(Setting::FocusStepTo);
        let (fall_high, rise_high) = if model_scaled(platform) {
            (platform.setting(Setting::FocusFallScaled), platform.setting(Setting::FocusRiseScaled))
        } else {
            (platform.setting(Setting::FocusFallHigh), platform.setting(Setting::FocusRiseHigh))
        };
        self.focus.fall_percent = interpolate(platform, log, fall_from, fall_to.wrapping_mul(100), Setting::FocusChangePercent, fall_high);
        self.focus.rise_percent = interpolate(platform, log, rise_from, rise_to.wrapping_mul(100), Setting::FocusRiseLow, rise_high);
        self.focus.step_base = if step_from > log {
            platform.setting(Setting::FocusStepLow)
        } else if step_to.wrapping_mul(100) < log {
            platform.setting(Setting::FocusStepHigh)
        } else {
            let high = platform.setting(Setting::FocusStepHigh);
            let low = platform.setting(Setting::FocusStepLow);
            log.wrapping_sub(step_from).wrapping_mul(high.wrapping_sub(low)).checked_div(step_to.wrapping_mul(100).wrapping_sub(step_from)).unwrap_or(0).wrapping_add(platform.setting(Setting::FocusStepLow))
        } as u8;
    }

    fn focus_trace(&self) -> FocusTrace {
        let focus = &self.focus;
        FocusTrace {
            samples: self.samples,
            movement: self.motion.movement,
            motion: [self.motion.level, self.motion.interval],
            focus_reported: self.transition.focus_reported,
            hold_mode: self.transition.hold_mode,
            restarted: self.transition.restarted,
            selected: self.frame.selected,
            count: focus.count,
            contrast: focus.contrast,
            peak_contrast: self.search.peak_contrast,
            fall_percent: focus.fall_percent,
            rise_percent: focus.rise_percent,
            step_base: focus.step_base,
            watch: self.frame.watch,
            watch_reference: self.watch_reference,
            watch_tens: focus.watch_tens,
            rises: focus.rises,
            falls: focus.falls,
            strikes: focus.strikes,
            strike_limit: focus.strike_limit,
            probed: self.search.probed,
        }
    }
}

/// The `low` setting at and below the logarithm `from`, `high` above `to`, interpolated
/// between.
fn interpolate(platform: &mut impl Platform, log: u32, from: u32, to: u32, low: Setting, high: u32) -> u32 {
    if from > log {
        return platform.setting(low);
    }
    if to < log {
        return high;
    }
    let base = platform.setting(low);
    let rise = log.wrapping_sub(from).wrapping_mul(high.wrapping_sub(base)).checked_div(to.wrapping_sub(from)).unwrap_or(0);
    rise.wrapping_add(platform.setting(low))
}
