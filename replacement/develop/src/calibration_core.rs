pub mod configuration;
mod curves;
mod mapped;
mod math;

pub use math::round_tenths;
use configuration::{CfgKey, Configuration, ConfigurationError, ConfigurationMemory, ConfigurationRegion};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct CurveIndex(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct ChannelIndex(pub i32);
impl ChannelIndex {
    pub(crate) fn rejected_value(self) -> Option<CurveValue> {
        if self.0 < 0 { Some(CurveValue(0xffff)) } else { None }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct ChannelSample(pub i16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct MappedWord(pub i16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct CurveValue(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Tenths(pub i32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct RoundedValue(pub i32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct SplitCount(u32);
impl SplitCount { pub const fn get(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct ChannelCount(u32);
impl ChannelCount { pub const fn get(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CurveClass { LowerBank = 0, UpperBank = 1, Invalid = 2 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BiasBank { Primary, Alternate }

pub struct Calibration<'data, M: ConfigurationMemory = ConfigurationRegion<'data>> { configuration: Configuration<'data, M> }
impl<'data, M: ConfigurationMemory> Calibration<'data, M> {
    pub const fn new(configuration: Configuration<'data, M>) -> Self { Self { configuration } }
    pub const fn configuration(&self) -> &Configuration<'data, M> { &self.configuration }
    pub fn configuration_mut(&mut self) -> &mut Configuration<'data, M> { &mut self.configuration }

    pub fn split_count(&mut self) -> Result<SplitCount, ConfigurationError<M::Error>> {
        let value = self.configuration.read_byte(CfgKey(0x3e0f82))? as u32;
        Ok(SplitCount(if value.wrapping_sub(1) & 255 > 30 { 1 } else { value }))
    }

    pub fn channel_count(&mut self) -> Result<ChannelCount, ConfigurationError<M::Error>> {
        Ok(ChannelCount((self.configuration.read_byte(CfgKey(0x3e0f80))? as u32).clamp(1, 100)))
    }

    pub fn classify_curve_index(&mut self, index: CurveIndex) -> Result<CurveClass, ConfigurationError<M::Error>> {
        let count = self.split_count()?.get();
        if index.0 < count { return Ok(CurveClass::LowerBank); }
        if index.0 < 31 { return Ok(CurveClass::Invalid); }
        let count = self.split_count()?.get();
        Ok(if index.0 >= count + 31 { CurveClass::Invalid } else { CurveClass::UpperBank })
    }

    pub fn collapse_curve_index(&mut self, index: CurveIndex) -> Result<CurveIndex, ConfigurationError<M::Error>> {
        Ok(CurveIndex(match self.classify_curve_index(index)? {
            CurveClass::LowerBank => index.0,
            CurveClass::UpperBank => index.0 - 31,
            CurveClass::Invalid => 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;
    use super::*;
    use super::configuration::{ConfigAddress, ConfigurationRegion};

    fn calibration(bytes: &[u8]) -> Calibration<'_> {
        let region = ConfigurationRegion::new(ConfigAddress(0), bytes).unwrap();
        Calibration::new(Configuration::new(region, ConfigAddress(0)))
    }

    fn set_half(bytes: &mut [u8], key: usize, value: u16) {
        bytes[key..key + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn curve_data() -> Vec<u8> {
        let mut bytes = vec![0; configuration::CONFIGURATION_RAM_SIZE];
        bytes[0x3e0f80] = 1;
        set_half(&mut bytes, 0x3e2640, 1000);
        set_half(&mut bytes, 0x3e0317, 17);
        set_half(&mut bytes, 0x3e03e0, 23);
        set_half(&mut bytes, 0x3e0572, 100);
        set_half(&mut bytes, 0x3e2643, 100);
        for index in 0..11 { set_half(&mut bytes, 0x3f1b00 + index * 2, 100 + index as u16 * 100); }
        bytes
    }

    #[test]
    fn signed_tenths_round_ties_away_from_zero() {
        for (input, expected) in [
            (i32::MIN,-214748365),(-35,-4),(-25,-3),(-15,-2),(-5,-1),(-4,0),
            (0,0),(4,0),(5,1),(15,2),(25,3),(35,4),(i32::MAX,214748365),
        ] {
            assert_eq!(round_tenths(Tenths(input)), RoundedValue(expected));
        }
    }

    #[test]
    fn mapped_words_use_both_banks_and_real_channel_strides() {
        let mut bytes = vec![0; configuration::CONFIGURATION_RAM_SIZE];
        bytes[0x3e0f82] = 2;
        set_half(&mut bytes, 0x3f2400, (-2000i16) as u16);
        set_half(&mut bytes, 0x3f2402, 2000);
        set_half(&mut bytes, 0x3f2440, 4321);
        let mut calibration = calibration(&bytes);
        for (index, expected) in [(0,-2000),(1,2000),(2,-2000),(31,-2000),(32,2000),(u32::MAX,-2000)] {
            assert_eq!(calibration.mapped_word(CurveIndex(index), ChannelIndex(0)), Ok(MappedWord(expected)));
        }
        assert_eq!(calibration.mapped_word(CurveIndex(32), ChannelIndex(1)), Ok(MappedWord(4321)));
        assert_eq!(calibration.transform_mapped_word(CurveIndex(0), ChannelIndex(0)), Ok(CurveValue(50)));
        assert_eq!(calibration.transform_mapped_word(CurveIndex(1), ChannelIndex(0)), Ok(CurveValue(200)));
    }

    #[test]
    fn channel_curve_uses_the_supplied_knots_and_bias_banks() {
        let bytes = curve_data();
        let mut calibration = calibration(&bytes);
        assert_eq!(calibration.channel_curve_origin(ChannelIndex(0)), Ok(CurveValue(100)));
        assert_eq!(calibration.channel_bias(ChannelIndex(0), BiasBank::Primary), Ok(ChannelSample(17)));
        assert_eq!(calibration.channel_bias(ChannelIndex(0), BiasBank::Alternate), Ok(ChannelSample(23)));
        assert_eq!(calibration.channel_biased_word(ChannelIndex(0), BiasBank::Primary), Ok(ChannelSample(117)));
        assert_eq!(calibration.channel_curve_value(ChannelIndex(0)), Ok(CurveValue(200)));
        assert_eq!(calibration.scaled_channel_curve(ChannelIndex(0)), Ok(CurveValue(200)));
        assert_eq!(calibration.scaled_channel_curve(ChannelIndex(-1)), Ok(CurveValue(0xffff)));
        assert_eq!(calibration.scaled_channel_curve(ChannelIndex(1)), Ok(CurveValue(0xffff)));
    }

    #[test]
    fn channel_scale_keeps_the_original_signed_wrapped_product() {
        let mut bytes = curve_data();
        set_half(&mut bytes, 0x3e2640, 65000);
        set_half(&mut bytes, 0x3e0572, 0);
        set_half(&mut bytes, 0x3f1b00, 65000);
        assert_eq!(calibration(&bytes).scaled_channel_curve(ChannelIndex(0)), Ok(CurveValue(61106)));
    }

    #[test]
    fn missing_configuration_is_an_error_except_for_the_defined_channel_sentinel() {
        let bytes = [0x5a];
        let mut calibration = calibration(&bytes);
        assert_eq!(calibration.split_count(), Err(ConfigurationError::OutOfBounds { address: ConfigAddress(0x3e0f82), length: 1 }));
        assert_eq!(calibration.scaled_channel_curve(ChannelIndex(0)), Err(ConfigurationError::OutOfBounds { address: ConfigAddress(0x3e0f80), length: 1 }));
        assert_eq!(calibration.scaled_channel_curve(ChannelIndex(-1)), Ok(CurveValue(0xffff)));
    }

    #[test]
    fn a_mapped_word_requires_the_complete_raw_halfword() {
        let mut bytes = vec![0; 0x3f2401];
        bytes[0x3e0f82] = 1;
        bytes[0x3f2400] = 0x5a;
        assert_eq!(calibration(&bytes).mapped_word(CurveIndex(0), ChannelIndex(0)), Err(ConfigurationError::OutOfBounds { address: ConfigAddress(0x3f2400), length: 2 }));
    }
}
