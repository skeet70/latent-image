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
/// The correction window's margin beyond the adjustment level, in tenths (0xf6a3b8).
pub const WINDOW_MARGIN: Property = Property(0x649b);
/// Signed bytes in tenths: the correction window's near and far bounds below and above the
/// adjustment level (0xf6a3d0..0xf6a430).
pub const NEAR_BELOW: Property = Property(0x6484);
pub const FAR_BELOW: Property = Property(0x6485);
pub const NEAR_ABOVE: Property = Property(0x6486);
pub const FAR_ABOVE: Property = Property(0x6487);
/// The weights of curve points beyond the near bound and beyond the far bound, below and above
/// (0xf6a448..0xf6a470).
pub const NEAR_BELOW_WEIGHT: Property = Property(0x6492);
pub const FAR_BELOW_WEIGHT: Property = Property(0x6493);
pub const NEAR_ABOVE_WEIGHT: Property = Property(0x6494);
pub const FAR_ABOVE_WEIGHT: Property = Property(0x6495);
/// Bit 1: the correction is requested even when the focus result is reused (0xf5ffcc).
pub const CORRECTION_FLAGS: Property = Property(0x65a8);
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

/// Two bytes, signed: the lowest level an adjustment step takes (0xf65aa8).
pub const ADJUSTMENT_STEP_FLOOR: Property = Property(0x60ff);
/// Two bytes, signed: the level an adjustment step is raised toward (0xf65ab4).
pub const ADJUSTMENT_RAISE_TARGET: Property = Property(0x60f5);
/// The largest raise of an adjustment step, in tenths (0xf65ac0).
pub const ADJUSTMENT_RAISE_LIMIT: Property = Property(0x60f4);
/// Two bytes, signed, each: the lowest level of an adjustment step's four upper bands, highest
/// band first (0xf65ad4..0xf65af8).
pub const ADJUSTMENT_BANDS: [Property; 4] = [Property(0x60f7), Property(0x60f9), Property(0x60fb), Property(0x60fd)];
/// The sensitivity value of each band and of the levels below the last (0xf65b04..0xf65b34).
pub const ADJUSTMENT_SENSITIVITIES: [Property; 5] = [Property(0x6101), Property(0x6102), Property(0x6103), Property(0x6104), Property(0x6105)];

/// Five bytes of scene enable bits, scene flag `n` at bit `n % 8` of byte `n / 8` (0xf6f8a8..0xf6fa40).
pub const fn scene_mask(byte: u8) -> Property { Property(0x64a3 + byte as u32) }

/// The focus-result reuse tolerance in tens of level steps (0xf6b924, 0xf6b9a8).
pub const REUSE_TOLERANCE: Property = Property(0x65a7);

/// Two bytes, signed: the offset the first program step carries (0xf645ac).
pub const PROGRAM_OFFSET: Property = Property(0x6004);
/// Two bytes, signed: the highest level a program step takes (0xf645c4).
pub const PROGRAM_LEVEL_LIMIT: Property = Property(0x600a);
/// The program's start above its low end, in tenths (0xf645d0).
pub const START_MARGIN: Property = Property(0x6008);
/// The largest program step, in tenths (0xf645e4).
pub const STEP_LIMIT: Property = Property(0x6009);
/// The program's distance below its high end and the evaluation's upper tolerance, in tenths
/// (0xf6495c, 0xf6920c).
pub const UPPER_TOLERANCE: Property = Property(0x639d);
/// The evaluation's lower tolerance, in tenths (0xf691f4).
pub const LOWER_TOLERANCE: Property = Property(0x639e);
/// A pinned start's distance below the snapshot's offset, in tenths (0xf64a2c).
pub const PINNED_MARGIN: Property = Property(0x60f2);
/// Two bytes, signed: added to the high end of a selected measurement's program (0xf64864).
pub const SELECTED_HIGH_OFFSET: Property = Property(0x6c05);
/// The first step the evaluation spans; 255 chooses it from the focus curve (0xf6587c).
pub const FIRST_STEP: Property = Property(0x6395);
/// Two bytes: added to an extended step's value (0xf64518).
pub const EXTENDED_OFFSET: Property = Property(0x65fd);
/// Nonzero: the correction request carries the first subject's drive rate (0xf69058).
pub const CORRECTION_RATE: Property = Property(0x657d);
/// An area's limits in tenths: its distance from the metered level, and the signed limit of its
/// distance from its lens level (0xf69804, 0xf69818).
pub const METERED_DISTANCE_LIMIT: Property = Property(0x6391);
pub const LENS_DISTANCE_LIMIT: Property = Property(0x6390);

/// The rate setting the correction request carries (0xf5ecc0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateSetting {
    /// Selection style 4, 5 or 7.
    Styled,
    /// Selection kinds 11 to 13.
    Kinds,
    /// Selection kinds 11 to 13 with flag bit 1.
    FlaggedKinds,
    /// Auxiliary byte 2.
    AuxiliaryTwo,
    /// Any other nonzero auxiliary byte.
    Auxiliary,
    Default,
}

pub const fn rate_setting(setting: RateSetting) -> Property {
    match setting {
        RateSetting::Styled => Property(0x6512),
        RateSetting::Kinds => Property(0x6513),
        RateSetting::FlaggedKinds | RateSetting::Default => Property(0x6516),
        RateSetting::AuxiliaryTwo => Property(0x6511),
        RateSetting::Auxiliary => Property(0x6515),
    }
}
