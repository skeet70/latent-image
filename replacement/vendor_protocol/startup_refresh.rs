//! Startup profile refresh at 0x007700fc, X-M1 1.50 pin 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Interpolation and curve rebuilding follow authenticated actual instruction extents.

use crate::metadata_calibration::{MetadataCalibration, RegionError};
use xm1_calibration_core::configuration::{CfgKey, Configuration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshChannel { First, Second, Third, Fourth }
impl RefreshChannel {
    const fn index(self) -> usize { match self { Self::First => 0, Self::Second => 1, Self::Third => 2, Self::Fourth => 3 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationWord(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelPosition(u8);
impl ChannelPosition {
    pub const fn new(value: u8) -> Option<Self> { if value < 55 { Some(Self(value)) } else { None } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurvePosition(u8);
impl CurvePosition {
    pub const fn new(value: u8) -> Option<Self> { if value < 31 { Some(Self(value)) } else { None } }
}

pub struct StartupRefresh {
    coefficients: [u16; 16],
    samples: [[u8; 44]; 8],
    weights: [[i16; 55]; 4],
    channels: [u8; 696],
    values: [[u16; 31]; 4],
}
impl StartupRefresh {
    pub const fn new() -> Self {
        Self { coefficients: [0; 16], samples: [[0; 44]; 8], weights: [[0; 55]; 4], channels: [0; 696], values: [[0; 31]; 4] }
    }
    pub fn channel(&self, channel: RefreshChannel, position: ChannelPosition) -> CalibrationWord {
        CalibrationWord(self.half(channel.index() * 110 + position.0 as usize * 2))
    }
    pub fn curve(&self, channel: RefreshChannel, position: CurvePosition) -> CalibrationWord {
        CalibrationWord(self.values[channel.index()][position.0 as usize])
    }
    pub fn channel_bytes(&self) -> &[u8; 696] { &self.channels }
    pub fn value_tables(&self) -> &[[u16; 31]; 4] { &self.values }
    pub fn refresh(&mut self, metadata: &mut MetadataCalibration<'_>) -> Result<(), RegionError> {
        let configuration = metadata.configuration();
        if configuration.read_byte(CfgKey(0x8e40))? != 0 {
            self.read_calibration(configuration)?;
            self.interpolate(configuration)?;
        }
        self.rebuild(configuration)
    }
    fn half(&self, offset: usize) -> u16 { u16::from_le_bytes([self.channels[offset], self.channels[offset + 1]]) }
    fn ratio(&self, index: usize) -> u64 {
        let offset = 664 + index * 8;
        u64::from_le_bytes(self.channels[offset..offset + 8].try_into().unwrap())
    }
    fn read_calibration(&mut self, configuration: &mut Configuration<'_>) -> Result<(), RegionError> {
        for (index, key) in [0x1d2a,0x1d2c,0x1d2e,0x1d30,0x1d36,0x1d38,0x1d32,0x1d34,0x1d3a,0x1d3c,0x1d3e,0x1d40,0x1d46,0x1d48,0x1d42,0x1d44].into_iter().enumerate() {
            self.coefficients[index] = configuration.read_half(CfgKey(key))?;
        }
        for index in 0..55 {
            let (first, second, row) = match index { 0..=20 => (0x8e45,0x8e9a,index), 21..=27 => (0x8eef,0x8f0c,index-21), 28..=34 => (0x8f29,0x8f46,index-28), _ => (0x8676,0x86c6,index-35) };
            for (channel, base) in [first,first+2,second,second+2].into_iter().enumerate() {
                self.weights[channel][index] = configuration.read_half(CfgKey(base + row as u32 * 4))? as i16;
            }
        }
        for (channel, base) in [0x10587,0x3e4380,0x105b4,0x1052e,0x10502,0x1055a,0x105e1,0x1060e].into_iter().enumerate() {
            for index in 0..44 { self.samples[channel][index] = configuration.read_byte(CfgKey(base + index as u32))?; }
        }
        Ok(())
    }
    fn interpolate(&mut self, configuration: &mut Configuration<'_>) -> Result<(), RegionError> {
        let mut ratios = [1024u64; 4];
        if configuration.read_byte(CfgKey(0x3e0009))? != 1 && configuration.read_byte(CfgKey(0x10500))? != 0 {
            let mut sums = [[[0u64; 3]; 2]; 2];
            let packed = configuration.read_byte(CfgKey(0xa8))? & 0x80 != 0;
            for index in 0..44 {
                for side in 0..2 {
                    for bank in 0..2 {
                        for term in 0..3 {
                            let product = self.samples[side][index] as u32 * self.samples[2][index] as u32 * self.samples[3 + term][index] as u32 * self.samples[6 + bank][index] as u32;
                            sums[side][bank][term] += product as u64;
                        }
                    }
                }
                if packed {
                    for (output, input) in [0,1,3,4,5].into_iter().enumerate() {
                        let offset = 440 + output * 44 + index / 4 * 4;
                        let mut word = u32::from_le_bytes(self.channels[offset..offset + 4].try_into().unwrap());
                        word |= (self.samples[input][index] as u32) << (24 - index % 4 * 8);
                        self.channels[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
                    }
                }
            }
            if sums.iter().flatten().flatten().all(|value| *value != 0) {
                for bank in 0..2 {
                    let reference = (sums[1][bank][0] << 10) / sums[0][bank][0];
                    for term in 1..3 {
                        ratios[bank + (term - 1) * 2] = (reference << 10) / ((sums[1][bank][term] << 10) / sums[0][bank][term]);
                    }
                }
            }
        }
        for (index, ratio) in ratios.into_iter().enumerate() { self.channels[664 + index * 8..672 + index * 8].copy_from_slice(&ratio.to_le_bytes()); }
        let scale = configuration.read_half(CfgKey(0x8e42))? as i64;
        for index in 0..55 {
            let special = index <= 10 && (1472u32 & (1 << index)) != 0;
            let base = if special { 4 } else { 0 };
            for channel in 0..4 {
                let bank = channel % 2;
                let group = channel / 2;
                let first = self.coefficients[base + bank] as i64;
                let second = self.coefficients[base + 2 + bank] as i64;
                let (first, second) = if group == 0 { (first,second) } else { (self.coefficients[base + 8 + bank] as i64,self.coefficients[base + 10 + bank] as i64) };
                let weight = self.weights[channel][index] as i64;
                let ratio0 = ratios[bank * 2] as i64;
                let ratio1 = ratios[bank * 2 + 1] as i64;
                let mixed = weight.wrapping_mul(second).wrapping_mul(ratio1).wrapping_add((1024 - weight).wrapping_mul(first).wrapping_mul(ratio0));
                let value = mixed.wrapping_mul(scale).wrapping_add(1 << 29) / (1 << 30);
                let offset = channel * 110 + index * 2;
                self.channels[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
            }
        }
        Ok(())
    }
    fn rebuild(&mut self, configuration: &mut Configuration<'_>) -> Result<(), RegionError> {
        let mut record = [0u16; 85];
        for (index, key) in [0x7cb6,0x1c76,0x1c74,0x1c06,0x1c00,0x1c08,0x1c02,0x1c0a,0x1c04,0x8186,0x8182,0x8188,0x8184,0x1c16,0x1c10,0x1c18,0x1c12,0x1c1a,0x1c14,0x818e,0x818a,0x8190,0x818c].into_iter().enumerate() {
            record[62 + index] = configuration.read_half(CfgKey(key))?;
        }
        for index in 0..31 {
            record[index] = configuration.read_half(CfgKey(0x8102 + index as u32 * 2))?;
            record[index + 31] = configuration.read_half(CfgKey(0x8142 + index as u32 * 2))?;
        }
        let gains = if configuration.read_byte(CfgKey(0x8e40))? == 0 { [1024; 4] } else { [self.ratio(1) as u32,self.ratio(0) as u32,self.ratio(3) as u32,self.ratio(2) as u32] };
        for (index, x) in CURVE_COORDINATES.into_iter().enumerate() {
            for group in 0..2 {
                let terms = 65 + group * 10;
                let a = record[64] as i64;
                let b = record[63] as i64;
                for side in 0..2 {
                    let order = if group == 0 { gains } else { [gains[3],gains[2],gains[1],gains[0]] };
                    let gain_a = order[side * 2];
                    let gain_b = order[side * 2 + 1];
                    let first = (x as i64 - a).wrapping_mul(record[terms] as i64).wrapping_mul(gain_a as i64).wrapping_shl(13);
                    let second = (x as i64 - b).wrapping_mul(record[terms + 1] as i64).wrapping_mul(gain_b as i64).wrapping_shl(13);
                    let d0 = record[terms + 2 + side * 2] as i64 * record[terms + 6 + side * 2] as i64;
                    let d1 = record[terms + 3 + side * 2] as i64 * record[terms + 7 + side * 2] as i64;
                    let first = if d0 == 0 { first } else { first.wrapping_div(d0) };
                    let second = if d1 == 0 { second } else { second.wrapping_div(d1) };
                    let factor = if a == b { (first as u32).wrapping_sub(second as u32).wrapping_shl(13) >> 10 } else { (first.wrapping_sub(second).wrapping_shl(13).wrapping_div(b - a) as u32) >> 10 };
                    let sample = record[index + side * 31] as u64 * 10;
                    let intermediate = ((sample.wrapping_mul(factor as u64) >> 13) as u32).wrapping_add(5) as i32 / 10;
                    let product = (intermediate as u32).wrapping_mul(record[62] as u32 * 10);
                    let value = (product >> 13).wrapping_add(5) as i32 / 10;
                    self.values[group * 2 + side][index] = value as u16;
                }
            }
        }
        Ok(())
    }
}
impl Default for StartupRefresh { fn default() -> Self { Self::new() } }
const CURVE_COORDINATES: [u32; 31] = [2500,2550,2650,2700,2800,2850,2950,3000,3100,3200,3300,3400,3600,3700,3800,4000,4200,4300,4500,4800,5000,5300,5600,5900,6300,6700,7100,7700,8300,9100,10000];
