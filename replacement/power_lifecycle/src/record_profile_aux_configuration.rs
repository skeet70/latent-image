//! X-M1 1.50 configuration leaves, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy)]
pub struct AuxiliaryIndex(u32);
impl AuxiliaryIndex {
    pub const fn new(index: u32) -> Option<Self> { if index < 20 { Some(Self(index)) } else { None } }
}
#[derive(Clone, Copy)]
pub struct ResourceHalfwordIndex(u32);
impl ResourceHalfwordIndex {
    pub const fn new(index: u32) -> Option<Self> { if index < (0x2158 - 0x14d8) / 2 { Some(Self(index)) } else { None } }
}

macro_rules! word {
    ($get:ident, $set:ident, $offset:literal) => {
        pub fn $get<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
            context.read_u32(record.checked_add($offset)?)
        }
        pub fn $set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, value: u32) -> Result<u32, MemoryError> {
            context.write_u32(record.checked_add($offset)?, value)?;
            Ok(record.get())
        }
    };
}
pub fn helper_75d474<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    context.read_u32(record.checked_add(0x146c)?)
}
word!(helper_75d4e4, helper_75d500, 0x14ec);
word!(helper_75d564, helper_75d57c, 0);
word!(helper_75d5d8, helper_75d5f0, 0x828);
word!(helper_75d608, helper_75d620, 0x82c);
word!(helper_75d638, helper_75d650, 0x830);

pub fn helper_75d4ac<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, record: GuestAddress) -> Result<GuestAddress, MemoryError> {
    record.checked_add(0x1470)
}
pub fn helper_75d4c8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> {
    let destination = record.checked_add(0x1470)?;
    context.copy_bytes(destination, source, ByteCount(0x68))?;
    Ok(destination.get())
}
pub fn helper_75d51c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, index: ResourceHalfwordIndex) -> Result<u32, MemoryError> {
    Ok(context.read_u16(record.checked_add(0x14d8 + index.0 * 2)?)? as i16 as i32 as u32)
}
pub fn helper_75d540<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, index: ResourceHalfwordIndex, value: u16) -> Result<u32, MemoryError> {
    context.write_u16(record.checked_add(0x14d8 + index.0 * 2)?, value)?;
    Ok(record.get())
}
pub fn helper_75d594<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, list: GuestAddress, index: AuxiliaryIndex) -> Result<GuestAddress, MemoryError> {
    list.checked_add(4 + index.0 * 0x68)
}
pub fn helper_75d5b4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, list: GuestAddress, index: AuxiliaryIndex, source: GuestAddress) -> Result<u32, MemoryError> {
    let destination = helper_75d594(context, list, index)?;
    context.copy_bytes(destination, source, ByteCount(0x68))?;
    Ok(destination.get())
}
