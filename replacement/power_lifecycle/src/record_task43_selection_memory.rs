use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub(crate) fn word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: u32) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(address))
}
pub(crate) fn byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: u32) -> Result<u8, MemoryError> {
    context.read_u8(GuestAddress::new(address))
}
pub(crate) fn half<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: u32) -> Result<u16, MemoryError> {
    context.read_u16(GuestAddress::new(address))
}
pub(crate) fn put<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: u32, value: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(address), value)
}
pub(crate) fn put_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: u32, value: u8) -> Result<(), MemoryError> {
    context.write_u8(GuestAddress::new(address), value)
}
pub(crate) fn put_half<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: u32, value: u16) -> Result<(), MemoryError> {
    context.write_u16(GuestAddress::new(address), value)
}
pub(crate) fn copy<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: u32, source: u32, length: u32) -> Result<(), MemoryError> {
    context.copy_bytes(GuestAddress::new(destination), GuestAddress::new(source), ByteCount(length))
}
pub(crate) fn clear<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: u32, length: u32) -> Result<(), MemoryError> {
    for offset in 0..length {
        let address = GuestAddress::new(destination).checked_add(offset)?;
        context.write_u8(address, 0)?;
    }
    Ok(())
}

macro_rules! load {
    ($context:expr, $operation:ident, $address:expr) => {{
        let address = $address;
        crate::record_task43_selection_memory::$operation($context, address)?
    }};
}
pub(crate) use load;

macro_rules! store {
    ($context:expr, $operation:ident, $address:expr, $value:expr) => {{
        let address = $address;
        let value = $value;
        crate::record_task43_selection_memory::$operation($context, address, value)?
    }};
}
pub(crate) use store;

macro_rules! transfer {
    ($context:expr, $destination:expr, $source:expr, $length:expr) => {{
        let destination = $destination;
        let source = $source;
        let length = $length;
        crate::record_task43_selection_memory::copy($context, destination, source, length)?
    }};
}
pub(crate) use transfer;
