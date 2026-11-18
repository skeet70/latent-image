#![forbid(unsafe_code)]

//! Row curves in the adjustment area: six signed levels per row (0x3f0000) and, per row and
//! level, signed points for eleven indices; points 0..=3 sit at 0x3f04b0, points 4..=10 at
//! 0x3f0fa0 for even levels and are interpolated between neighbours for odd ones.

use crate::adjustment::{byte, signed_byte, signed_half, Adjustments};
use crate::numeric::{divide, round_tenths};

const LEVELS: u32 = 0x3f0000;
const EARLY_POINTS: u32 = 0x3f04b0;
const LATE_POINTS: u32 = 0x3f0fa0;
const SCALE: u32 = 0x3e2712;
const LAST_INDEX: u32 = 10;

/// The row's levels, led by 0.
fn levels(area: &impl Adjustments, row: u32) -> [i16; 7] {
    let first = LEVELS.wrapping_add(12u32.wrapping_mul(row));
    let mut levels = [0; 7];
    for (slot, at) in levels[1..].iter_mut().zip((0..6).map(|level| first.wrapping_add(2 * level))) {
        *slot = signed_half(area, at);
    }
    levels
}

/// The point at `index` of level `level` (0x74665c).
fn point(area: &impl Adjustments, row: u32, index: u32, level: usize, levels: &[i16; 7]) -> i32 {
    let late = |level: usize| {
        LATE_POINTS.wrapping_add(28u32.wrapping_mul(row)).wrapping_add(index - 4).wrapping_add(7 * level as u32 / 2)
    };
    if index > LAST_INDEX {
        0
    } else if index <= 3 {
        let at = EARLY_POINTS.wrapping_add(4u32.wrapping_mul(7u32.wrapping_mul(row).wrapping_add(level as u32))).wrapping_add(index);
        i32::from(signed_byte(area, at))
    } else if level % 2 == 0 {
        i32::from(signed_byte(area, late(level)))
    } else {
        let low = i32::from(signed_byte(area, late(level - 1)));
        let high = i32::from(signed_byte(area, late(level + 1)));
        let (low_at, high_at) = (i32::from(levels[level - 1]), i32::from(levels[level + 1]));
        if low_at == high_at {
            low
        } else {
            let between = divide((i32::from(levels[level]) - low_at) * (high - low) * 10, high_at - low_at);
            i32::from(round_tenths(between + low * 10) as i8)
        }
    }
}

/// The row's curve at `level`, summing the points from `index` to the last and scaling by the
/// adjustment scale byte, interpolated between the bracketing levels and divided by ten; `level`
/// is first clamped to the row's levels (0x747e70).
pub fn curve_value(area: &impl Adjustments, row: u32, index: u32, level: i32) -> i16 {
    let levels = levels(area, row);
    let level = level.max(i32::from(levels[0]));
    let level = if level >= i32::from(levels[6]) { i32::from(levels[6]) } else { level };
    let upper = if level <= i32::from(levels[1]) {
        1
    } else {
        (2..7).find(|&upper| i32::from(levels[upper]) >= level).unwrap_or(6)
    };
    let lower = upper - 1;
    if levels[upper] == levels[lower] {
        return 0;
    }
    let (low_sum, high_sum) = (index..=LAST_INDEX).fold((0i32, 0i32), |(low, high), index| {
        (low + point(area, row, index, lower, &levels), high + point(area, row, index, upper, &levels))
    });
    let scale = i32::from(byte(area, SCALE));
    let (low_sum, high_sum) = (low_sum.wrapping_mul(scale), high_sum.wrapping_mul(scale));
    let numerator = (level - i32::from(levels[lower])).wrapping_mul(high_sum - low_sum).wrapping_mul(10);
    let between = divide(numerator, i32::from(levels[upper]) - i32::from(levels[lower]));
    round_tenths(between.wrapping_add(low_sum.wrapping_mul(10))) as i16
}
