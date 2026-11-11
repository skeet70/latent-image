//! Conditional startup calibration caches from X-M1 1.50 0x74c5e8, 0x749668 and 0x7319fc.
//! Evidence pin: 55436fc85c0560dd252b0410af6f2427fe198cd3, authenticated image-4 extents.

use xm1_calibration_core::{ChannelIndex, CurveIndex};
use xm1_calibration_core::configuration::CfgKey;
use crate::backend::{BackendError, ConfigImages, ConfigSpan};
use crate::metadata_calibration::{MetadataCalibration, RegionError};
#[path = "startup_calibration_geometry.rs"]
mod geometry;
#[path = "startup_calibration_spline.rs"]
mod spline;
use geometry::{LimitPair, limits, remap_from_zero};
use spline::CubicSpline;
pub use spline::SplineError;

pub const CHANNEL_SAMPLES: usize = 101;
pub const SAMPLE_CACHE_BYTES: usize = 0x4ee8;
pub const EXTREMA_CACHE_BYTES: usize = 600;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupCalibrationMode { Recompute, Reload }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupCalibrationError {
    Configuration(RegionError),
    Backend(BackendError),
    WindowSize { knots: u8 },
    InvalidKnotOrder { channel: ChannelIndex },
    Spline(SplineError),
}
impl From<RegionError> for StartupCalibrationError {
    fn from(error: RegionError) -> Self { Self::Configuration(error) }
}
impl From<BackendError> for StartupCalibrationError {
    fn from(error: BackendError) -> Self { Self::Backend(error) }
}
impl From<SplineError> for StartupCalibrationError {
    fn from(error: SplineError) -> Self { Self::Spline(error) }
}

pub struct StartupCalibration {
    samples: [u16; CHANNEL_SAMPLES],
    table: [u8; SAMPLE_CACHE_BYTES],
    extrema: [u8; EXTREMA_CACHE_BYTES],
    keys: [u16; 128],
    key_count: u8,
    persist_pending: bool,
}
impl Default for StartupCalibration { fn default() -> Self { Self::new() } }
impl StartupCalibration {
    pub const fn new() -> Self {
        Self { samples: [0; CHANNEL_SAMPLES], table: [0; SAMPLE_CACHE_BYTES], extrema: [0; EXTREMA_CACHE_BYTES], keys: [0xffff; 128], key_count: 0, persist_pending: false }
    }
    pub fn samples(&self) -> &[u16; CHANNEL_SAMPLES] { &self.samples }
    pub fn sample_cache(&self) -> &[u8; SAMPLE_CACHE_BYTES] { &self.table }
    pub fn extrema_cache(&self) -> &[u8; EXTREMA_CACHE_BYTES] { &self.extrema }
    pub fn calibration_keys(&self) -> &[u16] { &self.keys[..self.key_count as usize] }

    pub fn prepare(&mut self, calibration: &mut MetadataCalibration<'_>, mode: StartupCalibrationMode) -> Result<(), StartupCalibrationError> {
        self.persist_pending = false;
        let config = calibration.configuration();
        if config.read_byte(CfgKey(0x3e0009))? == 1 { return Ok(()); }
        for (index, sample) in self.samples.iter_mut().enumerate() {
            *sample = config.read_half(CfgKey(0x9526 + index as u32 * 2))?;
        }
        if mode == StartupCalibrationMode::Reload {
            for (index, byte) in self.table.iter_mut().enumerate() { *byte = config.read_byte(CfgKey(0x40ab00 + index as u32))?; }
            for (index, byte) in self.extrema.iter_mut().enumerate() { *byte = config.read_byte(CfgKey(0x40fa00 + index as u32))?; }
            return Ok(());
        }
        let channels = calibration.channel_count()?;
        let width = calibration.configuration().read_byte(CfgKey(0x9524))?.max(2);
        if width > 20 { return Err(StartupCalibrationError::WindowSize { knots: width }); }
        self.table.fill(0xff);
        for channel in 0..channels {
            self.interpolate_samples(calibration, ChannelIndex(channel as i32), width as usize)?;
        }
        self.extrema.fill(0);
        let profile = calibration.profile();
        let splits = profile.split_count()?.get();
        for channel in 0..channels {
            let channel_index = ChannelIndex(channel as i32);
            for pair in LimitPair::ALL {
                let (low, high) = limits(profile, channel_index, pair)?;
                let mut minimum = i16::MAX;
                let mut maximum = i16::MIN;
                let mut low_split = 0;
                let mut high_split = 0;
                for split in 0..splits {
                    let remapped_low = remap_from_zero(profile, channel_index, split, low)?;
                    let remapped_high = remap_from_zero(profile, channel_index, split, high)?;
                    if remapped_low < minimum { minimum = remapped_low; low_split = split as u8; }
                    if remapped_high > maximum { maximum = remapped_high; high_split = split as u8; }
                }
                let offset = channel as usize * 6 + pair.slot();
                self.extrema[offset] = low_split;
                self.extrema[offset + 1] = high_split;
            }
        }
        self.persist_pending = true;
        Ok(())
    }

    fn interpolate_samples(&mut self, calibration: &mut MetadataCalibration<'_>, channel: ChannelIndex, width: usize) -> Result<(), StartupCalibrationError> {
        let config = calibration.configuration();
        let step = config.read_half(CfgKey(0x3e2643 + channel.0 as u32 * 2))?;
        let mut knots = [0.0f32; 20];
        let mut values = [0.0f32; 20];
        knots[0] = f32::from_bits(0x43800080);
        for index in 1..20 {
            let value = config.read_half(CfgKey(0x3eb100 + channel.0 as u32 * 38 + (index as u32 - 1) * 2))?;
            knots[index] = reciprocal(value);
            values[index] = step.wrapping_mul(index as u16) as i16 as f32;
        }
        if knots.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(StartupCalibrationError::InvalidKnotOrder { channel });
        }
        let mut cursor = 0;
        let mut start = 0;
        let mut spline = CubicSpline::new();
        for index in (0..CHANNEL_SAMPLES).rev() {
            let query = reciprocal(self.samples[index]);
            if cursor + 1 < 20 && (cursor == 0 || knots[cursor] < query) {
                while cursor + width - 1 < 20 && query > knots[cursor + width - 1] { cursor += 1; }
                start = cursor.min(20 - width);
                spline.solve(&knots[start..start + width], &values[start..start + width])?;
                cursor = start + width - 1;
            }
            let last = start + width - 1;
            let value = if query < knots[start] { values[start] } else if query > knots[last] { values[last] } else {
                let value = spline.evaluate(&knots[start..start + width], query);
                let value = if value < values[0] { values[0] } else { value };
                if values[19] < value { values[19] } else { value }
            };
            let word = (value as i32) as u16;
            let offset = (channel.0 as usize * CHANNEL_SAMPLES + index) * 2;
            self.table[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
        }
        Ok(())
    }

    pub fn persist(&mut self, configuration: &mut ConfigImages<'_>) -> Result<(), StartupCalibrationError> {
        if !self.persist_pending { return Ok(()); }
        configuration.write(ConfigSpan::new(0x40ab00, SAMPLE_CACHE_BYTES as u32)?, &self.table)?;
        configuration.write(ConfigSpan::new(0x40fa00, EXTREMA_CACHE_BYTES as u32)?, &self.extrema)?;
        self.persist_pending = false;
        Ok(())
    }

    pub fn rebuild_keys(&mut self, calibration: &mut MetadataCalibration<'_>) -> Result<(), StartupCalibrationError> {
        self.keys.fill(0xffff);
        self.key_count = 0;
        for channel in 0..calibration.channel_count()? {
            let lower = calibration.configuration().read_byte(CfgKey(0x3e22c8 + channel))? as u32;
            let upper = calibration.configuration().read_byte(CfgKey(0x3e232d + channel))? as u32;
            for index in lower..=upper {
                let value = calibration.mapped_value(CurveIndex(index), ChannelIndex(channel as i32))?.0;
                let count = self.key_count as usize;
                if count == 128 { continue; }
                let mut position = 0;
                while position < count && self.keys[position] < value { position += 1; }
                if position < count && self.keys[position] == value && value != 0xffff { continue; }
                self.keys.copy_within(position..count, position + 1);
                self.keys[position] = value;
                self.key_count += 1;
            }
        }
        Ok(())
    }
}

fn reciprocal(value: u16) -> f32 {
    if value == 0 { 0.0 } else { f32::from_bits(0x4b7fffff) / value as f32 }
}

#[cfg(test)]
#[path = "startup_calibration_tests.rs"]
mod tests;
