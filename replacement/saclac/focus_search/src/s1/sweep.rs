#![forbid(unsafe_code)]

//! The S1 sweep (0xf287e8): runs through the travel range until a channel's contrast has risen
//! and passed its peak, or both ends were reached, and concludes at the best position.

use super::search::{Best, S1Phase, Search};
use crate::contrast::settings::{Setting, StepKind};
use crate::contrast::{recent_levels_high, Platform};
use xm1_saclac_controllers::search::LensPosition;

/// Samples a series keeps; a full series overwrites its last.
const SERIES: usize = 256;

/// The first pair's sums, as bits: the second sum, then the first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Channels(u8);

impl Channels {
    const NONE: Self = Self(0);
    const SECONDS: Self = Self(1);
    const COMBINED: Self = Self(2);
    const BOTH: Self = Self(3);

    const fn has(self, channel: Self) -> bool { self.0 & channel.0 != 0 }

    const fn meets(self, other: Self) -> bool { self.0 & other.0 != 0 }

    fn add(&mut self, channel: Self) { self.0 |= channel.0; }
}

/// One sample per evaluation from the third: each sum of the first pair and the reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Series {
    len: u16,
    seconds: [u32; SERIES],
    combined: [u32; SERIES],
    reference: [u32; SERIES],
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

    fn newest(series: &[u32; SERIES], len: u16) -> u32 { series[usize::from(len.max(1)) - 1] }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Sweep {
    /// The travel limits reached, low then high.
    pub ends: [bool; 2],
    /// The first sum, not the second, decides where to conclude.
    combined: bool,
    follows: Channels,
    rose: Channels,
    passed: Channels,
    /// Each sum's highest and lowest, the second sum first.
    highest: [u32; 2],
    lowest: [u32; 2],
    series: Series,
    best: LensPosition,
    best_class: u32,
    /// The record's near position when the sweep began.
    near: i16,
}

impl Sweep {
    pub(super) const fn new() -> Self {
        Self {
            ends: [false; 2],
            combined: false,
            follows: Channels::NONE,
            rose: Channels::NONE,
            passed: Channels::NONE,
            highest: [0; 2],
            lowest: [0; 2],
            series: Series::new(),
            best: LensPosition(0),
            best_class: 0,
            near: 0,
        }
    }

    const fn channel(&self) -> usize { self.combined as usize }
}

struct Thresholds {
    high: Setting,
    low: Setting,
    from: Setting,
    to: Setting,
}

const RISE: Thresholds = Thresholds { high: Setting::RiseHigh, low: Setting::RiseLow, from: Setting::RiseFrom, to: Setting::RiseTo };
const FALL: Thresholds = Thresholds { high: Setting::FallHigh, low: Setting::FallLow, from: Setting::FallFrom, to: Setting::FallTo };

impl Search {
    pub(super) fn sweep(&mut self, platform: &mut impl Platform) -> S1Phase {
        self.speed = 2;
        if self.values.flag != 0 {
            self.halt();
            return S1Phase::Climb;
        }
        if self.frame.phase_mode != self.frame.mode {
            self.direction = self.previous_direction;
            self.halt();
            return S1Phase::Sweep;
        }
        if self.samples == 1 {
            self.begin_sweep(platform);
        }
        if !self.record.scan && self.frame.mode == 0 && recent_levels_high(platform, self.samples) {
            self.record.scan = true;
            self.direction = self.previous_direction;
            self.halt();
            return S1Phase::Sweep;
        }
        if self.samples > 2 {
            self.record_sweep_sample(platform);
        }
        self.step = self.step_for(platform, StepKind::Sweep);
        let next = self.sweep_step(platform);
        self.limit_step();
        next
    }

    fn sweep_step(&mut self, platform: &mut impl Platform) -> S1Phase {
        if self.samples <= 3 {
            self.direction = self.previous_direction;
            return S1Phase::Sweep;
        }
        if platform.setting(Setting::LevelJump) == 1 && self.near_moved(platform) {
            self.halt();
            self.direction = self.previous_direction.wrapping_neg();
            return S1Phase::Climb;
        }
        let follows = self.sweep.follows;
        if follows != self.sweep.rose {
            self.test_rise(platform);
        }
        if follows != self.sweep.passed {
            self.test_fall(platform);
        }
        if follows.meets(self.sweep.passed) {
            let next = if !follows.meets(self.sweep.rose) {
                S1Phase::Sweep
            } else if platform.setting(Setting::LevelJump) != 1 && self.near_moved(platform) {
                S1Phase::Climb
            } else {
                let sweep = &self.sweep;
                self.best = Best { contrast: sweep.highest[sweep.channel()], position: sweep.best, speed: sweep.best_class };
                S1Phase::Conclude
            };
            self.direction = self.previous_direction.wrapping_neg();
            self.halt();
            return next;
        }
        self.direction = self.previous_direction;
        let position = self.positions[0];
        if self.limits.low >= position {
            self.direction = 1;
            self.step = platform.setting(Setting::EndStep) as u16;
        } else if self.limits.high <= position {
            self.direction = -1;
            self.step = platform.setting(Setting::EndStep) as u16;
        }
        let evaluated = self.positions[2];
        if self.limits.low >= evaluated {
            self.sweep.ends[0] = true;
        } else if self.limits.high <= evaluated {
            self.sweep.ends[1] = true;
        }
        if !(self.sweep.ends[0] && self.sweep.ends[1]) {
            return S1Phase::Sweep;
        }
        self.conclude_without_peak(platform);
        self.halt();
        S1Phase::Conclude
    }

    /// Both ends were reached before a channel passed its peak.
    fn conclude_without_peak(&mut self, platform: &mut impl Platform) {
        let channel = self.sweep.channel();
        let peak = self.sweep.highest[channel];
        self.best.contrast = peak;
        self.sweep.near = self.frame.near;
        let margin = platform.setting(Setting::HoldMargin).wrapping_mul(10);
        if peak >= self.sweep.lowest[channel].wrapping_add(margin) {
            self.best.position = self.sweep.best;
            self.best.speed = self.sweep.best_class;
            return;
        }
        self.best.position = if self.frame.mode == 0 || self.frame.interval_tracking == 1 {
            LensPosition(self.limits.offset as i16)
        } else {
            platform.drive_target(self.frame.lens, self.frame.pinned_setting, self.limits.low, self.limits.high)
        };
        self.best.speed = 1;
    }

    /// 0xf280e8.
    fn begin_sweep(&mut self, platform: &mut impl Platform) {
        let combined = platform.setting(Setting::SecondChannel) != 1 || self.frame.mode != 0;
        let follows = if self.frame.mode == 1 {
            Channels::COMBINED
        } else if platform.setting(Setting::PeakBothChannels) == 1 {
            Channels::BOTH
        } else if combined {
            Channels::COMBINED
        } else {
            Channels::SECONDS
        };
        self.sweep = Sweep {
            ends: self.sweep.ends,
            combined,
            follows,
            rose: Channels::NONE,
            passed: Channels::NONE,
            highest: [0; 2],
            lowest: [u32::MAX; 2],
            series: Series { len: 0, ..self.sweep.series },
            best: LensPosition(0),
            best_class: 2,
            near: self.frame.near,
        };
        self.fit.forget();
    }

    /// The series, a new best with the samples around it, and each channel's extremes.
    fn record_sweep_sample(&mut self, platform: &mut impl Platform) {
        let values = self.values;
        self.sweep.series.push(values.seconds, values.combined, values.reference);
        let (contrast, position) = (values.contrast, self.positions[2]);
        if self.sweep.highest[self.sweep.channel()] < contrast {
            self.sweep.best = position;
            self.sweep.best_class = self.classes[2];
            self.fit.note_best(contrast, position);
        } else if self.fit.complete(platform, contrast, position) {
            self.sweep.best = self.fit.fit(platform);
        }
        self.fit.note_newest(contrast, position);
        let sweep = &mut self.sweep;
        if self.frame.mode == 0 {
            sweep.highest[0] = sweep.highest[0].max(values.seconds);
        }
        sweep.highest[1] = sweep.highest[1].max(values.combined);
        if self.frame.mode == 0 {
            sweep.lowest[0] = sweep.lowest[0].min(values.seconds);
        }
        sweep.lowest[1] = sweep.lowest[1].min(values.combined);
    }

    /// 0xf28430.
    fn test_rise(&mut self, platform: &mut impl Platform) {
        let percent = self.threshold(platform, &RISE);
        let values = self.values;
        let sweep = &mut self.sweep;
        for (channel, index, value) in [(Channels::SECONDS, 0, values.seconds), (Channels::COMBINED, 1, values.combined)] {
            if sweep.follows.has(channel)
                && !sweep.rose.has(channel)
                && platform.below_threshold(value, sweep.highest[index], sweep.lowest[index], percent)
            {
                sweep.rose.add(channel);
            }
        }
    }

    /// 0xf285b0.
    fn test_fall(&mut self, platform: &mut impl Platform) {
        let percent = self.threshold(platform, &FALL);
        let scan = self.record.scan;
        let sweep = &mut self.sweep;
        let len = usize::from(sweep.series.len);
        for (channel, series) in [(Channels::SECONDS, &sweep.series.seconds), (Channels::COMBINED, &sweep.series.combined)] {
            if sweep.follows.has(channel) && !sweep.passed.has(channel) && platform.passed_peak(&series[..len], percent, scan).0 {
                sweep.passed.add(channel);
            }
        }
        if sweep.passed == Channels::NONE
            && Series::newest(&sweep.series.combined, sweep.series.len) < platform.setting(Setting::FallLevel).wrapping_mul(10)
            && platform.fell_from_peak(&sweep.series.reference[..len])
        {
            sweep.passed.add(Channels::COMBINED);
        }
    }

    /// A reference above 9999 takes the low percentage.
    fn threshold(&self, platform: &mut impl Platform, thresholds: &Thresholds) -> u32 {
        let log = platform.contrast_log(self.values.combined);
        let high = platform.setting(thresholds.high);
        let low = platform.setting(thresholds.low);
        let from = platform.setting(thresholds.from).wrapping_mul(100);
        let to = platform.setting(thresholds.to).wrapping_mul(100);
        if Series::newest(&self.sweep.series.reference, self.sweep.series.len) > 9999 {
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

    /// Moved by more than `LevelJumpTens` since the sweep began.
    fn near_moved(&self, platform: &mut impl Platform) -> bool {
        let tens = platform.setting(Setting::LevelJumpTens).wrapping_mul(10) as i32;
        let (near, start) = (i32::from(self.frame.near), i32::from(self.sweep.near));
        near > start + tens || near + tens < start
    }
}
