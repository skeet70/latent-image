//! Configuration ids the exposure engine reads through 0x69f8fc.

use crate::exposure::ports::Property;

/// Bit 0 selects the configured measurement level (0xf624f8, 0xf62544).
pub const ADJUSTMENT_FLAGS: Property = Property(0xa8);
/// Two bytes: the target level outside the adjustment path (0xf624e0).
pub const TARGET_LEVEL: Property = Property(0x6002);

/// Steps 1..=10 of the calibrated level table, two bytes each (0x772f68..0x772fd4).
pub const fn level_step(step: u8) -> Property { Property(0x1900 + 2 * step as u32) }

/// Lower and upper bounds of the adjustment tables, in tenths (0xf6aca0, 0xf6acc0).
pub const TABLE_FLOOR: Property = Property(0x6499);
pub const TABLE_CEILING: Property = Property(0x649a);
/// Copied into the dump frame (0xf6ad00, 0xf6ad18).
pub const DUMP_FIRST: Property = Property(0x4a07);
pub const DUMP_SECOND: Property = Property(0x4a08);
/// Two bytes: the weight sum at which the adjustment path is not taken (0xf6afd8).
pub const WEIGHT_LIMIT: Property = Property(0x6490);
/// Two bytes each: weight thresholds that set the adjustment state (0xf6b214, 0xf6b234).
pub const SECOND_WEIGHT_LIMIT: Property = Property(0x648e);
pub const THIRD_WEIGHT_LIMIT: Property = Property(0x648a);
/// The adjustment window's upper and lower bounds in tenths (0xf6b0a0..0xf6b17c): the record's
/// measurement with record kind 6 or 7, the record's measurement otherwise, record word 0x20d4
/// equal to 1, and the rest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowSource {
    RecordExtended,
    Record,
    Alternate,
    Default,
}

pub const fn window(source: WindowSource) -> (Property, Property) {
    match source {
        WindowSource::RecordExtended => (Property(0x64b7), Property(0x64af)),
        WindowSource::Record => (Property(0x64b6), Property(0x64ae)),
        WindowSource::Alternate => (Property(0x64b9), Property(0x64b1)),
        WindowSource::Default => (Property(0x64b8), Property(0x64b0)),
    }
}
