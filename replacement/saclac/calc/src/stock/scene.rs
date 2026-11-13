//! Configuration ids of the settings the shot and the exposure engine choose by scene kind.

use crate::exposure::ports::Property;

/// The nibble pair of an armed record of scene 0 or 48 (0xf6ef74).
pub const ARMED_NIBBLES: Property = Property(0x4a68);
/// The nibble pair of selector 4 (0xf6ef80).
pub const SELECTOR_NIBBLES: Property = Property(0x4a69);
/// The nibble pair of a record whose selection flags carry bit 1 (0xf6ef94).
pub const FLAGGED_NIBBLES: Property = Property(0x4a63);

/// The nibble pair a record's override selects (0xf6eca4).
pub const fn override_nibbles(choice: u8) -> Property {
    Property(match choice {
        1 => 0x4a6a,
        2 => 0x4a6b,
        3 => 0x4a70,
        4 => 0x4a6c,
        5 => 0x4a6d,
        6 => 0x4a6e,
        7..=12 => 0x4a6f,
        13 => 0x1_787c,
        14 => 0x4a71,
        _ => 0x4a4c,
    })
}

/// The nibble pair of a scene. Scenes 0, 43..=48 and 54..=57 choose by the record's policy while
/// the measured level is 0, and take a fixed pair once it is not (0xf6ed30..0xf6ef58).
pub const fn scene_nibbles(scene: u32, measured: bool, policy: u8) -> Property {
    Property(match scene {
        5..=7 | 34 | 37 | 40 => 0x4a51,
        10 => 0x4a52,
        0 | 43..=48 | 54..=57 if !measured => match policy {
            0 => 0x4a4e,
            1 => 0x4a4d,
            3 => 0x4a4b,
            4 => 0x4a4a,
            _ => 0x4a4c,
        },
        47 | 56 => 0x4a50,
        0 | 43..=46 | 48 | 54 | 55 | 57 => 0x4a4f,
        1 | 2 | 4 | 8 | 9 | 14 | 15 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42 => 0x4a53,
        3 => 0x4a54,
        11 => 0x4a56,
        12 | 13 => 0x4a55,
        16 | 33 => 0x4a57,
        17 => 0x4a58,
        18 => 0x4a5c,
        21 => 0x4a5b,
        24 => 0x4a59,
        25 => 0x4a5a,
        28 => 0x4a5d,
        29 => 0x4a61,
        30 => 0x4a5f,
        31 => 0x4a5e,
        32 => 0x4a60,
        49 => 0x4a63,
        50 => 0x4a62,
        51 => 0x4a64,
        52 => 0x4a65,
        53 => 0x4a66,
        _ => 0x4a4f,
    })
}

/// One bit of the scene adjustment enable bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnableBit {
    pub setting: Property,
    pub mask: u8,
}

const fn enable_bit(setting: u32, bit: u8) -> EnableBit { EnableBit { setting: Property(setting), mask: 1 << bit } }

/// The enable bit of a record whose selection flags carry bit 1 (0xf6eff8).
pub const FLAGGED_ENABLE: EnableBit = enable_bit(0x6595, 5);

/// How a scene's adjustment is enabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneEnable {
    Always,
    Never,
    Bit(EnableBit),
    /// Scenes 44 and 57 choose a bit by the record's selector.
    BySelector,
}

/// 0xf6f014's table.
pub const fn scene_enable(scene: u32) -> SceneEnable {
    match scene {
        1..=4 | 8 | 9 | 14 | 15 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42 => SceneEnable::Bit(enable_bit(0x6594, 0)),
        5..=7 | 34 | 37 | 40 => SceneEnable::Bit(enable_bit(0x6594, 1)),
        10 => SceneEnable::Bit(enable_bit(0x6594, 2)),
        11 | 12 => SceneEnable::Bit(enable_bit(0x6594, 3)),
        24 => SceneEnable::Bit(enable_bit(0x6594, 4)),
        25 => SceneEnable::Bit(enable_bit(0x6594, 5)),
        21 => SceneEnable::Bit(enable_bit(0x6594, 6)),
        18 => SceneEnable::Bit(enable_bit(0x6594, 7)),
        28 => SceneEnable::Bit(enable_bit(0x6595, 0)),
        31 => SceneEnable::Bit(enable_bit(0x6595, 1)),
        30 => SceneEnable::Bit(enable_bit(0x6595, 2)),
        29 => SceneEnable::Bit(enable_bit(0x6595, 3)),
        32 => SceneEnable::Bit(enable_bit(0x6595, 4)),
        49 => SceneEnable::Bit(enable_bit(0x6595, 5)),
        50 => SceneEnable::Bit(enable_bit(0x6595, 6)),
        51 => SceneEnable::Bit(enable_bit(0x6595, 7)),
        52 => SceneEnable::Bit(enable_bit(0x6596, 0)),
        13 | 45..=47 | 54..=56 => SceneEnable::Never,
        44 | 57 => SceneEnable::BySelector,
        _ => SceneEnable::Always,
    }
}

/// The enable bit of scenes 44 and 57 for a selector (0xf6f118).
pub const fn selector_enable(selector: u8) -> Option<EnableBit> {
    match selector {
        0 => Some(enable_bit(0x65a3, 4)),
        2 => Some(enable_bit(0x65a3, 1)),
        3 => Some(enable_bit(0x65a3, 2)),
        _ => None,
    }
}

/// The configuration table calc reads for a scene (0xf6fa84).
pub const fn curve_table(scene: u32) -> Property {
    Property(match scene {
        1..=4 | 8 | 9 | 14 | 15 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42 => 0x64d4,
        5..=7 | 24 | 25 | 34 | 37 | 40 => 0x64e0,
        10 => 0x64c8,
        11..=13 => 0x64ec,
        16 | 33 => 0x64f8,
        17 => 0x6504,
        _ => 0x64bc,
    })
}

pub const DEFAULT_CURVE_TABLE: Property = Property(0x64bc);

/// Where a scene's range is read: two one-byte values in tenths and a two-byte signed offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeSettings {
    pub above: Property,
    pub below: Property,
    pub offset: Property,
}

const fn range(above: u32, below: u32, offset: u32) -> RangeSettings {
    RangeSettings { above: Property(above), below: Property(below), offset: Property(offset) }
}

/// The range of a record whose selection flags carry bit 1, the same as scene 49's (0xf6f2e8).
pub const FLAGGED_RANGE: RangeSettings = range(0x60c2, 0x608b, 0x6018);
/// The range of an armed record (0xf6f388).
pub const ARMED_RANGE: RangeSettings = range(0x60bc, 0x6085, 0x600c);

/// The range of a record selected for measurement, by its measurement class (0xf6f32c..0xf6f37c).
pub const fn selected_range(class: u32) -> RangeSettings {
    match class {
        6 | 7 => range(0x60f0, 0x60b9, 0x6080),
        8..=10 => range(0x60f1, 0x60ba, 0x6082),
        _ => range(0x60ef, 0x60b8, 0x607e),
    }
}

const fn per_policy(above: u32, below: u32, offset: u32, policy: u32) -> RangeSettings { range(above + policy, below + policy, offset + 2 * policy) }

/// The range of a scene. Scenes 0 and 48 always read the entry of the record's policy; scenes 43..=47
/// and 54..=57 and the scenes no group names do when the level was not measured (0xf6f3a0's table).
pub const fn scene_range(scene: u32, unmeasured: bool, policy: u32, variant: u32) -> RangeSettings {
    match scene {
        0 | 48 => per_policy(0x60bd, 0x6086, 0x600e, policy),
        1..=4 | 8 | 9 | 14 | 15 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42 => range(0x60c4, 0x608d, 0x601c),
        5..=7 | 34 | 37 | 40 => range(0x60c5, 0x608e, 0x601e),
        10 => range(0x60c6, 0x608f, 0x6020),
        11 => range(0x60c7, 0x6090, 0x6022),
        12 | 13 => range(0x60c8, 0x6091, 0x6024),
        16 | 33 => range(0x60ca, 0x6093, 0x6028),
        17 => range(0x60c9, 0x6092, 0x6026),
        18 => range(0x60ce, 0x6097, 0x6030),
        21 => range(0x60cd, 0x6096, 0x602e),
        24 => range(0x60cb, 0x6094, 0x602a),
        25 => range(0x60cc, 0x6095, 0x602c),
        28 => range(0x60cf, 0x6098, 0x6032),
        29 => range(0x60d2, 0x609b, 0x6038),
        30 => range(0x60d1, 0x609a, 0x6036),
        31 => range(0x60d0, 0x6099, 0x6034),
        32 => range(0x60d3, 0x609c, 0x603a),
        45 | 55 if unmeasured => per_policy(0x60e4, 0x60ad, 0x6068, policy),
        45 | 55 => range(0x60e3, 0x60ac, 0x6066),
        46 | 54 if unmeasured => per_policy(0x60de, 0x60a7, if variant == 1 { 0x605c } else { 0x6050 }, policy),
        46 | 54 => range(0x60dd, 0x60a6, if variant == 1 { 0x605a } else { 0x604e }),
        47 | 56 if unmeasured => per_policy(0x60ea, 0x60b3, 0x6074, policy),
        47 | 56 => range(0x60e9, 0x60b2, 0x6072),
        49 => FLAGGED_RANGE,
        50 => range(0x60c3, 0x608c, 0x601a),
        51 => range(0x60d4, 0x609d, 0x603c),
        52 => range(0x60d5, 0x609e, 0x603e),
        53 => range(0x60d6, 0x609f, 0x6040),
        _ if unmeasured => per_policy(0x60d8, 0x60a1, 0x6044, policy),
        _ => range(0x60d7, 0x60a0, 0x6042),
    }
}
