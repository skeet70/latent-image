use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

use super::record_configuration::{ConfigurationKey, read_halfword};

pub(super) fn configuration_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u32, MemoryError> {
    super::record_configuration::helper_69f8fc(context, key)
}

fn configuration_half<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: ConfigurationKey) -> Result<u16, MemoryError> {
    read_halfword(context, key)
}

pub fn helper_ed8af4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(output, GuestAddress::new(0x0169_953c), super::record_address_space::ByteCount(72))
}

pub(super) fn entry_parameters<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<[u8; 72], MemoryError> {
    const STATE: GuestAddress = GuestAddress::new(0x016f_0d88);
    let entry = STATE.checked_add(index.checked_mul(20).ok_or(MemoryError::AddressOverflow)?)?.checked_add(296)?;
    let mut output = [0u8; 72];
    for (index, value) in output.iter_mut().enumerate() { *value = context.read_u8(GuestAddress::new(0x0169_953c).checked_add(index as u32)?)?; }
    output[0..4].copy_from_slice(&context.read_u32(entry.checked_add(4)?)?.to_le_bytes());
    output[4..6].copy_from_slice(&context.read_u16(entry.checked_add(12)?)?.to_le_bytes());
    output[6..8].copy_from_slice(&context.read_u16(entry.checked_add(14)?)?.to_le_bytes());
    output[8..12].copy_from_slice(&context.read_u32(entry.checked_add(8)?)?.to_le_bytes());
    output[12..16].copy_from_slice(&context.read_u32(STATE.checked_add(248)?)?.to_le_bytes());
    for index in 0..12 { output[16 + index] = context.read_u8(STATE.checked_add(252 + index as u32)?)?; }
    for index in 0..4 { output[30 + index] = context.read_u8(STATE.checked_add(264 + index as u32)?)?; }
    output[36..40].copy_from_slice(&1u32.to_le_bytes());
    output[40..44].fill(0);
    Ok(output)
}

pub fn helper_feb4cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, output: GuestAddress) -> Result<GuestAddress, MemoryError> {
    const STATE: GuestAddress = GuestAddress::new(0x016f_0d88);
    let entry = STATE.checked_add(index.checked_mul(20).ok_or(MemoryError::AddressOverflow)?)?.checked_add(296)?;
    helper_ed8af4(context, output)?;
    let value = context.read_u32(entry.checked_add(4)?)?;
    context.write_u32(output, value)?;
    let value = context.read_u16(entry.checked_add(12)?)?;
    context.write_u16(output.checked_add(4)?, value)?;
    let value = context.read_u16(entry.checked_add(14)?)?;
    context.write_u16(output.checked_add(6)?, value)?;
    let value = context.read_u32(entry.checked_add(8)?)?;
    context.write_u32(output.checked_add(8)?, value)?;
    let value = context.read_u32(STATE.checked_add(248)?)?;
    context.write_u32(output.checked_add(12)?, value)?;
    context.copy_register_words(output.checked_add(16)?, STATE.checked_add(252)?, super::record_address_space::RegisterWordCount::Three)?;
    context.copy_bytes(output.checked_add(30)?, STATE.checked_add(264)?, super::record_address_space::ByteCount(4))?;
    context.write_u32(output.checked_add(36)?, 1)?;
    context.write_u16(output.checked_add(40)?, 0)?;
    context.write_u16(output.checked_add(42)?, 0)?;
    Ok(output)
}

pub fn helper_ef5448<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<u32, MemoryError> {
    let row = GuestAddress::new(0x00ef_6de4).checked_add(profile.checked_mul(20).ok_or(MemoryError::AddressOverflow)?)?;
    Ok(u32::from(context.read_u8(row.checked_add(8)?)?))
}

pub fn helper_ef5310<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<i32, MemoryError> {
    let row = GuestAddress::new(0x00ef_6de4).checked_add(profile.checked_mul(20).ok_or(MemoryError::AddressOverflow)?)?;
    let selector = u32::from(context.read_u8(row.checked_add(14)?)?);
    Ok(context.read_u16(GuestAddress::new(0x00ef_66d0 + if selector > 1 { 0 } else { selector * 2 }))? as i16 as i32)
}

pub fn helper_ef5768<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(output, GuestAddress::new(0x00ef_6ddc), super::record_address_space::ByteCount(8))
}

pub(super) fn channel_areas<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, area: u32) -> Result<[u32; 5], MemoryError> {
    let denominator = u64::from(context.read_u8(GuestAddress::new(0x00ef_6dd8))?);
    let red = u64::from(context.read_u8(GuestAddress::new(0x00ef_6dd4))?);
    let green = u64::from(context.read_u8(GuestAddress::new(0x00ef_6dd5))?);
    let blue = u64::from(context.read_u8(GuestAddress::new(0x00ef_6dd6))?);
    let scale = |value| ((u64::from(area) * value) / denominator) as u32;
    Ok([scale(red), scale(green), scale(blue), scale(green), scale(green * 4)])
}

pub fn helper_ef56cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, area: u32) -> Result<GuestAddress, MemoryError> {
    let values = channel_areas(context, area)?;
    for (index, value) in values.iter().enumerate() { context.write_u32(output.checked_add(index as u32 * 4)?, *value)?; }
    Ok(output)
}

pub fn helper_ed7d84<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<u32, MemoryError> {
    if profile > 35 { Ok(3) } else { context.read_u32(GuestAddress::new(0x00ed_7e08 + profile * 16 + 12)) }
}

fn round_decimal(value: i32) -> i32 {
    let quotient = value / 10;
    let remainder = value % 10;
    if remainder > 4 { quotient + 1 } else if remainder < -5 { quotient - 1 } else { quotient }
}

pub fn helper_da1900<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: i32) -> Result<i32, MemoryError> { Ok(round_decimal(value)) }

pub fn helper_daa46c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: i32) -> Result<u32, MemoryError> {
    let value = (value / 10) as i16 as i32;
    let index = round_decimal(value).clamp(0, 13) as u32;
    Ok(context.read_u32(GuestAddress::new(0x00da_be18 + index * 4))? as u16 as u32)
}

fn calibrated_gain<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, gain: i32, unadjusted: u32) -> Result<([u16; 4], i32), MemoryError> {
    let index = helper_daa46c(context, gain)? as i16 as i32 as u32;
    let (primary, secondary): (u32, u32) = match first {
        0 => (0x00da_bfa0, 0x00da_c018),
        1 => (0x00da_bff0, 0x00da_bde8),
        2 => (0x00da_bca8, 0x00da_bfc8),
        _ => (0, 0),
    };
    let index = index.checked_mul(4).ok_or(MemoryError::AddressOverflow)?;
    let key = if primary == 0 { u32::MAX } else { context.read_u32(GuestAddress::new(primary).checked_add(index)?)? };
    let correction = if secondary == 0 { u32::MAX } else { context.read_u32(GuestAddress::new(secondary).checked_add(index)?)? };
    let mut values = [0u16; 4];
    let mut status = if key == u32::MAX { -31 } else { 0 };
    if key != u32::MAX {
        for (index, value) in values.iter_mut().enumerate() { *value = configuration_half(context, ConfigurationKey(key.checked_add(index as u32 * 2).ok_or(MemoryError::AddressOverflow)?))?; }
    }
    if unadjusted == 0 {
        if correction == u32::MAX { status = -31; }
        for (index, value) in values.iter_mut().enumerate() {
            let adjustment = if correction == u32::MAX { 0 } else { configuration_half(context, ConfigurationKey(correction.checked_add(index as u32 * 2).ok_or(MemoryError::AddressOverflow)?))? as i16 as i32 };
            *value = (i32::from(*value) - adjustment).max(0) as u16;
        }
    }
    Ok(([values[1], values[0], values[3], values[2]], status))
}

pub fn helper_dab6f8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, _second: u32, gain: i32, unadjusted: u32, output: GuestAddress) -> Result<i32, MemoryError> {
    let (values, status) = calibrated_gain(context, first, gain, unadjusted)?;
    for (index, value) in values.iter().enumerate() { context.write_u16(output.checked_add(index as u32 * 2)?, *value)?; }
    Ok(status)
}

fn gain_for_mode<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, gain: i32) -> Result<i32, MemoryError> {
    let profile = context.read_u32(GuestAddress::new(0x0169_953c))?;
    if helper_ef5448(context, profile)? > 1 {
        Ok(gain.wrapping_add(configuration_half(context, ConfigurationKey(0x1a7b))? as i16 as i32) as i16 as i32)
    } else { Ok(gain) }
}

pub fn helper_ee631c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32, gain: i32, mode: u32, output: GuestAddress) -> Result<i32, MemoryError> {
    context.write_u32(output, 0)?;
    context.write_u32(output.checked_add(4)?, 0)?;
    let gain = gain_for_mode(context, gain)?;
    helper_dab6f8(context, first, second, gain, mode, output)
}

pub(super) fn calibration_values<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, gain: i32, profile: u32, mode: u32) -> Result<[u32; 4], MemoryError> {
    let selector = helper_ed7d84(context, profile)?;
    if selector == 3 { return Ok([0; 4]); }
    let gain = gain_for_mode(context, gain)?;
    let (values, _) = calibrated_gain(context, selector, gain, mode)?;
    Ok(values.map(u32::from))
}

pub fn helper_ee6394<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, gain: i32, profile: u32, mode: u32, output: GuestAddress, _final_mode: u32) -> Result<(), MemoryError> {
    let values = calibration_values(context, gain, profile, mode)?;
    for (index, value) in values.iter().enumerate() { context.write_u32(output.checked_add(index as u32 * 4)?, *value)?; }
    Ok(())
}

pub fn helper_edb08c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, gain: i32, profile: u32, mode: u32, output: GuestAddress) -> Result<(), MemoryError> {
    helper_ee6394(context, gain, profile, mode, output, 1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatisticsDimensions {
    pub width: u32,
    pub height: u32,
}

pub fn minimum_dimensions<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<StatisticsDimensions, MemoryError> {
    let width = context.read_u32(GuestAddress::new(0x00ef_6ddc))?;
    let height = context.read_u32(GuestAddress::new(0x00ef_6de0))?;
    Ok(StatisticsDimensions { width, height })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn reads_configuration_calibration_and_publishes_channel_order() {
        let mut configuration_pointer = 0x2000u32.to_le_bytes();
        let mut configuration = [0u8; 128];
        for (index, value) in [100u16, 200, 300, 400].iter().enumerate() { configuration[32 + index * 2..34 + index * 2].copy_from_slice(&value.to_le_bytes()); }
        for (index, value) in [10u16, 20, 30, 40].iter().enumerate() { configuration[48 + index * 2..50 + index * 2].copy_from_slice(&value.to_le_bytes()); }
        let mut gain_tables = [0u8; 516];
        gain_tables[392..396].copy_from_slice(&32u32.to_le_bytes());
        gain_tables[512..516].copy_from_slice(&48u32.to_le_bytes());
        let mut selector = [0u8; 4];
        let mut profile = [0u8; 20];
        profile[8] = 1;
        let mut current_profile = [0u8; 4];
        let mut output = [0u8; 16];
        let region = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(region(0x0096_ca50, 4), &mut configuration_pointer).unwrap(),
            MemoryRegion::new(region(0x2000, 128), &mut configuration).unwrap(),
            MemoryRegion::new(region(0x00da_be18, 516), &mut gain_tables).unwrap(),
            MemoryRegion::new(region(0x00ed_7e14, 4), &mut selector).unwrap(),
            MemoryRegion::new(region(0x00ef_6de4, 20), &mut profile).unwrap(),
            MemoryRegion::new(region(0x0169_953c, 4), &mut current_profile).unwrap(),
            MemoryRegion::new(region(0x3000, 16), &mut output).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, ());
        assert_eq!(helper_edb08c(&mut context, 0, 0, 0, GuestAddress::new(0x3000)), Ok(()));
        assert_eq!(context.read_u32(GuestAddress::new(0x3000)), Ok(180));
        assert_eq!(context.read_u32(GuestAddress::new(0x3004)), Ok(90));
        assert_eq!(context.read_u32(GuestAddress::new(0x3008)), Ok(360));
        assert_eq!(context.read_u32(GuestAddress::new(0x300c)), Ok(270));
        assert_eq!(helper_da1900(&mut context, 5), Ok(1));
        assert_eq!(helper_da1900(&mut context, -5), Ok(0));
        assert_eq!(helper_da1900(&mut context, -6), Ok(-1));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatisticsWeights {
    pub channels: [u32; 4],
    pub total: u32,
}

pub fn weights<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, area: u32) -> Result<StatisticsWeights, MemoryError> {
    let values = channel_areas(context, area)?;
    Ok(StatisticsWeights { channels: [values[0], values[1], values[2], values[3]], total: values[4] })
}
