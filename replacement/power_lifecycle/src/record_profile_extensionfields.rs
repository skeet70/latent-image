//! X-M1 1.50 extension fields, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{copy_words, read_bank, resource_address, write_bank, ProfileField, ProfileHandle};

macro_rules! scalar_setter {
    ($name:ident, $permission:expr, $offset:expr, $write:ident, $kind:ty) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField($permission))? {
                context.$write(resource_address(bank).checked_add($offset)?, value as $kind)?;
            }
            Ok(())
        }
    };
}
scalar_setter!(helper_7520c4, 2, 0xcc, write_u32, u32);
scalar_setter!(helper_75236c, 2, 0xfc, write_u16, u16);
scalar_setter!(helper_752404, 2, 0x100, write_u32, u32);
scalar_setter!(helper_7524fc, 3, 0x144, write_u32, u32);
scalar_setter!(helper_7526cc, 3, 0x154, write_u32, u32);
scalar_setter!(helper_75466c, 11, 0x364, write_u32, u32);
scalar_setter!(helper_7548dc, 11, 0x37a, write_u16, u16);
scalar_setter!(helper_75496c, 11, 0x37c, write_u16, u16);
scalar_setter!(helper_7549fc, 11, 0x37e, write_u16, u16);
scalar_setter!(helper_754b88, 12, 0x380, write_u32, u32);
scalar_setter!(helper_754c20, 12, 0x384, write_u8, u8);
scalar_setter!(helper_754cb8, 12, 0x386, write_u16, u16);
scalar_setter!(helper_754d50, 12, 0x388, write_u16, u16);

pub fn helper_751ec8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(2))? {
        let count = context.read_u8(source)?;
        context.write_u8(resource_address(bank).checked_add(0x104)?, count)?;
    }
    Ok(())
}
macro_rules! descriptor_copy_setter {
    ($name:ident, $source_offset:expr, $output_offset:expr, $words:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(2))? {
                copy_words::<$words>(context, resource_address(bank).checked_add($output_offset)?, source.checked_add($source_offset)?)?;
            }
            Ok(())
        }
    };
}
descriptor_copy_setter!(helper_751f58, 4, 0x108, 5);
descriptor_copy_setter!(helper_751fe8, 24, 0x11c, 10);

macro_rules! word_copy_setter {
    ($name:ident, $permission:expr, $offset:expr, $words:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField($permission))? {
                copy_words::<$words>(context, resource_address(bank).checked_add($offset)?, source)?;
            }
            Ok(())
        }
    };
}
word_copy_setter!(helper_752178, 2, 0xd4, 4);
word_copy_setter!(helper_75222c, 2, 0xe4, 4);
word_copy_setter!(helper_7522d4, 2, 0xf4, 2);
word_copy_setter!(helper_754a9c, 3, 0x158, 2);

macro_rules! triplet_setter {
    ($name:ident, $permission:expr, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField($permission))? {
                context.copy_bytes(resource_address(bank).checked_add($offset)?, source, ByteCount(6))?;
            }
            Ok(())
        }
    };
}
triplet_setter!(helper_75259c, 3, 0x148);
triplet_setter!(helper_75263c, 3, 0x14e);
triplet_setter!(helper_75470c, 11, 0x368);
triplet_setter!(helper_7547ac, 11, 0x36e);
triplet_setter!(helper_75484c, 11, 0x374);

macro_rules! copy_getter {
    ($name:ident, $permission:expr, $offset:expr, $words:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField($permission))?;
            copy_words::<$words>(context, output, resource_address(bank).checked_add($offset)?)?;
            Ok(output)
        }
    };
}
copy_getter!(helper_752278, 2, 0xf4, 2);
copy_getter!(helper_752450, 3, 0x144, 7);
copy_getter!(helper_7545c0, 11, 0x364, 7);
copy_getter!(helper_754ae4, 12, 0x380, 3);

pub fn helper_752110<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, output: GuestAddress) -> Result<(), MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(2))?;
    copy_words::<4>(context, output, resource_address(bank).checked_add(0xd4)?)
}
