//! X-M1 1.50 profile slot fields, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{copy_words, read_bank, resource_address, write_bank, ProfileField, ProfileHandle};
use crate::record_profile_accessors::helper_750c88;

#[derive(Clone, Copy)]
struct SlotIndex(u32);

fn slot_offset(index: SlotIndex, member: u32) -> u32 {
    0x16cu32.wrapping_add(index.0.wrapping_mul(0xa8)).wrapping_add(member)
}

macro_rules! read_value {
    ($context:ident, $address:expr, u32) => { $context.read_u32($address)? };
    ($context:ident, $address:expr, i16) => { $context.read_u16($address)? as i16 as i32 as u32 };
}
macro_rules! write_value {
    ($context:ident, $address:expr, $value:ident, u32) => { $context.write_u32($address, $value)? };
    ($context:ident, $address:expr, $value:ident, i16) => { $context.write_u16($address, $value as u16)? };
}
macro_rules! slot_getter {
    ($name:ident, $permission:expr, $offset:expr, $kind:ident) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField($permission))?;
            let address = resource_address(bank).checked_add(slot_offset(SlotIndex(index), $offset))?;
            Ok(read_value!(context, address, $kind))
        }
    };
}
macro_rules! slot_setter {
    ($name:ident, $permission:expr, $offset:expr, $kind:ident) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, value: u32) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField($permission))? {
                let address = resource_address(bank).checked_add(slot_offset(SlotIndex(index), $offset))?;
                write_value!(context, address, value, $kind);
            }
            Ok(())
        }
    };
}

pub fn helper_752714<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let (field, offset, halfword) = if helper_750c88(context, handle)? == 1 { (0, 0x2c, true) } else { (4, 0x160, false) };
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(field))?;
    let address = resource_address(bank).checked_add(offset)?;
    if halfword { Ok(context.read_u16(address)? as u32) } else { context.read_u32(address) }
}

pub fn helper_752928<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, handle: u32, index: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(5))?;
    let source = resource_address(bank).checked_add(slot_offset(SlotIndex(index), 0))?;
    copy_words::<42>(context, output, source)?;
    Ok(output)
}

slot_getter!(helper_752a40, 5, 0x04, u32);
slot_getter!(helper_752af0, 5, 0x08, i16);
slot_getter!(helper_752ba0, 5, 0x0a, i16);
slot_getter!(helper_752c50, 5, 0x0c, i16);
slot_getter!(helper_752d00, 5, 0x0e, i16);
slot_getter!(helper_752db0, 5, 0x2e, i16);
slot_getter!(helper_752e60, 5, 0x30, i16);
slot_getter!(helper_752f10, 5, 0x10, i16);
slot_getter!(helper_753120, 5, 0x1a, i16);
slot_getter!(helper_753330, 5, 0x1c, u32);
slot_getter!(helper_753540, 5, 0x16, i16);
slot_getter!(helper_7535f0, 5, 0x28, u32);
slot_getter!(helper_753960, 7, 0x44, u32);
slot_getter!(helper_753a10, 7, 0x48, u32);
slot_getter!(helper_753ee0, 8, 0x84, i16);
slot_getter!(helper_753f90, 8, 0x86, i16);
slot_getter!(helper_754040, 8, 0x88, u32);
slot_getter!(helper_7540f0, 8, 0x8c, u32);
slot_getter!(helper_7541a0, 8, 0x90, u32);
slot_getter!(helper_754300, 8, 0x94, u32);
slot_getter!(helper_7543b0, 8, 0x98, u32);
slot_setter!(helper_7529e8, 5, 0x00, i16);
slot_setter!(helper_752a98, 5, 0x04, u32);
slot_setter!(helper_752b48, 5, 0x08, i16);
slot_setter!(helper_752bf8, 5, 0x0a, i16);
slot_setter!(helper_752ca8, 5, 0x0c, i16);
slot_setter!(helper_752d58, 5, 0x0e, i16);
slot_setter!(helper_752e08, 5, 0x2e, i16);
slot_setter!(helper_752eb8, 5, 0x30, i16);
slot_setter!(helper_752f68, 5, 0x10, i16);
slot_setter!(helper_753178, 5, 0x1a, i16);
slot_setter!(helper_753598, 5, 0x16, i16);
slot_setter!(helper_753648, 5, 0x28, u32);
slot_setter!(helper_7536f8, 6, 0x34, u32);
slot_setter!(helper_7537a8, 6, 0x38, i16);
slot_setter!(helper_753858, 6, 0x3c, u32);
slot_setter!(helper_753908, 6, 0x40, u32);
slot_setter!(helper_7539b8, 7, 0x44, u32);
slot_setter!(helper_753a68, 7, 0x48, u32);
slot_setter!(helper_753c78, 7, 0x54, u32);
slot_setter!(helper_753e88, 7, 0x80, u32);
slot_setter!(helper_754098, 8, 0x88, u32);
slot_setter!(helper_754148, 8, 0x8c, u32);
slot_setter!(helper_754568, 10, 0xa0, u32);

macro_rules! scalar_getter {
    ($name:ident, $permission:expr, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField($permission))?;
            context.read_u32(resource_address(bank).checked_add($offset)?)
        }
    };
}
macro_rules! scalar_setter {
    ($name:ident, $permission:expr, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField($permission))? {
                context.write_u32(resource_address(bank).checked_add($offset)?, value)?;
            }
            Ok(())
        }
    };
}
scalar_getter!(helper_7527f8, 4, 0x164);
scalar_getter!(helper_752890, 4, 0x168);
scalar_setter!(helper_7527ac, 4, 0x160);
scalar_setter!(helper_752844, 4, 0x164);
scalar_setter!(helper_7528dc, 4, 0x168);
scalar_getter!(helper_754d9c, 13, 0x20b0);
scalar_getter!(helper_754e3c, 13, 0x20b4);
scalar_getter!(helper_754f8c, 13, 0x20c8);
scalar_getter!(helper_75540c, 13, 0x20f8);
scalar_getter!(helper_7554ac, 13, 0x20fc);
scalar_getter!(helper_75569c, 31, 0x2110);
scalar_setter!(helper_754dec, 13, 0x20b0);
scalar_setter!(helper_754e8c, 13, 0x20b4);
scalar_setter!(helper_754fdc, 13, 0x20c8);
scalar_setter!(helper_75507c, 13, 0x20cc);
scalar_setter!(helper_75511c, 13, 0x20d0);
scalar_setter!(helper_7551bc, 13, 0x20d4);
scalar_setter!(helper_7553bc, 13, 0x20f0);
scalar_setter!(helper_75545c, 13, 0x20f4);
scalar_setter!(helper_7554fc, 13, 0x20f8);
scalar_setter!(helper_75559c, 13, 0x20fc);
scalar_setter!(helper_7556ec, 31, 0x2110);

pub fn helper_754edc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(13))?;
    copy_words::<4>(context, output, resource_address(bank).checked_add(0x20b8)?)?;
    Ok(output)
}
macro_rules! auxiliary_copy_setter {
    ($name:ident, $offset:expr, $count:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(13))? {
                copy_words::<$count>(context, resource_address(bank).checked_add($offset)?, source)?;
            }
            Ok(())
        }
    };
}
auxiliary_copy_setter!(helper_75526c, 0x20d8, 4);
auxiliary_copy_setter!(helper_75531c, 0x20e8, 2);
auxiliary_copy_setter!(helper_75564c, 0x2100, 4);

pub fn helper_7557dc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(30))?;
    let offset = 0x38cu32.wrapping_add(index.wrapping_add(0xa6c).wrapping_mul(2));
    Ok(context.read_u16(resource_address(bank).checked_add(offset)?)? as i16 as i32 as u32)
}
pub fn helper_75582c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, value: u32) -> Result<(), MemoryError> {
    if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(30))? {
        let offset = 0x38cu32.wrapping_add(index.wrapping_add(0xa6c).wrapping_mul(2));
        context.write_u16(resource_address(bank).checked_add(offset)?, value as u16)?;
    }
    Ok(())
}
macro_rules! configuration_count {
    ($name:ident, $offset:expr) => { scalar_getter!($name, 30, $offset); };
}
macro_rules! configuration_record {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<GuestAddress, MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField(30))?;
            resource_address(bank).checked_add(($offset as u32).wrapping_add(4).wrapping_add(index.wrapping_mul(104)))
        }
    };
}
macro_rules! configuration_count_setter {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField(30))?;
            context.write_u32(resource_address(bank).checked_add($offset)?, value)
        }
    };
}
macro_rules! configuration_record_setter {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, source: GuestAddress) -> Result<(), MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField(30))?;
            let output = resource_address(bank).checked_add(($offset as u32).wrapping_add(4).wrapping_add(index.wrapping_mul(104)))?;
            copy_words::<26>(context, output, source)
        }
    };
}
configuration_count!(helper_75587c, 0x38c);
configuration_count!(helper_7559bc, 0xbb4);
configuration_count!(helper_755b0c, 0xfc8);
configuration_count!(helper_755c5c, 0x13dc);
configuration_count!(helper_755dac, 0x187c);
configuration_count!(helper_755f0c, 0x1c90);
configuration_record!(helper_755914, 0x38c);
configuration_record!(helper_755a5c, 0xbb4);
configuration_record!(helper_755bac, 0xfc8);
configuration_record!(helper_755cfc, 0x13dc);
configuration_record!(helper_755e54, 0x187c);
configuration_record!(helper_755fb4, 0x1c90);
configuration_count_setter!(helper_755b58, 0xfc8);
configuration_count_setter!(helper_755dfc, 0x187c);
configuration_record_setter!(helper_755c00, 0xfc8);
configuration_record_setter!(helper_755eac, 0x187c);
