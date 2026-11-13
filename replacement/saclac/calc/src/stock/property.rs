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

/// Lens resources at or beyond this count narrow area kind 2 to kind 1 (0xfa76d4, 0xfa7a00).
pub const AREA_RESOURCES: Property = Property(0x3e1214);

/// The table an area's output offset is read from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OffsetTable { Standard, First, Second }

/// Two bytes per lens resource (0xfa7870, 0xfa78ac, 0xfa78d4).
pub const fn area_offset(table: OffsetTable, resource: u32) -> Property {
    let base = match table {
        OffsetTable::Standard => 0x3e07f7,
        OffsetTable::First => 0x3e08c0,
        OffsetTable::Second => 0x3e0989,
    };
    Property(base + 2 * resource)
}

/// Five bytes of scene enable bits, scene flag `n` at bit `n % 8` of byte `n / 8` (0xf6f8a8..0xf6fa40).
pub const fn scene_mask(byte: u8) -> Property { Property(0x64a3 + byte as u32) }

/// The focus-result reuse tolerance in tens of level steps (0xf6b924, 0xf6b9a8).
pub const REUSE_TOLERANCE: Property = Property(0x65a7);
