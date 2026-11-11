//! X-M1 profile normalization, calibrated channel mappings, and interpolation.
//! Authority: x70view55436fc85c0560dd252b0410af6f2427fe198cd3; image4 ranges in record_profile_mapping_evidence.json.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RecordError};
use crate::record_configuration::{helper_69f8fc as cfg_byte, helper_69fec8 as cfg_half, helper_69ff68 as cfg_signed};
use xm1_calibration_core::{BiasBank, Calibration, ChannelIndex, CurveClass, CurveIndex, Tenths};
use xm1_calibration_core::configuration::{ConfigAddress, Configuration, ConfigurationError, ConfigurationMemory};

pub struct GuestCalibrationMemory<'a, M, E> { context: &'a mut RecordContext<M, E> }
impl<M: GuestAddressSpace, E> ConfigurationMemory for GuestCalibrationMemory<'_, M, E> {
    type Error = MemoryError;
    fn read_byte(&mut self, address: ConfigAddress) -> Result<u8, ConfigurationError<Self::Error>> { self.context.read_u8(GuestAddress::new(address.0)).map_err(ConfigurationError::Backend) }
    fn read_half(&mut self, address: ConfigAddress) -> Result<u16, ConfigurationError<Self::Error>> { self.context.read_u16(GuestAddress::new(address.0)).map_err(ConfigurationError::Backend) }
    fn read_word(&mut self, address: ConfigAddress) -> Result<u32, ConfigurationError<Self::Error>> { self.context.read_u32(GuestAddress::new(address.0)).map_err(ConfigurationError::Backend) }
}
fn error(error: ConfigurationError<MemoryError>) -> MemoryError {
    match error {
        ConfigurationError::Backend(error) => error,
        ConfigurationError::OutOfBounds { address, .. } | ConfigurationError::PhysicalBorrowUnavailable { address, .. } => MemoryError::Unmapped(GuestAddress::new(address.0)),
        ConfigurationError::RegionAddressOverflow { .. } => MemoryError::AddressOverflow,
    }
}
fn calibration<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<Calibration<'_, GuestCalibrationMemory<'_, M, E>>, MemoryError> {
    let base = ConfigAddress(context.read_u32(GuestAddress::new(0x96ca50))?);
    Ok(Calibration::new(Configuration::new(GuestCalibrationMemory { context }, base)))
}
fn narrow(value: i32) -> u32 { value as i16 as i32 as u32 }
fn round(value: i32) -> i32 { xm1_calibration_core::round_tenths(Tenths(value)).0 }
fn signed_half<M: GuestAddressSpace>(memory: &mut M, address: u32) -> Result<i32, MemoryError> { memory.read_u16(GuestAddress::new(address)).map(|value| value as i16 as i32) }
fn mapped_word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, bank: u32) -> Result<i32, MemoryError> {
    calibration(context)?.mapped_word(CurveIndex(index), ChannelIndex(bank as i32)).map(|value| value.0 as i32).map_err(error)
}
pub fn helper_74677c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { calibration(context)?.split_count().map(|value| value.get()).map_err(error) }
pub fn helper_7467a4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { calibration(context)?.classify_curve_index(CurveIndex(value)).map(|value| value as u32).map_err(error) }
pub fn helper_746804<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(match calibration(context)?.classify_curve_index(CurveIndex(value)).map_err(error)? { CurveClass::LowerBank => value.wrapping_add(31), CurveClass::UpperBank => value, CurveClass::Invalid => 0 })
}
pub fn helper_746834<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { calibration(context)?.collapse_curve_index(CurveIndex(value)).map(|value| value.0).map_err(error) }
pub fn helper_746864<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, bank: u32) -> Result<u32, MemoryError> {
    let index = helper_746834(context, value)?.wrapping_add(bank.wrapping_mul(31));
    let entry = cfg_byte(context, 0x3f4600u32.wrapping_add(index))?;
    if entry == 255 { Ok(100) } else { cfg_half(context, (0x1f1298u32.wrapping_add(entry)).wrapping_mul(2)) }
}
pub fn helper_746944<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, bank: u32) -> Result<u32, MemoryError> { Ok(narrow(round(mapped_word(context, value, bank)?))) }
pub fn helper_7469a0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { helper_746944(context, value, 0) }
fn correction<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: i32) -> Result<i32, MemoryError> {
    if value < -100 { return cfg_signed(context, 0x1d71); }
    let mut index = 1u32;
    while index < 36 && value >= signed_half(context, 0x818994 + index * 8)? { index += 1; }
    if index == 36 { return cfg_signed(context, 0x1d94); }
    let high = signed_half(context, 0x818994 + index * 8)?;
    let upper_key = context.read_u32(GuestAddress::new(0x818990 + index * 8))?;
    let upper = cfg_signed(context, upper_key)?;
    let low = signed_half(context, 0x818994 + (index - 1) * 8)?;
    let lower_key = context.read_u32(GuestAddress::new(0x818990 + (index - 1) * 8))?;
    let lower = cfg_signed(context, lower_key)?;
    Ok(if high == low { 0 } else { value.wrapping_sub(low).wrapping_mul(upper.wrapping_sub(lower)).wrapping_div(high.wrapping_sub(low)).wrapping_add(lower) as i16 as i32 })
}
pub fn helper_746a80<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, bank: u32) -> Result<u32, MemoryError> {
    if cfg_byte(context, 0x3e0009)? == 1 { return Ok(0); }
    let normalized = helper_746834(context, value)?;
    let offset = if helper_7467a4(context, value)? == 1 { 0x3e0220u32 } else { 0x3e0200u32 };
    let base = cfg_signed(context, offset.wrapping_add(normalized))?;
    let rounded = helper_746944(context, value, bank)? as i32;
    let total = base.wrapping_add(correction(context, rounded)?) as u16;
    let negative = total & 0x8000 != 0;
    let mut magnitude = if negative { total.wrapping_neg() } else { total };
    let (upper, lower) = if negative { (0x1db5, 0x1db6) } else { (0x1db3, 0x1db4) };
    if magnitude as i16 as i32 >= cfg_byte(context, upper)? as i32 * 10 { magnitude = (cfg_byte(context, upper)? * 10) as u16; }
    if magnitude as i16 as i32 <= cfg_byte(context, lower)? as i32 * 10 { magnitude = (cfg_byte(context, lower)? * 10) as u16; }
    Ok(narrow((if negative { magnitude.wrapping_neg() } else { magnitude }) as i16 as i32))
}
pub fn helper_746bf8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, bank: u32) -> Result<u32, MemoryError> {
    let first = helper_746944(context, value, 0)?;
    Ok(narrow(first.wrapping_add(helper_746a80(context, value, bank)?) as i32))
}
pub fn helper_746c2c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<u32, MemoryError> {
    let first = mapped_word(context, 0, bank)?;
    Ok(narrow(round(first.wrapping_sub(mapped_word(context, 0, 0)?))))
}
fn lower_index<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, value: u32) -> Result<u32, MemoryError> {
    let mut index = cfg_byte(context, bank.wrapping_add(0x3e22c8))?;
    let upper = cfg_byte(context, bank.wrapping_add(0x3e232d))?;
    while index < upper && helper_746864(context, index, bank)? < value { index += 1; }
    Ok(index)
}
fn upper_index<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, value: u32) -> Result<u32, MemoryError> {
    let lower = cfg_byte(context, bank.wrapping_add(0x3e22c8))?;
    let mut index = cfg_byte(context, bank.wrapping_add(0x3e232d))?;
    if lower >= index { return Ok(index); }
    while helper_746864(context, index, bank)? > value { index -= 1; if index <= lower { return Ok(lower); } }
    Ok(index)
}
pub fn helper_74714c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: u32, value: u32, destination: u32) -> Result<u32, MemoryError> {
    if cfg_byte(context, 0x3e0009)? == 1 { return Ok(0); }
    let normalized = helper_746834(context, value)?;
    let lower = cfg_byte(context, source.wrapping_add(0x3e22c8))?;
    let upper = cfg_byte(context, source.wrapping_add(0x3e232d))?;
    let value = helper_746864(context, normalized.max(lower).min(upper), source)?;
    let upper = upper_index(context, destination, value)?;
    let lower = lower_index(context, destination, value)?;
    if upper == lower { return Ok(upper); }
    let upper_distance = helper_746864(context, upper, destination)?.abs_diff(value) as u16;
    let lower_distance = helper_746864(context, lower, destination)?.abs_diff(value) as u16;
    Ok(if upper_distance <= lower_distance { upper } else { lower })
}
pub fn helper_75c874<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress) -> Result<u32, MemoryError> { context.read_u32(profile.checked_add(4)?) }
pub fn helper_75c8a4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress) -> Result<u32, MemoryError> { context.read_u16(profile.checked_add(8)?).map(|value| value as i16 as i32 as u32) }
pub fn helper_76dbb4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { let value = context.read_u32(GuestAddress::new(0x942b88))?; helper_746834(context, value) }
pub fn helper_747aac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, from: u32, to: u32) -> Result<u32, MemoryError> {
    let offset = cfg_half(context, 0x3e0240)?;
    Ok(narrow(if from == 1 && to == 0 { value.wrapping_add(offset) } else if from == 0 && to == 1 { value.wrapping_sub(offset) } else { value } as i32))
}
pub fn helper_747374<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, selector: u32) -> Result<u32, MemoryError> {
    calibration(context)?.channel_bias(ChannelIndex(bank as i32), if selector == 254 { BiasBank::Alternate } else { BiasBank::Primary }).map(|value| value.0 as i32 as u32).map_err(error)
}
fn curve_limits<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, channel: u32, base: &[i32; 7]) -> Result<[i32; 7], MemoryError> {
    let mut output = *base;
    for (index, value) in output.iter_mut().enumerate() {
        for step in 1..=channel.min(9) {
            let offset = bank.wrapping_mul(63).wrapping_add(0x3ec4af).wrapping_add(index as u32 * 9).wrapping_add(step);
            let adjustment = cfg_signed(context, offset)?.wrapping_mul(cfg_signed(context, 0x3e270c)?);
            *value = value.wrapping_add(adjustment);
        }
    }
    if channel > 9 {
        for index in [0usize, 2, 4, 6] {
            for step in 10..=channel {
                let offset = bank.wrapping_mul(84).wrapping_add(0x3edd42).wrapping_add(index as u32 / 2 * 21).wrapping_add(step);
                output[index] = output[index].wrapping_add(cfg_signed(context, offset)?.wrapping_mul(cfg_signed(context, 0x3e270c)?));
            }
        }
        for index in [0usize, 2, 4] {
            let numerator = i64::from(base[index + 1].wrapping_sub(base[index])) * i64::from(output[index + 2].wrapping_sub(output[index]));
            let denominator = base[index + 2].wrapping_sub(base[index]);
            let interpolated = if denominator == 0 { numerator as i32 } else { (numerator / i64::from(denominator)) as i32 };
            output[index + 1] = output[index].wrapping_add(interpolated);
        }
    }
    Ok(output)
}
pub fn helper_746454<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, channel: u32, source: GuestAddress, output: GuestAddress) -> Result<(), MemoryError> {
    let mut curve = [0; 7];
    for (index, value) in curve.iter_mut().enumerate() { *value = context.read_u32(source.checked_add(index as u32 * 4)?)? as i32; }
    let curve = curve_limits(context, bank, channel, &curve)?;
    for (index, value) in curve.into_iter().enumerate() { context.write_u32(output.checked_add(index as u32 * 4)?, value as u32)?; }
    Ok(())
}
pub fn helper_74947c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, bank: u32, source: u32, destination: u32, _unused_a: u32, _unused_b: u32, direction: u32) -> Result<u32, MemoryError> {
    let mut value = helper_747aac(context, value, direction, 1)? as i32;
    if source != destination && source < helper_74677c(context)? && destination < helper_74677c(context)? {
        let base = helper_747374(context, bank, 0)? as i32;
        let mut curve = [base.wrapping_mul(10); 7];
        for index in 1..7 { curve[index] = base.wrapping_add(cfg_half(context, bank.wrapping_mul(12).wrapping_add((index as u32 - 1) * 2).wrapping_add(0x3ec000))? as i32).wrapping_mul(10); }
        let source_curve = curve_limits(context, bank, source, &curve)?;
        let destination_curve = curve_limits(context, bank, destination, &curve)?;
        let input = value.wrapping_mul(10);
        let mut index = 1;
        while index < 6 && input >= source_curve[index] { index += 1; }
        let low = source_curve[index - 1];
        let high = source_curve[index];
        let mapped = if high == low { destination_curve[index] } else {
            let numerator = i64::from(input.wrapping_sub(low)) * i64::from(destination_curve[index].wrapping_sub(destination_curve[index - 1]));
            destination_curve[index - 1].wrapping_add((numerator / i64::from(high.wrapping_sub(low))) as i32)
        };
        value = round(mapped) as u16 as i16 as i32;
    }
    helper_747aac(context, value as u32, 1, direction)
}

#[derive(Clone, Copy)]
pub enum ProfileCallback { ChargeTick }
pub trait MappingEffects {
    fn interrupt_lock(&mut self) -> u32;
    fn interrupt_unlock(&mut self, saved: u32);
    fn semaphore_wait(&mut self, id: u32, timeout: i32) -> i32;
    fn semaphore_signal(&mut self, id: u32) -> i32;
    fn current_task(&mut self) -> u32;
    fn current_tick(&mut self) -> Result<u32, RecordError>;
    fn register_cyclic(&mut self, period: u32, mode: u32, callback: ProfileCallback, output: GuestAddress) -> Result<i32, RecordError>;
    fn unregister_cyclic(&mut self, token: u32) -> Result<i32, RecordError>;
    fn post_empty_packet(&mut self, queue: u8, operation: u16, sender: u8, destination: u8) -> Result<i32, RecordError>;
}
pub fn helper_740888<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: GuestAddress, second: GuestAddress, third: GuestAddress, fourth: GuestAddress) -> Result<(), MemoryError> {
    for (output, offset) in [(first, 0x34), (second, 0x38), (third, 0x3c), (fourth, 0x40)] {
        if output.get() != 0 { let value = context.read_u32(GuestAddress::new(0x914db8 + offset))?; context.write_u32(output, value)?; }
    }
    Ok(())
}
pub fn helper_743dc8<M: GuestAddressSpace, E: MappingEffects>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), RecordError> { let tick = context.effects.current_tick()?; context.write_u32(output, tick)?; Ok(()) }
pub fn helper_746054<M: GuestAddressSpace, E: MappingEffects>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u8(GuestAddress::new(0x93adf1))? == 0 { return Ok(0); }
    let saved = context.effects.interrupt_lock();
    let kind = context.read_u8(GuestAddress::new(0x96b381));
    context.effects.interrupt_unlock(saved);
    Ok(match kind? { 1 => 2, 2 => 3, _ => 1 })
}
pub fn charge_tick<M: GuestAddressSpace, E: MappingEffects>(context: &mut RecordContext<M, E>) -> Result<(), RecordError> { let task = context.effects.current_task() as u8; context.effects.post_empty_packet(80, 0x6000, task, 80)?; Ok(()) }
pub fn helper_7409f0<M: GuestAddressSpace, E: MappingEffects>(context: &mut RecordContext<M, E>) -> Result<i32, RecordError> {
    context.effects.semaphore_wait(66, -1);
    let result = context.write_u32(GuestAddress::new(0x9552f4), 1);
    context.effects.semaphore_signal(66);
    result?;
    context.effects.register_cyclic(200, 0, ProfileCallback::ChargeTick, GuestAddress::new(0x89f7ac))
}
pub fn helper_740a00<M: GuestAddressSpace, E: MappingEffects>(context: &mut RecordContext<M, E>) -> Result<i32, RecordError> {
    let token = context.read_u32(GuestAddress::new(0x89f7ac))?;
    context.effects.unregister_cyclic(token)?;
    context.effects.semaphore_wait(66, -1);
    let result = context.write_u32(GuestAddress::new(0x9552f4), 0);
    let status = context.effects.semaphore_signal(66);
    result?;
    Ok(status)
}
