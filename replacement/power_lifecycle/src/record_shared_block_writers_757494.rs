//! Halfword and word writers over the shared block at 0x0091_904c, image-4 entries 0x00757494-0x00758c30.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::record_profile_accessors::{can_write, HeaderField, ProfileField, ProfileHandle};

const WRITE_FIELD: u32 = 27;

fn block(at: u32) -> GuestAddress { GuestAddress::new(SHARED_BLOCK + at) }

fn write_if_allowed<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, field: HeaderField, value: u32) -> Result<(), MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(WRITE_FIELD))? { return Ok(()); }
    crate::record_profile_accessors::write_header(context, GuestAddress::new(SHARED_BLOCK), field, value)
}

macro_rules! halfword_writer {
    ($name:ident, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            write_if_allowed(context, handle, HeaderField::Halfword($at), value)
        }
    };
}

macro_rules! word_writer {
    ($name:ident, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            write_if_allowed(context, handle, HeaderField::Word($at), value)
        }
    };
}

halfword_writer!(helper_757494, 0x244);
halfword_writer!(helper_7575d4, 0x24c);
halfword_writer!(helper_75789c, 0x262);
halfword_writer!(helper_7578ec, 0x264);
halfword_writer!(helper_75798c, 0x268);
halfword_writer!(helper_7579dc, 0x26a);
halfword_writer!(helper_758a30, 0x6f8);
halfword_writer!(helper_758a80, 0x6fa);
halfword_writer!(helper_758ad0, 0x6fc);
halfword_writer!(helper_758b20, 0x6fe);
halfword_writer!(helper_758b70, 0x700);
halfword_writer!(helper_758bc0, 0x702);
word_writer!(helper_758c0c, 0x704);

pub fn helper_758230<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(block(0x744))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SLOT: u32 = 0x0091_5d44;
    const WRITABLE: u32 = (63 << 18) | (1 << 17) | (0x123 << 4);

    struct Memory {
        slots: [u8; 16 * 0x38],
        block: [u8; 0x900],
    }

    impl Memory {
        fn new(permitted: bool) -> Self {
            let mut memory = Memory { slots: [0; 16 * 0x38], block: [0; 0x900] };
            memory.slots[..4].copy_from_slice(&0x123u32.to_le_bytes());
            memory.slots[12 + WRITE_FIELD as usize] = if permitted { 2 } else { 0 };
            memory
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SLOT, 16 * 0x38), &mut self.slots).unwrap(),
                MemoryRegion::new(descriptor(SHARED_BLOCK, 0x900), &mut self.block).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }

        fn half(&self, at: u32) -> u16 { u16::from_le_bytes(self.block[at as usize..at as usize + 2].try_into().unwrap()) }
        fn word(&self, at: u32) -> u32 { u32::from_le_bytes(self.block[at as usize..at as usize + 4].try_into().unwrap()) }
    }

    #[test]
    fn every_halfword_writer_stores_only_its_own_halfword() {
        let mut memory = Memory::new(true);
        memory.with_context(|context| {
            assert_eq!(helper_757494(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_7575d4(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_75789c(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_7578ec(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_75798c(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_7579dc(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_758a30(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_758a80(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_758ad0(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_758b20(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_758b70(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_758bc0(context, WRITABLE, 0xabcd), Ok(()));
        });
        for at in [0x244u32, 0x24c, 0x262, 0x264, 0x268, 0x26a, 0x6f8, 0x6fa, 0x6fc, 0x6fe, 0x700, 0x702] {
            assert_eq!(memory.half(at), 0xabcd, "offset {at:#x}");
        }
        assert_eq!(memory.half(0x246), 0);
    }

    #[test]
    fn the_word_writer_stores_a_word() {
        let mut memory = Memory::new(true);
        memory.with_context(|context| assert_eq!(helper_758c0c(context, WRITABLE, 0x1122_3344), Ok(())));
        assert_eq!(memory.word(0x704), 0x1122_3344);
    }

    #[test]
    fn a_handle_without_the_write_permission_leaves_the_block_alone() {
        let mut memory = Memory::new(false);
        memory.with_context(|context| {
            assert_eq!(helper_757494(context, WRITABLE, 0xabcd), Ok(()));
            assert_eq!(helper_758c0c(context, WRITABLE, 0x1122_3344), Ok(()));
            assert_eq!(helper_758a30(context, u32::MAX, 0xabcd), Ok(()));
        });
        assert_eq!(memory.half(0x244), 0);
        assert_eq!(memory.word(0x704), 0);
        assert_eq!(memory.half(0x6f8), 0);
    }

    #[test]
    fn the_744_getter_reads_the_block_word() {
        let mut memory = Memory::new(true);
        memory.block[0x744..0x748].copy_from_slice(&0x5566_7788u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_758230(context), Ok(0x5566_7788)));
    }
}
