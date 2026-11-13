#![forbid(unsafe_code)]

//! Limits read from the adjustment area by mode, and the curve positions derived from a level.

use xm1_saclac_shared::settings::Channel;

use crate::adjustment::{byte, half, signed_half, word, Adjustments};
use crate::channel;
use crate::numeric::{divide, divide_unsigned};
use crate::ports::{Derivations, ExportQuery, RangeQuery};
use crate::table::{self, Row, Step};

const SHIFT: u32 = 0x3e162f;
const DISABLED: u32 = 0x3e0009;

/// The four mode classes the limit tables are laid out by (0x748018).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeClass { Others, Modes9And10, Modes6And11And12, Modes1To4 }

impl ModeClass {
    pub const fn of(mode: u32) -> Self {
        match mode {
            1..=4 => Self::Modes1To4,
            6 | 11 | 12 => Self::Modes6And11And12,
            9 | 10 => Self::Modes9And10,
            _ => Self::Others,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Others => 0,
            Self::Modes9And10 => 1,
            Self::Modes6And11And12 => 2,
            Self::Modes1To4 => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind { First, Second }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side { One, Other }

/// A limit table: sixteen adjustment keys by kind, mode class and side, and how the key reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Limit {
    /// 0x748104: a word (keys at 0x818ab0).
    Word,
    /// 0x748090: a halfword shifted by adjustment 0x3e162f (keys at 0x774d94).
    ShiftedHalf,
    /// 0x7481c4: a word, 0xffff when 0 (keys at 0x81862c).
    OptionalWord,
    /// 0x748210: a shifted halfword, 0xffff when 0 (keys at 0x818940).
    OptionalShiftedHalf,
    /// 0x748144: a shifted halfword, 0xffff when 0 (keys at 0x774de4).
    OptionalShiftedNextHalf,
    /// 0x748270: a halfword (keys at 0x774e34).
    Half,
}

impl Limit {
    fn keys(self) -> [u32; 16] {
        match self {
            Self::Word => WORD_KEYS,
            Self::ShiftedHalf => SHIFTED_HALF_KEYS,
            Self::OptionalWord => OPTIONAL_WORD_KEYS,
            Self::OptionalShiftedHalf => OPTIONAL_SHIFTED_HALF_KEYS,
            Self::OptionalShiftedNextHalf => OPTIONAL_SHIFTED_NEXT_HALF_KEYS,
            Self::Half => HALF_KEYS,
        }
    }
}

const SHIFTED_HALF_KEYS: [u32; 16] = [
    0x3e145e, 0x3e145e, 0x3e14c9, 0x3e14c9, 0x3e146f, 0x3e1480, 0x3e14da, 0x3e14eb, 0x3e1491, 0x3e14a2, 0x3e14fc, 0x3e150d,
    0x3e145e, 0x3e145e, 0x3e14c9, 0x3e14c9,
];
const WORD_KEYS: [u32; 16] = shifted_by(SHIFTED_HALF_KEYS, 4);
const OPTIONAL_SHIFTED_NEXT_HALF_KEYS: [u32; 16] = [
    0x3e1676, 0x3e1676, 0x3e18a6, 0x3e18a6, 0x3e16c6, 0x3e1716, 0x3e18f6, 0x3e1946, 0x3e1766, 0x3e17b6, 0x3e1996, 0x3e19e6,
    0x3e1676, 0x3e1676, 0x3e18a6, 0x3e18a6,
];
const OPTIONAL_WORD_KEYS: [u32; 16] = shifted_by(OPTIONAL_SHIFTED_NEXT_HALF_KEYS, 0xa);
const OPTIONAL_SHIFTED_HALF_KEYS: [u32; 16] = shifted_by(OPTIONAL_SHIFTED_NEXT_HALF_KEYS, 2);
const HALF_KEYS: [u32; 16] = shifted_by(OPTIONAL_SHIFTED_NEXT_HALF_KEYS, 0x46);

const fn shifted_by(keys: [u32; 16], before: u32) -> [u32; 16] {
    let mut shifted = keys;
    let mut index = 0;
    while index < 16 {
        shifted[index] -= before;
        index += 1;
    }
    shifted
}

/// The halfword at `at` shifted left by adjustment 0x3e162f.
fn shifted(area: &impl Adjustments, at: u32) -> u32 { u32::from(half(area, at)).checked_shl(u32::from(byte(area, SHIFT))).unwrap_or(0) }

/// The shifted halfword at 0x3e15a6 (0x747af8).
pub fn shifted_floor(area: &impl Adjustments) -> u32 { shifted(area, 0x3e15a6) }

/// The shifted halfword at 0x3e15a8 (0x747b24).
pub fn shifted_second_floor(area: &impl Adjustments) -> u32 { shifted(area, 0x3e15a8) }

pub fn limit(area: &impl Adjustments, which: Limit, kind: Kind, side: Side, mode: u32) -> u32 {
    let index = 4 * ModeClass::of(mode).index() + 2 * usize::from(kind == Kind::Second) + usize::from(side == Side::Other);
    let at = which.keys()[index];
    let optional = |value: u32| if value == 0 { 0xffff } else { value };
    match which {
        Limit::Word => word(area, at),
        Limit::ShiftedHalf => shifted(area, at),
        Limit::OptionalWord => optional(word(area, at)),
        Limit::OptionalShiftedHalf | Limit::OptionalShiftedNextHalf => optional(shifted(area, at)),
        Limit::Half => u32::from(half(area, at)),
    }
}

/// The two channel limit tables (0x74a244, 0x74a298).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelLimit { Primary, Secondary }

/// The channel's limit and whether the channel is configured; an unconfigured channel reads
/// the table's first entry.
pub fn channel_limit(area: &impl Adjustments, which: ChannelLimit, channel: Channel) -> (i16, bool) {
    let table = match which {
        ChannelLimit::Primary => 0x3e0a6b,
        ChannelLimit::Secondary => 0x3e09a2,
    };
    if channel::is_valid(area, channel) {
        (signed_half(area, table + 2 * channel.0 as u32), true)
    } else {
        (signed_half(area, table), false)
    }
}

/// `value` times the channel scale times ten over the table scale of `step`, a zero table scale
/// read as 1 (0x749430).
pub fn scaled_value(ports: &mut impl Derivations, channel: Channel, step: u32, value: u32) -> u16 {
    let scale = u32::from(ports.channel_scale(channel));
    let divisor = u32::from(ports.table_scale(step, channel)).max(1);
    divide_unsigned(value.wrapping_mul(scale).wrapping_mul(10), divisor) as u16
}

/// How `selected_limit` bounds the scaled limit before it is converted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitMode {
    /// The value capped by ten times adjustment byte 0x3e144d, the scaled limit by halfword
    /// 0x3e1445; the mode's limit is `Limit::OptionalShiftedNextHalf`.
    Capped,
    /// As `Capped`, the scaled limit then kept within the bounds.
    Bounded { upper: i16, lower: i16 },
    /// No cap; the mode's limit is `Limit::OptionalShiftedHalf`.
    Uncapped,
}

/// 0x7492ac's arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitQuery {
    pub channel: Channel,
    pub first: u32,
    pub value: u32,
    pub divisor: u32,
    pub mode: LimitMode,
    pub kind: Kind,
    pub side: Side,
    pub limit_mode: u32,
}

/// The scaled limit 0x749254 gives, which callers also keep, and the limit converted by ten
/// thousand over the divisor, kept at least the shifted floor and at most the mode's limit
/// (0x7492ac).
pub fn selected_limit(area: &impl Adjustments, ports: &mut impl Derivations, query: LimitQuery) -> (u16, u32) {
    let convert = |value: u32| divide_unsigned(10_000u32.wrapping_mul(value), query.divisor).max(shifted_floor(area));
    if query.mode == LimitMode::Uncapped {
        let scaled = ports.scaled_limit(query.channel, query.first, query.value);
        let limit = limit(area, Limit::OptionalShiftedHalf, query.kind, query.side, query.limit_mode);
        return (scaled, convert(u32::from(scaled)).min(limit));
    }
    let cap = u32::from(byte(area, 0x3e144d)) * 10;
    let value = if cap != 0 && (query.value as i32) > cap as i32 { cap & 0xffff } else { query.value };
    let scaled = ports.scaled_limit(query.channel, query.first, value);
    let ceiling = u32::from(half(area, 0x3e1445));
    let mut bounded = u32::from(scaled);
    if ceiling != 0 && ceiling < bounded {
        bounded = ceiling;
    }
    if let LimitMode::Bounded { upper, lower } = query.mode {
        if bounded >= upper as i32 as u32 {
            bounded = upper as i32 as u32;
        }
        if bounded < lower as i32 as u32 {
            bounded = lower as i32 as u32;
        }
    }
    let limit = limit(area, Limit::OptionalShiftedNextHalf, query.kind, query.side, query.limit_mode);
    (scaled, convert(bounded).min(limit))
}

/// 0x749e40.
pub fn apply_reference(ports: &mut impl Derivations, channel: Channel, mode: u32) {
    let word = ports.reference_word(mode, channel);
    ports.apply_reference(word, channel);
}

/// Three positions on a channel's curve.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CurvePositions {
    pub centre: u32,
    pub lower: u32,
    pub upper: u32,
}

/// 0x749e98's arguments after the level and channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionQuery {
    pub first: u32,
    pub export_first: u32,
    pub direction_mode: u32,
    pub export_second: u32,
}

/// The curve positions of `level` less the channel's adjusted word and of half the scaled limit
/// to either side: the centre at most 100, `lower` under it and `upper` over it, at most 100.
/// All 0 while adjustment 0x3e0009 is 1 (0x749e98).
pub fn curve_positions(
    area: &impl Adjustments,
    ports: &mut impl Derivations,
    level: i16,
    channel: Channel,
    query: PositionQuery,
) -> CurvePositions {
    if byte(area, DISABLED) == 1 {
        return CurvePositions::default();
    }
    let exported = ports.export_word(channel, ExportQuery { selector: 0, first: query.export_first, second: query.export_second });
    let adjusted = ports.adjust_direction(exported, 1, query.direction_mode) as u16;
    let half_limit = (ports.scaled_limit(channel, query.first, 100) >> 1) as u16;
    let level = level as u16;
    let offset = level.wrapping_sub(adjusted);
    let centre = ports.curve_position(offset as i16, channel).min(100);
    let lower = ports.curve_position(half_limit.wrapping_add(offset) as i16, channel);
    let upper = ports.curve_position(level.wrapping_sub(half_limit).wrapping_sub(adjusted) as i16, channel);
    let lower = if centre == 0 { 0 } else { lower.min(centre - 1) };
    let upper = upper.max(centre + 1).min(100);
    CurvePositions { centre, lower, upper }
}

/// `curve_positions` between the channel and the next, `fraction` (sixteen-bit) of the way
/// across; a channel without a next one, a zero fraction or a zero curve span reads the
/// channel alone. Unchanged while adjustment 0x3e0009 is 1 (0x749fac).
pub fn interpolated_curve_positions(
    area: &impl Adjustments,
    ports: &mut impl Derivations,
    level: i16,
    channel: Channel,
    fraction: u32,
    query: PositionQuery,
) -> Option<CurvePositions> {
    if byte(area, DISABLED) == 1 {
        return None;
    }
    let alone = |ports: &mut _| Some(curve_positions(area, ports, level, channel, query));
    let last = channel::count(area) as i32 - 1;
    if last <= channel.0 || fraction == 0 {
        return alone(ports);
    }
    let next = Channel(channel.0 + 1);
    let export = ExportQuery { selector: 0, first: query.export_first, second: query.export_second };
    let low = ports.export_word(channel, export);
    let high = ports.export_word(next, export);
    let step = (i32::from(high) - i32::from(low)).wrapping_mul(fraction as i32);
    let step = if step < 0 { step + 0xffff } else { step };
    let blended = ports.adjust_direction((low as u16).wrapping_add((step >> 16) as u16) as i16, 1, query.direction_mode);
    let last = Channel(last);
    let reference = ports.channel_word(last, query.export_second, 0x3e063b) as u16;
    let bias = channel::bias(area, last, query.export_second) as u16;
    let position = ports.curve_position(reference.wrapping_sub(bias) as i16, last);
    let low_value = i32::from(ports.curve_value(position, channel));
    let high_value = i32::from(ports.curve_value(position, next));
    let span = low_value.wrapping_add((fraction.wrapping_mul((high_value - low_value) as u32) >> 16) as i32) as i16;
    if span == 0 {
        return alone(ports);
    }
    let excess = i32::from(level.wrapping_sub(blended));
    let span = i32::from(span);
    let high_level = (divide(high_value.wrapping_mul(excess), span) as u16).wrapping_add(high as u16);
    let low_level = (divide(low_value.wrapping_mul(excess), span) as u16).wrapping_add(low as u16);
    let both = PositionQuery { direction_mode: 1, ..query };
    let first = curve_positions(area, ports, low_level as i16, channel, both);
    let second = curve_positions(area, ports, high_level as i16, next, both);
    let mean = |first: u32, second: u32| first.wrapping_add(second) >> 1;
    Some(CurvePositions {
        centre: mean(first.centre, second.centre),
        lower: mean(first.lower, second.lower),
        upper: mean(first.upper, second.upper),
    })
}

/// 0x74ab14's arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WordQuery {
    pub level: i16,
    pub channel: Channel,
    pub step: Step,
    pub export_first: u32,
    pub export_second: u32,
    pub direction_mode: u32,
}

/// The channel's word for `step` relative to step 0, plus the level's position between the
/// channel's eleven reference words (0x3f3d00) by the channel's spacing (0x3e2643), plus ten
/// times the compensated word (0x74ab14).
pub fn channel_step_word(area: &impl Adjustments, ports: &mut impl Derivations, query: WordQuery) -> i32 {
    let row = Row(query.channel.0 as u32);
    let export = ExportQuery { selector: query.step.0, first: query.export_first, second: query.export_second };
    let exported = ports.export_word(query.channel, export);
    let adjusted = ports.adjust_direction(exported, 1, query.direction_mode);
    let mut excess = query.level.wrapping_sub(adjusted).max(0) as u16;
    let spacing = half(area, 0x3e2643u32.wrapping_add(2 * row.0));
    let (index, span) = if spacing == 0 {
        (0, 0)
    } else {
        let index = ((divide(i32::from(excess as i16), i32::from(spacing as i16)) + 1) as u32 >> 1).min(10);
        if index == 0 {
            (0, spacing)
        } else {
            excess = excess.wrapping_sub((spacing as i16).wrapping_mul((2 * index - 1) as i16) as u16);
            (index, spacing << 1)
        }
    };
    let at = index.wrapping_add(11u32.wrapping_mul(row.0));
    let low = i32::from(signed_half(area, 0x3f3d00u32.wrapping_add(2 * at)));
    let high = i32::from(signed_half(area, 0x3f3d02u32.wrapping_add(2 * at)));
    let word = i32::from(table::word(area, query.step, row));
    let base = i32::from(table::word(area, Step(0), row));
    let between = if excess as i16 <= 0 && index == 0 {
        base
    } else if index > 9 || span == 0 {
        low
    } else {
        low + divide(i32::from(excess as i16) * (high - low), i32::from(span as i16))
    };
    let compensated = i32::from(table::compensated_word(area, query.step, row));
    (word - base) + between + compensated * 10
}

/// The lens range the stock lens table gives for `query` (0x74c568).
pub fn lens_range(ports: &mut impl Derivations, query: RangeQuery) -> u32 {
    let [a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13] = query.0;
    let table = ports.lens_table(a2, a4, a12);
    ports.lens_limits(table, [a3, a0, a1, a5, a6, a7, a10, a8, a11, a9, a13])
}
