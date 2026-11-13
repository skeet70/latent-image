//! Stock layouts the adjustment layer reaches: its configuration ids, the configuration block
//! other tasks share with saclac, the motion and frame-interval records' bytes, and the motion
//! diagnostics in the work memory.

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::configuration::{Centroid, IntervalList, MotionClass, MotionDiagnostics, MotionRecord, MotionSource};
use crate::exposure::ports::Property;

const SHARED_BLOCK: u32 = 0x016d_8708;

/// Compared with 1 by 0xfa7ef4; no image stores to it by address.
pub const SHARED_MODE: SharedAddress = SharedAddress(SHARED_BLOCK);
/// Stored by 0xfa7f20, which other tasks enter through 0xef9788.
pub const SHARED_WORD: SharedAddress = SharedAddress(SHARED_BLOCK + 0x4);
/// Stored by 0xfa7f60, which other tasks enter through 0xef97d8.
pub const SHARED_FEATURE: SharedAddress = SharedAddress(SHARED_BLOCK + 0x8);
/// Cleared by other tasks through 0xef97ec; the controllers set it after reading it (0xf30fbc).
pub const SHARED_SEEN: SharedAddress = SharedAddress(SHARED_BLOCK + 0x54);
/// Set by other tasks through 0xef993c; the round clears it after reading it (0xf3a6d8).
pub const SHARED_REQUEST: SharedAddress = SharedAddress(SHARED_BLOCK + 0x58);

/// With the shared feature word at 1, enables the feature (0xfa7f9c).
pub const FEATURE_SETTING: Property = Property(0xe4b);
/// At 1, keeps the refresh flag clear (0xfa9568).
pub const REFRESH_SUPPRESSED: Property = Property(0x87fd);

/// Motion measurement settings for live view (0x5256) and the half press (0x5257): bit 1
/// measures, bit 0 classifies by the measured kind.
pub const fn motion_enable(source: MotionSource) -> Property {
    match source {
        MotionSource::Live => Property(0x5256),
        MotionSource::HalfPress => Property(0x5257),
    }
}

/// The cell difference at which a cell counts as changed (0xfa9604, 0xfa9630).
pub const fn motion_threshold(source: MotionSource) -> Property {
    match source {
        MotionSource::Live => Property(0x528c),
        MotionSource::HalfPress => Property(0x528d),
    }
}

/// The changed-cell count below which no motion is found (0xfa9610, 0xfa963c).
pub const fn motion_minimum_cells(source: MotionSource) -> Property {
    match source {
        MotionSource::Live => Property(0x528e),
        MotionSource::HalfPress => Property(0x528f),
    }
}

/// The two factors the published area is compared in units of (0xfa864c..0xfa8678).
pub const fn area_factors(source: MotionSource) -> [Property; 2] {
    match source {
        MotionSource::Live => [Property(0x5251), Property(0x5252)],
        MotionSource::HalfPress => [Property(0x5253), Property(0x5254)],
    }
}

/// The level at or above which the published speed level is recorded in place of the measured speed
/// (0xfa8568).
pub const fn offset_limit(source: MotionSource) -> Property {
    match source {
        MotionSource::Live => Property(0x160a7),
        MotionSource::HalfPress => Property(0x160a8),
    }
}

/// How many recorded offsets the average spans (0xfa8208).
pub const fn average_window(source: MotionSource) -> Property {
    match source {
        MotionSource::Live => Property(0x52ca),
        MotionSource::HalfPress => Property(0x52cb),
    }
}

/// How many recorded classes the dominant class spans (0xfa83d0).
pub const fn dominant_window(source: MotionSource) -> Property {
    match source {
        MotionSource::Live => Property(0x52b3),
        MotionSource::HalfPress => Property(0x52b4),
    }
}

/// The classes, in preference order, a tie for the most recorded class is settled by (0xfa848c..0xfa84b4).
pub const DOMINANT_PREFERENCE: [Property; 4] = [Property(0x52af), Property(0x52b0), Property(0x52b1), Property(0x52b2)];

/// At 1, the moving-subject rules apply (0xfa8688, 0xfa8744).
pub const MOVING_RULES: Property = Property(0x52a2);

/// The bounds the moving-subject rules check: area in factor units, changed cells and speed, each
/// as (lowest admitted, first refused). The first set applies while the published exposure is 0
/// (0xfa8698..0xfa8730), the second otherwise (0xfa8754..0xfa87dc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MovingBounds {
    pub area: [Property; 2],
    pub cells: [Property; 2],
    pub speed: [Property; 2],
}

pub const MOVING_STILL_EXPOSURE: MovingBounds = MovingBounds {
    area: [Property(0x1609f), Property(0x160a0)],
    cells: [Property(0x160a1), Property(0x160a2)],
    speed: [Property(0x160a3), Property(0x160a4)],
};

pub const MOVING_EXPOSED: MovingBounds = MovingBounds {
    area: [Property(0x52a9), Property(0x52aa)],
    cells: [Property(0x160a5), Property(0x160a6)],
    speed: [Property(0x52ad), Property(0x52ae)],
};

/// The configured class for a measured result and a published kind, both below 4 (table 0xfaaf70).
pub const fn configured_class(found: u32, kind: u32) -> Property {
    let base = match found {
        0 => 0x5296,
        1 => 0x529a,
        _ => 0x529e,
    };
    Property(base + kind)
}

/// The search's minimum step (0xfa7620).
pub const MINIMUM_STEP: Property = Property(0x8806);
/// The drive target's factor, base channel and two-byte duration (0xfa7cf8, 0xfa7d04, 0xfa7d48).
pub const TARGET_FACTOR: Property = Property(0xa491);
pub const TARGET_BASE_CHANNEL: Property = Property(0xa492);
pub const TARGET_DURATION: Property = Property(0xa493);
/// In tens, added to the search's thresholds (0xfa9338, 0xfa939c).
pub const THRESHOLD_MARGIN: Property = Property(0x8823);
/// The fall that passes the peak outright (0xfa9390), and in mode 1 (0xfa9384).
pub const PEAK_LENGTH: Property = Property(0x8831);
pub const PEAK_LENGTH_MODE_ONE: Property = Property(0x8833);
/// The fall from the peak, in samples and in hundreds (0xfa9498, 0xfa94a4).
pub const FALL_LENGTH: Property = Property(0x8832);
pub const FALL_DEPTH: Property = Property(0x8830);

/// The resource step the bias interpolation spans upward (0xfaa80c) and downward (0xfaa890).
pub const fn ascending_step(resource: u32) -> Property { Property(0x88f2 + resource) }

pub const fn descending_step(resource: u32) -> Property { Property(0x8919 + resource) }

/// Bit 0 admits scenes to the round's adjustment (0xfab26c).
pub const SCENE_ADMISSION: Property = Property(0xa7e4);
/// The extent thresholds by mapped-value band (0xfab454..0xfab47c).
pub const EXTENT_THRESHOLDS: [Property; 4] = [Property(0x8ba1), Property(0x8ba2), Property(0x8ba3), Property(0x8ba4)];

/// The 24-bit factor's high, middle and low bytes (0xfa659c..0xfa65b4).
pub const CONFIGURED_FACTOR: [Property; 3] = [Property(0x6399), Property(0x6398), Property(0x6397)];
/// The drive rate's second factor, its step count and its span (0xfa6958, 0xfa69cc, 0xfa69e4).
pub const RATE_SECOND_FACTOR: Property = Property(0xa4df);
pub const RATE_STEPS: Property = Property(0xa49b);
pub const RATE_SPAN: Property = Property(0xa49c);

/// Table 0xfaaed8: eight policies per group.
pub const fn policy_setting(policy: u8, second_group: bool) -> Property { Property(0x63a8 + 8 * second_group as u32 + policy as u32) }

/// Tables 0xfab050, 0xfab1f0, 0xfab0b0 and 0xfaafd0: a byte and two settings in tenths per policy.
pub const fn rate_settings(policy: u8, alternate: bool, kind_two: bool) -> [Property; 3] {
    let (byte, tenths) = match (alternate, kind_two) {
        (false, false) => (0x63fc, 0x63ba),
        (false, true) => (0x6404, 0x63ca),
        (true, false) => (0x640d, 0x63db),
        (true, true) => (0x6415, 0x63eb),
    };
    let policy = policy as u32;
    [Property(byte + policy), Property(tenths + 1 + 2 * policy), Property(tenths + 2 * policy)]
}

/// Read with every policy's settings (0xfa7b70).
pub const RATE_BASE: Property = Property(0x63b8);

/// The policy flag for selector 1, by the round's mode (0xfa8098..0xfa80ec).
pub const fn policy_flag_by_mode(mode: u32) -> Property {
    match mode {
        5 => Property(0x82bd),
        6 => Property(0x82bf),
        7 => Property(0x82ba),
        _ => Property(0x82be),
    }
}

/// The policy flag by sensitivity class (table 0xfaaf18, 22 entries; the rest name none).
pub const fn policy_flag_by_class(class: u8) -> Option<Property> {
    match class {
        2 => Some(Property(0x82b8)),
        4 => Some(Property(0x82b9)),
        8 => Some(Property(0x82ba)),
        9 => Some(Property(0x82bb)),
        10 | 11 => Some(Property(0x82bc)),
        _ => None,
    }
}

/// The primary scale settings (table 0xfac390, eight entries).
pub const fn primary_scale(index: u8) -> Option<Property> {
    if index < 8 { Some(Property(0x133f + index as u32)) } else { None }
}

/// How 0xfa89fc reads one configuration word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reading {
    /// 0x69f8fc.
    Byte(Property),
    /// 0x69fec8.
    Word(Property),
    /// 0x69ff7c.
    SignedWord(Property),
    /// 0x69ff68.
    SignedByte(Property),
    Zero,
}

/// The configuration words 0xfa89fc stores at +4 of the shot's block, word index first, in its
/// store order.
pub const CONFIGURATION_WORDS: [(usize, Reading); 161] = {
    use Reading::*;
    [
    (0, Byte(Property(0x520e))), (1, Byte(Property(0x520f))), (2, Word(Property(0x5210))),
    (3, Word(Property(0x5212))), (4, Byte(Property(0x5214))), (5, Byte(Property(0x5215))),
    (6, Word(Property(0x5216))), (7, Word(Property(0x5218))), (8, Byte(Property(0x5201))),
    (9, Word(Property(0x5202))), (10, Word(Property(0x5204))), (11, Byte(Property(0x5206))),
    (12, Byte(Property(0x1609a))), (13, Byte(Property(0x1609b))), (14, Byte(Property(0x1609c))),
    (15, Word(Property(0x521a))), (16, Byte(Property(0x521c))), (17, Byte(Property(0x521d))),
    (18, Byte(Property(0x1609d))), (19, Byte(Property(0x1609e))), (20, Byte(Property(0x521e))),
    (21, Byte(Property(0x521f))), (22, Byte(Property(0x5220))), (23, Byte(Property(0x5221))),
    (24, Byte(Property(0x5222))), (25, Byte(Property(0x5223))), (26, Byte(Property(0x5224))),
    (27, Word(Property(0x5225))), (28, Byte(Property(0x5227))), (29, Byte(Property(0x5228))),
    (30, Byte(Property(0x5229))), (31, Word(Property(0x522a))), (32, Word(Property(0x522c))),
    (33, Byte(Property(0x522e))), (34, Word(Property(0x522f))), (35, Byte(Property(0x5231))),
    (36, Byte(Property(0x5232))), (37, Byte(Property(0x5233))), (38, Byte(Property(0x5234))),
    (39, Byte(Property(0x5235))), (40, Word(Property(0x5236))), (41, Byte(Property(0x5238))),
    (42, Word(Property(0x5239))), (43, Byte(Property(0x523b))), (44, Byte(Property(0x523c))),
    (45, Byte(Property(0x523d))), (46, Byte(Property(0x523e))), (47, Byte(Property(0x523f))),
    (48, Byte(Property(0x5240))), (49, Byte(Property(0x5241))), (50, Byte(Property(0x5242))),
    (51, Byte(Property(0x5243))), (52, Byte(Property(0x5244))), (53, Byte(Property(0x5245))),
    (54, Byte(Property(0x5246))), (55, Byte(Property(0x5247))), (56, Byte(Property(0x5251))),
    (58, Byte(Property(0x5252))), (57, Byte(Property(0x5253))), (59, Byte(Property(0x5254))),
    (60, Byte(Property(0x5255))), (61, Byte(Property(0x5256))), (62, Byte(Property(0x5257))),
    (63, Byte(Property(0x525d))), (64, Word(Property(0x525e))), (65, Word(Property(0x5260))),
    (66, Word(Property(0x5262))), (67, Word(Property(0x5264))), (70, Word(Property(0x5268))),
    (72, Byte(Property(0x526a))), (74, Byte(Property(0x526b))), (76, Byte(Property(0x526c))),
    (78, Byte(Property(0x526d))), (80, Byte(Property(0x526e))), (82, Byte(Property(0x5284))),
    (83, Byte(Property(0x5285))), (84, Byte(Property(0x5286))), (85, Byte(Property(0x5287))),
    (86, Byte(Property(0x5288))), (87, Byte(Property(0x5289))), (88, Byte(Property(0x528c))),
    (89, Byte(Property(0x528d))), (90, Byte(Property(0x528e))), (91, Byte(Property(0x528f))),
    (92, Byte(Property(0x5296))), (93, Byte(Property(0x5297))), (94, Byte(Property(0x5298))),
    (95, Byte(Property(0x5299))), (96, Byte(Property(0x529a))), (97, Byte(Property(0x529b))),
    (98, Byte(Property(0x529c))), (99, Byte(Property(0x529d))), (100, Byte(Property(0x529e))),
    (101, Byte(Property(0x529f))), (102, Byte(Property(0x52a0))), (103, Byte(Property(0x52a1))),
    (104, Byte(Property(0x52a2))), (105, Byte(Property(0x1609f))), (107, Byte(Property(0x160a0))),
    (109, Byte(Property(0x160a1))), (111, Byte(Property(0x160a2))), (113, Byte(Property(0x160a3))),
    (115, Byte(Property(0x160a4))), (117, Byte(Property(0x52af))), (118, Byte(Property(0x52b0))),
    (119, Byte(Property(0x52b1))), (120, Byte(Property(0x52b2))), (121, Byte(Property(0x52b3))),
    (122, Byte(Property(0x52b4))), (123, Byte(Property(0x160a7))), (124, Byte(Property(0x160a8))),
    (125, Byte(Property(0x52ca))), (126, Byte(Property(0x52cb))), (127, Byte(Property(0x5258))),
    (146, Byte(Property(0x52eb))), (147, SignedWord(Property(0x52ec))), (148, SignedByte(Property(0x52ee))),
    (149, Byte(Property(0x52ef))), (150, Zero), (151, Byte(Property(0x52f0))), (152, Zero),
    (153, Byte(Property(0x52f1))), (154, Zero), (155, Byte(Property(0x52f2))), (156, Zero),
    (157, Byte(Property(0x52f3))), (158, Zero), (159, Byte(Property(0x5259))), (160, Byte(Property(0x525a))),
    (68, Byte(Property(0x5266))), (69, Byte(Property(0x5267))), (71, Word(Property(0x526f))),
    (73, Byte(Property(0x5271))), (75, Byte(Property(0x5272))), (77, Byte(Property(0x5273))),
    (79, Byte(Property(0x5274))), (81, Byte(Property(0x5275))), (106, Byte(Property(0x52a9))),
    (108, Byte(Property(0x52aa))), (110, Byte(Property(0x160a5))), (112, Byte(Property(0x160a6))),
    (114, Byte(Property(0x52ad))), (116, Byte(Property(0x52ae))), (128, Byte(Property(0x52d9))),
    (129, Byte(Property(0x52da))), (130, Byte(Property(0x52db))), (131, Byte(Property(0x52dc))),
    (132, Byte(Property(0x52dd))), (133, Byte(Property(0x52de))), (134, Byte(Property(0x52df))),
    (135, Byte(Property(0x52e0))), (136, Byte(Property(0x52e1))), (137, Byte(Property(0x52e2))),
    (138, Byte(Property(0x52e3))), (139, Byte(Property(0x52e4))), (140, Byte(Property(0x52e5))),
    (141, Byte(Property(0x52e6))), (142, Byte(Property(0x52e7))), (143, Byte(Property(0x52e8))),
    (144, Byte(Property(0x52e9))), (145, Byte(Property(0x52ea))),
    ]
};

fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }

fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }

fn put_word(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()) }

fn put_half(bytes: &mut [u8], at: usize, value: u16) { bytes[at..at + 2].copy_from_slice(&value.to_le_bytes()) }

/// The motion record's fields (shared field group 27 at +0x7ec, 0x7582c0/0x758300). Bytes the
/// layer does not name pass through unchanged.
impl MotionRecord {
    pub fn decode(bytes: [u8; 40]) -> Self {
        Self {
            found: word(&bytes, 0x00),
            speed: half(&bytes, 0x04),
            published_kind: word(&bytes, 0x08),
            published_speed: half(&bytes, 0x0c),
            dominant: word(&bytes, 0x10),
            average: half(&bytes, 0x14),
            average_scale: half(&bytes, 0x16),
            published_area: word(&bytes, 0x18),
            published_exposure: word(&bytes, 0x1c),
            source: word(&bytes, 0x24),
            bytes,
        }
    }

    pub fn encode(&self) -> [u8; 40] {
        let mut bytes = self.bytes;
        put_word(&mut bytes, 0x00, self.found);
        put_half(&mut bytes, 0x04, self.speed);
        put_word(&mut bytes, 0x08, self.published_kind);
        put_half(&mut bytes, 0x0c, self.published_speed);
        put_word(&mut bytes, 0x10, self.dominant);
        put_half(&mut bytes, 0x14, self.average);
        put_half(&mut bytes, 0x16, self.average_scale);
        put_word(&mut bytes, 0x18, self.published_area);
        put_word(&mut bytes, 0x1c, self.published_exposure);
        put_word(&mut bytes, 0x24, self.source);
        bytes
    }
}

/// The frame-interval list (shared field group 14 at +0x864, 0x757ff0/0x758028): ten words and a
/// halfword count at +0x28.
impl IntervalList {
    pub fn decode(bytes: [u8; 48]) -> Self {
        Self { intervals: core::array::from_fn(|index| word(&bytes, 4 * index)), count: half(&bytes, 0x28), bytes }
    }

    pub fn encode(&self) -> [u8; 48] {
        let mut bytes = self.bytes;
        for (index, interval) in self.intervals.iter().enumerate() {
            put_word(&mut bytes, 4 * index, *interval);
        }
        put_half(&mut bytes, 0x28, self.count);
        bytes
    }
}

const DIAGNOSTICS: u32 = 0x5ffe_6f80;

/// Where 0xfa91b0 stores the motion diagnostics in the work memory, one word each, in its store
/// order.
pub fn motion_diagnostic_words(diagnostics: MotionDiagnostics<'_>) -> impl Iterator<Item = (SharedAddress, u32)> + '_ {
    let at = |offset: u32| SharedAddress(DIAGNOSTICS + offset);
    let cell = |value: i16| value as i32 as u32;
    let centroid = |centroid: Option<Centroid>, axis: fn(Centroid) -> u32| centroid.map_or(u32::MAX, axis);
    let measurement = diagnostics.measurement;
    let grids = diagnostics.current.iter().zip(diagnostics.previous.iter()).enumerate().flat_map(move |(index, (current, previous))| {
        let index = index as u32;
        [(at(0x4 + 4 * index), cell(*current)), (at(0x404 + 4 * index), cell(*previous))]
    });
    let summary = [
        (at(0x804), u32::from(measurement.cells)),
        (at(0x808), u32::from(measurement.found)),
        (at(0x80c), centroid(measurement.centroid, |centroid| centroid.x)),
        (at(0x810), centroid(measurement.centroid, |centroid| centroid.y)),
        (at(0x814), centroid(measurement.previous, |centroid| centroid.x)),
        (at(0x818), centroid(measurement.previous, |centroid| centroid.y)),
        (at(0x81c), u32::from(measurement.rate)),
        (at(0x820), u32::from(measurement.speed)),
        (at(0x824), class_code(diagnostics.class)),
    ];
    let classes = diagnostics.classes.into_iter().enumerate().map(move |(index, class)| (at(0x828 + 4 * index as u32), class.map_or(100, class_code)));
    let tail = [(at(0x878), class_code(diagnostics.dominant)), (at(0x87c), u32::from(diagnostics.offset))];
    let offsets = diagnostics.offsets.into_iter().enumerate().map(move |(index, offset)| (at(0x880 + 4 * index as u32), u32::from(offset.unwrap_or(u16::MAX))));
    grids.chain(summary).chain(classes).chain(tail).chain(offsets).chain([(at(0x8d0), u32::from(diagnostics.average))])
}

/// The class as the motion record and the diagnostics store it.
pub const fn class_code(class: MotionClass) -> u32 {
    match class {
        MotionClass::Zero => 0,
        MotionClass::One => 1,
        MotionClass::Two => 2,
        MotionClass::Undetermined => 3,
    }
}
