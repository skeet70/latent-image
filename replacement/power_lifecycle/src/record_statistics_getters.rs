use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use super::record_configuration::ConfigurationKey;
use super::record_statistics_dimensions::{helper_734540, helper_73469c, helper_739500, helper_73958c, helper_741ee0};

#[derive(Clone, Copy)]
pub struct ProfileIndex(pub u32);

fn configuration_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: ConfigurationKey) -> Result<u32, MemoryError> {
    super::record_configuration::helper_69f8fc(context, key.0)
}

pub fn helper_ef53d0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    let index = ProfileIndex(index);
    Ok(u32::from(context.read_u8(GuestAddress::new(0x00ef_6de4).checked_add(index.0.wrapping_mul(20).wrapping_add(13))?)?))
}
pub fn helper_eef1c0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, enabled: u32) -> Result<u32, MemoryError> {
    context.write_u32(GuestAddress::new(0x0169_b950), enabled)?;
    Ok(0)
}
pub fn helper_eea094<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if configuration_byte(context, ConfigurationKey(0xbe))? & 15 == 0 { context.read_u32(GuestAddress::new(0x00ee_a2ac)) } else { Ok(1) }
}
pub fn helper_eea18c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let value = configuration_byte(context, ConfigurationKey(0xaa))?;
    let index = helper_eea094(context)?;
    let _kind = helper_ef53d0(context, index)?;
    Ok(if value as i16 <= 0 || value as i16 > 9 { 1 } else { value as u8 as u32 })
}
pub fn helper_eea014<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let index = helper_eea18c(context)?;
    context.read_u32(GuestAddress::new(0x00ee_a284 + index * 4))
}
pub fn helper_eea030<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: Option<GuestAddress>, first: Option<GuestAddress>, second: Option<GuestAddress>) -> Result<(), MemoryError> {
    if let Some(channel) = channel {
        let value = configuration_byte(context, ConfigurationKey(0xad))?;
        context.write_u8(channel, value as u8)?;
        if value >= super::record_profile_mapping::helper_74677c(context)? { context.write_u8(channel, 0)?; }
    }
    if let Some(first) = first {
        let low = configuration_byte(context, ConfigurationKey(0xae))?;
        let high = configuration_byte(context, ConfigurationKey(0xaf))?;
        context.write_u16(first, (low | high << 8) as u16)?;
    }
    if let Some(second) = second {
        let profile = helper_eea014(context)?;
        let value = super::record_profile_globals::helper_772f28(context, profile)?;
        context.write_u16(second, value as u16)?;
    }
    Ok(())
}
pub fn helper_ee8e70<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<u32, MemoryError> {
    helper_741ee0(context, 5, output)
}
pub fn helper_dca798<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, mode: u32, dimensions: GuestAddress) -> Result<u32, MemoryError> {
    if !(1..=10).contains(&mode) { return Ok(0); }
    let first = context.read_u16(dimensions)?;
    let column = match first {
        3 if context.read_u16(dimensions.checked_add(2)?)? == 4 => 0,
        2 if context.read_u16(dimensions.checked_add(2)?)? == 3 => 4,
        9 if context.read_u16(dimensions.checked_add(2)?)? == 16 => 8,
        1 if context.read_u16(dimensions.checked_add(2)?)? == 1 => 12,
        _ => return Ok(0),
    };
    let enabled = context.read_u32(GuestAddress::new(0x00dc_a8b0 + mode * 16 + column))?;
    if enabled == 1 {
        let ratio = helper_739500(context, dimensions)?;
        let index = helper_73958c(context, mode, ratio)?;
        if index == 255 { loop { core::hint::spin_loop(); } }
        let first = helper_734540(context, index, ratio)?;
        context.write_u32(output.checked_add(4)?, first)?;
        let second = helper_73469c(context, index, ratio)?;
        context.write_u32(output, second)?;
    }
    Ok(enabled)
}
pub fn helper_eed3d8<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, _query: GuestAddress, _list: GuestAddress) -> Result<u32, MemoryError> { Ok(0) }
pub fn helper_ee96e0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<GuestAddress, MemoryError> { Ok(output) }
pub fn helper_ee96f4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<GuestAddress, MemoryError> { Ok(output) }
pub fn helper_ef3350<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok((context.read_u32(GuestAddress::new(0xfff8_90d4))? >> 2) & 1)
}
pub fn helper_ee8bb0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { helper_ef3350(context) }
