//! Position-dependent calibration interpolation at 0x00748398.

use xm1_calibration_core::configuration::{CfgKey, Configuration};
use xm1_calibration_core::{BiasBank, Calibration, ChannelIndex};
use super::metadata_calibration::RegionError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationChannel(u8);

impl CalibrationChannel {
    pub fn new(value: u8) -> Option<Self> {
        (value < 100).then_some(Self(value))
    }
    pub fn get(self) -> u8 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensSelection(pub u32);

fn byte(configuration: &mut Configuration<'_>, key: u32) -> Result<u8, RegionError> {
    configuration.read_byte(CfgKey(key))
}

enum Endpoint { Origin, First, Second, Third }

fn endpoint(calibration: &mut Calibration<'_>, channel: CalibrationChannel, mode: BiasBank, point: Endpoint) -> Result<i16, RegionError> {
    let count = channel.0 as i32;
    let key = {
        let configuration = calibration.configuration_mut();
        match point {
            Endpoint::Origin => None,
            Endpoint::First => Some(CfgKey(0x3e0572)),
            Endpoint::Second | Endpoint::Third => {
                if matches!(point, Endpoint::Third) && byte(configuration, 0x3e0313)? != 0
                    && count < byte(configuration, 0x3e1214)? as i32 {
                    Some(CfgKey(0x3e07cd))
                } else if count < byte(configuration, 0x3e1213)? as i32 {
                    Some(CfgKey(0x3e063b))
                } else {
                    Some(CfgKey(0x3e0572))
                }
            }
        }
    };
    let base = calibration.channel_bias(ChannelIndex(channel.0 as i32), mode)?.0;
    match key {
        None => Ok(base),
        Some(key) => Ok(calibration.configuration_mut().read_half(key.wrapping_add(channel.0 as u32 * 2))?.wrapping_add(base as u16) as i16),
    }
}

fn correction(configuration: &mut Configuration<'_>, channel: CalibrationChannel, component: usize, distance: u32, reverse: bool) -> Result<i16, RegionError> {
    let keys = [0x3e2800, 0x3e2992, 0x3e2b24, 0x3e2b24];
    let key = keys[component] + if reverse { 0xc9 } else { 0 };
    let value = configuration.read_signed_half(CfgKey(key + channel.0 as u32 * 2))? as i32;
    let factor = if distance == 0 { 0 } else {
        byte(configuration, distance + if reverse { 0x3e2cd4 } else { 0x3e2cb5 })?.min(240) as i32
    };
    Ok((value * factor / 240) as i16)
}

pub fn adjust_selection(calibration: &mut Calibration<'_>, channel: CalibrationChannel, mode: BiasBank, selection: LensSelection, value: i16) -> Result<i16, RegionError> {
    if selection.0 & 0xc0000000 != 0 { return Ok(value); }
    let position = (selection.0 as i8 as i32).clamp(-5, 55);
    if position == 25 { return Ok(value); }
    let reverse = position < 25;
    let distance = (position - 25).unsigned_abs();
    let points = [
        endpoint(calibration, channel, mode, Endpoint::Origin)?,
        endpoint(calibration, channel, mode, Endpoint::First)?,
        endpoint(calibration, channel, mode, Endpoint::Second)?,
        endpoint(calibration, channel, mode, Endpoint::Third)?,
    ];
    let configuration = calibration.configuration_mut();
    let mut adjusted = [0u16; 4];
    for index in 0..3 {
        adjusted[index] = (points[index] as u16).wrapping_add(correction(configuration, channel, index, distance, reverse)? as u16);
    }
    let segment = if value <= points[1] || channel.0 as i32 >= byte(configuration, 0x3e1213)? as i32 { 0 }
    else if byte(configuration, 0x3e0313)? != 0 && (value < points[1] || value > points[2])
        && (channel.0 as i32) < byte(configuration, 0x3e1214)? as i32 { 2 }
    else { 1 };
    if segment == 2 {
        adjusted[3] = (points[3] as u16).wrapping_add(correction(configuration, channel, 3, distance, reverse)? as u16);
    }
    let span = (points[segment + 1] as u16).wrapping_sub(points[segment] as u16) as i16;
    let delta = adjusted[segment + 1].wrapping_sub(adjusted[segment]);
    if span <= 0 || delta & 0x8000 != 0 { return Ok(value); }
    let product = (delta as i16 as i32).wrapping_mul(value as i32 - points[segment] as i32);
    let output = adjusted[segment].wrapping_add(product.wrapping_div(span as i32) as u16);
    let increment = (product.wrapping_mul(10).wrapping_div(span as i32) % 10 > 4) as u16;
    Ok(output.wrapping_add(increment) as i16)
}
