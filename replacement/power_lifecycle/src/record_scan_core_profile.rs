//! Installed-byte profile queries used by f9d414.

use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use super::record_profile_accessors::{helper_75a910, helper_75e618, helper_75e1ec};

pub fn helper_7583b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(0x0091_904c + 0x79c))
}

fn read_profile_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32, offset: u32) -> Result<u32, MemoryError> {
    let bank = if helper_75a910(context, item, 0)? != 0 { helper_75e618(context, item)? } else { helper_75e1ec(context)? };
    let resource = GuestAddress::new(0x0096_ebd0u32.wrapping_add(bank.wrapping_mul(0x2158)));
    Ok(u32::from(context.read_u8(resource.checked_add(offset)?)?))
}

pub fn helper_758fa8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32) -> Result<u32, MemoryError> {
    read_profile_byte(context, item, 0x70)
}

pub fn helper_7590b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32) -> Result<u32, MemoryError> {
    read_profile_byte(context, item, 0x58)
}

pub fn helper_759030<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32) -> Result<u32, MemoryError> {
    read_profile_byte(context, item, 0x6e)
}
