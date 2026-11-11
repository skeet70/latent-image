//! Fixed startup callers of X-M1 1.50 0x747408, 0x746454 and 0x74947c.

use xm1_calibration_core::{BiasBank, Calibration, ChannelIndex, Tenths, round_tenths};
use xm1_calibration_core::configuration::CfgKey;
use crate::metadata_calibration::RegionError;

#[derive(Clone, Copy)]
pub(crate) enum LimitPair { Primary, Secondary, Extended }
impl LimitPair {
    pub(crate) const ALL: [Self; 3] = [Self::Primary, Self::Secondary, Self::Extended];
    fn keys(self) -> (u32, u32) {
        match self { Self::Primary => (0x3e0317, 0x3e0572), Self::Secondary => (0x3e0704, 0x3e063b), Self::Extended => (0x3e07e2, 0x3e07cd) }
    }
    pub(crate) const fn slot(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 2, Self::Extended => 4 } }
}

pub(crate) fn limits(profile: &mut Calibration<'_>, channel: ChannelIndex, pair: LimitPair) -> Result<(i16, i16), RegionError> {
    let (low, high) = pair.keys();
    Ok((limit(profile, channel, low)?, limit(profile, channel, high)?))
}

fn limit(profile: &mut Calibration<'_>, channel: ChannelIndex, mut key: u32) -> Result<i16, RegionError> {
    let config = profile.configuration_mut();
    if key == 0x3e07e2 || key == 0x3e07cd {
        let extended = config.read_byte(CfgKey(0x3e0313))?;
        if extended == 0 || channel.0 > config.read_byte(CfgKey(0x3e1214))? as i32 - 1 {
            key = if key == 0x3e07e2 { 0x3e0704 } else { 0x3e063b };
        }
    }
    if key == 0x3e0704 && channel.0 > config.read_byte(CfgKey(0x3e1213))? as i32 - 1 { key = 0x3e0317; }
    if key == 0x3e063b && channel.0 > config.read_byte(CfgKey(0x3e1213))? as i32 - 1 { key = 0x3e0572; }
    let bias = profile.channel_bias(channel, BiasBank::Primary)?.0;
    if key == 0x3e0317 { return Ok(bias); }
    Ok(profile.configuration_mut().read_half(CfgKey(key + channel.0 as u32 * 2))?.wrapping_add(bias as u16) as i16)
}

fn positions(profile: &mut Calibration<'_>, channel: ChannelIndex) -> Result<[i32; 7], RegionError> {
    let bias = profile.channel_bias(channel, BiasBank::Primary)?.0 as i32;
    let config = profile.configuration_mut();
    let mut result = [bias.wrapping_mul(10); 7];
    for (index, value) in result.iter_mut().enumerate().skip(1) {
        let offset = config.read_half(CfgKey(0x3ec000 + (channel.0 as u32 * 6 + index as u32 - 1) * 2))? as i32;
        *value = bias.wrapping_add(offset).wrapping_mul(10);
    }
    Ok(result)
}

fn geometry(profile: &mut Calibration<'_>, channel: ChannelIndex, split: u32, base: &[i32; 7]) -> Result<[i32; 7], RegionError> {
    let config = profile.configuration_mut();
    let scale = config.read_signed_byte(CfgKey(0x3e270c))? as i32;
    let mut result = *base;
    for (knot, value) in result.iter_mut().enumerate() {
        for index in 1..=split.min(9) {
            let adjustment = config.read_signed_byte(CfgKey(0x3ec4af + channel.0 as u32 * 63 + knot as u32 * 9 + index))? as i32;
            *value = value.wrapping_add(scale.wrapping_mul(adjustment));
        }
    }
    if split <= 9 { return Ok(result); }
    for knot in [0usize, 2, 4, 6] {
        for index in 10..=split {
            let adjustment = config.read_signed_byte(CfgKey(0x3edd42 + channel.0 as u32 * 84 + (knot / 2) as u32 * 21 + index))? as i32;
            result[knot] = result[knot].wrapping_add(scale.wrapping_mul(adjustment));
        }
    }
    for knot in [0usize, 2, 4] {
        let delta = base[knot + 1].wrapping_sub(base[knot]);
        let rise = result[knot + 2].wrapping_sub(result[knot]);
        let product = (delta as i64).wrapping_mul(rise as i64);
        let span = base[knot + 2].wrapping_sub(base[knot]);
        let adjustment = if span == 0 { product as i32 } else { (product / span as i64) as i32 };
        result[knot + 1] = result[knot].wrapping_add(adjustment);
    }
    Ok(result)
}

pub(crate) fn remap_from_zero(profile: &mut Calibration<'_>, channel: ChannelIndex, split: u32, sample: i16) -> Result<i16, RegionError> {
    if split == 0 { return Ok(sample); }
    let base = positions(profile, channel)?;
    let source = geometry(profile, channel, 0, &base)?;
    let target = geometry(profile, channel, split, &base)?;
    let sample = (sample as i32).wrapping_mul(10);
    let mut upper = 1;
    while upper < 6 && sample >= source[upper] { upper += 1; }
    let lower = upper - 1;
    let value = if source[upper] == source[lower] { target[upper] } else {
        let delta = sample.wrapping_sub(source[lower]);
        let rise = target[upper].wrapping_sub(target[lower]);
        let span = source[upper].wrapping_sub(source[lower]);
        target[lower].wrapping_add(((delta as i64 * rise as i64) / span as i64) as i32)
    };
    Ok(round_tenths(Tenths(value)).0 as i16)
}
