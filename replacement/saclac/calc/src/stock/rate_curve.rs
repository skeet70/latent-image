//! Configuration ids of the contrast-curve fits and the rate sample recorder.

use crate::exposure::ports::Property;

/// Nonzero fits five samples (0xf8ea58).
pub const FIVE_POINT_FIT: Property = Property(0x927a);
/// A sharpness at or above it takes the slope crossing in place of the parabola (0xf8dec8).
pub const PARABOLA_SHARPNESS: Property = Property(0x927b);
/// A sharpness at or above it fits three logarithms in place of four (0xf8cbe0).
pub const FIT_SHARPNESS: Property = Property(0x927c);
