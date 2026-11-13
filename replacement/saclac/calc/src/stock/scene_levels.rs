//! Configuration ids the library's scene levels read.

use crate::exposure::ports::Property;

/// The response limits: two-byte signed values, seven level bands of four columns
/// (0xf5f3f4..0xf5f544).
pub const fn response_limit(class_three: bool, band: u8, column: u8) -> Property {
    let table = if class_three { 0x6353 } else { 0x631b };
    Property(table + 8 * band as u32 + 2 * column as u32)
}

/// The configured rate, low byte first (0xf5ebdc..0xf5ebf8).
pub const CONFIGURED_RATE: [Property; 3] = [Property(0x639a), Property(0x639b), Property(0x639c)];

/// The scenes whose levels 0xf5f368 measures against their placement.
pub const PLACED_SCENES: [u32; 2] = [47, 56];
