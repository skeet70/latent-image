//! X-M1 1.50 table exporters and their fixed 4896 by 3264 radial search CPU.
//! Actual image-4 instructions resolve the pinned NONMATCHING reconstructions.

use crate::backend::{BackendError, ConfigImages, ConfigSpan};

const CHANNELS: usize = 15;
const SAMPLES: usize = 31;
const COPIES: [u32; 6] = [0x401200, 0x4017eb, 0x401dd6, 0x402400, 0x4029eb, 0x402fd6];

#[derive(Clone, Copy)]
#[repr(u32)]
enum Direction { Ascending = 0, Descending = 1 }
#[derive(Clone)]
#[repr(C)]
pub struct ChannelSetup {
    descending: [Direction; CHANNELS],
    knots: [[u32; SAMPLES]; CHANNELS],
}
impl ChannelSetup {
    pub const fn new() -> Self { Self { descending: [Direction::Ascending; CHANNELS], knots: [[0; SAMPLES]; CHANNELS] } }
    pub fn clear(&mut self) { *self = Self::new(); }
    pub fn set_channel(&mut self, index: usize, knots: &[u32; SAMPLES], descending: bool) {
        self.knots[index] = *knots;
        self.descending[index] = if descending { Direction::Descending } else { Direction::Ascending };
    }
}
impl Default for ChannelSetup { fn default() -> Self { Self::new() } }

#[derive(Clone, Copy)]
struct SearchOptions { attempts: u8, strength: u8, decrement: u8, crop: bool }
#[derive(Clone, Copy)]
struct Table { gains: [u32; 2], coefficients: [u16; 2], samples: [u32; SAMPLES] }
impl Table { const ZERO: Self = Self { gains: [0; 2], coefficients: [0; 2], samples: [0; SAMPLES] }; }
#[derive(Clone, Copy)]
struct Scratch { gains: [u32; 2], coefficients: [u16; 2] }
impl Scratch { const NEW: Self = Self { gains: [0x1000; 2], coefficients: [0; 2] }; }
#[derive(Clone, Copy)]
enum Axis { Horizontal, Vertical }
impl Axis { fn index(self) -> usize { match self { Self::Horizontal => 0, Self::Vertical => 1 } } }

fn byte(configuration: &ConfigImages<'_>, address: u32) -> Result<u8, BackendError> {
    configuration.byte(address).ok_or(BackendError::Capacity)
}
fn options(configuration: &ConfigImages<'_>) -> Result<SearchOptions, BackendError> {
    Ok(SearchOptions { attempts: byte(configuration, 0x18f41)?, strength: byte(configuration, 0x18f46)?, decrement: byte(configuration, 0x18f40)?, crop: byte(configuration, 0x18f44)? != 0 })
}
fn seed(configuration: &ConfigImages<'_>, address: u32) -> Result<[u32; 9], BackendError> {
    let mut result = [0x10000; 9];
    for (index, value) in result.iter_mut().enumerate().skip(1) {
        let key = address + (index as u32 - 1) * 2;
        let value16 = i16::from_le_bytes([byte(configuration, key)?, byte(configuration, key + 1)?]);
        *value = ((i64::from(value16) * 0x640000 + 0xfa0000000) / 0xfa000) as u32;
    }
    Ok(result)
}
fn expand(seed: &[u32; 9], strength: u8) -> [u32; SAMPLES] {
    let mut samples = [0u32; SAMPLES];
    for (index, value) in seed.iter().enumerate() {
        let product = value.wrapping_sub(0x10000).wrapping_shl(10).wrapping_mul(u32::from(strength)) as i32;
        samples[index * 3] = (product / 0x19000).wrapping_add(0x10000) as u32;
    }
    samples[27] = samples[24].wrapping_mul(2).wrapping_sub(samples[21]);
    samples[30] = samples[24].wrapping_mul(3).wrapping_sub(samples[21].wrapping_mul(2));
    samples[29] = samples[27].wrapping_add(samples[30].wrapping_mul(2)) / 3;
    for index in (0..27).step_by(3) { samples[index + 4] = samples[index + 3].wrapping_mul(4).wrapping_sub(samples[index]) / 3; }
    for index in (0..27).step_by(3) { samples[index + 2] = samples[index + 3].wrapping_mul(4).wrapping_sub(samples[index + 6]) / 3; }
    samples[23] = samples[21].wrapping_mul(5).wrapping_sub(samples[18].wrapping_mul(2)) / 3;
    samples[1] = samples[0];
    samples
}
fn divide(numerator: u32, denominator: u32) -> u32 { if denominator == 0 { 0 } else { numerator / denominator } }
fn coefficients(gain: u32, curvature: u32) -> Result<[u16; 2], BackendError> {
    let denominator = (4895u32.wrapping_mul(4895) >> 12).wrapping_add(3263u32.wrapping_mul(3263) >> 12);
    let scaled = gain.wrapping_mul(curvature) >> 13;
    let first = divide(0x800000, denominator);
    let second = divide((scaled.wrapping_mul(scaled) >> 14).wrapping_shl(7), denominator);
    if first >= 0x8000 || second >= 0x8000 { return Err(BackendError::Range); }
    Ok([first as u16, second as u16])
}
fn magnitude(value: u32) -> u32 { (value as i32).unsigned_abs() }
fn project(x: i32, y: i32, axis: Axis, scratch: &Scratch, samples: &[u32; SAMPLES]) -> i32 {
    let coordinates = [x.wrapping_mul(2), y.wrapping_mul(2)];
    let centers = [4895i32, 3263];
    let mut projected = [0u32; 2];
    let mut distance = [0u32; 2];
    for index in 0..2 {
        let scaled = (magnitude(coordinates[index].wrapping_sub(centers[index]) as u32).wrapping_mul(0x1000) >> 1).min(0xfffffff);
        let signed = if coordinates[index] < centers[index] { scaled.wrapping_neg() } else { scaled };
        projected[index] = signed.wrapping_add((centers[index] as u32).wrapping_shl(11));
        distance[index] = magnitude(projected[index].wrapping_sub((centers[index] as u32).wrapping_shl(11))).min(0xfffffff);
    }
    let radius_x = u64::from(distance[0]) >> 9;
    let radius_y = u64::from(distance[1]) >> 9;
    let radius = (radius_x.wrapping_mul(radius_x) >> 4).wrapping_add(radius_y.wrapping_mul(radius_y) >> 4);
    let index = axis.index();
    let parameter = (radius.wrapping_mul(u64::from(scratch.coefficients[index])) >> 19).min(0x13fff) as u32;
    let fraction = parameter & 0x1fff;
    let inverse = 0x2000 - fraction;
    let square = inverse.wrapping_mul(inverse) >> 10;
    let a = inverse.wrapping_mul(square) >> 13;
    let b = square.wrapping_mul(fraction.wrapping_mul(3)) >> 13;
    let c = (fraction.wrapping_mul(fraction) >> 10).wrapping_mul(inverse.wrapping_mul(3)) >> 13;
    let weights = [a, b, c, 0x10000u32.wrapping_sub(a).wrapping_sub(b).wrapping_sub(c)];
    let segment = (parameter >> 13) as usize * 3;
    let mut value = 0u32;
    for knot in 0..4 { value = value.wrapping_add((u64::from(samples[segment + knot]) * u64::from(weights[knot]) >> 12) as u32); }
    value = scratch.gains[index].wrapping_mul(value >> 4) >> 12;
    value = ((u64::from(value) * u64::from(distance[index])) >> 16).min(0x7ffffff) as u32;
    let source_center = (centers[index] as u32) << 11;
    if projected[index] as i32 <= source_center as i32 { source_center.wrapping_sub(value) as i32 } else { source_center.wrapping_add(value) as i32 }
}
fn round(value: i32, up: bool) -> i32 {
    let mut result = (value.unsigned_abs() as i32) >> 12;
    if value < 0 { result = result.wrapping_neg(); if !up && value & 0xfff != 0 { result = result.wrapping_sub(1); } }
    else if up && value > 0 && value & 0xfff != 0 { result = result.wrapping_add(1); }
    result
}
fn valid(scratch: &Scratch, samples: &[u32; SAMPLES], grid: u32) -> bool {
    let mut step = i32::MIN;
    for index in (3..31).step_by(3) { step = step.max(samples[index].wrapping_sub(samples[index - 3]) as i32); }
    let mut validation = *samples;
    for index in 0..24 { validation[index] = (samples[24] as i32).wrapping_sub(step.wrapping_mul(24 - index as i32) / 3) as u32; }
    for index in 25..31 { validation[index] = (samples[24] as i32).wrapping_add(step.wrapping_mul(index as i32 - 24) / 3) as u32; }
    for far in [false, true] {
        let (left, right, top, bottom) = if far { (2447 - grid as i32 / 2 - 1, 2448 + grid as i32 / 2 + 1, 1631 - 5, 1632 + 5) } else { (-1, grid as i32, -1, 8) };
        let points = [(left, top), (right, top), (left, bottom), (right, bottom)];
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let mut low = i32::MAX;
            let mut high = i32::MIN;
            for (index, &(x, y)) in points.iter().enumerate() {
                let up = match axis { Axis::Vertical => index >= 2, Axis::Horizontal => index & 1 != 0 };
                let value = round(project(x, y, axis, scratch, &validation), up);
                low = low.min(value); high = high.max(value);
            }
            let limit = match axis { Axis::Vertical => 32, Axis::Horizontal => 160 };
            if high.wrapping_sub(low) as u32 > limit { return false; }
        }
    }
    true
}
fn fit(samples: &[u32; SAMPLES], crop: bool) -> Result<Scratch, BackendError> {
    let mut scratch = Scratch::NEW;
    let first = 3264u32 * 3264 / ((4896u32 * 4896 + 3264u32 * 3264) / 24);
    let mut peak = first as usize;
    for index in first as usize + 1..25 { if samples[index] > samples[peak] { peak = index; } }
    if peak != first as usize && peak != 24 {
        let gain = divide(0x10000000, samples[peak]);
        scratch.gains = [gain; 2]; scratch.coefficients = coefficients(gain, samples[24])?;
    } else if peak == 24 && peak != first as usize || crop {
        scratch.coefficients[0] = coefficients(0x1000, samples[24])?[0];
        let (x, y, numerator) = if peak == 24 && peak != first as usize { (0, 0, 2448u32) } else { (816, 1632, 1632u32) };
        let horizontal = round(project(x, y, Axis::Horizontal, &scratch, samples), false);
        scratch.gains[0] = divide(numerator << 12, 2448u32.wrapping_sub(horizontal as u32));
        scratch.coefficients[1] = coefficients(scratch.gains[0], samples[24])?[1];
        let (x, y) = if peak == 24 && peak != first as usize { (0, 0) } else { (2448, 0) };
        let vertical = round(project(x, y, Axis::Vertical, &scratch, samples), false);
        scratch.gains[1] = divide(1632 << 12, 1632u32.wrapping_sub(vertical as u32));
    }
    Ok(scratch)
}
fn select(samples: &[u32; SAMPLES], crop: bool) -> Result<Option<Scratch>, BackendError> {
    let scratch = fit(samples, crop)?;
    for grid in [64, 32, 16] { if valid(&scratch, samples, grid) { return Ok(Some(scratch)); } }
    Ok(None)
}
fn search(seed: &[u32; 9], options: SearchOptions, output: &mut Table) -> Result<bool, BackendError> {
    let attempts = if u32::from(options.attempts) * u32::from(options.decrement) > 100 { 100 / options.decrement + 1 } else { options.attempts };
    for grid in [64, 32, 16] {
        let mut strength = options.strength;
        for _ in 0..attempts {
            let samples = expand(seed, strength);
            let scratch = fit(&samples, options.crop)?;
            if valid(&scratch, &samples, grid) { *output = Table { gains: scratch.gains, coefficients: scratch.coefficients, samples }; return Ok(strength != 0); }
            strength = strength.wrapping_sub(options.decrement);
        }
    }
    Ok(false)
}
fn publish(configuration: &mut ConfigImages<'_>, address: u32, table: &Table) -> Result<(), BackendError> {
    let mut output = [0u8; 101];
    for (index, value) in table.samples.iter().enumerate() { output[index * 3..index * 3 + 3].copy_from_slice(&value.to_le_bytes()[..3]); }
    for (index, value) in [table.gains[0] as u16, table.gains[1] as u16, table.coefficients[0], table.coefficients[1]].iter().enumerate() { output[93 + index * 2..95 + index * 2].copy_from_slice(&value.to_le_bytes()); }
    configuration.write(ConfigSpan::new(address, output.len() as u32)?, &output)
}
pub fn initialize_grid_tables(configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
    let options = options(configuration)?;
    let mut table = Table::ZERO;
    let mut success = true;
    for row in 0..45 {
        success &= search(&seed(configuration, 0x3e3a55 + row * 16)?, options, &mut table)?;
        publish(configuration, 0x400000 + row * 101, &table)?;
    }
    configuration.write(ConfigSpan::new(0x4011c1, 1)?, &[u8::from(success)])
}
pub fn initialize_registered_tables(configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
    if byte(configuration, 0x3e0009)? == 0 { return Ok(()); }
    let options = options(configuration)?;
    let mut table = Table::ZERO;
    let mut success = true;
    for row in 0..6 {
        success &= search(&seed(configuration, 0x33d0 + row * 16)?, options, &mut table)?;
        publish(configuration, 0x403600 + row * 101, &table)?;
    }
    configuration.write(ConfigSpan::new(0x4011c1, 1)?, &[u8::from(success)])
}
pub fn publish_channel_tables(configuration: &mut ConfigImages<'_>, setup: &ChannelSetup) -> Result<(), BackendError> {
    for channel in 0..CHANNELS {
        let samples = &setup.knots[channel];
        for (index, value) in samples.iter().enumerate() {
            for base in COPIES {
                configuration.write(ConfigSpan::new(base + channel as u32 * 101 + index as u32 * 3, 3)?, &value.to_le_bytes()[..3])?;
            }
        }
        let scratch = select(samples, byte(configuration, 0x18f44)? != 0)?;
        let metadata = match scratch { Some(scratch) => [scratch.gains[0] as u16, scratch.gains[1] as u16, scratch.coefficients[0], scratch.coefficients[1]], None => [0; 4] };
        for (index, value) in metadata.iter().enumerate() {
            for base in COPIES {
                configuration.write(ConfigSpan::new(base + channel as u32 * 101 + 93 + index as u32 * 2, 2)?, &value.to_le_bytes())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "startup_radial_tables_tests.rs"]
mod tests;
