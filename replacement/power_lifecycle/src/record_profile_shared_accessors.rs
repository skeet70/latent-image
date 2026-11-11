//! Shared profile CPU uses the canonical admitted address space on host and ARM.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{can_write, copy_words, ProfileField, ProfileHandle};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct ProfileHistory(pub [u32; 12]);
macro_rules! word {
    ($name:ident, $at:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> { context.read_u32(GuestAddress::new(0x0091_904c + $at)) } };
}
macro_rules! signed_halfword {
    ($name:ident, $at:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> { context.read_u16(GuestAddress::new(0x0091_904c + $at)).map(|value| value as i16 as i32 as u32) } };
}
word!(helper_7569a4, 0x13c);
word!(helper_7569ec, 0x140);
word!(helper_756a34, 0x144);
word!(helper_756a7c, 0x148);
word!(helper_756ac4, 0x14c);
word!(helper_756b0c, 0x150);
word!(helper_756b54, 0x154);
word!(helper_756b9c, 0x158);
word!(helper_756c74, 0x164);
word!(helper_756d04, 0x16c);
word!(helper_756d4c, 0x170);
word!(helper_756ddc, 0x178);
word!(helper_756fe4, 0x200);
word!(helper_757254, 0x230);
word!(helper_7573d4, 0x23c);
word!(helper_75a238, 0x894);
word!(helper_75a58c, 0x9c8);
word!(helper_75a61c, 0x9d0);
signed_halfword!(helper_75702c, 0x206);
signed_halfword!(helper_75707c, 0x208);
signed_halfword!(helper_75729c, 0x238);
signed_halfword!(helper_75a350, 0x96c);
signed_halfword!(helper_75a3a0, 0x96e);
signed_halfword!(helper_75a440, 0x972);

pub fn helper_75733c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> { context.read_u16(GuestAddress::new(0x0091_9280)).map(u32::from) }
pub fn helper_75738c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> { context.read_u8(GuestAddress::new(0x0091_9282)).map(u32::from) }
pub fn helper_75a5d4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> { context.read_u8(GuestAddress::new(0x0091_9a18)).map(u32::from) }
macro_rules! snapshot_getter {
    ($name:ident, $at:expr, $words:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32, destination: GuestAddress) -> Result<(), MemoryError> { copy_words::<$words>(context, destination, GuestAddress::new(0x0091_904c + $at)) } };
}
macro_rules! snapshot_setter {
    ($name:ident, $at:expr, $words:expr, $field:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
        if can_write(context, ProfileHandle(handle), ProfileField($field))? { copy_words::<$words>(context, GuestAddress::new(0x0091_904c + $at), source)?; }
        Ok(())
    } };
}
snapshot_getter!(helper_756940, 0x120, 7);
snapshot_setter!(helper_756970, 0x120, 7, 19);
snapshot_getter!(helper_75741c, 0x240, 1);
snapshot_setter!(helper_75743c, 0x240, 1, 26);
snapshot_getter!(helper_757ff0, 0x864, 12);
snapshot_setter!(helper_758028, 0x864, 12, 14);
snapshot_setter!(helper_75a258, 0x898, 12, 39);
pub fn helper_75a4ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, destination: GuestAddress) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(41))? { copy_words::<11>(context, destination, GuestAddress::new(0x0091_99e8))?; }
    Ok(())
}
pub fn helper_75a52c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32, source: GuestAddress) -> Result<(), MemoryError> { copy_words::<11>(context, GuestAddress::new(0x0091_99e8), source) }
pub fn helper_75a464<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(41))? || index > 4 { return Ok(()); }
    copy_words::<2>(context, GuestAddress::new(0x0091_99c0 + index * 8), source)
}
pub fn helper_75a4ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32, index: u32, destination: GuestAddress) -> Result<(), MemoryError> {
    if destination.get() == 0 || index > 4 { return Ok(()); }
    copy_words::<2>(context, destination, GuestAddress::new(0x0091_99c0 + index * 8))
}
macro_rules! tail_halfword_setter {
    ($name:ident, $at:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
        if can_write(context, ProfileHandle(handle), ProfileField(41))? { context.write_u16(GuestAddress::new(0x0091_904c + $at), value as u16)?; }
        Ok(())
    } };
}
tail_halfword_setter!(helper_75a324, 0x96c);
tail_halfword_setter!(helper_75a374, 0x96e);
tail_halfword_setter!(helper_75a3c4, 0x970);
tail_halfword_setter!(helper_75a414, 0x972);
macro_rules! tail_word_setter {
    ($name:ident, $at:expr, $field:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
        if can_write(context, ProfileHandle(handle), ProfileField($field))? { context.write_u32(GuestAddress::new(0x0091_904c + $at), value)?; }
        Ok(())
    } };
}
tail_word_setter!(helper_75a210, 0x894, 38);
tail_word_setter!(helper_75a564, 0x9c8, 42);
tail_word_setter!(helper_75a5f4, 0x9d0, 42);
pub fn helper_75a5ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(42))? { context.write_u8(GuestAddress::new(0x0091_9a18), value as u8)?; }
    Ok(())
}
