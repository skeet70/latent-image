use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69fec8;

pub use crate::record_configuration::ConfigurationKey;

#[derive(Clone, Copy)]
pub struct ProfileId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeometryKind(pub u32);

pub fn profile_row<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileId) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(0xef6de4u32.wrapping_add(profile.0.wrapping_mul(20)).wrapping_add(4)))
}
fn table<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileId, offset: u32) -> Result<u32, MemoryError> {
    Ok(0xef6aa4u32.wrapping_add(profile_row(context, profile)?.wrapping_mul(48)).wrapping_add(offset))
}
fn byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileId, offset: u32) -> Result<u32, MemoryError> {
    let address = table(context, profile, offset)?;
    context.read_u8(GuestAddress::new(address)).map(u32::from)
}
fn table_rectangle<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileId, base: u32) -> Result<[u32; 4], MemoryError> {
    let address = base.wrapping_add(profile_row(context, profile)?.wrapping_mul(16));
    let mut rectangle = [0; 4];
    for index in [2usize, 3, 0, 1] {
        let key = context.read_u32(GuestAddress::new(address.wrapping_add(index as u32 * 4)))?;
        rectangle[index] = if key == 0 { 1 } else { helper_69fec8(context, key)? };
    }
    Ok(rectangle)
}
pub fn helper_ef5d44<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<[u32; 4], MemoryError> {
    let profile = ProfileId(profile);
    let mut rectangle = table_rectangle(context, profile, 0xef71bc)?;
    if kind != 0 {
        let active = helper_ef5190(context, profile.0)?;
        rectangle[0] = rectangle[0].saturating_sub(active[0]);
        rectangle[1] = rectangle[1].saturating_sub(active[1]);
        let horizontal = helper_ef5a8c(context, profile.0)?;
        let vertical = if byte(context, profile, 0x27)? == 0 { 1 } else { 2 };
        rectangle[0] /= horizontal;
        rectangle[1] /= vertical;
        rectangle[3] /= horizontal;
        rectangle[2] /= vertical;
    }
    Ok(rectangle)
}
pub fn helper_ef5ab8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    let base = byte(context, ProfileId(profile), 0x24)?;
    if kind == 0 { Ok(base) } else { Ok(base.wrapping_mul(helper_ef5a8c(context, profile)?) as u16 as u32) }
}
pub fn helper_ef5cc4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    let base = byte(context, ProfileId(profile), 0x25)?;
    if kind == 0 { Ok(base) } else { Ok(base.wrapping_mul(if byte(context, ProfileId(profile), 0x27)? == 0 { 1 } else { 2 }) as u16 as u32) }
}
pub fn helper_ef5b5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<[u32; 2], MemoryError> {
    let mut pair = [context.read_u32(GuestAddress::new(0xef6ddc))?, context.read_u32(GuestAddress::new(0xef6de0))?];
    if pair != [2, 2] {
        if byte(context, ProfileId(profile), 0x26)? != 0 || byte(context, ProfileId(profile), 0x24)? != 1 { pair[1] >>= 1; }
        if byte(context, ProfileId(profile), 0x27)? != 0 || byte(context, ProfileId(profile), 0x25)? != 1 { pair[0] >>= 1; }
    }
    Ok(pair)
}
pub fn helper_ef5138<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<[u32; 4], MemoryError> {
    let address = table(context, ProfileId(profile), 0)?;
    Ok([0, 0, u32::from(context.read_u16(GuestAddress::new(address.wrapping_add(2)))?), u32::from(context.read_u16(GuestAddress::new(address))?)])
}
pub fn helper_ef5190<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<[u32; 4], MemoryError> { table_rectangle(context, ProfileId(profile), 0xef6994) }
pub fn helper_ef5a8c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<u32, MemoryError> {
    if byte(context, ProfileId(profile), 0x26)? != 0 { return Ok(2); }
    Ok(match byte(context, ProfileId(profile), 0x28)? { 1 => 2, 2 => 4, _ => 1 })
}
