#![forbid(unsafe_code)]

//! saclac's processing configuration of a completed capture (0xffd3a0, entered through 0xfeacac):
//! it keeps the capture's raw statistics, publishes them to the processing client and hands the
//! capture's calc jobs a grid corrected for black level, level gain and exposure. It answers
//! whether the first store's statistics went to the client (0xffde8c).

use super::ports::{Adjustments, PixelGeometry};
use super::statistics::{copy_grid, pixel_floor, quotient, rescale_grid, Area, CaptureParameters, ChannelPixels, GridBuffer, Plane, RawStatistics, RawStore, CHANNELS, SAMPLE_LIMIT};
use super::{AdjustmentKey, ProfileHandle};

/// The processing client's word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientWord(pub u32);

/// What the readout selection asks of the configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    /// The first store's statistics become the previous capture's less this one's, and the
    /// publication carries the previous buffers.
    pub difference: bool,
    /// The grid's black level is estimated from the profile's levels and removed.
    pub level_correction: bool,
}

/// The completed capture's record beside its parameters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureBlock {
    /// The pixel count below which a correction's channel pixels do not fall.
    pub window: Area,
    pub grid: Area,
}

#[derive(Clone, Copy, Debug)]
pub struct Publication<'a> {
    pub client: ClientWord,
    pub parameters: &'a CaptureParameters,
    pub store: Option<RawStore>,
    pub first: bool,
    /// The selection names the previous buffers.
    pub previous: bool,
}

/// The profile levels a level correction used.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelRecord {
    pub output: [i32; CHANNELS],
    pub signed: [i32; CHANNELS],
}

/// The statistics a level correction started from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsRecord {
    pub grid: Area,
    pub black: [u32; CHANNELS],
}

/// A raw store's four plane areas and black sums: the current buffer's to write, the previous
/// buffer's to read.
#[derive(Debug)]
pub struct RawPair<'a> {
    pub current: [&'a mut Plane; CHANNELS],
    pub current_black: &'a mut [u32; CHANNELS],
    pub previous: [&'a Plane; CHANNELS],
    pub previous_black: &'a [u32; CHANNELS],
}

pub trait ConfigurationPorts: PixelGeometry + Adjustments {
    /// The last completed capture, copied to the caller's parameters; `None` when it is another
    /// kind (0xffcfd0).
    fn completed(&mut self) -> Option<(CaptureParameters, CaptureBlock)>;
    /// Copies the completed capture's raw statistics into the store's current buffer.
    fn capture(&mut self, store: RawStore);
    /// Copies the store's current buffer over its previous one.
    fn keep(&mut self, store: RawStore);
    /// 0xedb05c.
    fn publish(&mut self, publication: &Publication);
    fn raw_pair(&mut self, store: RawStore) -> RawPair<'_>;
    fn raw(&mut self, store: RawStore) -> RawStatistics<'_>;
    /// Records into the caller's dump, when it passed one.
    fn record_levels(&mut self, record: &LevelRecord);
    fn record_statistics(&mut self, record: &StatisticsRecord);
    /// Writes the parameters' extent and grid back to the caller.
    fn update(&mut self, parameters: &CaptureParameters);
    /// 0xedba78.
    fn output_levels(&mut self, reference: i16, profile: ProfileHandle) -> [i32; CHANNELS];
    /// 0xedb0ac.
    fn signed_levels(&mut self, reference: i16, profile: ProfileHandle) -> [i32; CHANNELS];
    /// 0xedb9fc.
    fn selection_status(&mut self, reference: i16, profile: ProfileHandle) -> u32;
    /// The profile level a decade's thresholds encode, 0..=4 (0xef743c decoded by 0xef73e4).
    fn level_index(&mut self, decade: i16) -> u32;
    /// 0x772f28.
    fn calibrated_level(&mut self, step: u32) -> i16;
    /// 0xee9ab0.
    fn linearized(&mut self, level: i16) -> u32;
}

/// The level gain of each of the first three profile levels; the calibrated row takes its own.
const LEVEL_GAINS: [AdjustmentKey; 3] = [AdjustmentKey(0x80f4), AdjustmentKey(0x80f6), AdjustmentKey(0x80f8)];
const CALIBRATED_GAIN: AdjustmentKey = AdjustmentKey(0x80fa);
const CALIBRATED_STEP: u32 = 2;
/// Gains in 1/128; 0 also leaves the grid unscaled.
const UNIT_GAIN: u32 = 0x80;
/// Linearised exposure in hundredths.
const UNIT_EXPOSURE: u32 = 100;

fn difference(pair: RawPair) {
    for (current, previous) in pair.current.into_iter().zip(pair.previous) {
        for (cell, earlier) in current.iter_mut().zip(previous) {
            *cell = earlier.saturating_sub(*cell);
        }
    }
    for (sum, earlier) in pair.current_black.iter_mut().zip(pair.previous_black) {
        *sum = earlier.saturating_sub(*sum);
    }
}

/// Multiplies every cell by `factor / 2^shift`, capped at the channel's sample limit.
fn scale(planes: &mut [Plane; CHANNELS], factor: u32, shift: u32, pixels: &ChannelPixels) {
    for (plane, count) in planes.iter_mut().zip(pixels.channels) {
        let limit = count.wrapping_mul(SAMPLE_LIMIT);
        plane.iter_mut().for_each(|cell| *cell = ((u64::from(*cell) * u64::from(factor) >> shift) as u32).min(limit));
    }
}

/// The configuration's state: whether the first store's statistics went to the client.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureConfiguration {
    published: bool,
}

impl CaptureConfiguration {
    /// Cleared when the first store takes a capture, set when it is published (0xffde8c).
    pub const fn first_published(&self) -> bool { self.published }

    /// Configures the processing for the last completed capture and fills `output`; false when
    /// the last completed capture is another kind.
    pub fn configure(&mut self, ports: &mut impl ConfigurationPorts, selection: Selection, client: Option<ClientWord>, output: Option<GridBuffer>) -> bool {
        let Some((mut parameters, block)) = ports.completed() else { return false };
        let store = parameters.store;
        if let Some(store) = store {
            ports.capture(store);
            if store == RawStore::First {
                self.published = false;
            }
        }
        if !selection.difference {
            ports.keep(store.unwrap_or(RawStore::Second));
        }
        let first = store == Some(RawStore::First);
        if let Some(client) = client {
            ports.publish(&Publication { client, parameters: &parameters, store, first, previous: selection.difference });
            if first {
                self.published = true;
            }
        }
        if selection.difference && first {
            difference(ports.raw_pair(RawStore::First));
        }
        let (Some(store), Some(mut output)) = (store, output) else { return true };

        copy_grid(&ports.raw(store), &mut output);
        let black = ports.raw(store).black;
        let floor = pixel_floor(ports);
        let window = block.window.pixels();
        if selection.level_correction {
            correct_levels(ports, &mut parameters, &block, black, &mut output);
        }
        if let Some(gain) = level_gain(ports, &parameters) {
            let pixels = ports.channel_pixels(floor.max(window));
            scale(output.planes, gain, 7, &pixels);
        }
        let exposure = ports.linearized(parameters.exposure);
        if exposure != UNIT_EXPOSURE {
            let factor = (exposure << 10) / UNIT_EXPOSURE;
            let pixels = ports.channel_pixels(floor.max(window));
            scale(output.planes, factor, 10, &pixels);
        }
        rescale_grid(ports, output.planes, &mut parameters);
        ports.update(&parameters);
        true
    }
}

/// Removes the black level the profile's levels and the black sums estimate, and takes the
/// record's grid area.
fn correct_levels(ports: &mut impl ConfigurationPorts, parameters: &mut CaptureParameters, block: &CaptureBlock, black: [u32; CHANNELS], output: &mut GridBuffer) {
    let output_levels = ports.output_levels(parameters.reference, parameters.profile);
    let signed_levels = ports.signed_levels(parameters.reference, parameters.profile);
    ports.record_levels(&LevelRecord { output: output_levels, signed: signed_levels });

    let quarter = block.grid.pixels() >> 2;
    let mean = if ports.selection_status(parameters.reference, parameters.profile) == 1 {
        let residual: u64 = black.iter().zip(output_levels).map(|(sum, level)| u64::from(sum.saturating_sub((level as u32).wrapping_mul(quarter)))).sum();
        quotient(residual, u64::from(quarter << 2)) as u32
    } else {
        0
    };
    parameters.grid = block.grid;
    ports.record_statistics(&StatisticsRecord { grid: block.grid, black });

    let floor = pixel_floor(ports);
    let pixels = ports.channel_pixels(floor.max(block.window.pixels()));
    for ((plane, count), level) in output.planes.iter_mut().zip(pixels.channels).zip(signed_levels) {
        let level = count.wrapping_mul(mean.wrapping_add(level as u32));
        plane.iter_mut().for_each(|cell| *cell = cell.saturating_sub(level));
    }
    *output.totals = [0; CHANNELS];
}

/// The level gain in 1/128 for the decade's profile level, `None` for unit gain. Levels 3 and 4
/// have no gain key.
fn level_gain(ports: &mut impl ConfigurationPorts, parameters: &CaptureParameters) -> Option<u32> {
    let level = ports.level_index(parameters.decade);
    let row = (i32::from((parameters.decade as u16).wrapping_add(parameters.reference as u16)) + ports.aspect(parameters.profile)) as i16;
    let key = if row == ports.calibrated_level(CALIBRATED_STEP) { CALIBRATED_GAIN } else { *LEVEL_GAINS.get(level as usize)? };
    let gain = u32::from(ports.halfword(key));
    (gain != 0 && gain != UNIT_GAIN).then_some(gain)
}
