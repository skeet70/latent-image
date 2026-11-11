use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_profile_accessors::helper_74f5ec;
use crate::record_profile_shared_cpu::helper_756840;

pub fn helper_f363b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(context.read_u32(GuestAddress::new(0x016bfe4c))? as i32)
}

pub fn helper_f363d8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(0x016bfe48), request)
}

pub fn helper_f36760<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: u32) -> Result<i32, MemoryError> {
    context.with_guest_frame(ByteCount(64), StackAlignment::Doubleword, |context, frame| {
        let scratch = frame.at(ByteCount(4), ByteCount(48))?;
        helper_756840(context, request, scratch)?;
        let current = helper_74f5ec(context, request)?;
        let offset = context.read_u16(scratch.checked_add(40)?)? as u32;
        let reference = context.read_u16(GuestAddress::new(0x016b3d38))? as u32;
        Ok(reference.wrapping_sub(current.wrapping_sub(offset)) as i16 as i32)
    })
}

#[cfg(test)]
#[path = "record_task43_aux_state_tests.rs"]
mod tests;
