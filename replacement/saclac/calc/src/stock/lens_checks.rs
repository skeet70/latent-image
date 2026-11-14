//! Configuration ids the library's lens checks read.

use crate::exposure::ports::Property;

/// 1 replaces a point's zone with the configured one (0xf4e638).
pub const ZONE_OVERRIDE: Property = Property(0x003e_0009);
/// The configured zone (0xf4e64c).
pub const OVERRIDDEN_ZONE: Property = Property(0x94bc);
