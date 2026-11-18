#![forbid(unsafe_code)]

//! The S1 track (0xf27fcc): follows the peak, turning back when the contrast drops, and
//! concludes once it has turned often enough after a fall.

use super::search::{S1Phase, Search};
use crate::contrast::settings::{Setting, StepKind};
use crate::contrast::{percent_of, Platform};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Track {
    /// Samples since the last turning drop.
    rises: u8,
    /// Turning drops that followed a rise.
    turns: u8,
    unsteady: u8,
    rising: bool,
}

impl Track {
    pub(super) const START: Self = Self { rises: 0, turns: 0, unsteady: 0, rising: false };
}

impl Search {
    pub(super) fn track(&mut self, platform: &mut impl Platform) -> S1Phase {
        self.speed = 2;
        if self.values.flag != 0 {
            self.samples = 0;
            return S1Phase::Climb;
        }
        if self.frame.phase_mode != self.frame.mode {
            self.samples = 0;
            return S1Phase::Track;
        }
        self.begin_sample();
        self.step = self.step_for(platform, StepKind::Track);
        self.turn_on_drop(platform);
        self.turn_at_limits();
        let next = self.judge_track(platform);
        self.shift_trend();
        next
    }

    /// 0xf26270.
    fn turn_on_drop(&mut self, platform: &mut impl Platform) {
        if self.samples <= 3 || !self.trend.moved() {
            self.direction = self.previous_direction;
            return;
        }
        let mode = platform.setting(Setting::SelectedReturnMode);
        let low = (mode > 2 || mode == 1)
            && self.frame.mode == 1
            && self.trend.contrast < platform.setting(Setting::SelectedReturnContrast).wrapping_mul(100);
        let percent = platform.setting(if low { Setting::SelectedReturnFall } else { Setting::ReturnFall });
        let (contrast, previous) = (self.trend.contrast, self.trend.previous);
        let moved_up = self.trend.position > self.trend.previous_position;
        let track = &mut self.track;
        if previous > contrast.wrapping_add(percent_of(previous, percent)) {
            if track.rising {
                track.rising = false;
                track.turns = track.turns.wrapping_add(1);
            }
            track.rises = 0;
            self.direction = if moved_up { -1 } else { 1 };
        } else {
            if !low || contrast >= previous {
                track.rising = true;
                track.rises = track.rises.wrapping_add(1);
            }
            self.direction = if moved_up { 1 } else { -1 };
        }
    }

    /// 0xf27cbc.
    fn judge_track(&mut self, platform: &mut impl Platform) -> S1Phase {
        if self.samples <= 2 {
            return S1Phase::Track;
        }
        let (contrast, position) = (self.trend.contrast, self.trend.position);
        if contrast > self.best.contrast {
            self.best.contrast = contrast;
            self.best.position = position;
            self.best.speed = self.trend.class;
            if self.trend.moved() {
                self.fit.note_best(contrast, position);
            }
        } else if self.trend.moved() && self.fit.complete(platform, contrast, position) {
            self.best.position = self.fit.fit(platform);
        }
        if self.trend.moved() {
            self.fit.note_newest(contrast, position);
        }

        let mut next = S1Phase::Track;
        if self.unsteady(platform) {
            self.track.unsteady = self.track.unsteady.wrapping_add(1);
            if u32::from(self.track.unsteady) >= platform.setting(Setting::ReturnStrikes) {
                self.halt();
                next = S1Phase::Climb;
            }
        } else {
            self.track.unsteady = 0;
        }
        if u32::from(self.track.rises) >= platform.setting(Setting::ReturnRises) {
            self.samples = 0;
            return self.to_sweep();
        }
        if u32::from(self.track.turns) >= platform.setting(Setting::PeakReturns) {
            self.halt();
            let dropped = self.trend.previous > contrast.wrapping_add(platform.setting(Setting::ConcludeDropTens).wrapping_mul(10));
            return if dropped || platform.setting(Setting::TrendOverride) == 0 { S1Phase::Conclude } else { S1Phase::Climb };
        }
        next
    }

    /// The contrast is low, or outside the band around the best contrast.
    fn unsteady(&self, platform: &mut impl Platform) -> bool {
        let contrast = self.trend.contrast;
        if contrast < platform.setting(Setting::MotionReturn).wrapping_mul(10) {
            return true;
        }
        let best = self.best.contrast;
        best > band(platform, best, contrast) || contrast > band(platform, best, best)
    }
}

fn band(platform: &mut impl Platform, best: u32, from: u32) -> u32 {
    let configured = platform.setting(Setting::FocusStepLow);
    let percent = platform.minimum_step(configured);
    let margin = platform.setting(Setting::FocusReturnTens).wrapping_mul(10);
    from.wrapping_add(percent_of(best, percent)).wrapping_add(margin)
}
