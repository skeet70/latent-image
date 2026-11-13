use std::vec::Vec;

use super::gains::{GainClass, GainSources, Gains};
use super::ports::{Adjustments, ControlState, ControlTables, LoadTest, LoadTestSetting, Profiles, Reference, ReferenceIndex, Runtime};
use super::{AdjustmentKey, ProfileHandle, ProfileKind, Rectangle};

/// One profile with one reference record; the shutter-rate table is the exact power of two and
/// the decade table ten times its step.
pub struct FakeCapture {
    adjustments: Vec<(u32, u8)>,
    pub kind: Option<ProfileKind>,
    pub rate_threshold: u32,
    pub crop: Rectangle,
    pub reference: Reference,
    pub load_test: Option<LoadTestSetting>,
    pub configured_lines: u16,
    pub sample_period: (u16, u8),
    pub channel_divisors: [u16; 16],
    pub scale_keys: [AdjustmentKey; 2],
    pub ratio_refused: bool,
    pub class_gains: Vec<(GainClass, Gains)>,
    pub custom_gains: Gains,
    pub forced: GainClass,
    pub temperature_offset: u16,
    pub gain_calls: Vec<GainClass>,
}

impl FakeCapture {
    pub fn new(kind: ProfileKind, reference: Reference) -> Self {
        Self {
            adjustments: Vec::new(),
            kind: Some(kind),
            rate_threshold: 0,
            crop: Rectangle::default(),
            reference,
            load_test: None,
            configured_lines: 0,
            sample_period: (0, 0),
            channel_divisors: [0; 16],
            scale_keys: [AdjustmentKey(0x100), AdjustmentKey(0x200)],
            ratio_refused: false,
            class_gains: Vec::new(),
            custom_gains: [0; 3],
            forced: GainClass(0),
            temperature_offset: 0,
            gain_calls: Vec::new(),
        }
    }

    pub fn set_byte(&mut self, key: u32, value: u8) {
        self.adjustments.retain(|(at, _)| *at != key);
        self.adjustments.push((key, value));
    }

    pub fn set_halfword(&mut self, key: u32, value: u16) {
        let [low, high] = value.to_le_bytes();
        self.set_byte(key, low);
        self.set_byte(key + 1, high);
    }
}

impl Adjustments for FakeCapture {
    fn byte(&self, key: AdjustmentKey) -> u8 { self.adjustments.iter().find(|(at, _)| *at == key.0).map_or(0, |(_, value)| *value) }
}

impl Profiles for FakeCapture {
    fn kind(&self, _: ProfileHandle) -> Option<ProfileKind> { self.kind }

    fn rate_threshold(&self, _: ProfileHandle) -> u32 { self.rate_threshold }

    fn crop(&mut self, _: ProfileHandle, _: bool) -> Rectangle { self.crop }
}

impl ControlTables for FakeCapture {
    fn reference_index(&self, profile: ProfileHandle) -> ReferenceIndex { ReferenceIndex(profile.0) }

    fn reference(&self, _: ReferenceIndex) -> Reference { self.reference }

    fn shutter_rate(&self, step: u32) -> u32 { (1_000_000.0 * 2f64.powf((f64::from(step) - 100.0) / 100.0)).round() as u32 }

    fn decade(&self, step: u32) -> u32 { step * 10 }

    fn channel_divisor(&self, channel: u32) -> u16 { self.channel_divisors[channel as usize] }

    fn scale_keys(&self, _: u32, _: u32) -> [AdjustmentKey; 2] { self.scale_keys }
}

impl LoadTest for FakeCapture {
    fn load_test(&mut self) -> Option<LoadTestSetting> { self.load_test }
}

impl ControlState for FakeCapture {
    fn configured_lines(&mut self) -> u16 { self.configured_lines }

    fn sample_period(&mut self, _: ProfileHandle) -> (u16, u8) { self.sample_period }
}

impl Runtime for FakeCapture {
    fn log(&self, value: f32) -> f32 { value.ln() }

    fn reference_ratio(&mut self, value: u32) -> Option<u32> { (!self.ratio_refused).then_some(value) }
}

impl GainSources for FakeCapture {
    fn class_gains(&mut self, class: GainClass, _: i16) -> Gains {
        self.gain_calls.push(class);
        self.class_gains.iter().find(|(at, _)| *at == class).map_or([class.0 as u16; 3], |(_, gains)| *gains)
    }

    fn custom_gains(&mut self, _: u32, _: i16) -> Gains { self.custom_gains }

    fn forced_class(&mut self, _: u8) -> GainClass { self.forced }

    fn temperature_offset(&mut self) -> u16 { self.temperature_offset }
}
