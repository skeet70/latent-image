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
