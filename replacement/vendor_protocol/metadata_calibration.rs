//! Device-info calibration functions distinct from the shared profile-calibration exports.

use xm1_calibration_core::{Calibration, ChannelIndex, CurveIndex, Tenths, round_tenths};
use xm1_calibration_core::configuration::{CfgKey, ConfigAddress, Configuration, ConfigurationError, ConfigurationMemory};
use super::metadata_math::{CalibrationScale, CalibrationValue, scale_transformed_word, transform_mapped_value};
use super::metadata_selection::CalibrationChannel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind { OptionalPacked, OptionalWord, OptionalScaled, Packed, Scaled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitBank { First, Second }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitSide { One, Other }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeClass { Zero, One, Two, Three }

/// Vendor reads the configuration through the borrowed RAM image.
pub type RegionError = ConfigurationError<core::convert::Infallible>;

pub struct MetadataCalibration<'data> { profile: Calibration<'data> }

impl<'data> MetadataCalibration<'data> {
    pub const fn new(configuration: Configuration<'data>) -> Self {
        Self { profile: Calibration::new(configuration) }
    }
    pub fn profile(&mut self) -> &mut Calibration<'data> { &mut self.profile }
    pub fn configuration(&mut self) -> &mut Configuration<'data> { self.profile.configuration_mut() }

    pub fn mapped_value(&mut self, index: CurveIndex, channel: ChannelIndex) -> Result<CalibrationValue, RegionError> {
        let collapsed = self.profile.collapse_curve_index(index)?.0;
        let offset = 0x3f4600u32.wrapping_add((channel.0 as u32).wrapping_mul(31)).wrapping_add(collapsed);
        let configuration = self.configuration();
        let address = ConfigAddress(configuration.blob_address().0.wrapping_add(offset));
        let mapped = configuration.memory_mut().read_byte(address)?;
        Ok(CalibrationValue(if mapped == 0xff { 100 } else {
            configuration.read_half(CfgKey(0x3e2530 + mapped as u32 * 2))?
        }))
    }

    pub fn rounded_word(&mut self, index: CurveIndex, channel: ChannelIndex) -> Result<i16, RegionError> {
        let value = self.profile.mapped_word(index, channel)?;
        Ok(round_tenths(Tenths(value.0 as i32)).0 as i16)
    }

    pub fn transformed_value(&mut self, index: CurveIndex, channel: ChannelIndex) -> Result<i16, RegionError> {
        Ok(transform_mapped_value(self.mapped_value(index, channel)?))
    }

    pub fn scaled_mapped_word(&mut self, index: CurveIndex, channel: ChannelIndex) -> Result<CalibrationValue, RegionError> {
        if index.0 > self.profile.split_count()?.get().wrapping_sub(1) || channel.0 < 0 {
            return Ok(CalibrationValue(0xffff));
        }
        if channel.0 as u32 >= self.profile.channel_count()?.get() { return Ok(CalibrationValue(0xffff)); }
        let word = self.profile.transform_mapped_word(index, channel)?;
        let scale = self.configuration().read_half(CfgKey(0x9285))?;
        Ok(CalibrationValue(scale_transformed_word(CalibrationValue(word.0), CalibrationScale(scale))))
    }

    pub fn channel_count(&mut self) -> Result<u32, RegionError> { Ok(self.profile.channel_count()?.get()) }

    pub fn read_word(&mut self, key: CfgKey) -> Result<u32, RegionError> {
        let mut bytes = [0; 4];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = self.configuration().read_byte(key.wrapping_add(index as u32))?;
        }
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn scaled_limit(&mut self, key: CfgKey) -> Result<u32, RegionError> {
        let value = self.configuration().read_half(key)? as u32;
        let shift = self.configuration().read_byte(CfgKey(0x3e162f))?;
        Ok(if shift >= 32 { 0 } else { value << shift })
    }

    pub fn mode_limit(&mut self, kind: LimitKind, bank: LimitBank, side: LimitSide, mode: ModeClass) -> Result<u32, RegionError> {
        let second = u32::from(bank == LimitBank::Second);
        let other = u32::from(side == LimitSide::Other);
        let group = match mode { ModeClass::One => 1, ModeClass::Two => 2, _ => 0 };
        let optional = matches!(kind, LimitKind::OptionalPacked | LimitKind::OptionalWord | LimitKind::OptionalScaled);
        let (base, step, bank_step) = if optional { (0x3e1676, 0x50, 0x230) } else { (0x3e145e, 0x11, 0x6b) };
        let position = match group { 1 => 1 + other, 2 => 3 + other, _ => 0 };
        let key = base + second * bank_step + position * step;
        let value = match kind {
            LimitKind::OptionalPacked => self.read_word(CfgKey(key - 10))?,
            LimitKind::Packed => self.read_word(CfgKey(key - 4))?,
            LimitKind::OptionalWord => self.scaled_limit(CfgKey(key - 2))?,
            LimitKind::OptionalScaled | LimitKind::Scaled => self.scaled_limit(CfgKey(key))?,
        };
        Ok(if optional && value == 0 { 0xffff } else { value })
    }

    pub fn channel_index(channel: CalibrationChannel) -> ChannelIndex { ChannelIndex(channel.get() as i32) }
}
