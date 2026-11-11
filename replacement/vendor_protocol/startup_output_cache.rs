//! X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3:
//! 0x7450b0 exports 75 pairs of 28 signed halfwords at 0x408a00;
//! 0x744f34 builds each spline from eight signed configuration bytes.
//! Authenticated actual instructions resolve both NONMATCHING reconstructions.
//! 0x73fd60 clears the 31 channel and 10 threshold validity flags without changing values.

use crate::backend::{BackendError, ConfigImages, ConfigSpan};

pub const OUTPUT_BASE: u32 = 0x408a00;
pub const OUTPUT_BYTES: usize = 5 * 15 * 112;
const INPUT_BASE: u32 = 0x3e3dd0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelLookup {
    Channel(ConfigurationChannel),
    Unsupported,
}
impl ChannelLookup {
    pub const fn from_raw(value: u32) -> Self {
        if value < 31 { Self::Channel(ConfigurationChannel(value as u8)) }
        else { Self::Unsupported }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationChannel(u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LookupThreshold(pub u32);

#[derive(Clone, Copy)]
struct LookupEntry { value: u8, valid: bool }

pub struct ConfigurationLookupCache { entries: [LookupEntry; 41] }
impl ConfigurationLookupCache {
    pub const fn new() -> Self {
        Self { entries: [LookupEntry { value: 0, valid: false }; 41] }
    }
    pub fn invalidate(&mut self) {
        for entry in &mut self.entries { entry.valid = false; }
    }
    pub fn channel(&mut self, configuration: &ConfigImages<'_>, channel: ChannelLookup) -> Result<u8, BackendError> {
        match channel {
            ChannelLookup::Unsupported => Ok(0x40),
            ChannelLookup::Channel(channel) => self.read(configuration, channel.0 as usize),
        }
    }
    pub fn threshold(&mut self, configuration: &ConfigImages<'_>, threshold: LookupThreshold) -> Result<u8, BackendError> {
        let mut band = 0;
        while band < 9 {
            let limit = configuration.byte(0x3e451a + band as u32).ok_or(BackendError::Capacity)?;
            if u32::from(limit) >= threshold.0 { break; }
            band += 1;
        }
        self.read(configuration, 31 + band)
    }
    fn read(&mut self, configuration: &ConfigImages<'_>, index: usize) -> Result<u8, BackendError> {
        let entry = &mut self.entries[index];
        if !entry.valid {
            // Actual tables at 0x829a5c and 0x829a10 contain consecutive configuration offsets.
            entry.value = configuration.byte(0x3e44ed + index as u32).ok_or(BackendError::Capacity)?;
            entry.valid = true;
        }
        Ok(entry.value)
    }
}
impl Default for ConfigurationLookupCache {
    fn default() -> Self { Self::new() }
}

pub struct StartupOutputCache;
impl StartupOutputCache {
    pub fn rebuild(configuration: &mut ConfigImages<'_>, lookup: &mut ConfigurationLookupCache) -> Result<(), BackendError> {
        for row in 0..75 {
            let mut output = [0u8; 112];
            for component in 0..2 {
                let mut knots = [0i16; 8];
                for (index, knot) in knots.iter_mut().enumerate() {
                    let address = INPUT_BASE + row * 16 + component * 8 + index as u32;
                    *knot = i16::from(configuration.byte(address).ok_or(BackendError::Capacity)? as i8) * 2;
                }
                let spline = spline(knots);
                for (index, value) in spline.iter().enumerate() {
                    let offset = component as usize * 56 + index * 2;
                    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
                }
            }
            configuration.write(ConfigSpan::new(OUTPUT_BASE + row * 112, 112)?, &output)?;
        }
        lookup.invalidate();
        Ok(())
    }
}

fn floor_tenths(value: i32) -> i32 {
    let quotient = value / 10;
    if value % 10 < 0 { quotient - 1 } else { quotient }
}

fn spline(knots: [i16; 8]) -> [i16; 28] {
    let mut output = [0i16; 28];
    for (index, knot) in knots.into_iter().enumerate() { output[(index + 1) * 3] = knot; }
    output[0] = output[3] * 2 - output[6];
    output[1] = floor_tenths((i32::from(output[3]) + i32::from(output[0]) * 2) * 10 / 3) as i16;
    for index in (0..24).step_by(3) {
        let next = i32::from(output[index + 3]);
        output[index + 4] = (next + floor_tenths((next - i32::from(output[index])) * 10 / 3)) as i16;
    }
    output[26] = output[25] * 2 - output[24];
    output[27] = output[25] * 3 - output[24] * 2;
    for index in (0..24).step_by(3) {
        let next = i32::from(output[index + 3]);
        output[index + 2] = (next + floor_tenths((next - i32::from(output[index + 6])) * 10 / 3)) as i16;
    }
    for value in &mut output { *value = (*value).clamp(-512, 511); }
    output
}

#[cfg(test)]
mod tests {
    use super::{ChannelLookup, ConfigurationLookupCache, LookupThreshold, StartupOutputCache, OUTPUT_BASE, OUTPUT_BYTES};
    use crate::backend::{BackendError, ConfigImages, ConfigSpan};

    #[test]
    fn exports_all_groups_and_both_components_without_changing_neighbors() {
        let mut bytes = std::vec![0xa5; 0x420000];
        for row in 0..75 {
            bytes[0x3e3dd0 + row * 16..0x3e3dd8 + row * 16].fill(row as u8);
            bytes[0x3e3dd8 + row * 16..0x3e3de0 + row * 16].fill((-1 - row as i8) as u8);
        }
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let mut lookup = ConfigurationLookupCache::new();
        StartupOutputCache::rebuild(&mut configuration, &mut lookup).unwrap();
        for row in 0..75 {
            for halfword in 0..56 {
                let address = OUTPUT_BASE + row * 112 + halfword * 2;
                let value = i16::from_le_bytes([configuration.byte(address).unwrap(), configuration.byte(address + 1).unwrap()]);
                let expected = if halfword < 28 { row as i16 * 2 } else { (-1 - row as i16) * 2 };
                assert_eq!(value, expected);
            }
        }
        assert_eq!(configuration.byte(OUTPUT_BASE - 1), Some(0xa5));
        assert_eq!(configuration.byte(OUTPUT_BASE + OUTPUT_BYTES as u32), Some(0xa5));
    }

    #[test]
    fn signed_knots_round_down_extrapolate_and_saturate() {
        let mut bytes = std::vec![0; 0x420000];
        bytes[0x3e3dd0..0x3e3dd8].copy_from_slice(&[128, 127, 0, 1, 255, 2, 254, 3]);
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        StartupOutputCache::rebuild(&mut configuration, &mut ConfigurationLookupCache::new()).unwrap();
        let expected: [i16; 28] = [-512,-512,-426,-256,-86,338,254,424,-1,0,-85,3,2,2,-4,-2,-4,6,4,6,-8,-4,-7,3,6,9,12,15];
        for (index, value) in expected.into_iter().enumerate() {
            let address = OUTPUT_BASE + index as u32 * 2;
            assert_eq!(i16::from_le_bytes([configuration.byte(address).unwrap(), configuration.byte(address + 1).unwrap()]), value);
        }
    }

    #[test]
    fn lookup_is_lazy_and_rebuild_invalidates_both_banks() {
        let mut bytes = std::vec![0; 0x420000];
        bytes[0x3e44ed] = 11;
        bytes[0x3e450c] = 22;
        bytes[0x3e4515] = 33;
        bytes[0x3e451a..0x3e4523].copy_from_slice(&[10,20,30,40,50,60,70,80,90]);
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let mut lookup = ConfigurationLookupCache::new();
        assert_eq!(lookup.channel(&configuration, ChannelLookup::from_raw(0)), Ok(11));
        assert_eq!(lookup.channel(&configuration, ChannelLookup::from_raw(31)), Ok(0x40));
        assert_eq!(lookup.threshold(&configuration, LookupThreshold(10)), Ok(22));
        assert_eq!(lookup.threshold(&configuration, LookupThreshold(91)), Ok(33));
        configuration.write(ConfigSpan::new(0x3e44ed, 1).unwrap(), &[44]).unwrap();
        configuration.write(ConfigSpan::new(0x3e450c, 1).unwrap(), &[55]).unwrap();
        configuration.write(ConfigSpan::new(0x3e4515, 1).unwrap(), &[66]).unwrap();
        assert_eq!(lookup.channel(&configuration, ChannelLookup::from_raw(0)), Ok(11));
        assert_eq!(lookup.threshold(&configuration, LookupThreshold(10)), Ok(22));
        StartupOutputCache::rebuild(&mut configuration, &mut lookup).unwrap();
        assert_eq!(lookup.channel(&configuration, ChannelLookup::from_raw(0)), Ok(44));
        assert_eq!(lookup.threshold(&configuration, LookupThreshold(10)), Ok(55));
        assert_eq!(lookup.threshold(&configuration, LookupThreshold(u32::MAX)), Ok(66));
    }

    #[test]
    fn truncated_configuration_reports_capacity_without_invalidating_lookup() {
        let mut bytes = std::vec![0; 0x3e4523];
        bytes[0x3e44ed] = 7;
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let mut lookup = ConfigurationLookupCache::new();
        assert_eq!(lookup.channel(&configuration, ChannelLookup::from_raw(0)), Ok(7));
        configuration.write(ConfigSpan::new(0x3e44ed, 1).unwrap(), &[8]).unwrap();
        assert_eq!(StartupOutputCache::rebuild(&mut configuration, &mut lookup), Err(BackendError::Capacity));
        assert_eq!(lookup.channel(&configuration, ChannelLookup::from_raw(0)), Ok(7));
    }
}
