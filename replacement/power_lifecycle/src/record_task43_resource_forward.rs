use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

// X-M1 bytes f47300..f47330 forward seven words to 74947c.
pub fn helper_f47300<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    position: u32,
    zoom: u32,
    aperture: u32,
    target: u32,
    flags: u32,
    format: u32,
    direction: u32,
) -> Result<u32, MemoryError> {
    crate::record_profile_mapping::helper_74947c(context, position, zoom, aperture, target, flags, format, direction)
}

pub fn helper_f47100<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: u32) -> Result<i32, MemoryError> {
    Ok(i32::from(crate::record_profile_shared_accessors::helper_7569a4(context, resource)? & 2 == 0))
}

pub fn helper_f47330<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: u32) -> Result<i32, MemoryError> {
    let enabled = crate::record_configuration::helper_69f8fc(context, 0x887c)?;
    let category = crate::record_profile_accessors::helper_74e5c0(context, resource)?;
    let kind = crate::record_profile_accessors::helper_74fa2c(context, resource)?;
    Ok(i32::from(enabled != 0 && category == 0 && kind.wrapping_sub(4) <= 2))
}
