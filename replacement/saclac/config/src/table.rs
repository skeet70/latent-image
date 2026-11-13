#![forbid(unsafe_code)]

//! The step table in the adjustment area. Each row holds 31 signed words (0x3f2400) and 31
//! value codes (0x3f4600) that select a halfword of 0x3e2530; code 255 is the value 100. A step
//! selects a column in one of two banks of equal size.

use crate::adjustment::{byte, half, signed_byte, signed_half, Adjustments};
use crate::numeric::{divide, round_tenths};

const COLUMNS: u32 = 31;
const BANK_SIZE: u32 = 0x3e0f82;
const WORDS: u32 = 0x3f2400;
const CODES: u32 = 0x3f4600;
const VALUES: u32 = 0x3e2530;
const DISABLED: u32 = 0x3e0009;
const FIRST_BANK_OFFSETS: u32 = 0x3e0200;
const SECOND_BANK_OFFSETS: u32 = 0x3e0220;
const POSITIVE_LIMITS: [u32; 2] = [0x1db3, 0x1db4];
const NEGATIVE_LIMITS: [u32; 2] = [0x1db5, 0x1db6];
const BOUNDS_SELECTOR: u32 = 0x3e4760;
const PRIMARY_BOUNDS: (u32, u32) = (0x3e22c8, 0x3e232d);
const ALTERNATE_BOUNDS: (u32, u32) = (0x3e47c6, 0x3e4761);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Row(pub u32);

/// A step as callers number it: the first bank from 0, the second from 31.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Step(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bank { First, Second, Outside }

/// The steps a bank holds: the adjustment byte when it is 1..=31, else 1 (0x74677c).
pub fn bank_size(area: &impl Adjustments) -> u32 {
    match byte(area, BANK_SIZE) {
        size @ 1..=31 => u32::from(size),
        _ => 1,
    }
}

/// 0x7467a4.
pub fn bank(area: &impl Adjustments, step: Step) -> Bank {
    let size = bank_size(area);
    if step.0 < size {
        Bank::First
    } else if step.0 >= COLUMNS && step.0 < size + COLUMNS {
        Bank::Second
    } else {
        Bank::Outside
    }
}

/// The step's column; a step outside both banks reads column 0 (0x746834).
pub fn column(area: &impl Adjustments, step: Step) -> u32 {
    match bank(area, step) {
        Bank::First => step.0,
        Bank::Second => step.0 - COLUMNS,
        Bank::Outside => 0,
    }
}

fn cell(area: &impl Adjustments, step: Step, row: Row) -> u32 { row.0.wrapping_mul(COLUMNS).wrapping_add(column(area, step)) }

/// 0x74690c.
pub fn word(area: &impl Adjustments, step: Step, row: Row) -> i16 {
    signed_half(area, WORDS.wrapping_add(2 * cell(area, step, row)))
}

/// The step's value (0x746864).
pub fn value(area: &impl Adjustments, step: Step, row: Row) -> u16 {
    match byte(area, CODES.wrapping_add(cell(area, step, row))) {
        255 => 100,
        code => half(area, VALUES + 2 * u32::from(code)),
    }
}

/// The step's word divided by ten, halves away from zero (0x746944).
pub fn rounded_word(area: &impl Adjustments, step: Step, row: Row) -> i16 {
    round_tenths(i32::from(word(area, step, row))) as i16
}

/// 0x7469a0.
pub fn primary_rounded_word(area: &impl Adjustments, step: Step) -> i16 { rounded_word(area, step, Row(0)) }

/// Row 0's word at step 0 subtracted from `row`'s, divided by ten as `rounded_word` (0x746c2c).
pub fn relative_word(area: &impl Adjustments, row: Row) -> i16 {
    let difference = i32::from(word(area, Step(0), row)) - i32::from(word(area, Step(0), Row(0)));
    round_tenths(difference) as i16
}

/// Thresholds, each paired with the signed adjustment byte at 0x1d71 plus its index (the table
/// at 0x818990).
const CORRECTION_THRESHOLDS: [i16; 36] = [
    -100, -60, -30, 0, 30, 60, 100, 130, 160, 200, 230, 260, 300, 330, 360, 400, 430, 460, 500, 530, 560, 600, 630, 660,
    700, 730, 760, 800, 830, 860, 900, 930, 960, 1000, 1030, 1060,
];
const CORRECTIONS: u32 = 0x1d71;

/// The correction at `rounded`, interpolated between the bracketing thresholds; below the first
/// threshold it is the first entry, from the last threshold on the last entry (0x7469b4).
pub fn correction(area: &impl Adjustments, rounded: i16) -> i16 {
    let at = |index: usize| (i32::from(CORRECTION_THRESHOLDS[index]), i32::from(signed_byte(area, CORRECTIONS + index as u32)));
    let rounded = i32::from(rounded);
    if rounded < i32::from(CORRECTION_THRESHOLDS[0]) {
        return at(0).1 as i16;
    }
    let Some(upper) = (1..CORRECTION_THRESHOLDS.len()).find(|&index| i32::from(CORRECTION_THRESHOLDS[index]) > rounded) else {
        return at(CORRECTION_THRESHOLDS.len() - 1).1 as i16;
    };
    let ((high_at, high), (low_at, low)) = (at(upper), at(upper - 1));
    (divide((rounded - low_at) * (high - low), high_at - low_at) + low) as i16
}

/// The step's bank offset plus the correction of its rounded word, kept within the adjustment
/// limits (bytes times ten); 0 while adjustment 0x3e0009 is 1 (0x746a80).
pub fn compensated_word(area: &impl Adjustments, step: Step, row: Row) -> i16 {
    if byte(area, DISABLED) == 1 {
        return 0;
    }
    let offsets = if bank(area, step) == Bank::Second { SECOND_BANK_OFFSETS } else { FIRST_BANK_OFFSETS };
    let offset = signed_byte(area, offsets + column(area, step)) as i16;
    let sum = offset.wrapping_add(correction(area, rounded_word(area, step, row)));
    let limit = |at: u32| i16::from(byte(area, at)) * 10;
    let within = |magnitude: i16, [highest, lowest]: [u32; 2]| {
        let magnitude = if magnitude >= limit(highest) { limit(highest) } else { magnitude };
        if magnitude <= limit(lowest) { limit(lowest) } else { magnitude }
    };
    if sum >= 0 {
        within(sum, POSITIVE_LIMITS)
    } else {
        within(sum.wrapping_neg(), NEGATIVE_LIMITS).wrapping_neg()
    }
}

/// Row 0's rounded word plus the compensated word (0x746bf8).
pub fn combined_word(area: &impl Adjustments, step: Step, row: Row) -> i16 {
    rounded_word(area, step, Row(0)).wrapping_add(compensated_word(area, step, row))
}

/// Which bounds bytes a search reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bounds {
    /// 0x3e22c8 and 0x3e232d, by row.
    Primary,
    /// The alternate bounds 0x3e47c6 and 0x3e4761 while adjustment 0x3e4760 is 1, else primary.
    Selected,
}

fn bounds(area: &impl Adjustments, row: Row, which: Bounds) -> (u32, u32) {
    let (low, high) = match which {
        Bounds::Selected if byte(area, BOUNDS_SELECTOR) == 1 => ALTERNATE_BOUNDS,
        _ => PRIMARY_BOUNDS,
    };
    (u32::from(byte(area, low.wrapping_add(row.0))), u32::from(byte(area, high.wrapping_add(row.0))))
}

/// The first step from the row's low bound whose value reaches `limit`, or the high bound;
/// the low bound when the bounds are empty (0x746c60, 0x746da0).
pub fn first_reaching(area: &impl Adjustments, row: Row, limit: u32, which: Bounds) -> Step {
    let (low, high) = bounds(area, row, which);
    if low >= high {
        return Step(low);
    }
    Step((low..high).find(|&step| u32::from(value(area, Step(step), row)) >= limit).unwrap_or(high))
}

/// The last step down from the row's high bound whose value is within `limit`, or the low
/// bound; the high bound when the bounds are empty (0x746cfc, 0x746f54).
pub fn last_within(area: &impl Adjustments, row: Row, limit: u32, which: Bounds) -> Step {
    let (low, high) = bounds(area, row, which);
    if low >= high {
        return Step(high);
    }
    Step(((low + 1)..=high).rev().find(|&step| u32::from(value(area, Step(step), row)) <= limit).unwrap_or(low))
}

/// The row's low bound, or with a limit the first step reaching it (0x746cc8 with
/// `Bounds::Primary`, 0x746e48 with `Bounds::Selected`).
pub fn lower_step(area: &impl Adjustments, row: Row, limit: Option<u32>, which: Bounds) -> Step {
    match limit {
        None => Step(bounds(area, row, which).0),
        Some(limit) => first_reaching(area, row, limit, which),
    }
}

/// The row's high bound, or with a limit the last step within it (0x746d6c with
/// `Bounds::Primary`, 0x746ffc with `Bounds::Selected`).
pub fn upper_step(area: &impl Adjustments, row: Row, limit: Option<u32>, which: Bounds) -> Step {
    match limit {
        None => Step(bounds(area, row, which).1),
        Some(limit) => last_within(area, row, limit, which),
    }
}

/// The step of `to` whose value is nearest the value `step` has in `from`, `step` first clamped
/// to `from`'s primary bounds; of two equally near steps the upper one. 0 while the table's
/// compensation is disabled (0x74714c).
pub fn nearest_step(area: &impl Adjustments, from: Row, step: Step, to: Row) -> Step {
    if byte(area, DISABLED) == 1 {
        return Step(0);
    }
    let (low, high) = bounds(area, from, Bounds::Primary);
    let clamped = column(area, step).max(low).min(high);
    let target = value(area, Step(clamped), from);
    let above = last_within(area, to, u32::from(target), Bounds::Primary);
    let below = first_reaching(area, to, u32::from(target), Bounds::Primary);
    if above == below {
        return above;
    }
    let distance = |step: Step| value(area, step, to).abs_diff(target);
    if distance(above) > distance(below) { below } else { above }
}
