use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub(crate) const STATE: u32 = 0x016c0fb8;
pub(crate) const CHANNEL: u32 = 0x016c21fc;
pub(crate) fn word(context: &mut impl GuestAddressSpace, address: u32) -> Result<u32, MemoryError> { context.read_u32(GuestAddress::new(address)) }
pub(crate) fn half(context: &mut impl GuestAddressSpace, address: u32) -> Result<u16, MemoryError> { context.read_u16(GuestAddress::new(address)) }
pub(crate) fn byte(context: &mut impl GuestAddressSpace, address: u32) -> Result<u8, MemoryError> { context.read_u8(GuestAddress::new(address)) }
pub(crate) fn store(context: &mut impl GuestAddressSpace, address: u32, value: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(address), value) }
pub(crate) fn store_half(context: &mut impl GuestAddressSpace, address: u32, value: u16) -> Result<(), MemoryError> { context.write_u16(GuestAddress::new(address), value) }
pub(crate) fn store_byte(context: &mut impl GuestAddressSpace, address: u32, value: u8) -> Result<(), MemoryError> { context.write_u8(GuestAddress::new(address), value) }
pub(crate) fn w(context: &mut impl GuestAddressSpace, offset: u32) -> Result<u32, MemoryError> { word(context, STATE + offset) }
pub(crate) fn h(context: &mut impl GuestAddressSpace, offset: u32) -> Result<u16, MemoryError> { half(context, STATE + offset) }
pub(crate) fn b(context: &mut impl GuestAddressSpace, offset: u32) -> Result<u8, MemoryError> { byte(context, STATE + offset) }
pub(crate) fn s(context: &mut impl GuestAddressSpace, offset: u32) -> Result<i32, MemoryError> { Ok(i32::from(h(context, offset)? as i16)) }
pub(crate) fn put(context: &mut impl GuestAddressSpace, offset: u32, value: u32) -> Result<(), MemoryError> { store(context, STATE + offset, value) }
pub(crate) fn ph(context: &mut impl GuestAddressSpace, offset: u32, value: u16) -> Result<(), MemoryError> { store_half(context, STATE + offset, value) }
pub(crate) fn pb(context: &mut impl GuestAddressSpace, offset: u32, value: u8) -> Result<(), MemoryError> { store_byte(context, STATE + offset, value) }
pub(crate) fn reset_step(context: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { pb(context, 4, 0)?; let direction = b(context, 0x2d)?; pb(context, 0x2c, direction)?; ph(context, 0x2a, 0) }
pub(crate) fn setting<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u32, MemoryError> { crate::record_configuration::helper_69f8fc(context, key) }

pub fn helper_f3d3e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { store(context, CHANNEL + 4, value) }
pub fn helper_f3d400<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { store(context, CHANNEL + 8, value) }
pub fn helper_f3d420<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let request = word(context, 0x0169d380)?; store(context, CHANNEL + 12, request)?;
    let position = half(context, 0x0169d384)?; store_half(context, CHANNEL + 16, position)?;
    let sample = word(context, 0x0169d388)?; store(context, CHANNEL + 20, sample)
}
pub fn helper_f3d45c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { let position = half(context, CHANNEL + 16)?; ph(context, 0x34, position)?; let sample = word(context, CHANNEL + 20)?; put(context, 0x50, sample) }
pub fn helper_f3d490<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    if w(context, 0xf58)? != 0 || h(context, 0xf5c)? == 0 { return Ok(0); }
    pb(context, 0xf5c, 0)?; pb(context, 0xf5d, 0)?; reset_step(context)?; Ok(1)
}
pub fn helper_f3e32c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    pb(context, 4, 0)?; put(context, 0xf64, 1)?; put(context, 0xf68, 0)?; put(context, 0xf6c, 0)?;
    put(context, 0xf70, 2)?; put(context, 0xf74, 2)?; put(context, 0xf7c, 0)?; put(context, 0xf80, 0)?; put(context, 0xf84, 0)?; ph(context, 0xf88, 0)
}
pub fn helper_f3e3c8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { put(context, 0xf68, 1) }
pub fn helper_f3eaf0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> { Ok(i32::from(w(context, 0x244)? == 1 && !matches!(w(context, 0x274)?, 0 | 2))) }
pub fn helper_f3eb3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from((w(context, 8)? == 14 || w(context, 0xf64)? == 1) && (w(context, 0xf70)? == 14 || w(context, 0xf64)? != 1) && w(context, 0x244)? == 1 && b(context, 0x278)? != 1))
}
pub fn helper_f3ebd4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> { Ok(i32::from(w(context, 0x244)? != 0 || word(context, CHANNEL + 8)? != 1)) }
pub fn helper_f3e8e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(match w(context, 8)? { 13 => 0, 14 => 1, 15 => 2, 16 => 3, 17 => 4, 18 => 5, 19 => 6, 20 => match w(context, 0xf70)? { 16 => 3, 14 => 1, _ => 7 }, _ => 8 })
}
pub fn helper_f3e994<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> { Ok(i32::from(helper_f3e8e0(context)? == 1 && w(context, 0x244)? == 1)) }
