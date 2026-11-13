//! The shot's metered levels over the image task's 16 by 16 cell sums, and its level bound for a
//! lens.

use crate::adjust::rate_policy::scaled_by_configured_factor;
use crate::exposure::ports::{CaptureProfiles, Lens, Properties};
use crate::stock::adjust as stock;

pub const CELLS: usize = 256;

/// ProfileGeometry's tables and numerics the metering reads.
pub trait MeteringTables {
    /// The green, red and blue weights 0xef3434 fills for table 5, row 0.
    fn channel_weights(&mut self) -> [u16; 3];
    /// The profile's red and blue scales in 128ths (0xef643c).
    fn feature_bytes(&mut self, profile: u32) -> [u8; 2];
    /// 0xee99f0.
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16;
}

/// Image 4's diagnostic trace points (0x72872c).
pub trait DiagnosticTrace {
    fn trace(&mut self, point: u32);
}

/// Each cell's channel sums, as the image task reports them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellSums {
    pub red: [u32; CELLS],
    pub green: [u32; CELLS],
    pub blue: [u32; CELLS],
    pub second_green: [u32; CELLS],
}

/// What the shot meters against.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeteringInputs {
    pub exposure: u32,
    pub gain: u32,
    pub level: u16,
    pub offset: u16,
    pub profile: u32,
    pub lens: u32,
    pub reference: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeteredLevels {
    pub mean: i16,
    pub weighted: i16,
}

/// A cell's luminance from its weighted channels (0xfa6c04 for channel 0, the only one its caller
/// asks for).
fn luminance([green_weight, red_weight, blue_weight]: [u16; 3], red: u32, green: u32, blue: u32) -> u32 {
    let scaled = |value: u32, weight: u16| (u64::from(value) * u64::from(weight) >> 8) as u32;
    scaled(green.wrapping_mul(3), green_weight).wrapping_add(scaled(blue, blue_weight)).wrapping_add(scaled(red.wrapping_mul(3), red_weight))
}

/// The level of the cells' mean luminance and of their zone-weighted mean (0xfaf8a0).
pub fn metered_levels(
    ports: &mut (impl Properties + Lens + CaptureProfiles),
    tables: &mut impl MeteringTables,
    trace: &mut impl DiagnosticTrace,
    sums: &CellSums,
    inputs: &MeteringInputs,
) -> MeteredLevels {
    trace.trace(stock::METERING_START);
    let weights = tables.channel_weights();
    let [red_scale, blue_scale] = tables.feature_bytes(inputs.profile);
    let (mut total, mut weighted, mut weight_sum) = (0u64, 0u64, 0u64);
    for cell in 0..CELLS {
        let red = sums.red[cell].wrapping_mul(u32::from(red_scale)) >> 7;
        let green = sums.second_green[cell].wrapping_add(sums.green[cell]);
        let blue = sums.blue[cell].wrapping_mul(u32::from(blue_scale)) >> 7;
        let value = luminance(weights, red, green, blue);
        let weight = ports.property(stock::zone_weight(cell)) as u8;
        total += u64::from(value);
        weighted += u64::from(value.wrapping_mul(u32::from(weight)));
        weight_sum += u64::from(weight);
    }
    let reference = ports.reference_offset(inputs.reference) as u16;
    let profile = ports.level_offset(inputs.profile) as u16;
    let scale = scaled_by_configured_factor(ports, inputs.exposure.wrapping_mul(inputs.gain));
    let lens = ports.level(inputs.lens, inputs.reference) as u16;
    let base = inputs.level.wrapping_sub(inputs.offset).wrapping_add(lens).wrapping_sub(profile);
    let mean = tables.log2_difference((total >> 8) as u32, scale) as u16;
    let average = (weighted / weight_sum.max(1)) as u32;
    let weighted_level = tables.log2_difference(average, scale) as u16;
    trace.trace(stock::METERING_END);
    MeteredLevels {
        mean: base.wrapping_add(reference.wrapping_add(mean)) as i16,
        weighted: base.wrapping_add(reference.wrapping_add(weighted_level)) as i16,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensLevel {
    pub lens: u32,
    pub level: i16,
    /// `level` less its excess.
    pub unclipped: i16,
    /// The capture profile the offset was read for.
    pub profile: u32,
}

/// The shot's level for a lens: `level` less its excess over the configured start, plus `offset`
/// and capture profile 8's offset, less the lens's level, clamped to the bounds `alternate`
/// selects (0xfaf730).
pub fn lens_level(ports: &mut (impl Properties + Lens + CaptureProfiles), lens: u32, reference: u32, offset: i16, level: i32, alternate: bool) -> LensLevel {
    let start = (ports.property(stock::EXCESS_START) as u16).wrapping_mul(10) as i16;
    let step = (ports.property(stock::EXCESS_STEP) as i8 as i16).wrapping_mul(10);
    let excess = if level <= i32::from(start) {
        0
    } else if level - i32::from(step) < i32::from(start) {
        (level as i16).wrapping_sub(start)
    } else {
        step
    };
    let profile_offset = ports.level_offset(stock::LENS_LEVEL_PROFILE);
    let [upper, lower] = stock::lens_level_bounds(alternate).map(|id| (ports.property(id) as u16).wrapping_mul(10) as i16);
    let lower = if lower <= 0x1b7 { 440 } else { lower };
    let lens_offset = ports.level(lens, reference);
    let value = (level as i16).wrapping_add(offset).wrapping_add(profile_offset).wrapping_sub(excess).wrapping_sub(lens_offset);
    let clamped = if value < lower { lower } else { value.min(upper) };
    LensLevel { lens, level: clamped, unclipped: (level as i16).wrapping_sub(excess), profile: stock::LENS_LEVEL_PROFILE }
}
