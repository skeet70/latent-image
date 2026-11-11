use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{read_bank, resource_address, ProfileField, ProfileHandle};

pub fn helper_75502c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(13))?;
    context.read_u32(resource_address(bank).checked_add(0x20cc)?)
}

pub fn helper_75516c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(13))?;
    context.read_u32(resource_address(bank).checked_add(0x20d4)?)
}
