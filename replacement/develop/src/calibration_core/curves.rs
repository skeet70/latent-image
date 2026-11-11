use super::{BiasBank, Calibration, ChannelIndex, ChannelSample, CurveValue, Tenths, round_tenths};
use super::configuration::{CfgKey, ConfigurationError, ConfigurationMemory};

struct CurveKnot { value: CurveValue, position: ChannelSample }

impl<M: ConfigurationMemory> Calibration<'_, M> {
    pub fn channel_curve_origin(&mut self, channel: ChannelIndex) -> Result<CurveValue, ConfigurationError<M::Error>> {
        let key = CfgKey(0x3f1b00u32.wrapping_add((channel.0 as u32).wrapping_mul(22)));
        Ok(CurveValue(self.configuration.read_half(key)?))
    }

    pub fn channel_bias(&mut self, channel: ChannelIndex, bank: BiasBank) -> Result<ChannelSample, ConfigurationError<M::Error>> {
        let offset = (channel.0 as u32).wrapping_mul(2);
        let base: u32 = match bank { BiasBank::Primary => 0x3e0317, BiasBank::Alternate => 0x3e03e0 };
        let initial = self.configuration.read_half(CfgKey(base.wrapping_add(offset)))?;
        let signed = self.configuration.read_signed_byte(CfgKey(0x1fc))? as i32;
        let first = self.channel_curve_origin(channel)?.0 as u32;
        let scale = self.configuration.read_half(CfgKey(0x3e2640))? as u32;
        let correction = if scale == 0 || first == 0 { 0 } else {
            let quotient = signed.wrapping_mul(1000000) / scale.wrapping_mul(first) as i32;
            round_tenths(Tenths(quotient.wrapping_neg())).0 as u16
        };
        Ok(ChannelSample(initial.wrapping_add(correction) as i16))
    }

    pub fn channel_biased_word(&mut self, channel: ChannelIndex, bank: BiasBank) -> Result<ChannelSample, ConfigurationError<M::Error>> {
        let key = CfgKey(0x3e0572u32.wrapping_add((channel.0 as u32).wrapping_mul(2)));
        let initial = self.configuration.read_half(key)?;
        let bias = self.channel_bias(channel, bank)?;
        Ok(ChannelSample(initial.wrapping_add(bias.0 as u16) as i16))
    }

    pub fn interpolate_channel_curve(&mut self, channel: ChannelIndex, sample: ChannelSample) -> Result<CurveValue, ConfigurationError<M::Error>> {
        let spacing = self.configuration.read_half(CfgKey(0x3e2643u32.wrapping_add((channel.0 as u32).wrapping_mul(2))))? as i16;
        let key = 0x3f1b00u32.wrapping_add((channel.0 as u32).wrapping_mul(22));
        let mut knot = |index: u32| -> Result<CurveKnot, ConfigurationError<M::Error>> {
            let value = CurveValue(self.configuration.read_half(CfgKey(key.wrapping_add(index * 2)))?);
            let position = if index == 0 { 0 } else { (index as i16 * 2 - 1).wrapping_mul(spacing) };
            Ok(CurveKnot { value, position: ChannelSample(position) })
        };
        let knots = [knot(0)?,knot(1)?,knot(2)?,knot(3)?,knot(4)?,knot(5)?,knot(6)?,knot(7)?,knot(8)?,knot(9)?,knot(10)?];
        let bias = self.channel_bias(channel, BiasBank::Primary)?;
        let sample = sample.0.wrapping_sub(bias.0) as i32;
        if sample <= knots[0].position.0 as i32 { return Ok(knots[0].value); }
        if sample >= knots[10].position.0 as i32 { return Ok(knots[10].value); }
        let mut index = 0;
        while sample >= knots[index + 1].position.0 as i32 { index += 1; }
        let lower = knots[index].position.0 as i32;
        let upper = knots[index + 1].position.0 as i32;
        if lower >= upper { return Ok(knots[index].value); }
        let delta = (knots[index + 1].value.0 as i32).wrapping_sub(knots[index].value.0 as i32);
        let numerator = sample.wrapping_sub(lower).wrapping_mul(delta);
        let adjustment = numerator / (upper - lower);
        Ok(CurveValue((knots[index].value.0 as i32).wrapping_add(adjustment) as u16))
    }

    pub fn channel_curve_value(&mut self, channel: ChannelIndex) -> Result<CurveValue, ConfigurationError<M::Error>> {
        let sample = self.channel_biased_word(channel, BiasBank::Primary)?;
        self.interpolate_channel_curve(channel, sample)
    }

    pub fn scaled_channel_curve(&mut self, channel: ChannelIndex) -> Result<CurveValue, ConfigurationError<M::Error>> {
        if let Some(value) = channel.rejected_value() { return Ok(value); }
        if channel.0 as u32 >= self.channel_count()?.get() { return Ok(CurveValue(0xffff)); }
        let scale = self.configuration.read_half(CfgKey(0x3e2640))? as u32;
        let value = self.channel_curve_value(channel)?.0 as u32;
        if scale == 0xffff || value == 0xffff { return Ok(CurveValue(0xffff)); }
        let numerator = value.wrapping_mul(scale).wrapping_add(500) as i32;
        Ok(CurveValue((numerator / 1000) as u16))
    }
}
