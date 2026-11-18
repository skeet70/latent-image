#![forbid(unsafe_code)]

//! The S1 climb (0xf2751c): steps toward rising contrast and decides, from trend counts or
//! from settled evaluations, whether to sweep the travel range or track a peak.

use super::search::{S1Phase, Search};
use crate::contrast::settings::{Setting, StepKind, TrendBasis};
use crate::contrast::Platform;
use xm1_saclac_controllers::search::LensPosition;
use xm1_saclac_interface::search::BridgeStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Climb {
    samples: u8,
    /// Samples whose contrast trend puts the peak below the lens, then above it.
    toward: [u8; 2],
    still: u8,
    /// Rises that followed a fall.
    turns: u8,
    rising: bool,
    unsettled: u8,
    /// A settled evaluation arrived while the lens was ready.
    settled: bool,
    /// The reported position at this sample, then at the one before.
    reported: [LensPosition; 2],
}

const LOWER: usize = 0;
const HIGHER: usize = 1;

impl Climb {
    pub(super) const START: Self = Self {
        samples: 0,
        toward: [0; 2],
        still: 0,
        turns: 0,
        rising: false,
        unsettled: 0,
        settled: false,
        reported: [LensPosition(0); 2],
    };

    pub(super) fn restart(&mut self) { *self = Self { reported: self.reported, ..Self::START }; }
}

impl Search {
    pub(super) fn climb(&mut self, platform: &mut impl Platform) -> S1Phase {
        let next = self.climb_step(platform);
        self.turn_at_limits();
        self.shift_trend();
        next
    }

    fn climb_step(&mut self, platform: &mut impl Platform) -> S1Phase {
        self.speed = 2;
        self.begin_sample();
        let step = self.step_for(platform, StepKind::Climb);
        let limit = platform.setting(Setting::ClimbStepLimit);
        self.step = if i32::from(step as i16) >= limit as i32 { limit as u16 } else { step };
        self.climb.reported = [self.reported, self.climb.reported[0]];
        if self.report_status != BridgeStatus::Moved {
            self.direction = self.previous_direction;
            return S1Phase::Climb;
        }
        if self.samples == 1 {
            let (position, step) = (i32::from(self.positions[0].0), i32::from(self.step as i16));
            if position > i32::from(self.limits.high.0) - step {
                self.direction = -1;
            } else if position < i32::from(self.limits.low.0) + step {
                self.direction = 1;
            }
        }
        if self.values.flag != 0 {
            self.step = 0;
            return S1Phase::Climb;
        }
        if platform.setting(Setting::TrendDirection) == 1 {
            return self.climb_on_trend(platform);
        }
        if (self.values.settled == 1 || self.climb.settled) && platform.lens_ready() {
            return self.climb_on_settled(platform);
        }
        self.step = 0;
        S1Phase::Climb
    }

    /// 0xf25e00 and 0xf25f0c.
    fn climb_on_trend(&mut self, platform: &mut impl Platform) -> S1Phase {
        if self.climb.samples == 0 {
            self.climb.toward = [0; 2];
            self.climb.samples = 1;
        } else {
            self.climb.samples = self.climb.samples.wrapping_add(1);
        }
        if self.climb.reported[1] == self.climb.reported[0] {
            return S1Phase::Climb;
        }
        self.trend.change = self.trend.contrast.wrapping_sub(self.trend.previous);
        self.count_trend();
        self.decide_on_trend(platform)
    }

    /// A contrast of 0 that did not fall counts for neither side.
    fn count_trend(&mut self) {
        let falling = self.trend.contrast < self.trend.previous;
        let side = match (self.direction, falling) {
            (1, false) | (-1, true) => HIGHER,
            (1, true) | (-1, false) => LOWER,
            _ => {
                self.climb.toward = [0; 2];
                return;
            }
        };
        let toward = &mut self.climb.toward;
        toward[1 - side] = 0;
        toward[side] = if !falling && self.trend.contrast == 0 { 0 } else { toward[side].wrapping_add(1) };
    }

    fn decide_on_trend(&mut self, platform: &mut impl Platform) -> S1Phase {
        let still_change = 10 * platform.setting(Setting::ClimbStillChange) as i32;
        let changes_still = [self.trend.change, self.trend.previous_change].map(|change| (change as i32).wrapping_abs() < still_change);
        let trend_samples = platform.setting(Setting::TrendSamples);
        let toward = self.climb.toward;
        let decided = if u32::from(self.climb.samples) >= platform.setting(Setting::TrendLimit { selected: false, basis: TrendBasis::Plain }) {
            Some(u32::from(self.climb.still) < platform.setting(Setting::StillSamples) && platform.setting(Setting::TrendOverride) != 0)
        } else if u32::from(toward[HIGHER]) >= trend_samples || u32::from(toward[LOWER]) >= trend_samples {
            Some(!changes_still[0] && !changes_still[1] && platform.setting(Setting::TrendOverride) == 1)
        } else {
            None
        };
        if let Some(tracks) = decided {
            if toward[HIGHER] != 0 {
                self.direction = 1;
            } else if toward[LOWER] != 0 {
                self.direction = -1;
            }
            let next = if tracks {
                self.best.contrast = 0;
                S1Phase::Track
            } else {
                self.climb.rising = false;
                self.climb.turns = 0;
                self.to_sweep()
            };
            self.climb.toward = [0; 2];
            self.samples = 0;
            self.climb.samples = 0;
            self.climb.still = 0;
            return next;
        }
        self.direction = if self.climb.reported[1].0 as u16 > self.climb.reported[0].0 as u16 { 1 } else { -1 };
        if platform.setting(Setting::TrendOverride) == 1 {
            let lead = platform.setting(Setting::OverrideSamples);
            if u32::from(toward[HIGHER]) >= lead {
                self.direction = 1;
            } else if u32::from(toward[LOWER]) >= lead {
                self.direction = -1;
            }
        }
        self.climb.still = if changes_still[0] && changes_still[1] { self.climb.still.wrapping_add(1) } else { 0 };
        S1Phase::Climb
    }

    fn climb_on_settled(&mut self, platform: &mut impl Platform) -> S1Phase {
        self.climb.settled = true;
        if platform.setting(Setting::ProbeAtOnce) == 0 {
            return self.probe(platform, 8);
        }
        self.climb.samples = self.climb.samples.wrapping_add(1);
        if self.values.settled != 1 {
            self.climb.unsettled = match self.climb.unsettled.wrapping_add(1) {
                255 => 254,
                unsettled => unsettled,
            };
            if u32::from(self.climb.unsettled) >= platform.setting(Setting::ProbeSamples) {
                self.step = 0;
                return S1Phase::Climb;
            }
            return self.judge_climb(platform);
        }
        if u32::from(self.climb.unsettled) >= platform.setting(Setting::ProbeSamples) {
            return self.probe(platform, 4);
        }
        self.climb.unsettled = 0;
        if self.trend.contrast < 10 * platform.setting(Setting::ProbeContrast) {
            return self.probe(platform, 1);
        }
        if self.trend.contrast < self.trend.previous {
            self.climb.rising = false;
        } else {
            self.climb.turns = if self.climb.rising { 0 } else { self.climb.turns.wrapping_add(1) };
            self.climb.rising = true;
        }
        self.judge_climb(platform)
    }

    /// 0xf25c8c.
    fn judge_climb(&mut self, platform: &mut impl Platform) -> S1Phase {
        if u32::from(self.climb.turns) >= platform.setting(Setting::TrendSamples) {
            return self.probe(platform, 2);
        }
        if u32::from(self.climb.samples) >= platform.setting(Setting::TrendLimit { selected: false, basis: TrendBasis::Plain }) {
            self.samples = 0;
            self.direction = -1;
            return self.to_sweep();
        }
        self.direction = self.previous_direction.wrapping_neg();
        S1Phase::Climb
    }

    /// Sweeps in the direction `ProbeDirection` sets for `bit`.
    fn probe(&mut self, platform: &mut impl Platform, bit: u32) -> S1Phase {
        self.samples = 0;
        self.direction = Self::probe_direction(platform, bit);
        self.to_sweep()
    }

    pub(super) fn to_sweep(&mut self) -> S1Phase {
        self.sweep.ends = [false; 2];
        self.step = 0;
        S1Phase::Sweep
    }
}
