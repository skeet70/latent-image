#![forbid(unsafe_code)]

//! Image profile parameters from the adjustment area: level-indexed value quadruples, coordinate
//! point lists and their window, and the profile mode keys (image 5, 0xda09f4..0xdabbac).

use crate::adjustment::{byte, half, signed_half, word, Adjustments};
use crate::numeric::round_tenths_up;

/// The level index each rounded level selects (the table at 0xdabe18).
const LEVEL_INDICES: [u8; 14] = [0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

/// A level's index: the level divided by a hundred, halves up, between 0 and 13 (0xdaa46c).
pub fn level_index(level: i32) -> u8 {
    let rounded = round_tenths_up(i32::from((level / 10) as i16)) as u16;
    let clamped = if rounded & 0x8000 != 0 { 0 } else { rounded.min(13) };
    LEVEL_INDICES[usize::from(clamped)]
}

/// The three parameter sets 0xdab6f8 reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterSet { Zero, One, Two }

impl ParameterSet {
    pub const fn new(set: u32) -> Option<Self> {
        match set {
            0 => Some(Self::Zero),
            1 => Some(Self::One),
            2 => Some(Self::Two),
            _ => None,
        }
    }

    /// Base and trim keys by level index; entry 0 is absent.
    const fn keys(self) -> ([u32; 10], [u32; 10]) {
        match self {
            Self::Zero => (keys_from(0x2079), keys_from(0x2151)),
            Self::One => (keys_from(0x1fe9), keys_from(0x20c1)),
            Self::Two => (keys_from(0x2031), keys_from(0x2109)),
        }
    }
}

const ABSENT: u32 = u32::MAX;

/// Keys `first`, `first + 8`, .. after the absent entry (the tables at 0xdabfa0, 0xdac018,
/// 0xdabff0, 0xdabde8, 0xdabca8 and 0xdabfc8).
const fn keys_from(first: u32) -> [u32; 10] {
    let mut keys = [ABSENT; 10];
    let mut index = 1;
    while index < 10 {
        keys[index] = first + 8 * (index as u32 - 1);
        index += 1;
    }
    keys
}

/// A parameter quadruple and whether every key it needed was present.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Adjusted {
    pub values: [u16; 4],
    pub complete: bool,
}

/// The base quadruple at the level's index, in the order second, first, fourth, third; unless
/// `base_only`, each less its trim, floored at 0. A missing base reads zeros, a missing trim
/// zero trims, and either leaves the result incomplete (0xdab6f8, which returns -31 for it).
pub fn adjusted_values(area: &impl Adjustments, set: Option<ParameterSet>, level: i32, base_only: bool) -> Adjusted {
    let index = usize::from(level_index(level));
    let (base, trim) = set.map_or((ABSENT, ABSENT), |set| {
        let (base, trim) = set.keys();
        (base[index], trim[index])
    });
    let mut complete = true;
    let mut quadruple = |key: u32, read: &dyn Fn(u32) -> i32| {
        if key == ABSENT {
            complete = false;
            [0; 4]
        } else {
            [0, 2, 4, 6].map(|offset| read(key + offset))
        }
    };
    let [b0, b1, b2, b3] = quadruple(base, &|at| i32::from(half(area, at)));
    if base_only {
        return Adjusted { values: [b1, b0, b3, b2].map(|value| value as u16), complete };
    }
    let [t0, t1, t2, t3] = quadruple(trim, &|at| i32::from(signed_half(area, at)));
    let less = |base: i32, trim: i32| if base >= trim { (base - trim) as u16 } else { 0 };
    Adjusted { values: [less(b1, t1), less(b0, t0), less(b3, t3), less(b2, t2)], complete }
}

/// The adjustment key of each profile mode's value (0xdabb38).
pub const fn mode_key(mode: u32) -> Option<u32> {
    match mode {
        0..=4 => Some(0x1de7),
        5 => Some(0x1de5),
        6..=13 => Some(0x1de6),
        _ => None,
    }
}

/// 0xdabbac.
pub const fn is_kind_five(kind: u32) -> bool { kind == 5 }

/// Adjustment halfword 0x21a3 (0xda1af8).
pub fn output_limit(area: &impl Adjustments) -> u16 { half(area, 0x21a3) }

/// 2^20 over `value`, truncated; 0 counts as 4 (0xda1acc).
pub fn reciprocal_scale(value: u32) -> u32 {
    let value = if value == 0 { 4 } else { value };
    (1_048_576.0f32 / value as f32) as u32
}

/// X-M1 has no capture fields (0xdaa524 clears their enable byte), so the capture configuration
/// 0xda09f4 copies is always empty and its length 0.
pub const CAPTURE_CONFIGURATION_BYTES: u32 = 0;

/// Each profile index's point-list key and shift-pair key, 0 for none (halfwords 4 and 6 of the
/// entries at 0xdabbcc).
const POINT_KEYS: [(u16, u16); 14] =
    [(0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (2, 8), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6), (7, 7), (1, 9)];

/// Where a point list's entries are.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointSource {
    /// Packed words in the adjustment area from this key.
    Adjustment(u32),
    /// The pipeline's point buffer, after filtering.
    Buffer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PointMode {
    #[default]
    Off,
    First,
    Second,
}

/// The window points must fall in, after shifting and less the origin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PointWindow {
    pub enabled: bool,
    pub x: u32,
    pub y: u32,
    pub height: u32,
    pub width: u32,
    pub x_origin: u32,
    pub y_origin: u32,
}

/// A coordinate point list as the profile's capture configuration carries it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointRecord {
    pub source: PointSource,
    pub count: u16,
    pub shift: [i16; 2],
    pub mode: PointMode,
    pub window: PointWindow,
}

/// The buffer filtered points are packed into (0x14c59e8, 4096 bytes).
pub const POINT_BUFFER_WORDS: usize = 1024;

const X: u32 = 0x1fff;
const QUALITY: u32 = 0xc000;

/// Keeps the entries that fall in the window once shifted, packed into `buffer` with the shift
/// and origin applied; the record then names the buffer with no shift. With no entry kept the
/// count is 0. Nothing changes while the window is disabled (0xda17b0). Stock writes past the
/// buffer when more than 1024 entries are kept; owned code keeps at most 1024.
pub fn filter_points(area: &impl Adjustments, record: &mut PointRecord, buffer: &mut [u32; POINT_BUFFER_WORDS]) {
    let window = record.window;
    if !window.enabled {
        return;
    }
    buffer.fill(0);
    let PointSource::Adjustment(first) = record.source else {
        record.count = 0;
        return;
    };
    let [x_shift, y_shift] = record.shift;
    let shifted = |raw: u32, shift: i16, origin: u32| raw.wrapping_add(shift as i32 as u32).wrapping_sub(origin);
    let mut kept = 0;
    for entry in (0..u32::from(record.count)).map(|index| word(area, first.wrapping_add(4 * index))) {
        let x = shifted(entry & X, x_shift, window.x_origin);
        let y = shifted((entry >> 16) & X, y_shift, window.y_origin);
        let inside = x >= window.x && x < window.x.wrapping_add(window.width) && y >= window.y && y < window.y.wrapping_add(window.height);
        if inside && kept < POINT_BUFFER_WORDS {
            buffer[kept] = (y & X) << 16 | entry & QUALITY | x & X;
            kept += 1;
        }
    }
    if kept == 0 {
        record.count = 0;
    } else {
        record.count = kept as u16;
        record.source = PointSource::Buffer;
        record.shift = [0, 0];
    }
}

/// Fills the record for profile `index` from the point table: its list and count (indices 5 to
/// 13), its shift pair, the filtered points and the mode adjustment bytes 0xb4 and 0xc9 select
/// (0xdaba08 with 0xdab920, 0xdaa4c4).
pub fn prepare_points(area: &impl Adjustments, index: u32, record: &mut PointRecord, buffer: &mut [u32; POINT_BUFFER_WORDS]) {
    if (5..=13).contains(&index) {
        let (count_key, pair_key) = POINT_KEYS[index as usize];
        if count_key == 0 {
            record.count = 0;
            record.mode = PointMode::Off;
            return;
        }
        let count_key = u32::from(count_key);
        record.source = PointSource::Adjustment((count_key + 80) << 12);
        record.count = half(area, 2 * count_key + 0x277b);
        if pair_key == 0 {
            record.shift = [0, 0];
            record.mode = PointMode::Off;
            return;
        }
        let pair_key = u32::from(pair_key);
        record.shift = [signed_half(area, 4 * pair_key + 0x27b0), signed_half(area, 4 * pair_key + 0x27b2)];
    } else {
        record.count = 0;
    }
    filter_points(area, record, buffer);
    record.mode = if record.count == 0 || byte(area, 0xb4) & 0x20 == 0 {
        PointMode::Off
    } else if byte(area, 0xc9) & 0x20 != 0 {
        PointMode::Second
    } else {
        PointMode::First
    };
}
