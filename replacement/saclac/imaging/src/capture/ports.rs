#![forbid(unsafe_code)]

//! What the capture-pipeline layer reads outside itself. Each trait is bound at saclac's
//! composition root to the layer or stock service that owns it.

use super::statistics::ChannelPixels;
use super::{AdjustmentKey, ProfileHandle, ProfileKind, Rectangle};

/// The adjustment area, a byte at a time (0x69f8fc).
pub trait Adjustments {
    fn byte(&self, key: AdjustmentKey) -> u8;

    fn signed_byte(&self, key: AdjustmentKey) -> i8 { self.byte(key) as i8 }

    /// Low byte at `key`.
    fn halfword(&self, key: AdjustmentKey) -> u16 { u16::from_le_bytes([self.byte(key), self.byte(key.next())]) }
}

/// The profile table and the geometry layer's view of a profile.
pub trait Profiles {
    /// `None` for a kind byte the table does not declare.
    fn kind(&self, profile: ProfileHandle) -> Option<ProfileKind>;
    /// The profile's frame-rate threshold, in milliseconds (0xef5360).
    fn rate_threshold(&self, profile: ProfileHandle) -> u32;
    /// The profile's crop rectangle; `preserve_axes` false halves the untimed kind's vertical
    /// axis (0xef5470).
    fn crop(&mut self, profile: ProfileHandle, preserve_axes: bool) -> Rectangle;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceIndex(pub u32);

/// A readout reference record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Reference {
    pub lines: u32,
    /// Pixel-clock cycles per line.
    pub line_length: u32,
    /// Frame period in microseconds.
    pub frame_period: u32,
    pub left_reference: u16,
    /// The crop rectangle's left edge for the exposed kind.
    pub left: u16,
}

/// The control layer's tables in image 5.
pub trait ControlTables {
    fn reference_index(&self, profile: ProfileHandle) -> ReferenceIndex;
    fn reference(&self, index: ReferenceIndex) -> Reference;
    /// One second's rate times `2^((step - 100) / 100)`, `step` in 1..=199.
    fn shutter_rate(&self, step: u32) -> u32;
    /// The decade table, `step` in 1..=19999.
    fn decade(&self, step: u32) -> u32;
    /// The divisor of an interval channel; `channel` is below 16.
    fn channel_divisor(&self, channel: u32) -> u16;
    /// The two adjustment halfwords whose ratio is a group entry's reference scale.
    fn scale_keys(&self, group: u32, entry: u32) -> [AdjustmentKey; 2];
}

/// The load-test setting stock's test interface reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadTestSetting(pub u32);

pub trait LoadTest {
    /// `None` while no load test runs (0x742054, 0x742064).
    fn load_test(&mut self) -> Option<LoadTestSetting>;
}

/// The control subsystem's state, which it writes.
pub trait ControlState {
    /// The frame line count it configures for the configured kinds.
    fn configured_lines(&mut self) -> u16;
    /// The profile's sample period and its flag byte (0x1006360).
    fn sample_period(&mut self, profile: ProfileHandle) -> (u16, u8);
}

/// Stock's numeric runtime.
pub trait Runtime {
    /// The runtime's logarithm (0x894778); every use here is a ratio of two, so its base cancels.
    fn log(&self, value: f32) -> f32;
    /// 0x10034b4's quotient for `value`, `None` where it refuses.
    fn reference_ratio(&mut self, value: u32) -> Option<u32>;
}

/// The profile layer's pixel geometry.
pub trait PixelGeometry {
    /// The scale divisors every class shares (0xef5768).
    fn shared_divisors(&mut self) -> [u32; 2];
    /// The profile's scale divisors (0xef5b5c).
    fn scale_divisors(&mut self, profile: ProfileHandle) -> [u32; 2];
    /// 0xef56cc.
    fn channel_pixels(&mut self, pixels: u32) -> ChannelPixels;
    /// 0xef5310.
    fn aspect(&mut self, profile: ProfileHandle) -> i32;
}
