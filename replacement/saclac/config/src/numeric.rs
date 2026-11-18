#![forbid(unsafe_code)]

//! Integer conversions the configuration words share, and the runtime's logarithm.

/// The runtime's single-precision natural logarithm (0x894778).
pub trait FloatLog {
    fn ln(&mut self, value: f32) -> f32;
}

/// Divided by ten, halves away from zero (0x741fa0).
pub const fn round_tenths(value: i32) -> i32 {
    let quotient = value / 10;
    match value % 10 {
        remainder if remainder > 4 => quotient + 1,
        remainder if remainder < -4 => quotient - 1,
        _ => quotient,
    }
}

/// Divided by ten, halves toward positive infinity (0xda1900).
pub const fn round_tenths_up(value: i32) -> i32 {
    let quotient = value / 10;
    match value % 10 {
        remainder if remainder > 4 => quotient + 1,
        remainder if remainder < -5 => quotient - 1,
        _ => quotient,
    }
}

/// Truncating division. A zero denominator gives 0; stock's runtime faults there (svc 0x1010 at
/// 0x43604).
pub const fn divide(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 { 0 } else { numerator.wrapping_div(denominator) }
}

/// Unsigned truncating division, 0 for a zero denominator as `divide`.
pub const fn divide_unsigned(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 { 0 } else { numerator / denominator }
}
