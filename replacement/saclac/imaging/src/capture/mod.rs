#![forbid(unsafe_code)]

//! saclac's capture-pipeline layer (L10): the readout timing of capture profiles and the gains a
//! capture request selects.

pub mod control;
pub mod gains;
pub mod ports;

/// A capture profile: the index the profile table, the readout pairs and the control reference
/// table share; X-M1 1.50 declares profiles 0..=35.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileHandle(pub u32);

/// A profile's readout kind, as the control layer treats it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileKind {
    /// No frame period.
    Untimed,
    /// The frame period is the reference table's line count.
    Tabled,
    /// The frame period is the one the control state holds, unless a load test overrides it.
    Configured,
    /// As `Configured`, but without an exposure interval.
    ConfiguredAlternate,
    /// The line count stretched to the profile's frame-rate threshold.
    RateLimited,
    /// The line count, or under a load test the stretched one; its exposure interval follows the
    /// shutter.
    Exposed,
}

/// A byte address in the adjustment area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjustmentKey(pub u32);

impl AdjustmentKey {
    const fn next(self) -> Self { Self(self.0 + 1) }
}

/// A shutter speed in hundredths of a stop, zero at one second, rising as the shutter shortens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShutterLevel(pub i32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rectangle {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

/// The profile's kind has no value for the operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unsupported;

/// `value / 10`, rounded half up on the tenth.
const fn round_tenth(value: u32) -> u32 { value / 10 + (value % 10 > 4) as u32 }

const fn round_tenth_wide(value: u64) -> u64 { value / 10 + (value % 10 > 4) as u64 }

#[cfg(test)]
mod fake;
#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod gains_tests;
