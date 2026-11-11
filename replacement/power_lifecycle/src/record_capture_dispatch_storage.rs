use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const QUEUE_CONTEXT: GuestAddress = GuestAddress::new(0x016f_0aa4);
pub const CURRENT_REQUEST: GuestAddress = GuestAddress::new(0x016f_0aa8);
pub const SAVED_REQUEST: GuestAddress = GuestAddress::new(0x016f_0b9c);
pub const CONTROL_CONTEXT: GuestAddress = GuestAddress::new(0x016f_08c0);
pub const PENDING_REQUEST: GuestAddress = GuestAddress::new(0x016f_08c8);
pub const REQUEST_BYTES: ByteCount = ByteCount(244);

pub fn initialize_payload_addresses<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u32(CURRENT_REQUEST.checked_add(8)?, CURRENT_REQUEST.checked_add(12)?.get())?;
    context.write_u32(SAVED_REQUEST.checked_add(8)?, SAVED_REQUEST.checked_add(12)?.get())
}

pub fn helper_fe9540<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<GuestAddress, MemoryError> {
    context.copy_bytes(SAVED_REQUEST, CURRENT_REQUEST, REQUEST_BYTES)?;
    Ok(SAVED_REQUEST)
}

pub fn helper_fe9564<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<GuestAddress, MemoryError> {
    Ok(CURRENT_REQUEST)
}

pub fn restore_pending<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.copy_bytes(CURRENT_REQUEST, PENDING_REQUEST, REQUEST_BYTES)?;
    context.write_u32(CURRENT_REQUEST.checked_add(12)?, 0)?;
    context.write_u16(CURRENT_REQUEST, 0x3f43)
}

pub fn helper_fe8ff8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, length: ByteCount) -> Result<ByteCount, MemoryError> {
    let length = ByteCount(length.0.min(252));
    context.copy_bytes(destination, CONTROL_CONTEXT, length)?;
    Ok(length)
}

pub fn helper_fe9474<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, length: ByteCount) -> Result<ByteCount, MemoryError> {
    let length = ByteCount(length.0.min(492));
    context.copy_bytes(destination, QUEUE_CONTEXT, length)?;
    Ok(length)
}

pub fn operation<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
    context.read_u16(CURRENT_REQUEST)
}
pub fn state<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(CONTROL_CONTEXT)
}
pub fn set_state<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, state: u32) -> Result<(), MemoryError> {
    context.write_u32(CONTROL_CONTEXT, state)
}

#[cfg(test)]
#[path = "record_capture_dispatch_storage_tests.rs"]
mod tests;
