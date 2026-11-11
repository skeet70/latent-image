//! Global profile CPU shares its actual configuration and state with every recording consumer.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::{read_byte, ConfigurationKey};
fn address(offset: u32) -> GuestAddress { GuestAddress::new(0x0094_2ab0 + offset) }
pub fn helper_74a214<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: u32) -> Result<u32, MemoryError> {
    if (resource as i32) < 0 { return Ok(0); }
    let count = u32::from(read_byte(context, ConfigurationKey(0x003e_0f80))?).clamp(1, 100);
    Ok(u32::from((resource as i32) <= (count as i32).wrapping_sub(1)))
}
pub fn helper_74a740<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, high: u32, low: u32) -> Result<u32, MemoryError> { Ok(low | (high << 16)) }
pub fn helper_76d9c8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let resource = context.read_u32(address(0))?;
    if helper_74a214(context, resource)? != 0 { Ok(resource) } else { Ok(0) }
}
pub fn helper_76d9b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { context.read_u32(address(0)) }
pub fn helper_76d9ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { context.read_u32(address(0x100)) }
pub fn helper_76da84<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { context.read_u16(address(0xd4)).map(|value| value as i16 as i32 as u32) }
pub fn helper_76dad4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(match context.read_u32(address(0x104))? { 0 => 0, 1 => 1, 2 => 2, _ => 3 })
}
macro_rules! selection {
    ($name:ident, $forced:expr, $selected:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        if context.read_u32(address($forced))? == 1 { Ok(2) } else { context.read_u32(address($selected)).map(|value| u32::from(value == 1)) }
    } };
}
selection!(helper_76d9a8, 0xf4, 0xdc);
selection!(helper_76da38, 0xf8, 0xe0);
selection!(helper_76dba4, 0xfc, 0xe4);
pub fn helper_76da48<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if helper_76da38(context)? == 2 { return Ok(128); }
    if context.read_u32(address(0x2c))? != 0 { context.read_u32(address(0x1c)) } else { Ok(128) }
}
pub fn helper_76dac4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u32(address(0xe0))? == 1 { Ok(1) } else { context.read_u32(address(0xec)).map(|value| u32::from(value != 0)) }
}
pub fn helper_76f340<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { context.read_u32(address(0x138)) }
pub fn helper_76f360<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(match context.read_u32(address(0x134))? { value @ 0..=1 => value, 3 => 1, _ => 0 }) }
pub fn helper_76f420<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(0) }
pub fn helper_772f28<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    let key = match index { 0 => return Ok(0), 1..=10 => 0x1900 + index * 2, _ => 0x1906 };
    crate::record_configuration::helper_69ff7c(context, key).map(|value| value as u32)
}
pub fn helper_772ff8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    if value == 0 { return Ok(0); }
    for index in 2..=10 { if (value as i32) < (helper_772f28(context, index)? as i32) { return Ok(index - 1); } }
    Ok(10)
}
