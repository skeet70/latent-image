#![forbid(unsafe_code)]

//! The direction search: which way the lens should move to raise the contrast (0xf429d0,
//! 0xf41c48 and its helpers).

use super::settings::{Setting, StepKind, TrendBasis};
use super::{Context, FitPoint, Phase, Platform};
use xm1_saclac_controllers::search::LensPosition;
use xm1_saclac_interface::search::BridgeStatus;

/// What the direction search and the peak search entry keep between samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Search {
    pub probes: u8,
    pub turns: u8,
    pub falling: u8,
    pub rising: u8,
    pub still: u8,
    pub climbing: bool,
    pub probed: u8,
    pub waits: u8,
    pub lens_seen: bool,
    pub decision: u8,
    /// The previous and newest reported positions, as raw halfwords.
    pub reported: [u16; 2],
    pub peak_flags: [u8; 4],
    /// The way the reported position last moved by more than the jump setting.
    pub trend: i8,
    pub previous_trend: i8,
    pub sum: u32,
    pub average: u32,
    pub previous_average: u32,
    pub count: u32,
    pub contrast: u32,
    pub previous_contrast: u32,
    pub start_contrast: u32,
    pub change: i32,
    pub previous_change: i32,
    pub position: LensPosition,
    pub previous_position: LensPosition,
    pub class: u32,
    pub settle: u8,
    pub settle_armed: u8,
    pub peak_start: LensPosition,
    pub peak_marks: [u8; 2],
    /// The channel's highest contrast when a peak search ended.
    pub peak_contrast: u32,
    pub focus_motion: u32,
    pub focus_marks: [u16; 2],
    pub focus_bytes: [u8; 2],
    /// A restart the next transition takes (0xf3d490).
    pub restart_pending: bool,
    /// The channel choice takes each pair's second sum; also a pending restart.
    pub second_channel: bool,
    pub fit: u16,
    pub fit_points: [FitPoint; 4],
    pub selected_bytes: [u8; 2],
}

impl Search {
    pub(crate) const fn new() -> Self {
        Self {
            probes: 0,
            turns: 0,
            falling: 0,
            rising: 0,
            still: 0,
            climbing: false,
            probed: 0,
            waits: 0,
            lens_seen: false,
            decision: 0,
            reported: [0; 2],
            peak_flags: [0; 4],
            trend: 1,
            previous_trend: 1,
            sum: 0,
            average: 0,
            previous_average: 0,
            count: 0,
            contrast: 0,
            previous_contrast: 0,
            start_contrast: 0,
            change: 0,
            previous_change: 0,
            position: LensPosition(0),
            previous_position: LensPosition(0),
            class: 2,
            settle: 0,
            settle_armed: 0,
            peak_start: LensPosition(0),
            peak_marks: [0; 2],
            peak_contrast: 0,
            focus_motion: 0,
            focus_marks: [0; 2],
            focus_bytes: [0; 2],
            restart_pending: false,
            second_channel: false,
            fit: 0,
            fit_points: [FitPoint::EMPTY; 4],
            selected_bytes: [0; 2],
        }
    }
}

/// How a direction sample ends before the common tail.
enum Outcome {
    Stay,
    Probe,
    Return,
}

impl Context {
    /// The direction-search fields 0xf3c17c clears.
    pub(crate) fn forget_direction(&mut self) {
        let search = &mut self.search;
        search.trend = 1;
        search.previous_trend = 1;
        search.sum = 0;
        search.average = 0;
        search.previous_average = 0;
        search.count = 0;
        search.settle = 0;
        search.settle_armed = 0;
        search.probed = 0;
        search.focus_motion = 0;
        search.focus_marks = [0; 2];
        search.focus_bytes = [0; 2];
        search.restart_pending = false;
        search.second_channel = false;
        search.selected_bytes = [0; 2];
        self.tracks = super::tracks::Tracks::new();
    }

    /// 0xf429d0.
    pub(crate) fn direction_step(&mut self, platform: &mut impl Platform) -> Phase {
        self.bridge.speed = 2;
        if self.samples == 1 {
            self.search.focus_marks = [0; 2];
            self.search.focus_bytes = [0; 2];
        }
        if self.holding() { self.hold_lens(platform) } else { self.search_direction(platform) }
    }

    /// 0xf41c48.
    fn search_direction(&mut self, platform: &mut impl Platform) -> Phase {
        let model = platform.setting(Setting::ModelLetter) as u8;
        self.transition.hold_mode = 0;
        self.note_direction_sample();
        self.bridge.lens_mode = platform.lens_mode();
        let step = self.step_size(platform, StepKind::Direction);
        self.step = step.size;
        self.bridge.rate = step.rate;
        self.scale_direction_step(platform, model);
        self.follow_reported_position(platform);
        let outcome = self.direction_outcome(platform);
        let chosen = match outcome {
            Ok(Outcome::Stay) => Phase::Direction,
            Ok(Outcome::Probe) => Phase::PeakSearch,
            Ok(Outcome::Return) => Phase::Return,
            Err(phase) => phase,
        };
        self.limit_direction();
        if platform.setting(Setting::ReversalSteps) == 1 && self.direction != self.previous_direction {
            let reversal = platform.setting(Setting::ReversalStep) as u16;
            self.step = reversal.wrapping_mul(self.direction as u16).wrapping_add(self.step);
        }
        self.shift_direction_history();
        chosen
    }

    /// 0xf41b70.
    fn note_direction_sample(&mut self) {
        if self.samples == 1 {
            let search = &mut self.search;
            search.peak_flags = [0; 4];
            search.contrast = 0;
            search.previous_contrast = 0;
            search.previous_position = LensPosition(0);
            search.position = LensPosition(0);
            search.turns = 0;
            search.probes = 0;
            search.climbing = false;
            search.waits = 0;
            search.class = 2;
            self.bridge.last_report = BridgeStatus::Moved;
            search.lens_seen = false;
            self.previous_lens = self.lens;
            search.start_contrast = self.values.contrast;
            search.change = 0;
            search.previous_change = 0;
            search.rising = 0;
            search.falling = 0;
            search.still = 0;
            search.peak_contrast = 0;
            search.fit_points = [FitPoint::EMPTY; 4];
            self.forget_tracks();
        }
        self.search.decision = 0;
        self.search.contrast = self.values.contrast;
        self.search.position = self.positions[2];
        self.search.class = self.classes[2];
    }

    /// The step scaling of 0xf41c48: a lens change scales it for a few samples, a paired
    /// evaluation scales it by its level band, and the travel range bounds it.
    fn scale_direction_step(&mut self, platform: &mut impl Platform, model: u8) {
        if self.lens_changed {
            self.search.settle = platform.setting(Setting::DirectionSettle) as u8;
        } else if self.search.settle != 0 {
            self.search.settle -= 1;
        }
        if self.search.settle != 0 {
            self.step = tenths(self.step, platform.setting(Setting::DirectionSettleScale));
            self.values.flag = 0;
        } else if platform.setting(Setting::PairedEvaluation) == 1 {
            let own = platform.setting(Setting::ModelScaling);
            let model_scaled = own == 255 || (own == 0 && matches!(model, b'M' | b'S'));
            let band = level_band(platform, self.values.level);
            self.step = tenths(self.step, platform.setting(Setting::DirectionScale { band, model_scaled }));
        }
        let divisor = platform.setting(Setting::DirectionStepDivisor);
        if divisor != 0 {
            let range = i32::from(self.limits.high.0) - i32::from(self.limits.low.0);
            let bound = range / divisor as i32;
            if i32::from(self.step as i16) >= bound {
                self.step = bound as u16;
            }
        }
    }

    /// 0xf41c48: a reported position that jumps restarts the contrast average.
    fn follow_reported_position(&mut self, platform: &mut impl Platform) {
        let search = &mut self.search;
        let previous = search.reported[1];
        let current = self.reported.0 as u16;
        search.reported = [previous, current];
        if previous != current {
            let jump = platform.setting(Setting::PositionJump);
            if (i32::from(previous) - i32::from(current)).unsigned_abs() > jump {
                search.previous_trend = search.trend;
                if search.count == 0 {
                    search.count = 1;
                }
                search.previous_average = search.average;
                search.average = search.sum / search.count;
                search.sum = 0;
                search.count = 0;
                search.trend = if previous < current { 1 } else { -1 };
            }
        }
        search.sum = search.sum.wrapping_add(search.contrast);
        search.count = search.count.wrapping_add(1);
    }

    /// The decision part of 0xf41c48. `Err` carries a phase the hold or trend chose directly.
    fn direction_outcome(&mut self, platform: &mut impl Platform) -> Result<Outcome, Phase> {
        if self.bridge.last_report != BridgeStatus::Moved {
            self.direction = self.previous_direction;
            return Ok(Outcome::Stay);
        }
        if self.samples == 1 {
            let trend = self.search.trend;
            let confirmed = (trend == 1 && self.search.rising != 0) || (trend == -1 && self.search.falling != 0);
            self.direction = if confirmed { trend } else { trend.wrapping_neg() };
        }
        if self.previous_lens != self.lens {
            self.step = 0;
            return Ok(Outcome::Stay);
        }
        if self.values.flag != 0
            && platform.setting(Setting::FlagRestart) & 2 != 0
            && (self.frame.selected == 0 || (self.frame.selected == 1 && platform.setting(Setting::SelectedFlagRestart) == 1))
        {
            self.step = 0;
            self.lens_count = 0;
            self.reverse_and_forget_trend();
            return Ok(Outcome::Stay);
        }
        if self.lens_count != 0xff {
            self.lens_count += 1;
            if self.lens_count > 0xff {
                self.lens_count = 0xff;
            } else if self.lens_count == 1 {
                self.motion.level = 0;
                self.motion.interval = 0;
                self.search.rising = 0;
                self.search.falling = 0;
                self.motion.forget();
                if platform.setting(Setting::MotionCheck) == 1 {
                    platform.request_angular_rate();
                }
            }
        }
        if self.camera_moving(platform) {
            self.step = 0;
            self.reverse_and_forget_trend();
            return Ok(Outcome::Stay);
        }
        if u32::from(self.lens_count) < platform.setting(Setting::DirectionSamples) {
            self.step = 0;
            self.search.rising = 0;
            self.search.falling = 0;
            return Ok(Outcome::Stay);
        }
        self.lens_changed = false;
        if self.transition.hold_mode == 0
            && !self.monitor.restart
            && self.search.focus_motion > platform.setting(Setting::MotionReturn).wrapping_mul(10)
            && platform.setting(Setting::MotionReturns) == 1
        {
            self.samples = 0;
            return Ok(Outcome::Return);
        }
        if self.monitor.restart && self.monitor.watched == 16 {
            self.samples = 0;
            self.monitor.restart = false;
            return Ok(Outcome::Return);
        }
        self.monitor.restart = false;
        if platform.setting(Setting::TrendDirection) == 1 {
            self.search.probes = if self.search.probes == 0 {
                self.search.rising = 0;
                self.search.falling = 0;
                1
            } else {
                self.search.probes.wrapping_add(1)
            };
            self.search.change = self.search.contrast.wrapping_sub(self.search.previous_contrast) as i32;
            self.count_trend();
            let outcome = self.decide_direction(platform);
            self.search.start_contrast = self.search.contrast;
            return Ok(outcome);
        }
        if self.values.unsettled && !self.search.lens_seen {
            self.step = 0;
            return Ok(Outcome::Stay);
        }
        if !platform.lens_ready() {
            self.step = 0;
            return Ok(Outcome::Stay);
        }
        self.search.lens_seen = true;
        if self.search.probed == 0 && platform.setting(Setting::ProbeAtOnce) == 0 {
            return Ok(self.probe(platform, 8));
        }
        self.search.probes = self.search.probes.wrapping_add(1);
        if self.values.unsettled {
            self.search.waits = match self.search.waits.wrapping_add(1) {
                0xff => 0xfe,
                waits => waits,
            };
            if u32::from(self.search.waits) >= platform.setting(Setting::ProbeSamples) {
                self.step = 0;
                return Ok(Outcome::Stay);
            }
            return Ok(self.turn_or_probe(platform));
        }
        if u32::from(self.search.waits) >= platform.setting(Setting::ProbeSamples) {
            return Ok(self.probe(platform, 4));
        }
        self.search.waits = 0;
        if self.search.contrast < platform.setting(Setting::ProbeContrast).wrapping_mul(10) {
            return Ok(self.probe(platform, 1));
        }
        if self.search.contrast < self.search.previous_contrast {
            self.search.climbing = false;
        } else {
            self.search.turns = if self.search.climbing { 0 } else { self.search.turns.wrapping_add(1) };
            self.search.climbing = true;
        }
        Ok(self.turn_or_probe(platform))
    }

    fn reverse_and_forget_trend(&mut self) {
        self.direction = self.previous_direction.wrapping_neg();
        self.search.rising = 0;
        self.search.falling = 0;
    }

    /// Start a peak search from where the lens is, in the direction `bit` of the probe
    /// setting gives.
    fn probe(&mut self, platform: &mut impl Platform, bit: u32) -> Outcome {
        self.search.peak_marks = [0; 2];
        self.search.peak_start = self.positions[0];
        self.samples = 0;
        self.direction = if platform.setting(Setting::ProbeDirection) & bit != 0 { 1 } else { -1 };
        self.step = 0;
        Outcome::Probe
    }

    /// 0xf3c8d8.
    fn turn_or_probe(&mut self, platform: &mut impl Platform) -> Outcome {
        if u32::from(self.search.turns) >= platform.setting(Setting::TrendSamples) {
            let outcome = self.probe(platform, 2);
            self.search.decision = 1;
            return outcome;
        }
        if u32::from(self.search.probes) >= platform.setting(Setting::TrendLimit { selected: false, basis: TrendBasis::Plain }) {
            return self.probe(platform, 2);
        }
        self.direction = self.previous_direction.wrapping_neg();
        self.search.decision = 4;
        Outcome::Stay
    }

    /// 0xf3c6c0: count rising and falling samples along the trend and its predecessor.
    fn count_trend(&mut self) {
        let search = &mut self.search;
        let (contrast, previous) = (search.contrast, search.previous_contrast);
        match search.trend {
            1 => count(search, contrast < previous, contrast == 0),
            -1 => count_reversed(search, contrast < previous, contrast == 0),
            _ => {
                search.rising = 0;
                search.falling = 0;
            }
        }
        let (average, previous) = (search.average, search.previous_average);
        match search.previous_trend {
            1 => count(search, average > previous, average == 0),
            -1 => count_reversed(search, average > previous, average == 0),
            _ => {}
        }
        if self.values.unsettled {
            self.search.rising = 0;
            self.search.falling = 0;
        }
    }

    /// 0xf3c9f4: decide from the trend counts.
    fn decide_direction(&mut self, platform: &mut impl Platform) -> Outcome {
        self.search.decision = 4;
        let end_step = self.step_size(platform, StepKind::Scan);
        self.bridge.rate = end_step.rate;
        if self.search.settle != 0 {
            let samples = platform.setting(Setting::TrendSamples);
            if u32::from(self.search.rising) >= samples {
                self.search.rising = 1;
            } else if u32::from(self.search.falling) >= samples {
                self.search.falling = 1;
            }
        }
        let basis = if self.search.settle != 0 && !self.values.unsettled {
            TrendBasis::Settling
        } else if self.frame.step_class == 1 {
            TrendBasis::StepClass
        } else {
            TrendBasis::Plain
        };
        let selected = platform.measurement_selection() == 1;
        let limit = platform.setting(Setting::TrendLimit { selected, basis });
        let samples = platform.setting(Setting::TrendSamples);
        let decided = u32::from(self.search.rising) >= samples || u32::from(self.search.falling) >= samples;
        if platform.setting(Setting::PairedEvaluation) == 1 {
            if u32::from(self.search.probes) >= limit || decided {
                self.direction_from_counts();
                let outcome = if self.ends_in_return(platform, end_step.size) {
                    self.search.peak_contrast = 0;
                    Outcome::Return
                } else {
                    self.enter_probe();
                    Outcome::Probe
                };
                self.forget_counts();
                self.search.trend = 0;
                return outcome;
            }
            self.direction = self.previous_direction.wrapping_neg();
            self.override_from_counts(platform);
            if self.values.level >= platform.setting(Setting::HandOffLevel) || self.values.unsettled {
                return Outcome::Stay;
            }
            self.search.probes = self.search.probes.wrapping_add(limit as u8);
            return Outcome::Stay;
        }
        if u32::from(self.search.probes) >= limit {
            self.direction_from_counts();
            let outcome = if u32::from(self.search.still) >= platform.setting(Setting::StillSamples)
                || platform.setting(Setting::TrendOverride) == 0
            {
                self.enter_probe();
                Outcome::Probe
            } else {
                self.search.peak_contrast = 0;
                Outcome::Return
            };
            self.forget_counts();
            return outcome;
        }
        let still_change = platform.setting(Setting::StillChange).wrapping_mul(10) as i32;
        let changed = [self.search.change, self.search.previous_change].map(|change| !signed_abs_below(change, still_change));
        if decided {
            self.direction_from_counts();
            let outcome = if changed == [true, true] && platform.setting(Setting::TrendOverride) == 1 {
                self.search.peak_contrast = 0;
                Outcome::Return
            } else {
                self.enter_probe();
                Outcome::Probe
            };
            self.forget_counts();
            return outcome;
        }
        self.direction = if self.search.reported[0] > self.search.reported[1] { 1 } else { -1 };
        self.override_from_counts(platform);
        self.search.still = if changed == [false, false] { self.search.still.wrapping_add(1) } else { 0 };
        Outcome::Stay
    }

    fn direction_from_counts(&mut self) {
        if self.search.rising != 0 {
            self.direction = 1;
        } else if self.search.falling != 0 {
            self.direction = -1;
        }
    }

    fn override_from_counts(&mut self, platform: &mut impl Platform) {
        if platform.setting(Setting::TrendOverride) == 1 {
            let samples = platform.setting(Setting::OverrideSamples);
            if u32::from(self.search.rising) >= samples {
                self.direction = 1;
            } else if u32::from(self.search.falling) >= samples {
                self.direction = -1;
            }
        }
    }

    /// A paired direction decision returns instead of probing near a travel end it is
    /// heading for, or when the level is low or exceeds the contrast.
    fn ends_in_return(&mut self, platform: &mut impl Platform, end_step: u16) -> bool {
        if self.values.level < platform.setting(Setting::LevelTens).wrapping_mul(10)
            && self.values.combined[0] > self.values.seconds[0].wrapping_add(platform.setting(Setting::PairedMargin).wrapping_mul(10))
        {
            return true;
        }
        if self.frame.step_class == 1 {
            return true;
        }
        let position = i32::from(self.positions[0].0);
        let ends = platform.setting(Setting::EndSteps) as i32 * i32::from(end_step as i16);
        if position > i32::from(self.limits.high.0) - ends && self.direction == 1 {
            return true;
        }
        if position < i32::from(self.limits.low.0) + ends && self.direction == -1 {
            return true;
        }
        self.values.level > self.search.contrast
    }

    fn enter_probe(&mut self) {
        self.search.peak_marks = [0; 2];
        self.search.peak_start = self.positions[0];
        self.step = 0;
        self.search.decision = 1;
        self.search.climbing = false;
        self.search.turns = 0;
    }

    fn forget_counts(&mut self) {
        self.search.rising = 0;
        self.search.falling = 0;
        self.samples = 0;
        self.search.probes = 0;
        self.search.still = 0;
    }

    /// 0xf3d554: point back into the travel limits.
    fn limit_direction(&mut self) {
        let position = self.positions[0].0;
        if self.limits.low.0 >= position {
            self.direction = 1;
        } else if self.limits.high.0 <= position {
            self.direction = -1;
        }
    }

    /// 0xf3d5c4.
    fn shift_direction_history(&mut self) {
        self.values.previous_level = self.values.level;
        self.search.previous_change = self.search.change;
        self.search.previous_contrast = self.search.contrast;
        self.search.previous_position = self.search.position;
    }
}

/// The step scaled by `scale` tenths, truncated as stock divides.
fn tenths(step: u16, scale: u32) -> u16 { ((i32::from(step as i16).wrapping_mul(scale as i32)) / 10) as u16 }

/// Which band of the level ladder `level` falls in, from the top.
fn level_band(platform: &mut impl Platform, level: u32) -> u8 {
    if level >= platform.setting(Setting::LevelThousands).wrapping_mul(1000) {
        0
    } else if level >= platform.setting(Setting::LevelHundreds).wrapping_mul(100) {
        1
    } else if level >= platform.setting(Setting::LevelTens).wrapping_mul(10) {
        2
    } else if level >= platform.setting(Setting::LevelUnits) {
        3
    } else {
        4
    }
}

/// One sample along a rising trend: `fell` counts a fall, a zero value clears both counts.
fn count(search: &mut Search, fell: bool, zero: bool) {
    if fell {
        search.rising = 0;
        search.falling = search.falling.wrapping_add(1);
    } else if zero {
        search.rising = 0;
        search.falling = 0;
    } else {
        search.rising = search.rising.wrapping_add(1);
        search.falling = 0;
    }
}

/// One sample along a falling trend.
fn count_reversed(search: &mut Search, fell: bool, zero: bool) {
    if fell {
        search.rising = search.rising.wrapping_add(1);
        search.falling = 0;
    } else if zero {
        search.rising = 0;
        search.falling = 0;
    } else {
        search.rising = 0;
        search.falling = search.falling.wrapping_add(1);
    }
}

/// Stock's `|value| < bound` on signed words; the magnitude of `i32::MIN` stays negative.
fn signed_abs_below(value: i32, bound: i32) -> bool { value.wrapping_abs() < bound }
