//! Where a contrast curve peaks between its sampled lens positions: a three-point parabola, a
//! slope intersection for sharp peaks, and with the five-point setting on a parabola through the
//! logarithms of the samples.

use crate::exposure::ports::Properties;
use crate::stock::rate_curve;

/// Five samples of a contrast curve, the middle one the best so far.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeakCurve {
    /// 3 fits a parabola through the logarithms of the middle three samples, 4 a least-squares
    /// parabola through four of them; any other count keeps the sampled position or fits none.
    pub points: u8,
    pub values: [i32; 5],
    pub positions: [i32; 5],
}

impl PeakCurve {
    /// Three samples with the outer ones repeated, as the search builds its curve (0xf3e63c).
    pub fn from_three(values: [i32; 3], positions: [i16; 3]) -> Self {
        let [low, middle, high] = values;
        let [first, centre, last] = positions.map(i32::from);
        Self { points: 3, values: [low, low, middle, high, high], positions: [first, first, centre, last, last] }
    }

    const fn middle(&self) -> i16 { self.positions[2] as i16 }
}

/// Logarithms in hundredths of a binary octave.
pub trait Logarithms {
    /// `100 * log2(value)`; `None` for 0 (0xee97f0).
    fn centi_log2(&mut self, value: u64) -> Option<u32>;
}

/// Division truncating toward zero; 0 for a zero divisor.
const fn divide(dividend: i32, divisor: i32) -> i32 { if divisor == 0 { 0 } else { dividend.wrapping_div(divisor) } }

const fn divide_wide(dividend: i64, divisor: i64) -> i64 { if divisor == 0 { 0 } else { dividend.wrapping_div(divisor) } }

/// Tenths rounded half away from zero (0x741fa0).
const fn round_tenths(value: i32) -> i32 {
    let quotient = value / 10;
    match value % 10 {
        rest if rest > 4 => quotient + 1,
        rest if rest < -4 => quotient - 1,
        _ => quotient,
    }
}

/// A position in tenths rounded to whole units, half away from zero.
fn whole(tenths: i32) -> i32 {
    let rounded = if tenths.wrapping_abs() % 10 > 4 { tenths.wrapping_add(if tenths < 0 { -10 } else { 10 }) } else { tenths };
    rounded / 10
}

/// How sharply the middle three samples bend, relative to the peak value (0xf8cae8).
fn sharpness(curve: &PeakCurve) -> u32 {
    let [_, first, centre, last, _] = curve.positions;
    let [_, low, peak, high, _] = curve.values;
    if centre == first || centre == last || peak == 0 {
        return 0;
    }
    let rise = divide(peak.wrapping_sub(low), centre.wrapping_sub(first));
    let fall = divide(high.wrapping_sub(peak), last.wrapping_sub(centre));
    let bend = divide(rise.wrapping_sub(fall).wrapping_mul(10), last.wrapping_sub(first));
    let scaled = i64::from(bend)
        .wrapping_mul(100)
        .wrapping_mul(i64::from(centre.wrapping_sub(first).wrapping_abs()))
        .wrapping_mul(i64::from(centre.wrapping_sub(last).wrapping_abs()));
    round_tenths(divide_wide(scaled, i64::from(peak)) as i32).wrapping_abs() as u32
}

/// Whether the curve bends at least the configured minimum (0xf8cbd0).
fn sharp_enough(properties: &mut impl Properties, curve: &PeakCurve) -> bool { sharpness(curve) >= properties.property(rate_curve::FIT_SHARPNESS) }

/// The vertex of the parabola through three points, in tenths; `None` when they are collinear.
fn vertex_tenths(x: [i64; 3], y: [i64; 3]) -> Option<i64> {
    let differences = [y[1].wrapping_sub(y[2]), y[2].wrapping_sub(y[0]), y[0].wrapping_sub(y[1])];
    let denominator = (0..3).fold(0i64, |sum, index| sum.wrapping_add(x[index].wrapping_mul(differences[index]))).wrapping_mul(2);
    if denominator == 0 {
        return None;
    }
    let numerator = (0..3).fold(0i64, |sum, index| sum.wrapping_add(x[index].wrapping_mul(x[index]).wrapping_mul(differences[index])));
    Some(divide_wide(numerator.wrapping_mul(10), denominator))
}

/// Where the two slopes through the middle three samples meet (0xf8dd3c).
fn slope_crossing(curve: &PeakCurve) -> i16 {
    let [_, first, centre, last, _] = curve.positions;
    let [_, low, peak, high, _] = curve.values;
    let left = i64::from(centre.wrapping_sub(first).wrapping_abs() as u16);
    let right = i64::from(last.wrapping_sub(centre).wrapping_abs() as u16);
    let rise = peak.wrapping_sub(low);
    let drop = peak.wrapping_sub(high);
    let numerator = i64::from(rise).wrapping_mul(right).wrapping_sub(i64::from(drop).wrapping_mul(left));
    let steeper = if (rise < 0) == (numerator > 0) { drop } else { rise };
    let denominator = i64::from(steeper).wrapping_mul(2);
    if denominator == 0 {
        return centre as i16;
    }
    let mut tenths = divide_wide(numerator.wrapping_mul(10), denominator) as i32;
    if tenths.wrapping_abs() % 10 > 4 {
        tenths = tenths.wrapping_add(if tenths < 0 { -10 } else { 10 });
    }
    if first > last {
        tenths = tenths.wrapping_neg();
    }
    centre.wrapping_add(tenths / 10) as i16
}

/// The parabola through the middle three samples, or their slope crossing for a sharp or
/// collinear peak (0xf8deb0).
fn three_point(properties: &mut impl Properties, curve: &PeakCurve) -> i16 {
    if sharpness(curve) >= properties.property(rate_curve::PARABOLA_SHARPNESS) {
        return slope_crossing(curve);
    }
    let x = [1, 2, 3].map(|index| i64::from(curve.positions[index]));
    let y = [1, 2, 3].map(|index| i64::from(curve.values[index]));
    match vertex_tenths(x, y) {
        Some(tenths) => whole(tenths as i32) as i16,
        None => slope_crossing(curve),
    }
}

/// Positions rescaled so the widest step between the fitted samples is about 500, and the scale
/// in hundredths (0xf8a390).
fn normalized(curve: &PeakCurve, points: u8) -> ([i32; 5], i32) {
    let (start, end) = match points {
        4 if curve.values[1] > curve.values[3] => (0, 3),
        4 => (1, 4),
        0 | 1 => return (curve.positions, 100),
        points => (1, usize::from(points)),
    };
    let widest = (start..end).map(|index| curve.positions[index].wrapping_sub(curve.positions[index + 1]).wrapping_abs()).fold(i32::MIN, i32::max);
    if widest <= 500 {
        return (curve.positions, 100);
    }
    let scale = divide(50_000, widest);
    (curve.positions.map(|position| scale.wrapping_mul(position) / 100), scale)
}

/// The fitted position in tenths of normalized units, back in whole positions.
fn rescaled(tenths: i32, scale: i32) -> i16 { whole(divide(tenths.wrapping_mul(100), scale)) as i16 }

/// Least-squares parabola through four samples' logarithms; the vertex in tenths.
fn least_squares_tenths(x: [i64; 4], y: [i64; 4]) -> Option<i64> {
    let count = x.len() as i64;
    let mean_x = divide_wide(x.iter().fold(0i64, |sum, value| sum.wrapping_add(*value)), count);
    let mean_y = divide_wide(y.iter().fold(0i64, |sum, value| sum.wrapping_add(*value)), count);
    let mut sums = [0i64; 7];
    for (x, y) in x.into_iter().zip(y) {
        let x = x.wrapping_sub(mean_x);
        let y = y.wrapping_sub(mean_y);
        let square = x.wrapping_mul(x);
        for (sum, term) in sums.iter_mut().zip([x, y, x.wrapping_mul(y), square, square.wrapping_mul(y), x.wrapping_mul(square), square.wrapping_mul(square)]) {
            *sum = sum.wrapping_add(term);
        }
    }
    let [sx, sy, sxy, sx2, sx2y, sx3, sx4] = sums;
    let sx_sx2 = sx.wrapping_mul(sx2);
    let n_sx3 = count.wrapping_mul(sx3);
    let denominator = sx.wrapping_mul(sx).wrapping_sub(count.wrapping_mul(sx2)).wrapping_mul(sx2y)
        .wrapping_sub(sx_sx2.wrapping_mul(sxy))
        .wrapping_add(sx2.wrapping_mul(sx2).wrapping_sub(sx.wrapping_mul(sx3)).wrapping_mul(sy))
        .wrapping_add(n_sx3.wrapping_mul(sxy))
        .wrapping_mul(2);
    if denominator == 0 {
        return None;
    }
    let numerator = sx_sx2.wrapping_sub(n_sx3).wrapping_mul(sx2y)
        .wrapping_add(count.wrapping_mul(sx4).wrapping_mul(sxy))
        .wrapping_add(sx2.wrapping_mul(sx3).wrapping_mul(sy).wrapping_sub(sxy.wrapping_mul(sx2.wrapping_mul(sx2))))
        .wrapping_sub(sx.wrapping_mul(sx4).wrapping_mul(sy));
    Some(divide_wide(numerator.wrapping_mul(10), denominator).wrapping_add(mean_x.wrapping_mul(10)))
}

/// A parabola through the samples' logarithms (0xf8e0c0).
fn logarithmic(ports: &mut (impl Properties + Logarithms), curve: &PeakCurve) -> i16 {
    if curve.values.iter().any(|value| *value < 0) {
        return three_point(ports, curve);
    }
    let logs = curve.values.map(|value| i64::from(ports.centi_log2(value as u64).unwrap_or(0)));
    if curve.points == 3 || sharp_enough(ports, curve) {
        let (positions, scale) = normalized(curve, 3);
        let x = [1, 2, 3].map(|index| i64::from(positions[index]));
        let y = [logs[1], logs[2], logs[3]];
        return match vertex_tenths(x, y) {
            Some(tenths) => rescaled(tenths as i32, scale),
            None => three_point(ports, curve),
        };
    }
    if curve.points != 4 {
        return 0;
    }
    let (positions, scale) = normalized(curve, 4);
    let first = if curve.values[1] > curve.values[3] { 0 } else { 1 };
    let x: [i64; 4] = core::array::from_fn(|index| i64::from(positions[first + index]));
    let y: [i64; 4] = core::array::from_fn(|index| logs[first + index]);
    match least_squares_tenths(x, y) {
        Some(tenths) => rescaled(tenths as i32, scale),
        None => three_point(ports, curve),
    }
}

/// Positions strictly increasing or decreasing through the middle, the outer pairs allowed to
/// repeat.
fn monotone(positions: &[i32; 5]) -> bool {
    let [a, b, c, d, e] = *positions;
    (a <= b && b < c && c < d && d <= e) || (a >= b && b > c && c > d && d >= e)
}

/// Where the curve peaks: the middle sample's position unless it is the highest or lowest sample
/// and the positions run one way (0xf8e9e4).
pub fn peak_position(ports: &mut (impl Properties + Logarithms), curve: &PeakCurve) -> i16 {
    let peak = curve.values[2];
    let others = [curve.values[0], curve.values[1], curve.values[3], curve.values[4]];
    if !(others.iter().all(|value| peak >= *value) || others.iter().all(|value| peak <= *value)) {
        return curve.middle();
    }
    if ports.property(rate_curve::FIVE_POINT_FIT) == 0 {
        let [_, first, centre, last, _] = curve.positions;
        return if (first < centre && centre < last) || (first > centre && centre > last) { three_point(ports, curve) } else { curve.middle() };
    }
    if curve.points == 0 || !monotone(&curve.positions) {
        return curve.middle();
    }
    logarithmic(ports, curve)
}
