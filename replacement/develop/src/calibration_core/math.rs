use super::{CurveValue, MappedWord, RoundedValue, Tenths};

pub fn round_tenths(value: Tenths) -> RoundedValue {
    let result = value.0 / 10;
    RoundedValue(match value.0 % 10 { 5..=9 => result + 1, -9..=-5 => result - 1, _ => result })
}

pub(super) fn transform_mapped_word(mapped: MappedWord) -> CurveValue {
    let tenths = (mapped_power_two(mapped) * 1000.0) as u32;
    CurveValue(round_tenths(Tenths(tenths as i32)).0 as u16)
}

/*
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 * Developed at SunPro, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this software is freely
 * granted, provided that this notice is preserved.
 */
fn mapped_power_two(mapped: MappedWord) -> f32 {
    let exponent = mapped.0 as f32 / 2000.0;
    match exponent.to_bits() {
        0 => return 1.0,
        0x3f800000 => return 2.0,
        0xbf800000 => return 0.5,
        0x40000000 => return 4.0,
        0x3f000000 => return f32::from_bits(0x3fb504f3),
        _ => {}
    }
    let mut high = f32::from_bits(exponent.to_bits() & 0xfffff000);
    let low = exponent - high;
    let bits = exponent.to_bits() as i32;
    let absolute = bits as u32 & 0x7fffffff;
    let mut integral = 0i32;
    if absolute > 0x3f000000 {
        let power = ((absolute >> 23) as i32) - 127;
        let rounded = bits.wrapping_add((0x800000u32 >> (power + 1)) as i32);
        let power = (((rounded as u32 & 0x7fffffff) >> 23) as i32) - 127;
        let integer_float = f32::from_bits(rounded as u32 & !(0x7fffffu32 >> power));
        integral = (((rounded as u32 & 0x7fffff) | 0x800000) >> (23 - power)) as i32;
        if bits < 0 { integral = -integral; }
        high -= integer_float;
    }
    let reduced = f32::from_bits((low + high).to_bits() & 0xfffff000);
    let upper = reduced * f32::from_bits(0x3f317200);
    let lower = (low - (reduced - high)) * f32::from_bits(0x3f317218) + reduced * f32::from_bits(0x35bfbe8c);
    let value = upper + lower;
    let correction = lower - (value - upper);
    let square = value * value;
    let polynomial = f32::from_bits(0x3e2aaaab) + square * (f32::from_bits(0xbb360b61) + square * (f32::from_bits(0x388ab355) + square * (f32::from_bits(0xb5ddea0e) + square * f32::from_bits(0x3331bb4c))));
    let denominator = value - square * polynomial;
    let remainder = (value * denominator) / (denominator - 2.0) - (correction + value * correction);
    let result = 1.0 - (remainder - value);
    f32::from_bits((result.to_bits() as i32).wrapping_add(integral.wrapping_shl(23)) as u32)
}
