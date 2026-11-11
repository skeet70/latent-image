//! Shared block 0x0091_904c: the zeroing at 0x0075_6700 and the profile-gated
//! field stores 0x0075_7534-0x0075_7f80 that follow the getters of
//! `record_shared_block_757510`.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::record_profile_accessors::{can_write, write_header, HeaderField, ProfileField, ProfileHandle};

/// Bytes the image-4 body at 0x0075_6700 clears.
pub const BLOCK_BYTES: u32 = 0x9d4;

fn block() -> GuestAddress {
    GuestAddress::new(SHARED_BLOCK)
}

/// The image-4 body at 0x0075_6700.
pub fn helper_756700<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    for offset in 0..BLOCK_BYTES {
        context.write_u8(block().checked_add(offset)?, 0)?;
    }
    Ok(())
}

/// The store every body of this family shares: field 27's write permission,
/// then `field`'s width at the block.
pub fn store<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, field: HeaderField, value: u32) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(27))? {
        write_header(context, block(), field, value)?;
    }
    Ok(())
}

macro_rules! block_store {
    ($name:ident, $field:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            store(context, handle, $field, value)
        }
    };
}

block_store!(helper_757534, HeaderField::Halfword(0x248));
block_store!(helper_757584, HeaderField::Halfword(0x24a));
block_store!(helper_757c84, HeaderField::Halfword(0x710));
block_store!(helper_757d24, HeaderField::Halfword(0x718));
block_store!(helper_757d74, HeaderField::Halfword(0x714));
block_store!(helper_757e10, HeaderField::Word(0x71c));
block_store!(helper_757ea0, HeaderField::Word(0x724));
block_store!(helper_757eec, HeaderField::Halfword(0x728));
block_store!(helper_757f38, HeaderField::Byte(0x72a));
block_store!(helper_757f80, HeaderField::Word(0x72c));

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0075_6700-0x0075_7f80.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;

    struct Memory {
        slots: [u8; 0x80],
        block: [u8; BLOCK_BYTES as usize],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0091_5d38, 0x80), &mut memory.slots).unwrap(),
            MemoryRegion::new(descriptor(SHARED_BLOCK, BLOCK_BYTES), &mut memory.block).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    fn admitted(memory: &mut Memory, field: usize, permission: u8) {
        memory.slots[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        memory.slots[0x18 + field] = permission;
    }

    fn word(memory: &Memory, at: usize) -> u32 { u32::from_le_bytes(memory.block[at..at + 4].try_into().unwrap()) }
    fn half(memory: &Memory, at: usize) -> u16 { u16::from_le_bytes(memory.block[at..at + 2].try_into().unwrap()) }

    #[test]
    fn the_block_fill_clears_every_byte_of_the_block() {
        let mut memory = Memory { slots: [0; 0x80], block: [0xaa; BLOCK_BYTES as usize] };
        with_memory(&mut memory, |context| assert_eq!(helper_756700(context), Ok(())));
        assert!(memory.block.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn each_store_keeps_its_own_offset_and_width() {
        let mut memory = Memory { slots: [0; 0x80], block: [0xaa; BLOCK_BYTES as usize] };
        admitted(&mut memory, 27, 3);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_757534(context, HANDLE, 0x1122_8001), Ok(()));
            assert_eq!(helper_757584(context, HANDLE, 0x3344_7fff), Ok(()));
            assert_eq!(helper_757c84(context, HANDLE, 0x5566_8005), Ok(()));
            assert_eq!(helper_757d24(context, HANDLE, 0x7788_0008), Ok(()));
            assert_eq!(helper_757d74(context, HANDLE, 0x99aa_8002), Ok(()));
            assert_eq!(helper_757e10(context, HANDLE, 0xbbcc_ddee), Ok(()));
            assert_eq!(helper_757ea0(context, HANDLE, 0x0f10_1112), Ok(()));
            assert_eq!(helper_757eec(context, HANDLE, 0x1314_1516), Ok(()));
            assert_eq!(helper_757f38(context, HANDLE, 0x1718_191a), Ok(()));
            assert_eq!(helper_757f80(context, HANDLE, 0x1b1c_1d1e), Ok(()));
        });
        assert_eq!(half(&memory, 0x248), 0x8001);
        assert_eq!(half(&memory, 0x24a), 0x7fff);
        assert_eq!(half(&memory, 0x710), 0x8005);
        assert_eq!(half(&memory, 0x718), 0x0008);
        assert_eq!(half(&memory, 0x714), 0x8002);
        assert_eq!(word(&memory, 0x71c), 0xbbcc_ddee);
        assert_eq!(word(&memory, 0x724), 0x0f10_1112);
        assert_eq!(half(&memory, 0x728), 0x1516);
        assert_eq!(memory.block[0x72a], 0x1a);
        assert_eq!(word(&memory, 0x72c), 0x1b1c_1d1e);
    }

    #[test]
    fn a_handle_without_the_write_permission_stores_nothing() {
        let mut memory = Memory { slots: [0; 0x80], block: [0xaa; BLOCK_BYTES as usize] };
        admitted(&mut memory, 27, 1);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_757534(context, HANDLE, 0x1122_8001), Ok(()));
            assert_eq!(helper_757f80(context, HANDLE, 0x1b1c_1d1e), Ok(()));
        });
        assert_eq!(half(&memory, 0x248), 0xaaaa);
        assert_eq!(word(&memory, 0x72c), 0xaaaa_aaaa);
    }

    #[test]
    fn an_unmatched_serial_leaves_the_block_alone() {
        let mut memory = Memory { slots: [0; 0x80], block: [0xaa; BLOCK_BYTES as usize] };
        admitted(&mut memory, 27, 3);
        memory.slots[0x0c..0x10].copy_from_slice(&7u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_757e10(context, HANDLE, 0xbbcc_ddee), Ok(())));
        assert_eq!(word(&memory, 0x71c), 0xaaaa_aaaa);
    }
}
