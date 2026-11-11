//! Entries 0x00756720-0x00756f10 over the shared block at 0x0091_904c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::record_profile_accessors::{can_write, copy_words, ProfileField, ProfileHandle};

fn block(at: u32) -> GuestAddress { GuestAddress::new(SHARED_BLOCK.wrapping_add(at)) }

macro_rules! block_word_setter {
    ($name:ident, $field:literal, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? { context.write_u32(block($at), value)?; }
            Ok(())
        }
    };
}

macro_rules! block_copy_out {
    ($name:ident, $at:expr, $words:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _request: u32, destination: GuestAddress) -> Result<(), MemoryError> {
            copy_words::<$words>(context, destination, block($at))
        }
    };
}

/// Byte offsets into the block; the suffix is each slice's word count.
const SNAPSHOT_8: u32 = 0x84;
const SNAPSHOT_3: u32 = 0xa4;
const SNAPSHOT_12: u32 = 0xb0;
const SNAPSHOT_2: u32 = 0x1a4;

block_copy_out!(helper_756788, SNAPSHOT_8, 8);
block_copy_out!(helper_7567ec, SNAPSHOT_3, 3);
block_copy_out!(helper_756840, SNAPSHOT_12, 12);

block_word_setter!(helper_756a0c, 20, 0x140);
block_word_setter!(helper_756a54, 20, 0x144);
block_word_setter!(helper_756a9c, 20, 0x148);
block_word_setter!(helper_756b2c, 20, 0x150);
block_word_setter!(helper_756dfc, 21, 0x178);

pub fn helper_756f10<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(24))? { copy_words::<2>(context, block(SNAPSHOT_2), source)?; }
    Ok(())
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_756788, helper_756788_abi, (), (request: u32, destination: u32) -> (), (request: u32, destination: GuestAddress), (request, GuestAddress::new(destination)), crate::record_shared_block_756720::helper_756788, |_| ());
    stock_export!(xm1_power_record_helper_7567ec, helper_7567ec_abi, (), (request: u32, destination: u32) -> (), (request: u32, destination: GuestAddress), (request, GuestAddress::new(destination)), crate::record_shared_block_756720::helper_7567ec, |_| ());
    stock_export!(xm1_power_record_helper_756840, helper_756840_abi, (), (request: u32, destination: u32) -> (), (request: u32, destination: GuestAddress), (request, GuestAddress::new(destination)), crate::record_shared_block_756720::helper_756840, |_| ());
    stock_export!(xm1_power_record_helper_756a0c, helper_756a0c_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_756720::helper_756a0c, |_| ());
    stock_export!(xm1_power_record_helper_756a54, helper_756a54_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_756720::helper_756a54, |_| ());
    stock_export!(xm1_power_record_helper_756a9c, helper_756a9c_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_756720::helper_756a9c, |_| ());
    stock_export!(xm1_power_record_helper_756b2c, helper_756b2c_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_756720::helper_756b2c, |_| ());
    stock_export!(xm1_power_record_helper_756dfc, helper_756dfc_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_756720::helper_756dfc, |_| ());
    stock_export!(xm1_power_record_helper_756f10, helper_756f10_abi, (), (handle: u32, source: u32) -> (), (handle: u32, source: GuestAddress), (handle, GuestAddress::new(source)), crate::record_shared_block_756720::helper_756f10, |_| ());
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x00756788-0x00756f10.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;
    const SOURCE: u32 = 0x0010_0000;
    const DESTINATION: u32 = 0x0010_1000;

    struct Memory {
        slots: [u8; 0x80],
        block: [u8; 0x1000],
        source: [u8; 0x20],
        destination: [u8; 0x40],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0091_5d38, 0x80), &mut memory.slots).unwrap(),
            MemoryRegion::new(descriptor(SHARED_BLOCK, 0x1000), &mut memory.block).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 0x20), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(DESTINATION, 0x40), &mut memory.destination).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn admitted(memory: &mut Memory, field: usize, permission: u8) {
        memory.slots[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        memory.slots[0x18 + field] = permission;
    }

    fn word(memory: &Memory, at: usize) -> u32 { u32::from_le_bytes(memory.block[at..at + 4].try_into().unwrap()) }

    #[test]
    fn the_snapshot_copies_carry_their_own_word_counts() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x20], destination: [0; 0x40] };
        for index in 0..0x1a8 { memory.block[index] = (index as u8).wrapping_add(1); }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756788(context, HANDLE, GuestAddress::new(DESTINATION)), Ok(()));
        });
        assert_eq!(&memory.destination[0..32], &memory.block[0x84..0xa4]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_7567ec(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(&memory.source[0..12], &memory.block[0xa4..0xb0]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756840(context, HANDLE, GuestAddress::new(DESTINATION)), Ok(()));
        });
        assert_eq!(&memory.destination[0..48], &memory.block[0xb0..0xe0]);
    }

    #[test]
    fn the_setters_store_only_for_an_admitted_field() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x20], destination: [0; 0x40] };
        admitted(&mut memory, 20, 3);
        admitted(&mut memory, 21, 3);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756a0c(context, HANDLE, 0x11), Ok(()));
            assert_eq!(helper_756a54(context, HANDLE, 0x22), Ok(()));
            assert_eq!(helper_756a9c(context, HANDLE, 0x33), Ok(()));
            assert_eq!(helper_756b2c(context, HANDLE, 0x44), Ok(()));
            assert_eq!(helper_756dfc(context, HANDLE, 0x55), Ok(()));
        });
        assert_eq!(word(&memory, 0x140), 0x11);
        assert_eq!(word(&memory, 0x144), 0x22);
        assert_eq!(word(&memory, 0x148), 0x33);
        assert_eq!(word(&memory, 0x150), 0x44);
        assert_eq!(word(&memory, 0x178), 0x55);
    }

    #[test]
    fn a_handle_without_the_field_permission_stores_nothing() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x20], destination: [0; 0x40] };
        admitted(&mut memory, 20, 1);
        admitted(&mut memory, 21, 2);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756a0c(context, HANDLE, 0x11), Ok(()));
            assert_eq!(helper_756dfc(context, HANDLE, 0x55), Ok(()));
        });
        assert_eq!(word(&memory, 0x140), 0);
        assert_eq!(word(&memory, 0x178), 0x55);
    }

    #[test]
    fn the_source_copy_stores_two_words_behind_the_permission() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x20], destination: [0; 0x40] };
        memory.source[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756f10(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(word(&memory, 0x1a4), 0);
        admitted(&mut memory, 24, 3);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756f10(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(&memory.block[0x1a4..0x1ac], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
