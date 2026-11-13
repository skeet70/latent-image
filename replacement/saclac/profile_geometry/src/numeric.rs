#![forbid(unsafe_code)]

//! Base-2 logarithms in hundredths from the image's fraction tables, and integer division
//! helpers.

/// The fractional hundredths of log2 the image tabulates.
pub trait FractionTables {
    /// Hundredths of log2(1 + index / 256) for `index` in 0..256.
    fn fraction(&self, index: u8) -> u8;
    /// Hundredths of log2(1 + index / 100) for `index` in 0..100.
    fn wide_fraction(&self, index: u8) -> u8;
}

/// The runtime's single-precision natural logarithm (0x894778).
pub trait FloatLog {
    fn ln(&mut self, value: f32) -> f32;
}

/// log2 of `value` in hundredths (0xee98a0), or `None` for zero (status 0x84).
pub fn centi_log2(tables: &impl FractionTables, value: u32) -> Option<u16> {
    if value == 0 {
        return None;
    }
    let exponent = 31 - value.leading_zeros();
    let mantissa = if exponent > 8 { value >> (exponent - 8) } else { value << (8 - exponent) };
    let fraction = tables.fraction((mantissa - 256) as u8);
    Some(fraction as u16 + exponent as u16 * 100)
}

/// log2 of a 64-bit `value` in hundredths (0xee97f0), or `None` for zero.
pub fn centi_log2_wide(tables: &impl FractionTables, value: u64) -> Option<u32> {
    if value == 0 {
        return None;
    }
    let exponent = 63 - value.leading_zeros();
    let scaled = value.wrapping_mul(100) / (1u64 << exponent);
    let fraction = tables.wide_fraction(scaled.wrapping_sub(100) as u8);
    Some(exponent * 100 + fraction as u32)
}

/// `centi_log2(value) * 100 / 332` (0xee9928). Stock leaves its result undefined for zero.
pub fn scaled_log2(tables: &impl FractionTables, value: u32) -> Option<u32> {
    centi_log2(tables, value).map(|log| log as u32 * 100 / 332)
}

/// log2(first) - log2(second) in hundredths (0xee99f0).
pub fn log2_difference(tables: &impl FractionTables, first: u32, second: u32) -> i16 {
    match (centi_log2(tables, first), centi_log2(tables, second)) {
        (None, _) => -1000,
        (Some(first), Some(second)) => first.wrapping_sub(second) as i16,
        (Some(_), None) => 1000,
    }
}

/// log2(value) - offset in hundredths (0xee9a78).
pub fn log2_offset(tables: &impl FractionTables, value: u32, offset: i16) -> i16 {
    match centi_log2(tables, value) {
        Some(log) => log.wrapping_sub(offset as u16) as i16,
        None => -1000,
    }
}

/// log10 of `value` (0xee996c).
pub fn log10(log: &mut impl FloatLog, value: f32) -> f32 {
    let numerator = log.ln(value);
    numerator / log.ln(10.0)
}

/// `numerator / denominator` rounded away from zero (0xee9ce8).
pub fn divide_away_from_zero(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        return 0;
    }
    let quotient = numerator.wrapping_div(denominator);
    if numerator.wrapping_rem(denominator) == 0 {
        quotient
    } else if (numerator < 0) != (denominator < 0) {
        quotient.wrapping_sub(1)
    } else {
        quotient.wrapping_add(1)
    }
}

/// The runtime's unsigned 64-bit quotient (0x89bed0).
pub const fn runtime_quotient(numerator: u64, divisor: u64) -> u64 {
    match numerator.checked_div(divisor) {
        Some(quotient) => quotient,
        None if numerator == 0 => 0,
        None => u64::MAX,
    }
}
