//! Numeric startup geometry from the authenticated X-M1 1.50 main stream.

use super::backend::{BackendError, ConfigImages};

const WIDTH: u32 = 4896;
const HEIGHT: u32 = 3264;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RadialRecord {
    knots: [u32; 31],
    metadata: [u16; 8],
    valid: bool,
}

impl RadialRecord {
    pub const fn new() -> Self { Self { knots: [0; 31], metadata: [0; 8], valid: false } }
    pub fn clear(&mut self) { *self = Self::new(); }
    pub const fn knots(&self) -> &[u32; 31] { &self.knots }
    /// Halfwords at retained offsets 0x7c through 0x8a.
    pub const fn metadata(&self) -> &[u16; 8] { &self.metadata }
    pub const fn gain(&self) -> u16 { self.metadata[4] }
    pub const fn reciprocal(&self) -> u16 { self.metadata[5] }
    pub const fn valid(&self) -> bool { self.valid }
}

impl Default for RadialRecord { fn default() -> Self { Self::new() } }

#[derive(Clone, Copy, Eq, PartialEq)]
enum Aspect { ThreeTwo, SixteenNine, FourThree }

fn aspect(width: u32, height: u32) -> Aspect {
    if height.wrapping_mul(3) == width.wrapping_mul(2) { Aspect::ThreeTwo }
    else if width.wrapping_mul(9) == height.wrapping_mul(16) { Aspect::SixteenNine }
    else if width.wrapping_mul(3) == height.wrapping_mul(4) { Aspect::FourThree }
    else { Aspect::ThreeTwo }
}

fn divide(numerator: u32, denominator: u32) -> Result<u32, BackendError> {
    numerator.checked_div(denominator).ok_or(BackendError::Range)
}

fn seeds(samples: &[u16; 8], strength: i32) -> [u32; 9] {
    let mut result = [0x10000; 9];
    for (output, sample) in result[1..].iter_mut().zip(samples) {
        let product = (*sample as i16 as i64).wrapping_mul(strength as u32 as i64);
        *output = (product.wrapping_add(1024000).wrapping_shl(16) / 1024000) as u32;
    }
    result
}

fn expand(seed: &[u32; 9], output: &mut [u32; 31]) {
    for (index, value) in seed.iter().enumerate() { output[index * 3] = *value; }
    output[1] = output[0];
    let last = seed[8];
    let previous = seed[7];
    for (offset, multiplier) in [(25, 4u32), (26, 5), (28, 7), (29, 8)] {
        output[offset] = last.wrapping_mul(multiplier).wrapping_sub(previous.wrapping_mul(multiplier - 3)) / 3;
    }
    output[27] = last.wrapping_mul(2).wrapping_sub(previous);
    output[30] = last.wrapping_mul(3).wrapping_sub(previous.wrapping_mul(2));
    for index in (0..24).step_by(3) {
        output[index + 4] = output[index + 3].wrapping_mul(4).wrapping_sub(output[index]) / 3;
    }
    for index in (0..27).step_by(3) {
        output[index + 2] = output[index + 3].wrapping_mul(4).wrapping_sub(output[index + 6]) / 3;
    }
}

fn peak(knots: &[u32; 31], mode: Aspect) -> Result<Option<[(u32, u32); 2]>, BackendError> {
    let step = (HEIGHT * HEIGHT + WIDTH * WIDTH) / 48;
    let height = if mode == Aspect::SixteenNine { (WIDTH * 9 >> 4) & 0xffff } else { HEIGHT };
    let radius = height * height;
    let begin = (divide(radius, step)? >> 1) as usize;
    let end = ((divide(radius + WIDTH * WIDTH, step)? + 1) >> 1) as usize;
    let mut index = begin;
    for candidate in begin + 1..=end {
        if knots[candidate] > knots[index] { index = candidate; }
    }
    if index == begin { return Ok(Some([(0, height), (height, 0)])); }
    if index == end { return Ok(Some([(WIDTH, height), (WIDTH, height)])); }
    let square = (index as u32).wrapping_mul(step * 2);
    if radius > square { return Ok(None); }
    let square = square - radius;
    let mut root = square;
    for _ in 0..20 { root = divide(square, root)?.wrapping_add(root) >> 1; }
    let root = root & 0xffff;
    Ok(Some([(height, root), (root, height)]))
}

fn reciprocal_at(knots: &[u32; 31], point: (u32, u32), scale: u16) -> Result<u16, BackendError> {
    let (x, y) = point;
    let value = (scale as u32).wrapping_mul((x.wrapping_mul(x) >> 12).wrapping_add(y.wrapping_mul(y) >> 12));
    let fraction = (value >> 7) & 0x1fff;
    let index = ((value >> 20).wrapping_mul(3) & 255) as usize;
    let values = knots.get(index..index + 4).ok_or(BackendError::Range)?;
    let complement = 8192 - fraction;
    let square = complement.wrapping_mul(complement) >> 10;
    let first = complement.wrapping_mul(square) >> 13;
    let second = square.wrapping_mul(fraction.wrapping_mul(3)) >> 13;
    let third = (fraction.wrapping_mul(fraction) >> 10).wrapping_mul(complement.wrapping_mul(3)) >> 13;
    let fourth = 65536u32.wrapping_sub(third).wrapping_sub(first.wrapping_add(second));
    // X-M1 truncates the first three products to 32 bits before shifting.
    let sum = (values[0].wrapping_mul(first) >> 12)
        .wrapping_add(values[1].wrapping_mul(second) >> 12)
        .wrapping_add(values[2].wrapping_mul(third) >> 12)
        .wrapping_add(((values[3] as u64 * fourth as u64) >> 12) as u32);
    Ok(divide(0x10000000, sum >> 4)? as u16)
}

fn geometry(config: &ConfigImages<'_>, seed: &[u32; 9], mode: Aspect, record: &mut RadialRecord) -> Result<bool, BackendError> {
    let base = divide(0x800000, (((WIDTH - 1) * (WIDTH - 1) >> 12) + ((HEIGHT - 1) * (HEIGHT - 1) >> 12)) & 0xffff)? as u16;
    record.metadata[2] = base;
    let Some(points) = peak(&record.knots, mode)? else { return Ok(false); };
    let configured = config.byte(0x18f45).ok_or(BackendError::Capacity)? != 0;
    let enabled = seed.iter().any(|value| *value > 65536) || configured;
    let gain = if enabled { reciprocal_at(&record.knots, points[0], base)? } else { 4096 };
    record.metadata[4] = gain;
    let axis = if enabled {
        let horizontal = (WIDTH - 1).wrapping_mul(gain as u32) >> 12;
        divide(0x800000, ((HEIGHT - 1) * (HEIGHT - 1) >> 12).wrapping_add(horizontal.wrapping_mul(horizontal) >> 12))? as u16
    } else { base };
    record.metadata[3] = axis;
    record.metadata[5] = if enabled { reciprocal_at(&record.knots, points[1], axis)? } else { gain };
    Ok(true)
}

fn product_shift(left: u32, right: u64, shift: u32) -> u32 { ((left as u64).wrapping_mul(right) >> shift) as u32 }

fn grid(seed: &[u32; 9], target: (u32, u32), record: &RadialRecord) -> Result<u16, BackendError> {
    let (width, height) = target;
    if width == 0 || height == 0 { return Ok(4); }
    let jump = seed[5..].windows(2).map(|pair| (pair[0].wrapping_sub(pair[1]) as i32).unsigned_abs()).max().unwrap_or(0).wrapping_shl(3);
    let source_height = match aspect(width, height) {
        Aspect::ThreeTwo => HEIGHT,
        Aspect::SixteenNine => WIDTH * 9 >> 4,
        Aspect::FourThree => WIDTH * 3 >> 2,
    };
    let gain_x = divide(divide(WIDTH, record.metadata[0] as u32)?.wrapping_shl(12), width)?;
    let mut gain_y = divide(divide(source_height, record.metadata[1] as u32)?.wrapping_shl(12), height)?;
    if gain_y >= 16384 { gain_y >>= 2; } else if gain_y >= 8192 { gain_y >>= 1; }
    let rows = height.wrapping_mul(gain_y) >> 13;
    let scaled = record.metadata[0] as u64 * gain_x as u64;
    let cost = |value| {
        let value = product_shift(value, record.metadata[3] as u64, 17);
        let value = product_shift(value, jump as u64, 10);
        let value = product_shift(value, record.metadata[5] as u64, 12);
        value.wrapping_mul(rows) >> 10
    };
    let value = product_shift(width.wrapping_sub(128).wrapping_mul(128), scaled, 12);
    if cost(product_shift(value, scaled, 12)) <= 8192 { return Ok(4); }
    let mut candidate = 128u32;
    loop {
        candidate >>= 1;
        let value = product_shift(candidate.wrapping_mul(width.wrapping_sub(candidate)), scaled, 12);
        if cost(product_shift(value, scaled, 12)) <= 8192 { break; }
    }
    Ok(match candidate { 16 => 1, 32 => 2, 64 => 3, 128 => 4, _ => 0 })
}

#[derive(Clone, Copy)]
struct Channel { mode: Aspect, horizontal: u16, vertical: u16, targets: [(u32, u32); 2] }

fn channel(index: usize) -> Channel {
    let targets = if index == 4 { [(1920, 1080), (720, 408)] }
        else if index == 5 { [(1280, 720), (720, 408)] }
        else { [(640, 426), (720, 480)] };
    Channel {
        mode: if index == 4 || index == 5 { Aspect::SixteenNine } else { Aspect::ThreeTwo },
        horizontal: 2,
        vertical: match index { 0 => 4, 1 => 8, 2 => 16, 3 => 28, 4 | 5 => 4, _ => 2 },
        targets,
    }
}

fn validate(config: &ConfigImages<'_>, seed: &[u32; 9], channel: Channel, record: &mut RadialRecord) -> Result<bool, BackendError> {
    record.metadata[0] = channel.horizontal;
    record.metadata[1] = channel.vertical;
    if !geometry(config, seed, channel.mode, record)? { return Ok(false); }
    if channel.mode != Aspect::SixteenNine { return Ok(true); }
    for (index, target) in channel.targets.iter().enumerate() {
        if index != 0 && *target == channel.targets[0] { break; }
        record.metadata[6] = grid(seed, *target, record)?;
        if record.metadata[6] < 2 { return Ok(false); }
    }
    Ok(true)
}

pub fn build_primary(config: &ConfigImages<'_>, samples: &[u16; 8], strength: i32, record: &mut RadialRecord) -> Result<bool, BackendError> {
    build(config, samples, strength, 0..7, record)
}

pub fn build_secondary(config: &ConfigImages<'_>, samples: &[u16; 8], strength: i32, record: &mut RadialRecord) -> Result<bool, BackendError> {
    build(config, samples, strength, 6..7, record)
}

fn build(config: &ConfigImages<'_>, samples: &[u16; 8], strength: i32, channels: core::ops::Range<usize>, record: &mut RadialRecord) -> Result<bool, BackendError> {
    let seed = seeds(samples, strength);
    expand(&seed, &mut record.knots);
    record.valid = false;
    for index in channels {
        if !validate(config, &seed, channel(index), record)? { return Ok(false); }
    }
    record.valid = true;
    Ok(true)
}

#[cfg(test)]
#[path = "startup_radial_geometry_tests.rs"]
mod tests;
