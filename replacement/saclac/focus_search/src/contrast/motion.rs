#![forbid(unsafe_code)]

//! Camera motion from the lens's angular-rate samples (0xf3fa34 and its ring 0xf3f938,
//! 0xf3f968, 0xf3f998).

use super::settings::Setting;
use super::{Context, Platform};

/// An angular-rate sample with both axes at rest.
const AT_REST: u32 = 0x8000_8000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Motion {
    /// Newest first: two axes in each word.
    pub samples: [u32; 256],
    /// The motion level, which decays by the sample interval.
    pub level: u16,
    /// The sample interval in milliseconds.
    pub interval: u16,
    /// The summed movement of the high and the low axis.
    pub movement: [u32; 2],
}

impl Motion {
    pub(crate) const fn new() -> Self { Self { samples: [0; 256], level: 0, interval: 0, movement: [0; 2] } }

    /// 0xf3f938.
    pub(crate) fn forget(&mut self) { self.samples = [AT_REST; 256]; }

    /// 0xf3f968.
    fn push(&mut self, sample: u32) {
        self.samples.copy_within(..255, 1);
        self.samples[0] = sample;
    }

    /// 0xf3f998: the larger axis movement over the configured number of samples.
    fn movement(&mut self, samples: u32) -> u32 {
        let pairs = if samples == 1 { 0 } else { usize::from((samples as u8).wrapping_sub(2)) + 1 };
        let (high, low) = self.samples.windows(2).take(pairs).fold((0u32, 0u32), |(high, low), pair| {
            let axis = |word: u32, shift: u32| ((word >> shift) & 0xffff) as i32;
            (
                high.wrapping_add((axis(pair[0], 16) - axis(pair[1], 16)).unsigned_abs()),
                low.wrapping_add((axis(pair[0], 0) - axis(pair[1], 0)).unsigned_abs()),
            )
        });
        self.movement = [high, low];
        high.max(low)
    }
}

impl Context {
    /// 0xf3fa34: sample the angular rate and find whether the camera moves too much to
    /// search.
    pub(crate) fn camera_moving(&mut self, platform: &mut impl Platform) -> bool {
        if platform.setting(Setting::MotionCheck) == 0 {
            return false;
        }
        if self.frame.selected == 1 && platform.setting(Setting::SelectedMotionCheck) == 0 {
            return false;
        }
        let class = platform.record_class();
        let period = platform.profile_period(class);
        let scale = platform.setting(Setting::MotionScale);
        let floor = platform.setting(Setting::MotionFloor).wrapping_mul(10) as u16;
        let ceiling = platform.setting(Setting::MotionCeiling);
        let sample = match platform.angular_rate() {
            0 => AT_REST,
            sample => sample,
        };
        platform.request_angular_rate();
        self.motion.push(sample);
        let samples = platform.setting(Setting::MotionSamples);
        let movement = self.motion.movement(samples);
        let rate = divide(u64::from(movement) * 100_000, u64::from(period)) as u32;
        let motion = &mut self.motion;
        motion.level = motion.level.saturating_sub(motion.interval);
        if rate >= u32::from(floor) {
            let ceiling = ceiling.wrapping_mul(10) as u16;
            let mut raised = scale.wrapping_mul(10) as u16;
            if rate < u32::from(ceiling) {
                raised = ((rate - u32::from(floor)).wrapping_mul(u32::from(raised)) / u32::from(ceiling - floor)) as u16;
            }
            if motion.level < raised {
                motion.level = raised;
            }
        }
        motion.interval = (period / 1000) as u16;
        platform.setting(Setting::MotionHolds) != 0 && motion.level >= motion.interval
    }
}

/// The runtime's unsigned 64-bit division (0x89bed0): a zero divisor gives all ones, or zero
/// for a zero dividend.
fn divide(dividend: u64, divisor: u64) -> u64 {
    match (dividend, divisor) {
        (0, 0) => 0,
        (_, 0) => u64::MAX,
        _ => dividend / divisor,
    }
}
