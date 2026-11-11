//! Numerical operations used by the X-M1 1.50 calibration metadata producer.

use xm1_calibration_core::{round_tenths, Tenths};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationValue(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationScale(pub u16);
/// The logarithm domain is zero or a positive normal float.
fn calibration_log(value: f32) -> f32 {
    if value == 0.0 {
        return f32::NEG_INFINITY;
    }
    let bits = value.to_bits();
    let fraction = bits & 0x7fffff;
    let normalization = (fraction + 0x4afb20) & 0x800000;
    let exponent = ((bits >> 23) as i32 - 127) + (normalization >> 23) as i32;
    let reduced = f32::from_bits(fraction | (normalization ^ 0x3f800000)) - 1.0;
    let high_ln2 = f32::from_bits(0x3f317180);
    let low_ln2 = f32::from_bits(0x3717f7d1);
    let power = exponent as f32;
    if ((fraction + 15) & 0x7fffff) <= 15 {
        if reduced == 0.0 {
            return power * high_ln2 + power * low_ln2;
        }
        let residual = reduced * reduced * (0.5 - f32::from_bits(0x3eaaaaab) * reduced);
        return if exponent == 0 { reduced - residual }
        else { power * high_ln2 - ((residual - power * low_ln2) - reduced) };
    }
    let ratio = reduced / (2.0 + reduced);
    let square = ratio * ratio;
    let fourth = square * square;
    let odd = square * (f32::from_bits(0x3f2aaaab)
        + fourth * (f32::from_bits(0x3e924925)
        + fourth * (f32::from_bits(0x3e3a3325)
        + fourth * f32::from_bits(0x3e178897))));
    let even = fourth * (f32::from_bits(0x3ecccccd)
        + fourth * (f32::from_bits(0x3e638e29)
        + fourth * f32::from_bits(0x3e1cd04f)));
    let residual = odd + even;
    let near_boundary = (fraction as i32 - 0x30a3d0) | (0x35c288 - fraction as i32);
    if near_boundary > 0 {
        let half_square = 0.5 * reduced * reduced;
        if exponent == 0 {
            reduced - (half_square - ratio * (half_square + residual))
        } else {
            power * high_ln2 - ((half_square - (ratio * (half_square + residual) + power * low_ln2)) - reduced)
        }
    } else if exponent == 0 {
        reduced - ratio * (reduced - residual)
    } else {
        power * high_ln2 - ((ratio * (reduced - residual) - power * low_ln2) - reduced)
    }
}

pub fn transform_mapped_value(value: CalibrationValue) -> i16 {
    let scaled = value.0 as f32 / 100.0;
    let logarithm = calibration_log(scaled);
    let reference = f32::from_bits(0x3f317218);
    let tenths = ((logarithm / reference) * 2000.0) as i32;
    round_tenths(Tenths(tenths)).0 as i16
}

pub fn scale_transformed_word(value: CalibrationValue, scale: CalibrationScale) -> u16 {
    if value.0 == u16::MAX || scale.0 == u16::MAX {
        return u16::MAX;
    }
    let numerator = (value.0 as i32).wrapping_mul(scale.0 as i32)
        .wrapping_add(25).wrapping_shl(1);
    (numerator / 100) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logarithmic_units_include_reciprocals_and_octaves() {
        for (input, expected) in [(25, -400), (50, -200), (100, 0), (200, 200), (400, 400), (800, 600), (65535, 1871)] {
            assert_eq!(transform_mapped_value(CalibrationValue(input)), expected);
        }
        assert_eq!(transform_mapped_value(CalibrationValue(0)), 13107);
    }

    #[test]
    fn word_scaling_retains_wrapping_signed_arithmetic_and_sentinels() {
        for (value, scale, expected) in [(100, 100, 200), (1, 25, 1), (1, 24, 0), (65534, 65534, 60294), (65535, 1, 65535), (1, 65535, 65535)] {
            assert_eq!(scale_transformed_word(CalibrationValue(value), CalibrationScale(scale)), expected);
        }
    }
}
