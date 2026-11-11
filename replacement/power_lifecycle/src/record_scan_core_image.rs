//! Installed X-M1 bytes at 10538a4..10538d8.

use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_10538a4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(0x0170_6a34))
}

pub fn helper_10538c4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, _output: GuestAddress) -> Result<(), MemoryError> {
    Ok(())
}
