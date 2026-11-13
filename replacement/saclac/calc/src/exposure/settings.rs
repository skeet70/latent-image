//! Composite reads of the camera configuration values the engine uses.

use crate::exposure::ports::{Properties, Property};
use crate::stock::property;

/// A one-byte value read as a signed level (0x69fffc).
pub fn level(properties: &mut impl Properties, id: Property) -> i16 { properties.property(id) as i16 }

/// A one-byte value in tenths: the value times ten.
pub fn tenths(properties: &mut impl Properties, id: Property) -> u16 { (properties.property(id) as u16).wrapping_mul(10) }

/// A two-byte value, low byte first (0x69fec8).
pub fn word(properties: &mut impl Properties, id: Property) -> u16 {
    let low = properties.property(id);
    let high = properties.property(id.high());
    (low | high << 8) as u16
}

/// A two-byte signed value, low byte first (0x69ff7c).
pub fn signed_word(properties: &mut impl Properties, id: Property) -> i16 { word(properties, id) as i16 }

/// Step `step` of the calibrated level table (0x772f28): 0 for step 0, step 3's value past step 10.
pub fn level_step(properties: &mut impl Properties, step: u8) -> i16 {
    match step {
        0 => 0,
        1..=10 => signed_word(properties, property::level_step(step)),
        _ => signed_word(properties, property::level_step(3)),
    }
}
