use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{mark_mask, read_bank, resource_address, write_bank, ProfileField, ProfileHandle};

macro_rules! byte_setter {
    ($name:ident, $offset:expr, $mask:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(0))? {
                let resource = resource_address(bank);
                context.write_u8(resource.checked_add($offset)?, value as u8)?;
                mark_mask(context, resource, $mask, 0x8000_0000)?;
            }
            Ok(())
        }
    };
}

byte_setter!(helper_758d34, 0x56, 0x400);
byte_setter!(helper_758dbc, 0x57, 0x800);
byte_setter!(helper_758f64, 0x71, 0x4000);
byte_setter!(helper_758fec, 0x70, 0x10000);
byte_setter!(helper_759074, 0x6e, 0x20000);
byte_setter!(helper_7590fc, 0x58, 0x40000);
byte_setter!(helper_759184, 0x59, 0x80000);
byte_setter!(helper_75920c, 0x5a, 0x100000);

macro_rules! block_setter {
    ($name:ident, $offset:expr, $mask:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(0))? {
                let resource = resource_address(bank);
                context.copy_bytes(resource.checked_add($offset)?, source, ByteCount(4))?;
                mark_mask(context, resource, $mask, 0x8000_0000)?;
            }
            Ok(())
        }
    };
}

block_setter!(helper_758e4c, 0x72, 0x1000);
block_setter!(helper_758edc, 0x76, 0x2000);

macro_rules! byte_getter {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField(0))?;
            Ok(u32::from(context.read_u8(resource_address(bank).checked_add($offset)?)?))
        }
    };
}

byte_getter!(helper_7591c8, 0x5a);
byte_getter!(helper_758cf0, 0x56);
byte_getter!(helper_758d78, 0x57);

pub fn helper_758f20<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(0))?;
    Ok(context.read_u8(resource_address(bank).checked_add(0x71)?)? as i8 as i32 as u32)
}

macro_rules! block_getter {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, output: GuestAddress) -> Result<(), MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField(0))?;
            context.copy_bytes(output, resource_address(bank).checked_add($offset)?, ByteCount(4))
        }
    };
}

block_getter!(helper_758e00, 0x72);
block_getter!(helper_758e90, 0x76);
