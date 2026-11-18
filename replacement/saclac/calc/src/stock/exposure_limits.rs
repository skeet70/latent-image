//! Configuration ids of the exposure limits (id5 0xf6fb74..0xf72ea4), by the record's scene kind.
//! `flagged` is selection flags bit 1; `accepting` is 0xf704c4's answer: the alternate settings of
//! scenes 0, 43, 44, 48 and 57 while not flagged.

use crate::exposure::ports::Property;

/// The first byte of each parameter set; a set is thirteen bytes (0xf73278).
pub const PARAMETER_SETS: [u32; 10] = [0x6125, 0x6132, 0x613f, 0x614c, 0x6159, 0x6166, 0x6173, 0x6180, 0x618d, 0x619a];

/// A value of a parameter set (0xf71fa8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterField {
    /// One byte in tenths: the threshold level above the threshold step.
    Above,
    /// One byte in tenths: the threshold level at or below the threshold step.
    AtOrBelow,
    /// Two bytes in tenths: the lens limit of the threshold step.
    ThresholdLimit,
    /// One byte in tenths.
    Upper,
    /// One byte in tenths: less 100 per cell column, it bounds the upper level of a nonzero column.
    Ceiling,
    /// Signed byte in tenths.
    TargetCeiling,
    /// Signed byte in tenths.
    TargetFloor,
}

/// Where a parameter set keeps a value; the second bank's first four values are six bytes on.
pub const fn parameter(set: u32, second_bank: bool, field: ParameterField) -> Property {
    let bank = if second_bank { 6 } else { 0 };
    Property(match field {
        ParameterField::Above => set + bank,
        ParameterField::AtOrBelow => set + bank + 1,
        ParameterField::ThresholdLimit => set + bank + 2,
        ParameterField::Upper => set + bank + 4,
        ParameterField::Ceiling => set + 5,
        ParameterField::TargetCeiling => set + 11,
        ParameterField::TargetFloor => set + 12,
    })
}

/// One byte: the parameter set of an unthresholded scene with its alternate settings (0xf71520).
pub const ACCEPTING_SET: Property = Property(0x611f);
/// One byte: the parameter set of a thresholded scene at or below `SET_THRESHOLD` (0xf71520).
pub const LOW_SET: Property = Property(0x65ad);
/// Two bytes, signed: the threshold level above which a scene keeps its own parameter set.
pub const SET_THRESHOLD: Property = Property(0x65ae);

/// Signed byte in tenths: the offset subtracted from a lens channel's level to give the target
/// (0xf720e4).
pub const TARGET_OFFSET: Property = Property(0x61e1);
/// Signed byte in tenths: the further offset a tracking target takes (0xf720b4).
pub const TRACKING_OFFSET: Property = Property(0x61e0);
/// Signed byte in tenths: the cap of a class-5 measurement away from its first profile
/// (0xf72420).
pub const CLASS_FIVE_CAP: Property = Property(0x6597);

/// One byte in tenths: the cap of a profile's level (0xf723b0).
pub const fn profile_cap(profile: u32) -> Property { Property(0x61c0u32.wrapping_add(profile)) }

const fn plain(scene: u32) -> bool { matches!(scene, 1..=4 | 8 | 9 | 14 | 15 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42) }

const fn secondary(scene: u32) -> bool { matches!(scene, 5..=7 | 34 | 37 | 40) }

/// The parameter set selector of a scene (0xf71520).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetSelector {
    /// One byte naming the set.
    Setting(Property),
    /// Chosen by the scene's threshold, with this byte when the threshold does not decide.
    Thresholded { setting: Property, wide: bool },
}

pub const fn set_selector(scene: u32, variant: bool, flagged: bool) -> SetSelector {
    if flagged {
        return SetSelector::Setting(Property(0x611a));
    }
    SetSelector::Setting(Property(match scene {
        scene if plain(scene) => 0x610a,
        scene if secondary(scene) => 0x610b,
        10 => 0x610c,
        11 => 0x610e,
        12 | 13 => 0x610d,
        16 | 33 => 0x610f,
        17 => 0x6111,
        18 => 0x6114,
        21 => 0x6113,
        24 => 0x6110,
        25 => 0x6112,
        28 => 0x6115,
        29 => 0x6118,
        30 => 0x6117,
        31 => 0x6116,
        32 => 0x6119,
        43 | 44 | 57 => return SetSelector::Thresholded { setting: Property(0x6109), wide: true },
        45 | 55 => 0x6122,
        46 | 54 => {
            if variant {
                0x6121
            } else {
                0x6120
            }
        }
        47 | 56 => 0x6123,
        49 => 0x611a,
        50 => 0x611b,
        51 => 0x611c,
        52 => 0x611d,
        53 => 0x611e,
        _ => return SetSelector::Thresholded { setting: Property(0x6108), wide: false },
    }))
}

/// Two bytes, unsigned, in tenths: the limit of the near lens step; `None` reads no limit
/// (0xf70d54).
pub const fn near_limit(scene: u32, accepting: bool, auxiliary: bool, flagged: bool) -> Option<Property> {
    if flagged {
        return Some(Property(0x623d));
    }
    Some(Property(match scene {
        0 if auxiliary => 0x623b,
        0 if accepting => 0x6247,
        0 => 0x6217,
        48 => 0x623b,
        scene if plain(scene) => 0x621b,
        scene if secondary(scene) => 0x621d,
        10 => 0x621f,
        11 => 0x6223,
        12 | 13 => 0x6221,
        16 | 33 => 0x6225,
        17 => 0x6229,
        18 => 0x622f,
        21 => 0x622d,
        24 => 0x6227,
        25 => 0x622b,
        28 => 0x6231,
        29 => 0x6237,
        30 => 0x6235,
        31 => 0x6233,
        32 => 0x6239,
        43 | 44 | 57 if accepting => 0x6247,
        43 | 44 | 57 => 0x6219,
        45 | 55 => 0x6249,
        49 => 0x623d,
        50 => 0x623f,
        51 => 0x6241,
        52 => 0x6243,
        53 => 0x6245,
        _ => return None,
    }))
}

/// Two bytes, unsigned, in tenths: the limit of the far lens step; `None` reads no limit
/// (0xf70fac).
pub const fn far_limit(scene: u32, accepting: bool, auxiliary: bool, flagged: bool) -> Option<Property> {
    if flagged {
        return Some(Property(0x6208));
    }
    Some(Property(match scene {
        0 if auxiliary => 0x6206,
        0 if accepting => 0x6212,
        0 => 0x61e2,
        48 => 0x6206,
        scene if plain(scene) => 0x61e6,
        scene if secondary(scene) => 0x61e8,
        10 => 0x61ea,
        11 => 0x61ee,
        12 | 13 => 0x61ec,
        16 | 33 => 0x61f0,
        17 => 0x61f4,
        18 => 0x61fa,
        21 => 0x61f8,
        24 => 0x61f2,
        25 => 0x61f6,
        28 => 0x61fc,
        29 => 0x6202,
        30 => 0x6200,
        31 => 0x61fe,
        32 => 0x6204,
        43 | 44 | 57 if accepting => 0x6212,
        43 | 44 | 57 => 0x61e4,
        45 | 55 => 0x6214,
        49 => 0x6208,
        50 => 0x620a,
        51 => 0x620c,
        52 => 0x620e,
        53 => 0x6210,
        _ => return None,
    }))
}

/// Signed byte in tenths: the lower level of a scene without a parameter bank (0xf71204).
pub const fn lower_level(scene: u32, accepting: bool, variant: bool, flagged: bool) -> Property {
    if flagged {
        return Property(0x61ae);
    }
    Property(match scene {
        11 => 0x61ab,
        12 | 13 => 0x61aa,
        17 => 0x61ad,
        24 => 0x61ac,
        43 | 44 | 57 => 0x61a9,
        45 | 55 => 0x61b7,
        46 if variant => 0x61b6,
        46 => 0x61b5,
        47 | 56 => 0x61b8,
        49 => 0x61ae,
        50 => 0x61af,
        51 => 0x61b0,
        52 => 0x61b1,
        53 => 0x61b2,
        54 => 0x61b3,
        _ if accepting => 0x61b4,
        _ => 0x61a8,
    })
}

/// Signed byte in tenths: the lower level of a scene whose parameter bank carries bit 2
/// (0xf6fb74).
pub const fn bank_lower_level(scene: u32, variant: bool) -> Property {
    Property(match scene {
        11 => 0x61bc,
        12 | 13 => 0x61bb,
        24 => 0x61ac,
        43 | 44 | 57 => 0x61ba,
        45 | 55 => 0x61b7,
        46 | 54 if variant => 0x61be,
        46 | 54 => 0x61bd,
        47 | 56 => 0x61b8,
        _ => 0x61b9,
    })
}

/// A pair of two-byte signed limits: the first serves stages 3 to 5, the second the rest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitPair {
    pub first: u32,
    pub second: u32,
}

const fn pair(first: u32, second: u32) -> LimitPair { LimitPair { first, second } }

impl LimitPair {
    /// Two bytes, signed: the limit at `index` of the chosen half, indexed limits two bytes apart.
    pub const fn setting(self, first: bool, index: u32) -> Property { Property((if first { self.first } else { self.second }) + 2 * index) }
}

const fn by_policy(policy: u32, pairs: [LimitPair; 5]) -> LimitPair {
    match policy {
        0 => pairs[0],
        1 => pairs[1],
        3 => pairs[3],
        4 => pairs[4],
        _ => pairs[2],
    }
}

const fn choice_pair(choice: u32, pairs: [LimitPair; 9], other: LimitPair) -> LimitPair {
    match choice {
        1 => pairs[0],
        2 => pairs[1],
        3 => pairs[2],
        4 => pairs[3],
        5 => pairs[4],
        6 => pairs[5],
        7..=12 => pairs[6],
        13 => pairs[7],
        14 => pairs[8],
        _ => other,
    }
}

/// What chooses a mode limit beside the scene.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitKey {
    pub scene: u32,
    pub policy: u32,
    /// Record byte 0x3e; 0 leaves the choice to the scene.
    pub choice: u32,
    pub armed: bool,
}

const fn armed_scene(key: &LimitKey) -> bool { key.armed && matches!(key.scene, 0 | 48) }

/// The mode limit of a scene (0xf6fcb0).
pub const fn mode_limit(key: &LimitKey, flagged: bool) -> LimitPair {
    let chosen = if key.choice != 0 {
        choice_pair(
            key.choice,
            [
                pair(0x6a56, 0x6906),
                pair(0x6a58, 0x6908),
                pair(0x6a62, 0x6912),
                pair(0x6a5a, 0x690a),
                pair(0x6a5c, 0x690c),
                pair(0x6a5e, 0x690e),
                pair(0x6a60, 0x6910),
                pair(0x1_789a, 0x1_7890),
                pair(0x6a64, 0x6914),
            ],
            pair(0x6a1a, 0x68ca),
        )
    } else {
        match key.scene {
            scene if plain(scene) => pair(0x6a2a, 0x68da),
            scene if secondary(scene) => pair(0x6a2c, 0x68dc),
            10 => pair(0x6a2e, 0x68de),
            11 => pair(0x6a32, 0x68e2),
            12 | 13 => pair(0x6a30, 0x68e0),
            16 | 33 => pair(0x6a34, 0x68e4),
            17 => pair(0x6a36, 0x68e6),
            18 => pair(0x6a3e, 0x68ee),
            21 => pair(0x6a3c, 0x68ec),
            24 => pair(0x6a38, 0x68e8),
            25 => pair(0x6a3a, 0x68ea),
            28 => pair(0x6a40, 0x68f0),
            29 => pair(0x6a46, 0x68f6),
            30 => pair(0x6a44, 0x68f4),
            31 => pair(0x6a42, 0x68f2),
            32 => pair(0x6a48, 0x68f8),
            47 | 56 => by_policy(key.policy, [pair(0x6a20, 0x68d0), pair(0x6a22, 0x68d2), pair(0x6a24, 0x68d4), pair(0x6a26, 0x68d6), pair(0x6a28, 0x68d8)]),
            49 => pair(0x6a4a, 0x68fa),
            50 => pair(0x6a4c, 0x68fc),
            51 => pair(0x6a4e, 0x68fe),
            52 => pair(0x6a50, 0x6900),
            _ => by_policy(key.policy, [pair(0x6a16, 0x68c6), pair(0x6a18, 0x68c8), pair(0x6a1a, 0x68ca), pair(0x6a1c, 0x68cc), pair(0x6a1e, 0x68ce)]),
        }
    };
    let chosen = if armed_scene(key) { pair(0x6a14, 0x68c4) } else { chosen };
    if flagged {
        pair(0x6a4a, 0x68fa)
    } else {
        chosen
    }
}

/// The first of three indexed limits of a scene, two bytes apart; `None` for the scenes that
/// take their mode limit instead (0xf70080).
pub const fn indexed_limit(key: &LimitKey) -> Option<LimitPair> {
    let chosen = if key.choice != 0 {
        choice_pair(
            key.choice,
            [
                pair(0x6b33, 0x69e3),
                pair(0x6b39, 0x69e9),
                pair(0x6b57, 0x6a07),
                pair(0x6b3f, 0x69ef),
                pair(0x6b45, 0x69f5),
                pair(0x6b4b, 0x69fb),
                pair(0x6b51, 0x6a01),
                pair(0x1_789d, 0x1_7893),
                pair(0x6b5d, 0x6a0d),
            ],
            pair(0x6a79, 0x6929),
        )
    } else {
        match key.scene {
            0 | 43 | 44 | 48 | 57 => by_policy(key.policy, [pair(0x6a6d, 0x691d), pair(0x6a73, 0x6923), pair(0x6a79, 0x6929), pair(0x6a7f, 0x692f), pair(0x6a85, 0x6935)]),
            scene if plain(scene) => pair(0x6ae5, 0x6995),
            10 => pair(0x6aeb, 0x699b),
            11 => pair(0x6af1, 0x69a1),
            16 | 33 => pair(0x6af7, 0x69a7),
            17 => pair(0x6afd, 0x69ad),
            18 => pair(0x6b0f, 0x69bf),
            21 => pair(0x6b09, 0x69b9),
            25 => pair(0x6b03, 0x69b3),
            28 => pair(0x6b15, 0x69c5),
            30 => pair(0x6b1b, 0x69cb),
            32 => pair(0x6b21, 0x69d1),
            45 | 55 => by_policy(key.policy, [pair(0x6aa9, 0x6959), pair(0x6aaf, 0x695f), pair(0x6ab5, 0x6965), pair(0x6abb, 0x696b), pair(0x6ac1, 0x6971)]),
            46 | 54 => by_policy(key.policy, [pair(0x6a8b, 0x693b), pair(0x6a91, 0x6941), pair(0x6a97, 0x6947), pair(0x6a9d, 0x694d), pair(0x6aa3, 0x6953)]),
            47 | 56 => by_policy(key.policy, [pair(0x6ac7, 0x6977), pair(0x6acd, 0x697d), pair(0x6ad3, 0x6983), pair(0x6ad9, 0x6989), pair(0x6adf, 0x698f)]),
            51 => pair(0x6b27, 0x69d7),
            52 => pair(0x6b2d, 0x69dd),
            _ => return None,
        }
    };
    Some(if armed_scene(key) { pair(0x6a67, 0x6917) } else { chosen })
}

/// One byte in tenths: the margin of a tracking measurement; `None` for a margin of 0
/// (0xf70524).
pub const fn tracking_margin(scene: u32, accepting: bool, flagged: bool) -> Option<Property> {
    if flagged {
        return Some(Property(0x62a7));
    }
    Some(Property(match scene {
        0 | 48 if accepting => 0x62ac,
        0 | 48 => 0x6296,
        scene if plain(scene) => 0x6298,
        scene if secondary(scene) => 0x6299,
        10 => 0x629a,
        11 => 0x629c,
        12 | 13 => 0x629b,
        16 | 33 => 0x629d,
        17 => 0x629e,
        18 => 0x62a1,
        21 => 0x62a0,
        25 => 0x629f,
        28 => 0x62a2,
        29 => 0x62a5,
        30 => 0x62a4,
        31 => 0x62a3,
        32 => 0x62a6,
        43 | 44 | 57 if accepting => 0x62ac,
        43 | 44 | 57 | 46 => 0x6297,
        49 => 0x62a7,
        50 => 0x62a8,
        51 => 0x62a9,
        52 => 0x62aa,
        53 | 54 => 0x62ab,
        _ => return None,
    }))
}

/// One byte in tenths: the margin of a measurement that is not tracking; `None` for a margin of 0
/// (0xf70750).
pub const fn still_margin(scene: u32, accepting: bool) -> Option<Property> {
    match scene {
        50 => Some(Property(0x62a8)),
        53 | 54 => Some(Property(0x62ab)),
        _ if accepting => Some(Property(0x62ac)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BandSource {
    /// Both ends are the request's level.
    Level,
    /// One byte each in tenths, the high end first.
    Settings(Property, Property),
}

const fn settings(high: u32, low: u32) -> BandSource { BandSource::Settings(Property(high), Property(low)) }

const fn both(setting: u32) -> BandSource { settings(setting, setting) }

/// The band of a tracking measurement (0xf707b4).
pub const fn tracking_band(scene: u32, accepting: bool, flagged: bool) -> BandSource {
    if flagged {
        return settings(0x6289, 0x628a);
    }
    match scene {
        46 => settings(0x6269, 0x626a),
        scene if plain(scene) => settings(0x626b, 0x626c),
        43 | 44 | 57 if accepting => settings(0x6293, 0x6294),
        43 | 44 | 57 => settings(0x6269, 0x626a),
        24 | 45 | 47 | 55 | 56 => BandSource::Level,
        scene if secondary(scene) => settings(0x626d, 0x626e),
        10 => settings(0x626f, 0x6270),
        12 | 13 => settings(0x6271, 0x6272),
        11 => settings(0x6273, 0x6274),
        16 | 33 => settings(0x6275, 0x6276),
        17 => settings(0x6277, 0x6278),
        25 => settings(0x6279, 0x627a),
        21 => settings(0x627b, 0x627c),
        18 => settings(0x627d, 0x627e),
        28 => settings(0x627f, 0x6280),
        31 => settings(0x6281, 0x6282),
        30 => settings(0x6283, 0x6284),
        29 => settings(0x6285, 0x6286),
        32 => settings(0x6287, 0x6288),
        49 => settings(0x6289, 0x628a),
        50 => settings(0x628b, 0x628c),
        51 => settings(0x628d, 0x628e),
        52 => settings(0x628f, 0x6290),
        53 | 54 => settings(0x6291, 0x6292),
        _ if accepting => settings(0x6293, 0x6294),
        _ => settings(0x6267, 0x6268),
    }
}

/// The band of a measurement that is not tracking (0xf70a8c).
pub const fn still_band(scene: u32, accepting: bool, flagged: bool) -> BandSource {
    if flagged {
        return both(0x625d);
    }
    match scene {
        46 => both(0x624d),
        scene if plain(scene) => both(0x624e),
        43 | 44 | 57 if accepting => settings(0x6264, 0x6265),
        43 | 44 | 57 => both(0x624d),
        24 | 45 | 47 | 55 | 56 => BandSource::Level,
        scene if secondary(scene) => both(0x624f),
        10 => both(0x6250),
        12 | 13 => both(0x6251),
        11 => both(0x6252),
        16 | 33 => both(0x6253),
        17 => both(0x6254),
        25 => both(0x6255),
        21 => both(0x6256),
        18 => both(0x6257),
        28 => both(0x6258),
        31 => both(0x6259),
        30 => both(0x625a),
        29 => both(0x625b),
        32 => both(0x625c),
        49 => both(0x625d),
        50 => settings(0x625e, 0x625f),
        51 => both(0x6260),
        52 => both(0x6261),
        53 | 54 => settings(0x6262, 0x6263),
        _ if accepting => settings(0x6264, 0x6265),
        _ => both(0x624c),
    }
}
