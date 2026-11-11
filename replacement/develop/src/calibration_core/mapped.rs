use super::{Calibration, ChannelIndex, CurveIndex, CurveValue, MappedWord, math};
use super::configuration::{ConfigAddress, ConfigurationError, ConfigurationMemory};

impl<M: ConfigurationMemory> Calibration<'_, M> {
    pub fn mapped_word(&mut self, index: CurveIndex, channel: ChannelIndex) -> Result<MappedWord, ConfigurationError<M::Error>> {
        let index = self.collapse_curve_index(index)?;
        let offset = (channel.0 as u32).wrapping_mul(31).wrapping_add(index.0).wrapping_mul(2);
        let address = ConfigAddress(self.configuration.blob_address().0.wrapping_add(0x3f2400).wrapping_add(offset));
        Ok(MappedWord(self.configuration.memory_mut().read_half(address)? as i16))
    }

    pub fn transform_mapped_word(&mut self, index: CurveIndex, channel: ChannelIndex) -> Result<CurveValue, ConfigurationError<M::Error>> {
        Ok(math::transform_mapped_word(self.mapped_word(index, channel)?))
    }
}
