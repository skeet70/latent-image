//! Configuration ids and the dump-frame trace of the library functions the shot's finish calls.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::ports::Property;

/// The target rate's factor in hundredths; the second replaces it for a record whose byte +0x41
/// is 1, the third for scene 28 or record byte +0x48 of 7 (0xf60f1c, 0xf61000, 0xf611a4).
pub const RATE_FACTOR: Property = Property(0x12d8);
pub const REPORTED_RATE_FACTOR: Property = Property(0x12da);
pub const SCENE_RATE_FACTOR: Property = Property(0x12d9);
/// 1 fixes the target's rate class; the second id names it, 0 above and 1 within (0xf60f74,
/// 0xf60f88).
pub const FIXED_TARGET: Property = Property(0x3e_0009);
pub const FIXED_TARGET_CLASS: Property = Property(0x94bc);
/// Low nibble 1 or 2 puts a record with an auxiliary selection byte below its band; 2 lifts a
/// class below the band into it (0xf611fc..0xf61244).
pub const TARGET_ADJUSTMENT: Property = Property(0x1389);

/// The resource value at or above which band `band + 1` applies, in hundreds (0xf61018..0xf610ec).
pub const fn band_threshold(band: usize) -> Property { Property(0x12c0 + band as u32) }

/// Two two-byte bounds of band `band`; the second is the band's upper bound (0xf610f0..0xf61100).
pub const fn band_bounds(band: usize) -> [Property; 2] { [Property(0x12c4 + 4 * band as u32), Property(0x12c6 + 4 * band as u32)] }

/// Word `word` of the target selection's trace in the client's frame (0xf60f48..0xf60f70,
/// 0xf61250..0xf6126c).
pub const fn target_trace(frame: SharedAddress, word: usize) -> SharedAddress { SharedAddress(frame.0.wrapping_add(0xa_5680 + 4 * word as u32)) }
