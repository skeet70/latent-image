#![forbid(unsafe_code)]

//! Per-channel adjustment words and the classes the channel level reads.

use crate::adjustment::{byte, half, signed_byte, triple, Adjustments};
use crate::numeric::{divide, divide_unsigned, round_tenths};

const COUNT: u32 = 0x3e0f80;
const BASES: u32 = 0x3e21fd;
const PACKED: u32 = 0x3e2007;
const PACKED_SCALE: (u32, u32) = (0x9287, 0x9285);
const STEPS: u32 = 0x3e0572;
const BIASES: u32 = 0x3e0317;
const ALTERNATE_BIASES: u32 = 0x3e03e0;
const BIAS_FACTOR: u32 = 0x1fc;
const DIVISORS: u32 = 0x3e1b00;
const BIAS_SCALE: u32 = 0x3e2640;
const FIRST_CLASS_BOUNDS: [u32; 3] = [0x3e1ab1, 0x3e1ab2, 0x3e1ab3];
const SECOND_CLASS_BOUNDS: [u32; 3] = [0x3e1ab5, 0x3e1ab7, 0x3e1ab9];
const CLASS_OFFSETS: u32 = 0x3e1aa0;

/// A channel number as callers pass it; negative and too large channels read as absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel(pub i32);

/// The bias kind that reads the alternate bias words.
pub const ALTERNATE_BIAS: u32 = 254;

/// The configured channel count: the adjustment byte, 1 for 0, at most 100 (0x747b50).
pub fn count(area: &impl Adjustments) -> u32 {
    match byte(area, COUNT) {
        0 => 1,
        count => u32::from(count.min(100)),
    }
}

fn index(area: &impl Adjustments, channel: Channel) -> Option<u32> {
    u32::try_from(channel.0).ok().filter(|&index| index < count(area))
}

/// Whether the channel is below the configured count (0x74a214).
pub fn is_valid(area: &impl Adjustments, channel: Channel) -> bool { index(area, channel).is_some() }

/// The channel's base halfword times ten, 0 for an absent channel (0x747bf8).
pub fn base(area: &impl Adjustments, channel: Channel) -> u32 {
    index(area, channel).map_or(0, |index| u32::from(half(area, BASES + 2 * index)) * 10)
}

/// The channel's three-byte packed value, 0xffff for an absent channel (0x747d48).
pub fn packed(area: &impl Adjustments, channel: Channel) -> u32 {
    index(area, channel).map_or(0xffff, |index| triple(area, PACKED + 3 * index))
}

/// The packed value times the scale numerator over its denominator, unscaled while the
/// denominator is 0 (0x747d8c).
pub fn scaled_packed(area: &impl Adjustments, channel: Channel) -> u32 {
    let packed = packed(area, channel);
    let (numerator, denominator) = (half(area, PACKED_SCALE.0), half(area, PACKED_SCALE.1));
    if denominator == 0 {
        packed
    } else {
        divide_unsigned(packed.wrapping_mul(u32::from(numerator)), u32::from(denominator))
    }
}

/// The channel's bias: its bias word, plus -10^6 times a factor over the scale and the channel's
/// divisor, divided by ten, when both are nonzero (0x747374).
pub fn bias(area: &impl Adjustments, channel: Channel, kind: u32) -> i16 {
    let at = channel.0 as u32;
    let word = if kind == ALTERNATE_BIAS { ALTERNATE_BIASES.wrapping_add(2 * at) } else { BIASES.wrapping_add(2 * at) };
    let word = half(area, word);
    let divisor = i32::from(half(area, DIVISORS.wrapping_add(22u32.wrapping_mul(at))));
    let scale = i32::from(half(area, BIAS_SCALE));
    let correction = if scale == 0 || divisor == 0 {
        0
    } else {
        let factor = i32::from(signed_byte(area, BIAS_FACTOR));
        round_tenths(divide(1_000_000i32.wrapping_mul(factor), scale.wrapping_mul(divisor)).wrapping_neg()) as u16
    };
    word.wrapping_add(correction) as i16
}

/// The channel's step word plus its bias (0x7479c8).
pub fn biased_step(area: &impl Adjustments, channel: Channel, kind: u32) -> i16 {
    let step = half(area, STEPS.wrapping_add(2 * channel.0 as u32));
    step.wrapping_add(bias(area, channel, kind) as u16) as i16
}

/// Which of the four ranges the channel number plus one falls in under the three bounds
/// (0x747234).
pub fn channel_class(area: &impl Adjustments, channel: Channel) -> u32 {
    let position = channel.0.wrapping_add(1);
    let bounds = FIRST_CLASS_BOUNDS.map(|at| i32::from(byte(area, at)));
    match position {
        position if position <= bounds[0] => 0,
        position if position <= bounds[1] => 1,
        position if position > bounds[2] => 3,
        _ => 2,
    }
}

/// Which of the four ranges `value` falls in under the three halfword bounds (0x747290).
pub fn value_class(area: &impl Adjustments, value: u32) -> u32 {
    let bounds = SECOND_CLASS_BOUNDS.map(|at| u32::from(half(area, at)));
    match value {
        value if value <= bounds[0] => 0,
        value if value <= bounds[1] => 1,
        value if value > bounds[2] => 3,
        _ => 2,
    }
}

/// The runtime's logarithm (0x894778).
pub trait FloatLog {
    fn ln(&mut self, value: f32) -> f32;
}

/// The channel's level: one hundred times log2 of its base over 100, truncated, less the class
/// offset when `offset` is set; 500 for a channel without a base (0x747c40).
pub fn level(area: &impl Adjustments, log: &mut impl FloatLog, channel: Channel, value: u32, offset: bool) -> i16 {
    let base = base(area, channel);
    if base == 0 {
        return 500;
    }
    let offset = if offset {
        let at = CLASS_OFFSETS + 4 * channel_class(area, channel) + value_class(area, value);
        i32::from(byte(area, at)) * 10
    } else {
        0
    };
    let ratio = log.ln(100.0 / base as f32) / log.ln(2.0);
    let level = (-ratio * 100.0) as i32;
    level.wrapping_sub(offset) as i16
}

/// The first of `count - 1` adjustment bytes from `table` that reaches `value`, or the last
/// index searched; 0 when `count` is at most 1 (0x747304).
pub fn first_reaching(area: &impl Adjustments, value: i32, table: u32, count: i32) -> u32 {
    let mut index: u32 = 0;
    if count <= 1 {
        return 0;
    }
    loop {
        if i32::from(byte(area, table.wrapping_add(index))) >= value {
            return index;
        }
        index = (index + 1) & 0xff;
        if (index as i32) + 1 >= count {
            return index;
        }
    }
}
