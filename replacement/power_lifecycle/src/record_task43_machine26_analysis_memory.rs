use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc as configuration;

pub(crate) fn configuration_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u16, MemoryError> {
    let low = configuration(context, key)?;
    Ok((low | (configuration(context, key.wrapping_add(1))? << 8)) as u16)
}
pub(crate) fn configuration_signed_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<i32, MemoryError> {
    Ok(configuration(context, key)? as u8 as i8 as i32)
}
pub(crate) fn configuration_signed_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<i32, MemoryError> {
    Ok(configuration_halfword(context, key)? as i16 as i32)
}
