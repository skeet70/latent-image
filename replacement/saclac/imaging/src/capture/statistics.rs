#![forbid(unsafe_code)]

//! A completed capture's statistics as saclac takes them: the channel grid and wide regions,
//! corrected for black level and transfer and normalised to the profile's pixel count (0xffd04c
//! with 0xffc3fc, 0xffc97c and 0xffc668), and the grid copy the processing configuration starts
//! from (0xffd244).

use super::ports::{Adjustments, PixelGeometry};
use super::{AdjustmentKey, ProfileHandle};

pub const CELLS: usize = 256;
pub const CHANNELS: usize = 4;
pub const REGIONS: usize = 3;
/// The region areas the parameters carry; a capture fills `REGIONS` of them.
pub const REGION_AREAS: usize = 10;

/// A channel's 16 x 16 grid of sums.
pub type Plane = [u32; CELLS];
/// A wide region's channel sums.
pub type Region = [u64; CHANNELS];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Area {
    pub width: u32,
    pub height: u32,
}

impl Area {
    pub const fn pixels(self) -> u32 { self.width.wrapping_mul(self.height) }
}

/// The two raw statistics stores a configuration keeps, each a current and a previous buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawStore {
    First,
    Second,
}

/// The parameters the imaging pipeline records with a capture's statistics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureParameters {
    /// The grid's area, rescaled with it.
    pub extent: Area,
    /// The area the transfer correction divides by.
    pub grid: Area,
    /// Each region's area, rescaled with the regions.
    pub regions: [Area; REGION_AREAS],
    pub level: u16,
    /// With the profile's aspect, the row the transfer correction compares with its threshold.
    pub reference: i16,
    pub reference_span: u16,
    pub decade: i16,
    pub companion: u16,
    pub setting: u32,
    pub profile: ProfileHandle,
    pub lens: u32,
    pub lens_reference: u32,
    /// The store the configuration keeps the raw statistics in.
    pub store: Option<RawStore>,
    /// The exposure level the configuration scales the grid to.
    pub exposure: i16,
}

/// A region's pixels per channel and four times its second channel's (0xef56cc).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChannelPixels {
    pub channels: [u32; CHANNELS],
    pub total: u32,
}

/// A completed capture's raw statistics by grid channel.
#[derive(Clone, Copy, Debug)]
pub struct RawStatistics<'a> {
    pub planes: [&'a Plane; CHANNELS],
    /// Each region's first three channel sums; the fourth channel is the second's.
    pub regions: [[u64; 3]; REGIONS],
    /// Black sums.
    pub black: [u32; CHANNELS],
}

#[derive(Debug)]
pub struct Prepared<'a> {
    pub planes: &'a mut [Plane; CHANNELS],
    pub regions: &'a mut [Region; REGIONS],
}

#[derive(Debug)]
pub struct GridBuffer<'a> {
    pub planes: &'a mut [Plane; CHANNELS],
    pub cleared: &'a mut [u16; 8],
    pub totals: &'a mut [u32; CHANNELS],
}

/// Black level per channel.
const BLACK: [AdjustmentKey; CHANNELS] = [AdjustmentKey(0x1378), AdjustmentKey(0x1376), AdjustmentKey(0x137c), AdjustmentKey(0x137a)];
/// Added to the black level for the transfer correction's pedestal.
const PEDESTAL: [AdjustmentKey; CHANNELS] = [AdjustmentKey(0x1380), AdjustmentKey(0x137e), AdjustmentKey(0x1384), AdjustmentKey(0x1382)];
/// A tenth of the lowest row the transfer correction applies at.
const TRANSFER_ROW: AdjustmentKey = AdjustmentKey(0x1386);
/// The largest sample per pixel.
pub(crate) const SAMPLE_LIMIT: u32 = 0x3fff;

/// `numerator / denominator`; a zero denominator saturates a nonzero numerator, as the runtime's
/// 64-bit division does.
pub(crate) fn quotient(numerator: u64, denominator: u64) -> u64 {
    match numerator.checked_div(denominator) {
        Some(quotient) => quotient,
        None if numerator == 0 => 0,
        None => u64::MAX,
    }
}

/// The pixel count below which the profile layer's shared divisors put a region.
pub(crate) fn pixel_floor(geometry: &mut impl PixelGeometry) -> u32 {
    let [first, second] = geometry.shared_divisors();
    first.wrapping_mul(second)
}

pub(crate) fn levels(adjustments: &impl Adjustments, keys: [AdjustmentKey; CHANNELS]) -> [u32; CHANNELS] {
    keys.map(|key| u32::from(adjustments.halfword(key)))
}

fn subtract_black(planes: &mut [Plane; CHANNELS], pixels: &ChannelPixels, black: [u32; CHANNELS]) {
    for ((plane, count), level) in planes.iter_mut().zip(pixels.channels).zip(black) {
        let threshold = count.wrapping_mul(level);
        plane.iter_mut().for_each(|cell| *cell = cell.saturating_sub(threshold));
    }
}

fn subtract_region_black(region: &mut Region, pixels: &ChannelPixels, black: [u32; CHANNELS]) {
    for ((sum, count), level) in region.iter_mut().zip(pixels.channels).zip(black) {
        *sum = sum.saturating_sub(u64::from(count) * u64::from(level));
    }
}

/// The transfer correction: each channel's pedestal added and its share of the black sums removed.
struct Transfer {
    black_total: u64,
    /// The area's pixels, at least one.
    divisor: u64,
    pedestal: [u32; CHANNELS],
}

impl Transfer {
    /// `None` below the threshold row.
    fn new(ports: &mut (impl PixelGeometry + Adjustments), parameters: &CaptureParameters, black: [u32; CHANNELS]) -> Option<Self> {
        let row = (i32::from(parameters.reference) + ports.aspect(parameters.profile)) as i16;
        if i32::from(row) < i32::from(ports.byte(TRANSFER_ROW)) * 10 {
            return None;
        }
        let black_levels = levels(ports, BLACK);
        let pedestal = levels(ports, PEDESTAL);
        Some(Self {
            black_total: black.iter().map(|sum| u64::from(*sum)).sum(),
            divisor: u64::from(parameters.grid.pixels().max(1)),
            pedestal: core::array::from_fn(|channel| black_levels[channel] + pedestal[channel]),
        })
    }

    fn share(&self, count: u32) -> u64 { quotient(self.black_total.wrapping_mul(u64::from(count)), self.divisor) & u64::from(u32::MAX) }

    fn apply(&self, planes: &mut [Plane; CHANNELS], pixels: &ChannelPixels) {
        for (plane, (count, pedestal)) in planes.iter_mut().zip(pixels.channels.into_iter().zip(self.pedestal)) {
            let share = self.share(count) as u32;
            let added = count.wrapping_mul(pedestal);
            let limit = count.wrapping_mul(SAMPLE_LIMIT);
            plane.iter_mut().for_each(|cell| *cell = cell.wrapping_add(added).saturating_sub(share).min(limit));
        }
    }

    fn apply_region(&self, region: &mut Region, pixels: &ChannelPixels) {
        for (sum, (count, pedestal)) in region.iter_mut().zip(pixels.channels.into_iter().zip(self.pedestal)) {
            let added = u64::from(count.wrapping_mul(pedestal));
            *sum = sum.wrapping_add(added).saturating_sub(self.share(count)).min(u64::from(count) * u64::from(SAMPLE_LIMIT));
        }
    }
}

/// The area a sample normalised to `reference_total` pixels stands for.
const fn normalised_area(total: u32, reference_total: u32) -> Area {
    Area { width: reference_total, height: match total.checked_div(reference_total) { Some(height) => height, None => 0 } }
}

/// Normalises the grid and regions to the profile's scaled pixel count, and records the areas
/// they now stand for in `parameters` (0xffc668).
fn rescale(geometry: &mut impl PixelGeometry, planes: &mut [Plane; CHANNELS], regions: Option<&mut [Region; REGIONS]>, parameters: &mut CaptureParameters) {
    let [first, second] = geometry.scale_divisors(parameters.profile);
    let reference = first.wrapping_mul(second);
    let reference_total = geometry.channel_pixels(reference).total;

    let pixels = geometry.channel_pixels(parameters.extent.pixels());
    let scale = (pixels.total >> 2).max(reference);
    let limit = scale.wrapping_mul(SAMPLE_LIMIT);
    for (plane, count) in planes.iter_mut().zip(pixels.channels) {
        for cell in plane.iter_mut() {
            *cell = (quotient(u64::from(*cell) * u64::from(scale), u64::from(count)) as u32).min(limit);
        }
    }
    parameters.extent = normalised_area(pixels.total, reference_total);

    let Some(regions) = regions else { return };
    for (index, area) in parameters.regions.iter_mut().enumerate() {
        let pixels = geometry.channel_pixels(area.pixels().max(reference));
        let scale = (pixels.total >> 2).max(reference);
        if let Some(region) = regions.get_mut(index) {
            let limit = u64::from(scale) * u64::from(SAMPLE_LIMIT);
            for (sum, count) in region.iter_mut().zip(pixels.channels) {
                *sum = quotient(sum.wrapping_mul(u64::from(scale)), u64::from(count)).min(limit);
            }
        }
        *area = normalised_area(pixels.total, reference_total);
    }
}

/// Fills `output` from a completed capture's raw statistics, corrected and normalised, and
/// rescales `parameters`' grid and region areas to match (0xffd04c).
pub fn prepare(ports: &mut (impl PixelGeometry + Adjustments), raw: &RawStatistics, parameters: &mut CaptureParameters, output: Prepared) {
    let floor = pixel_floor(ports);
    let black = levels(ports, BLACK);
    let transfer = Transfer::new(ports, parameters, raw.black);

    for (plane, source) in output.planes.iter_mut().zip(raw.planes) {
        *plane = *source;
    }
    let pixels = ports.channel_pixels(parameters.extent.pixels().max(floor));
    subtract_black(output.planes, &pixels, black);
    if let Some(transfer) = &transfer {
        transfer.apply(output.planes, &pixels);
    }

    for ((region, [first, second, third]), area) in output.regions.iter_mut().zip(raw.regions).zip(parameters.regions) {
        *region = [first, second, third, second];
        let pixels = ports.channel_pixels(area.pixels().max(floor));
        subtract_region_black(region, &pixels, black);
        if let Some(transfer) = &transfer {
            transfer.apply_region(region, &pixels);
        }
    }

    rescale(ports, output.planes, Some(output.regions), parameters);
}

/// Copies a completed capture's raw grid and black sums (0xffd244).
pub fn copy_grid(raw: &RawStatistics, output: &mut GridBuffer) {
    for (plane, source) in output.planes.iter_mut().zip(raw.planes) {
        *plane = *source;
    }
    *output.cleared = [0; 8];
    *output.totals = raw.black;
}

pub(crate) fn rescale_grid(geometry: &mut impl PixelGeometry, planes: &mut [Plane; CHANNELS], parameters: &mut CaptureParameters) {
    rescale(geometry, planes, None, parameters);
}
