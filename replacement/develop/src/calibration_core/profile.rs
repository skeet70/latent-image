use super::{Calibration, configuration::{CfgKey, ConfigurationError, ConfigurationMemory}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeltaIndex(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitIndex(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileSelector(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactorMode { Enabled, Disabled }
impl FactorMode { pub const fn from_native(value: i32) -> Self { if matches!(value, 0 | 2) { Self::Enabled } else { Self::Disabled } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureHundredths(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileRate(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClampClass(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Percent(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitTableIndex(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryIndex(pub i16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Triplet(pub [u16; 3]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClampFlags { pub upper: u32, pub lower: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeometryClamp { pub flags: ClampFlags, pub rate: ProfileRate }

pub trait Tables {
    fn split_keys(&self, index: SplitTableIndex) -> [CfgKey; 2];
    fn history_fraction(&self, index: HistoryIndex) -> u8;
}

pub trait Values {
    fn read(&self, lane: usize) -> u16;
    fn write(&mut self, lane: usize, value: u16);
}
impl Values for Triplet {
    fn read(&self, lane: usize) -> u16 { self.0[lane] }
    fn write(&mut self, lane: usize, value: u16) { self.0[lane] = value; }
}

pub trait FactorInput {
    fn mode(&self) -> FactorMode;
    fn split(&self) -> SplitIndex;
    fn selector(&self) -> ProfileSelector;
    fn deltas(&self) -> [DeltaIndex; 2];
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FactorRequest { pub mode: FactorMode, pub split: SplitIndex, pub selector: ProfileSelector, pub deltas: [DeltaIndex; 2] }
impl FactorInput for FactorRequest {
    fn mode(&self) -> FactorMode { self.mode }
    fn split(&self) -> SplitIndex { self.split }
    fn selector(&self) -> ProfileSelector { self.selector }
    fn deltas(&self) -> [DeltaIndex; 2] { self.deltas }
}

pub trait FactorBanks {
    fn read(&self, half: usize) -> u16;
    fn write(&mut self, half: usize, value: u16);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileFactors(pub [u16; 12]);
impl FactorBanks for ProfileFactors {
    fn read(&self, half: usize) -> u16 { self.0[half] }
    fn write(&mut self, half: usize, value: u16) { self.0[half] = value; }
}
struct Bank<'a, B>(&'a mut B, usize);
impl<B: FactorBanks> Values for Bank<'_, B> {
    fn read(&self, lane: usize) -> u16 { self.0.read(self.1 + lane) }
    fn write(&mut self, lane: usize, value: u16) { self.0.write(self.1 + lane, value); }
}

fn delta_key(index: DeltaIndex, first: bool) -> Option<CfgKey> {
    Some(CfgKey(match index.0 {
        0..=8 => (if first { 0x7ed7 } else { 0x7efb }) - index.0 as u32 * 2,
        10..=18 => (if first { 0x7eb5 } else { 0x7ed9 }) + (index.0 as u32 - 10) * 2,
        _ => return None,
    }))
}

impl<M: ConfigurationMemory> Calibration<'_, M> {
    pub fn delta_factors(&mut self, indices: [DeltaIndex; 2], output: &mut impl Values) -> Result<(), ConfigurationError<M::Error>> {
        output.write(2, 1024);
        for (lane, index) in indices.into_iter().enumerate() {
            let value = match delta_key(index, lane == 0) { Some(key) => self.configuration.read_half(key)?, None => 1024 };
            output.write(lane, value);
        }
        Ok(())
    }

    pub fn split_factors(&mut self, selection: SplitIndex, tables: &impl Tables, output: &mut impl Values) -> Result<(), ConfigurationError<M::Error>> {
        let index = (selection.0 as u8 as u32).min(self.split_count()?.get() - 1);
        let keys = tables.split_keys(SplitTableIndex(index as u8));
        output.write(0, self.configuration.read_half(keys[0])?);
        output.write(1, self.configuration.read_half(keys[1])?);
        Ok(())
    }

    pub fn selected_factors(&mut self, selection: ProfileSelector, output: &mut impl Values) -> Result<(), ConfigurationError<M::Error>> {
        let key = match selection.0 { 1 => Some(0x80d1), 2 => Some(0x80d5), 3 => Some(0x80e9), 4 => Some(0x80d9), 5 => Some(0x80dd), 6 => Some(0x80e1), 7..=12 => Some(0x80e5), 14 => Some(0x80ed), _ => None };
        if let Some(key) = key {
            output.write(0, self.configuration.read_half(CfgKey(key))?);
            output.write(1, self.configuration.read_half(CfgKey(key + 2))?);
        } else { output.write(0, 1024); output.write(1, 1024); }
        Ok(())
    }

    pub fn build_profile_factors(&mut self, input: &impl FactorInput, tables: &impl Tables, output: &mut impl FactorBanks) -> Result<(), ConfigurationError<M::Error>> {
        for half in [11, 0, 1, 3, 4, 6, 7] { output.write(half, 1024); }
        if input.mode() == FactorMode::Enabled {
            self.split_factors(input.split(), tables, &mut Bank(output, 3))?;
            self.selected_factors(input.selector(), &mut Bank(output, 6))?;
            self.delta_factors(input.deltas(), &mut Bank(output, 0))?;
        }
        for lane in 0..2 {
            let selected = output.read(6 + lane) as u64;
            let split = output.read(3 + lane) as u64;
            let delta = output.read(lane) as u64;
            output.write(9 + lane, (selected * split * delta >> 20) as u16);
        }
        Ok(())
    }

    pub fn calibrate_profile(&mut self, input: &impl FactorInput, tables: &impl Tables, values: &mut impl Values) -> Result<ProfileFactors, ConfigurationError<M::Error>> {
        let mut factors = ProfileFactors([0; 12]);
        self.build_profile_factors(input, tables, &mut factors)?;
        for lane in 0..3 {
            let value = values.read(lane) as u32;
            let result = if lane == 0 { value } else { value.wrapping_mul(factors.0[8 + lane] as u32).wrapping_add(512) >> 10 };
            values.write(lane, result as u16);
        }
        Ok(factors)
    }

    pub fn clamp_profile(&mut self, class: ClampClass, values: &mut impl Values, rate: ProfileRate) -> Result<ClampFlags, ConfigurationError<M::Error>> {
        let mut flags = ClampFlags { upper: 0, lower: 0 };
        if class.0 <= 1 {
            let maximum = [32767, self.configuration.read_half(CfgKey(0x7930))? as u32, self.configuration.read_half(CfgKey(0x7934))? as u32];
            let keys = if class.0 == 0 { [0x792a, 0x7928, 0x792c] } else { [0x7924, 0x7922, 0x7926] };
            let minimum = [self.configuration.read_half(CfgKey(keys[0]))? as u32, self.configuration.read_half(CfgKey(keys[1]))? as u32, self.configuration.read_half(CfgKey(keys[2]))? as u32];
            let scale = self.configuration.read_half(CfgKey(if class.0 == 0 { 0x7938 } else { 0x7936 }))? as u32;
            let ceiling = ((rate.0.wrapping_mul(scale) as i32) / 10) as u16 as u32;
            for lane in 0..3 {
                let previous = values.read(lane);
                if previous as u32 >= minimum[lane] { continue; }
                rescale(values, lane, minimum[lane], previous);
                flags.lower |= clamp_bit(class, lane as u32);
            }
            if flags.lower != 0 {
                let previous = values.read(0);
                if previous as u32 > ceiling { rescale(values, 0, ceiling, previous); flags.lower |= clamp_bit(class, 3); }
                for lane in 1..3 {
                    if (values.read(lane) as u32) < minimum[lane] { values.write(lane, minimum[lane] as u16); flags.lower |= clamp_bit(class, lane as u32 + 3); }
                }
            }
            for lane in 0..3 {
                if values.read(lane) as u32 > maximum[lane] { values.write(lane, maximum[lane] as u16); flags.upper |= clamp_bit(class, lane as u32); }
            }
        } else {
            let (maximum, minimum) = if class.0 == 2 {
                let second_maximum = self.configuration.read_half(CfgKey(0x7cba))?;
                let third_maximum = self.configuration.read_half(CfgKey(0x7cbe))?;
                let second_minimum = self.configuration.read_half(CfgKey(0x7cb4))?;
                let third_minimum = self.configuration.read_half(CfgKey(0x7cb8))?;
                let _ = self.configuration.read_half(CfgKey(0x7938))?;
                ([32767, second_maximum, third_maximum], [256, second_minimum, third_minimum])
            } else { ([32767; 3], [256; 3]) };
            for lane in 0..3 {
                let value = values.read(lane);
                if value > maximum[lane] { values.write(lane, maximum[lane]); flags.upper |= clamp_bit(class, lane as u32); }
                else if value < minimum[lane] { values.write(lane, minimum[lane]); flags.lower |= clamp_bit(class, lane as u32); }
            }
        }
        Ok(flags)
    }

    pub fn geometry_class(&mut self, exposure: ExposureHundredths) -> Result<ClampClass, ConfigurationError<M::Error>> {
        let threshold = self.configuration.read_byte(CfgKey(0x792e))? as i32 * 100;
        Ok(ClampClass(u32::from((exposure.0 as i16 as i32) < threshold)))
    }

    pub fn clamp_geometry(&mut self, values: &mut impl Values, rate: ProfileRate, exposure: ExposureHundredths) -> Result<GeometryClamp, ConfigurationError<M::Error>> {
        let class = self.geometry_class(exposure)?;
        let rate = ProfileRate(rate.0.max(10));
        let flags = self.clamp_profile(class, values, rate)?;
        Ok(GeometryClamp { flags, rate })
    }
}

fn clamp_bit(class: ClampClass, lane: u32) -> u32 { 1u32.checked_shl(((class.0 as u8 as u32) * 3 + lane) & 255).unwrap_or(0) }
fn rescale(values: &mut impl Values, lane: usize, target: u32, previous: u16) {
    for other in 0..3 {
        if other == lane { continue; }
        let product = (values.read(other) as u32).wrapping_mul(target);
        let result = if previous == 0 { product } else { ((product as i32) / previous as i32) as u32 };
        values.write(other, result as u16);
    }
    values.write(lane, target as u16);
}

pub fn history_percent(exposure: ExposureHundredths, tables: &impl Tables) -> Percent {
    let quotient = exposure.0 / 100;
    let index = exposure.0.wrapping_sub(quotient.wrapping_mul(100)).wrapping_add(100) as i16;
    let value = tables.history_fraction(HistoryIndex(index)) as u32;
    let quotient = quotient as i16;
    Percent(if quotient < 0 { 65536u32.checked_shr(quotient.wrapping_neg() as u32 & 255).unwrap_or(0).wrapping_mul(value) >> 16 }
        else { 1u32.checked_shl(quotient as u32 & 255).unwrap_or(0).wrapping_mul(value) })
}
pub fn scale_percent(values: &mut impl Values, percent: Percent) {
    for lane in 0..3 { values.write(lane, (values.read(lane) as u64 * percent.0 as u64 / 100) as u16); }
}
pub fn apply_positive(exposure: ExposureHundredths, values: &mut impl Values, tables: &impl Tables) {
    if exposure.0 > 0 { scale_percent(values, history_percent(exposure, tables)); }
}
