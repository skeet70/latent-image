use crate::backend::{BackendError, ConfigImages, ConfigSpan};
use crate::startup_radial_geometry::{build_primary, build_secondary, RadialRecord};
use crate::startup_radial_tables::{initialize_grid_tables, initialize_registered_tables, publish_channel_tables, ChannelSetup};

pub struct StartupWorkers {
    registered: RadialRecord,
    primary: RadialRecord,
    secondary: RadialRecord,
    channel_setup: ChannelSetup,
}
impl StartupWorkers {
    pub const fn new() -> Self {
        Self { registered: RadialRecord::new(), primary: RadialRecord::new(), secondary: RadialRecord::new(), channel_setup: ChannelSetup::new() }
    }
    pub fn channel_setup(&self) -> &ChannelSetup { &self.channel_setup }

    pub fn prepare_registered(&mut self, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        self.rebuild_registered(configuration)?;
        initialize_registered_tables(configuration)
    }

    pub fn initialize(&mut self, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        initialize_lookup(configuration)?;
        self.rebuild_primary(configuration)?;
        self.rebuild_secondary(configuration)?;
        initialize_grid_tables(configuration)?;
        initialize_reciprocals(configuration)
    }

    fn rebuild_registered(&mut self, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        if byte(configuration, ConfigurationKey(0x3e0009))? == 0 { return Ok(()); }
        let strength = i32::from(byte(configuration, ConfigurationKey(0x18f47))?);
        self.registered.clear();
        for attempt in 0..=5 {
            let mut complete = true;
            for (index, source) in [0x3420, 0x3410, 0x3400, 0x33f0, 0x33e0, 0x33d0].into_iter().enumerate() {
                let samples = read_samples(configuration, ConfigurationKey(source))?;
                if !build_primary(configuration, &samples, strength - attempt * 10, &mut self.registered)? {
                    complete = false;
                    break;
                }
                publish_record(configuration, ConfigurationKey(0x404b00 + index as u32 * 0x61), &self.registered)?;
            }
            if complete || attempt == 5 { return Ok(()); }
            self.registered.clear();
        }
        Ok(())
    }

    fn rebuild_primary(&mut self, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        if byte(configuration, ConfigurationKey(0x3e0009))? != 0 {
            return write(configuration, ConfigurationKey(0x403eaf), &[0]);
        }
        let strength = i32::from(byte(configuration, ConfigurationKey(0x18f47))?);
        let bank = selected_bank(configuration)?;
        self.primary.clear();
        let mut attempt = 0u8;
        let mut current = strength;
        loop {
            let mut complete = true;
            for curve in 0..15 {
                let samples = read_samples(configuration, ConfigurationKey(0x3e3a55 + (bank * 15 + curve) * 16))?;
                if !build_primary(configuration, &samples, current, &mut self.primary)? {
                    complete = false;
                    break;
                }
                publish_record(configuration, ConfigurationKey(0x403900 + curve * 0x61), &self.primary)?;
            }
            if complete { return write(configuration, ConfigurationKey(0x403eaf), &[1]); }
            if attempt >= byte(configuration, ConfigurationKey(0x3e3d27))? {
                return write(configuration, ConfigurationKey(0x403eaf), &[0]);
            }
            attempt = attempt.wrapping_add(1);
            current = strength - i32::from(byte(configuration, ConfigurationKey(0x3e3d26))?) * i32::from(attempt);
            self.primary.clear();
        }
    }

    fn rebuild_secondary(&mut self, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        if byte(configuration, ConfigurationKey(0x3e0009))? != 0 {
            return write(configuration, ConfigurationKey(0x403eaf), &[0]);
        }
        self.channel_setup.clear();
        let strength = i32::from(byte(configuration, ConfigurationKey(0x18f47))?);
        let bank = selected_bank(configuration)?;
        self.secondary.clear();
        let mut attempt = 0u8;
        let mut force = false;
        let mut current = strength;
        loop {
            let mut complete = true;
            for curve in 0..15 {
                let samples = read_samples(configuration, ConfigurationKey(0x3e3a55 + (bank * 15 + curve) * 16))?;
                if !build_secondary(configuration, &samples, current, &mut self.secondary)? && !force {
                    complete = false;
                    break;
                }
                let knots = self.secondary.knots();
                self.channel_setup.set_channel(curve as usize, knots, knots[0] >= knots[24]);
                let mut gains = [0; 4];
                gains[..2].copy_from_slice(&self.secondary.gain().to_le_bytes());
                gains[2..].copy_from_slice(&self.secondary.reciprocal().to_le_bytes());
                write(configuration, ConfigurationKey(0x403f00 + curve * 4), &gains)?;
                write(configuration, ConfigurationKey(0x404500 + curve * 4), &gains)?;
            }
            if complete || attempt >= byte(configuration, ConfigurationKey(0x3e3d27))? { break; }
            attempt = attempt.wrapping_add(1);
            force = attempt == byte(configuration, ConfigurationKey(0x3e3d27))?;
            current = strength - i32::from(byte(configuration, ConfigurationKey(0x3e3d26))?) * i32::from(attempt);
            self.secondary.clear();
            self.channel_setup.clear();
        }
        publish_channel_tables(configuration, &self.channel_setup)
    }
}
impl Default for StartupWorkers {
    fn default() -> Self { Self::new() }
}

fn selected_bank(configuration: &ConfigImages<'_>) -> Result<u32, BackendError> {
    Ok(match byte(configuration, ConfigurationKey(0x3e3a53))? { 1 => 1, 2 => 2, _ => 0 })
}

fn read_samples(configuration: &ConfigImages<'_>, source: ConfigurationKey) -> Result<[u16; 8], BackendError> {
    let mut samples = [0; 8];
    for (index, sample) in samples.iter_mut().enumerate() {
        *sample = half(configuration, ConfigurationKey(source.0 + index as u32 * 2))?;
    }
    Ok(samples)
}

fn publish_record(configuration: &mut ConfigImages<'_>, destination: ConfigurationKey, record: &RadialRecord) -> Result<(), BackendError> {
    let bytes = configuration.encoded_region_mut(ConfigSpan::new(destination.0, 0x61)?)?;
    for (word, output) in record.knots().iter().zip(bytes[..0x5d].chunks_exact_mut(3)) {
        output.copy_from_slice(&word.to_le_bytes()[..3]);
    }
    bytes[0x5d..0x5f].copy_from_slice(&record.gain().to_le_bytes());
    bytes[0x5f..].copy_from_slice(&record.reciprocal().to_le_bytes());
    Ok(())
}

#[derive(Clone, Copy)]
struct ConfigurationKey(u32);

fn byte(configuration: &ConfigImages<'_>, key: ConfigurationKey) -> Result<u8, BackendError> {
    configuration.byte(key.0).ok_or(BackendError::Capacity)
}

fn half(configuration: &ConfigImages<'_>, key: ConfigurationKey) -> Result<u16, BackendError> {
    Ok(u16::from_le_bytes([byte(configuration, key)?, byte(configuration, ConfigurationKey(key.0 + 1))?]))
}

fn write(configuration: &mut ConfigImages<'_>, key: ConfigurationKey, bytes: &[u8]) -> Result<(), BackendError> {
    configuration.write(ConfigSpan::new(key.0, bytes.len() as u32)?, bytes)
}

// X-M1 1.50 0x006e48c0, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; control flow recovered from authenticated id4 bytes.
pub fn initialize_lookup(configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
    let mut lookup = [1; 64];
    if byte(configuration, ConfigurationKey(0x3e0009))? == 0 {
        for column in 0..4 {
            for row in 0..4 {
                let sample = half(configuration, ConfigurationKey(0x3226 + column * 8 + row * 2))? as u32;
                let mut category = 8;
                for candidate in 1..8 {
                    let lower = half(configuration, ConfigurationKey(0x3e2716 + (candidate - 1) * 2))? as u32;
                    let upper = half(configuration, ConfigurationKey(0x3e2716 + candidate * 2))? as u32;
                    if sample < (lower + upper) / 2 {
                        category = candidate as u8;
                        break;
                    }
                }
                lookup[(column + row * 8) as usize] = category;
            }
        }
        for column in 0..4 {
            for row in 0..5 {
                let source = row * 8 + column;
                let destination = row * 8 + 7 - column;
                lookup[destination] = lookup[source];
            }
        }
        for destination in (32..=56).rev().step_by(8) {
            let source = 56 - destination;
            lookup.copy_within(source..source + 8, destination);
        }
    }
    write(configuration, ConfigurationKey(0x31d80), &lookup)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ReciprocalCurve(pub [u16; 28]);

impl ReciprocalCurve {
    pub fn from_samples(samples: [u8; 8]) -> Self {
        let mut words = [0u32; 28];
        for (index, sample) in samples.into_iter().enumerate() {
            words[3 + index * 3] = if sample == 0 { 0x800 } else { 0x7f800 / sample as u32 };
        }
        words[25] = words[24].wrapping_mul(4).wrapping_sub(words[21]) / 3;
        words[26] = words[24].wrapping_mul(5).wrapping_sub(words[21].wrapping_mul(2)) / 3;
        words[27] = words[24].wrapping_mul(2).wrapping_sub(words[21]);
        words[0] = 0x800;
        words[1] = 0x800;
        for index in 2..25 {
            match index % 3 {
                1 => words[index] = words[index - 1].wrapping_mul(4).wrapping_sub(words[index - 4]) / 3,
                2 => words[index] = words[index + 1].wrapping_mul(4).wrapping_sub(words[index + 4]) / 3,
                _ => {},
            }
        }
        Self(words.map(|word| word as u16))
    }
}

// 0x00745f7c delegates to 0x00745d48. Arithmetic is unsigned and wraps before division.
pub fn initialize_reciprocals(configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
    for bank in 0..4 {
        for channel in 0..6 {
            for curve in 0..15 {
                let source = 0x3e3170 + bank * 0x2d0 + channel * 0x78 + curve * 8;
                let mut samples = [0; 8];
                for (index, sample) in samples.iter_mut().enumerate() {
                    *sample = byte(configuration, ConfigurationKey(source + index as u32))?;
                }
                let curve_words = ReciprocalCurve::from_samples(samples);
                let mut bytes = [0; 56];
                for (word, output) in curve_words.0.into_iter().zip(bytes.chunks_exact_mut(2)) {
                    output.copy_from_slice(&word.to_le_bytes());
                }
                write(configuration, ConfigurationKey(0x404e00 + bank * 0x13b0 + channel * 0x348 + curve * 0x38), &bytes)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::{initialize_lookup, initialize_reciprocals, ReciprocalCurve};
    use crate::backend::{BackendError, ConfigImages};
    use std::vec;

    #[test]
    fn disabled_calibration_publishes_identity_lookup() {
        let mut bytes = vec![0; 0x420000];
        bytes[0x3e0009] = 1;
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        initialize_lookup(&mut configuration).unwrap();
        for key in 0x31d80..0x31dc0 { assert_eq!(configuration.byte(key), Some(1)); }
    }

    #[test]
    fn lookup_reflects_classification_across_both_axes() {
        let mut bytes = vec![0; 0x420000];
        for index in 0..8usize {
            bytes[0x3e2716 + index * 2..0x3e2718 + index * 2].copy_from_slice(&(index as u16 * 100).to_le_bytes());
        }
        bytes[0x3226..0x3228].copy_from_slice(&250u16.to_le_bytes());
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        initialize_lookup(&mut configuration).unwrap();
        for index in [0, 7, 56, 63] { assert_eq!(configuration.byte(0x31d80 + index), Some(4)); }
        assert_eq!(configuration.byte(0x31d81), Some(1));
    }

    #[test]
    fn zero_samples_produce_constant_unity_reciprocals() {
        assert_eq!(ReciprocalCurve::from_samples([0; 8]), ReciprocalCurve([0x800; 28]));
        assert_eq!(ReciprocalCurve::from_samples([255; 8]), ReciprocalCurve([0x800; 28]));
    }

    #[test]
    fn reciprocal_banks_cover_the_complete_published_extent() {
        let mut bytes = vec![0xa5; 0x420000];
        bytes[0x3e3170..0x3e3cb0].fill(255);
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        initialize_reciprocals(&mut configuration).unwrap();
        assert_eq!(configuration.byte(0x404dff), Some(0xa5));
        assert_eq!(configuration.byte(0x409cbf), Some(8));
        assert_eq!(configuration.byte(0x409cc0), Some(0xa5));
    }

    #[test]
    fn missing_inputs_do_not_report_success() {
        let mut bytes = [0; 2];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(initialize_lookup(&mut configuration), Err(BackendError::Capacity));
        assert_eq!(initialize_reciprocals(&mut configuration), Err(BackendError::Capacity));
    }
}
