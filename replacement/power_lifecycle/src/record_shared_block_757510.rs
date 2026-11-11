//! Entries 0x00757510-0x00758578 over the shared block at 0x0091_904c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::record_profile_accessors::{can_write, copy_words, offset, read_bank, read_header, resource_address, write_header, HeaderField, ProfileField, ProfileHandle};

fn block() -> GuestAddress { GuestAddress::new(SHARED_BLOCK) }

macro_rules! block_getter {
    ($name:ident, $field:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
            read_header(context, block(), $field)
        }
    };
}

macro_rules! block_setter {
    ($name:ident, $profile_field:literal, $field:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($profile_field))? { write_header(context, block(), $field, value)?; }
            Ok(())
        }
    };
}

block_getter!(helper_757510, HeaderField::SignedHalfword(0x248));
block_getter!(helper_757650, HeaderField::SignedHalfword(0x250));
block_getter!(helper_7577e0, HeaderField::Word(0x258));
block_getter!(helper_757828, HeaderField::SignedHalfword(0x25c));
block_getter!(helper_757878, HeaderField::SignedHalfword(0x262));
block_getter!(helper_7578c8, HeaderField::SignedHalfword(0x264));
block_getter!(helper_757968, HeaderField::SignedHalfword(0x268));
block_getter!(helper_7579b8, HeaderField::SignedHalfword(0x26a));
block_getter!(helper_757b78, HeaderField::Word(0x708));
block_getter!(helper_757bc0, HeaderField::SignedHalfword(0x70c));
block_getter!(helper_757c10, HeaderField::SignedHalfword(0x70e));
block_getter!(helper_757c60, HeaderField::SignedHalfword(0x710));
block_getter!(helper_757cb0, HeaderField::SignedHalfword(0x712));
block_getter!(helper_757d00, HeaderField::SignedHalfword(0x718));
block_getter!(helper_757da0, HeaderField::SignedHalfword(0x716));
block_getter!(helper_757df0, HeaderField::Word(0x71c));
block_getter!(helper_757e38, HeaderField::Word(0x720));
block_getter!(helper_757e80, HeaderField::Word(0x724));
block_getter!(helper_757ec8, HeaderField::Halfword(0x728));
block_getter!(helper_757f18, HeaderField::Byte(0x72a));
block_getter!(helper_757f60, HeaderField::Word(0x72c));
block_getter!(helper_7580ec, HeaderField::Word(0x734));
block_getter!(helper_758134, HeaderField::Word(0x738));
block_getter!(helper_75817c, HeaderField::Word(0x838));
block_getter!(helper_7581e8, HeaderField::Byte(0x742));
block_getter!(helper_758278, HeaderField::Word(0x748));
block_getter!(helper_758340, HeaderField::Word(0x890));

block_setter!(helper_75784c, 27, HeaderField::Halfword(0x25c));
block_setter!(helper_757b98, 27, HeaderField::Word(0x708));
block_setter!(helper_757be4, 27, HeaderField::Halfword(0x70c));
block_setter!(helper_757c34, 27, HeaderField::Halfword(0x70e));
block_setter!(helper_757cd4, 27, HeaderField::Halfword(0x712));
block_setter!(helper_757dc4, 27, HeaderField::Halfword(0x716));
block_setter!(helper_757e58, 27, HeaderField::Word(0x720));
block_setter!(helper_758154, 27, HeaderField::Word(0x738));
block_setter!(helper_758298, 27, HeaderField::Word(0x748));

const SLICE_96CC: u32 = 0x0091_96cc;
const SLICE_9798: u32 = 0x0091_9798;
const RESOURCE_2140: u32 = 0x2140;

pub fn helper_757a74<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, _request: u32) -> Result<GuestAddress, MemoryError> {
    copy_words::<10>(context, destination, GuestAddress::new(SLICE_96CC))?;
    Ok(destination)
}

pub fn helper_758068<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _request: u32, destination: GuestAddress) -> Result<(), MemoryError> {
    copy_words::<6>(context, destination, GuestAddress::new(SLICE_9798))
}

pub fn helper_758098<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(15))? { copy_words::<6>(context, GuestAddress::new(SLICE_9798), source)?; }
    Ok(())
}

pub fn helper_758578<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(32))?;
    copy_words::<5>(context, destination, offset(resource_address(bank), RESOURCE_2140))?;
    Ok(destination)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_757510, helper_757510_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757510, |value| value);
    stock_export!(xm1_power_record_helper_757650, helper_757650_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757650, |value| value);
    stock_export!(xm1_power_record_helper_7577e0, helper_7577e0_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_7577e0, |value| value);
    stock_export!(xm1_power_record_helper_757828, helper_757828_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757828, |value| value);
    stock_export!(xm1_power_record_helper_757878, helper_757878_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757878, |value| value);
    stock_export!(xm1_power_record_helper_7578c8, helper_7578c8_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_7578c8, |value| value);
    stock_export!(xm1_power_record_helper_757968, helper_757968_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757968, |value| value);
    stock_export!(xm1_power_record_helper_7579b8, helper_7579b8_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_7579b8, |value| value);
    stock_export!(xm1_power_record_helper_757b78, helper_757b78_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757b78, |value| value);
    stock_export!(xm1_power_record_helper_757bc0, helper_757bc0_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757bc0, |value| value);
    stock_export!(xm1_power_record_helper_757c10, helper_757c10_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757c10, |value| value);
    stock_export!(xm1_power_record_helper_757c60, helper_757c60_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757c60, |value| value);
    stock_export!(xm1_power_record_helper_757cb0, helper_757cb0_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757cb0, |value| value);
    stock_export!(xm1_power_record_helper_757d00, helper_757d00_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757d00, |value| value);
    stock_export!(xm1_power_record_helper_757da0, helper_757da0_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757da0, |value| value);
    stock_export!(xm1_power_record_helper_757df0, helper_757df0_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757df0, |value| value);
    stock_export!(xm1_power_record_helper_757e38, helper_757e38_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757e38, |value| value);
    stock_export!(xm1_power_record_helper_757e80, helper_757e80_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757e80, |value| value);
    stock_export!(xm1_power_record_helper_757ec8, helper_757ec8_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757ec8, |value| value);
    stock_export!(xm1_power_record_helper_757f18, helper_757f18_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757f18, |value| value);
    stock_export!(xm1_power_record_helper_757f60, helper_757f60_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_757f60, |value| value);
    stock_export!(xm1_power_record_helper_7580ec, helper_7580ec_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_7580ec, |value| value);
    stock_export!(xm1_power_record_helper_758134, helper_758134_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_758134, |value| value);
    stock_export!(xm1_power_record_helper_75817c, helper_75817c_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_75817c, |value| value);
    stock_export!(xm1_power_record_helper_7581e8, helper_7581e8_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_7581e8, |value| value);
    stock_export!(xm1_power_record_helper_758278, helper_758278_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_758278, |value| value);
    stock_export!(xm1_power_record_helper_758340, helper_758340_abi, (), () -> u32, (), (), crate::record_shared_block_757510::helper_758340, |value| value);

    stock_export!(xm1_power_record_helper_75784c, helper_75784c_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_75784c, |_| ());
    stock_export!(xm1_power_record_helper_757b98, helper_757b98_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_757b98, |_| ());
    stock_export!(xm1_power_record_helper_757be4, helper_757be4_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_757be4, |_| ());
    stock_export!(xm1_power_record_helper_757c34, helper_757c34_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_757c34, |_| ());
    stock_export!(xm1_power_record_helper_757cd4, helper_757cd4_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_757cd4, |_| ());
    stock_export!(xm1_power_record_helper_757dc4, helper_757dc4_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_757dc4, |_| ());
    stock_export!(xm1_power_record_helper_757e58, helper_757e58_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_757e58, |_| ());
    stock_export!(xm1_power_record_helper_758154, helper_758154_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_758154, |_| ());
    stock_export!(xm1_power_record_helper_758298, helper_758298_abi, (), (handle: u32, value: u32) -> (), (handle: u32, value: u32), (handle, value), crate::record_shared_block_757510::helper_758298, |_| ());

    stock_export!(xm1_power_record_helper_757a74, helper_757a74_abi, (), (destination: u32, request: u32) -> u32, (destination: GuestAddress, request: u32), (GuestAddress::new(destination), request), crate::record_shared_block_757510::helper_757a74, |destination| destination.get());
    stock_export!(xm1_power_record_helper_758068, helper_758068_abi, (), (request: u32, destination: u32) -> (), (request: u32, destination: GuestAddress), (request, GuestAddress::new(destination)), crate::record_shared_block_757510::helper_758068, |_| ());
    stock_export!(xm1_power_record_helper_758098, helper_758098_abi, (), (handle: u32, source: u32) -> (), (handle: u32, source: GuestAddress), (handle, GuestAddress::new(source)), crate::record_shared_block_757510::helper_758098, |_| ());
    stock_export!(xm1_power_record_helper_758578, helper_758578_abi, (), (destination: u32, handle: u32) -> u32, (destination: GuestAddress, handle: u32), (GuestAddress::new(destination), handle), crate::record_shared_block_757510::helper_758578, |destination| destination.get());
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x00757510-0x00758578.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;
    const SOURCE: u32 = 0x0010_0000;
    const DESTINATION: u32 = 0x0010_1000;

    struct Memory {
        slots: [u8; 0x80],
        block: [u8; 0x1000],
        resource: [u8; 0x42b0],
        source: [u8; 0x40],
        destination: [u8; 0x40],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0091_5d38, 0x80), &mut memory.slots).unwrap(),
            MemoryRegion::new(descriptor(SHARED_BLOCK, 0x1000), &mut memory.block).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ebd0, 0x42b0), &mut memory.resource).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 0x40), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(DESTINATION, 0x40), &mut memory.destination).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn admitted(memory: &mut Memory, field: usize, permission: u8) {
        memory.slots[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        memory.slots[0x18 + field] = permission;
    }

    fn put_half(memory: &mut Memory, at: usize, value: u16) { memory.block[at..at + 2].copy_from_slice(&value.to_le_bytes()); }
    fn put_word(memory: &mut Memory, at: usize, value: u32) { memory.block[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
    fn word(memory: &Memory, at: usize) -> u32 { u32::from_le_bytes(memory.block[at..at + 4].try_into().unwrap()) }
    fn half(memory: &Memory, at: usize) -> u16 { u16::from_le_bytes(memory.block[at..at + 2].try_into().unwrap()) }

    fn fill_fields(memory: &mut Memory) {
        put_half(memory, 0x248, 0x8001);
        put_half(memory, 0x250, 0x7fff);
        put_word(memory, 0x258, 0x1122_3344);
        put_half(memory, 0x25c, 0x8002);
        put_half(memory, 0x262, 0xfffe);
        put_half(memory, 0x264, 0x0001);
        put_half(memory, 0x268, 0x8000);
        put_half(memory, 0x26a, 0x7fff);
        put_word(memory, 0x708, 0x5566_7788);
        put_half(memory, 0x70c, 0x8003);
        put_half(memory, 0x70e, 0x0004);
        put_half(memory, 0x710, 0x8005);
        put_half(memory, 0x712, 0x0006);
        put_half(memory, 0x716, 0x8007);
        put_half(memory, 0x718, 0x0008);
        put_word(memory, 0x71c, 0x99aa_bbcc);
        put_word(memory, 0x720, 0x0102_0304);
        put_word(memory, 0x724, 0x0506_0708);
        put_half(memory, 0x728, 0xffff);
        memory.block[0x72a] = 0x7f;
        put_word(memory, 0x72c, 0x0a0b_0c0d);
        put_word(memory, 0x734, 0x0e0f_1011);
        put_word(memory, 0x738, 0x1213_1415);
        memory.block[0x742] = 0x81;
        put_word(memory, 0x748, 0x1617_1819);
        put_word(memory, 0x838, 0x1a1b_1c1d);
        put_word(memory, 0x890, 0x1e1f_2021);
    }

    #[test]
    fn every_getter_reads_its_own_field_and_width() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], resource: [0; 0x42b0], source: [0; 0x40], destination: [0; 0x40] };
        fill_fields(&mut memory);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_757510(context), Ok(0xffff_8001));
            assert_eq!(helper_757650(context), Ok(0x7fff));
            assert_eq!(helper_7577e0(context), Ok(0x1122_3344));
            assert_eq!(helper_757828(context), Ok(0xffff_8002));
            assert_eq!(helper_757878(context), Ok(0xffff_fffe));
            assert_eq!(helper_7578c8(context), Ok(1));
            assert_eq!(helper_757968(context), Ok(0xffff_8000));
            assert_eq!(helper_7579b8(context), Ok(0x7fff));
            assert_eq!(helper_757b78(context), Ok(0x5566_7788));
            assert_eq!(helper_757bc0(context), Ok(0xffff_8003));
            assert_eq!(helper_757c10(context), Ok(4));
            assert_eq!(helper_757c60(context), Ok(0xffff_8005));
            assert_eq!(helper_757cb0(context), Ok(6));
            assert_eq!(helper_757da0(context), Ok(0xffff_8007));
            assert_eq!(helper_757d00(context), Ok(8));
            assert_eq!(helper_757df0(context), Ok(0x99aa_bbcc));
            assert_eq!(helper_757e38(context), Ok(0x0102_0304));
            assert_eq!(helper_757e80(context), Ok(0x0506_0708));
            assert_eq!(helper_757ec8(context), Ok(0xffff));
            assert_eq!(helper_757f18(context), Ok(0x7f));
            assert_eq!(helper_757f60(context), Ok(0x0a0b_0c0d));
            assert_eq!(helper_7580ec(context), Ok(0x0e0f_1011));
            assert_eq!(helper_758134(context), Ok(0x1213_1415));
            assert_eq!(helper_75817c(context), Ok(0x1a1b_1c1d));
            assert_eq!(helper_7581e8(context), Ok(0x81));
            assert_eq!(helper_758278(context), Ok(0x1617_1819));
            assert_eq!(helper_758340(context), Ok(0x1e1f_2021));
        });
    }

    #[test]
    fn every_setter_stores_its_own_field_at_its_own_width() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], resource: [0; 0x42b0], source: [0; 0x40], destination: [0; 0x40] };
        admitted(&mut memory, 27, 3);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_75784c(context, HANDLE, 0x1122_8002), Ok(()));
            assert_eq!(helper_757b98(context, HANDLE, 0x3344_5566), Ok(()));
            assert_eq!(helper_757be4(context, HANDLE, 0x7788_8003), Ok(()));
            assert_eq!(helper_757c34(context, HANDLE, 0x99aa_0004), Ok(()));
            assert_eq!(helper_757cd4(context, HANDLE, 0xbbcc_0006), Ok(()));
            assert_eq!(helper_757dc4(context, HANDLE, 0xddee_8007), Ok(()));
            assert_eq!(helper_757e58(context, HANDLE, 0x0f10_1112), Ok(()));
            assert_eq!(helper_758154(context, HANDLE, 0x1314_1516), Ok(()));
            assert_eq!(helper_758298(context, HANDLE, 0x1718_191a), Ok(()));
        });
        assert_eq!(half(&memory, 0x25c), 0x8002);
        assert_eq!(word(&memory, 0x708), 0x3344_5566);
        assert_eq!(half(&memory, 0x70c), 0x8003);
        assert_eq!(half(&memory, 0x70e), 0x0004);
        assert_eq!(half(&memory, 0x712), 0x0006);
        assert_eq!(half(&memory, 0x716), 0x8007);
        assert_eq!(word(&memory, 0x720), 0x0f10_1112);
        assert_eq!(word(&memory, 0x738), 0x1314_1516);
        assert_eq!(word(&memory, 0x748), 0x1718_191a);
    }

    #[test]
    fn a_handle_without_the_write_permission_stores_nothing() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], resource: [0; 0x42b0], source: [0; 0x40], destination: [0; 0x40] };
        admitted(&mut memory, 27, 1);
        admitted(&mut memory, 15, 1);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_757b98(context, HANDLE, 0x3344_5566), Ok(()));
            assert_eq!(helper_758298(context, HANDLE, 0x1718_191a), Ok(()));
            assert_eq!(helper_758098(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(word(&memory, 0x708), 0);
        assert_eq!(word(&memory, 0x748), 0);
        assert_eq!(word(&memory, 0x74c), 0);
    }

    #[test]
    fn the_slice_copies_carry_their_word_counts() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], resource: [0; 0x42b0], source: [0; 0x40], destination: [0; 0x40] };
        for index in 0..0x28 { memory.block[0x680 + index] = (index as u8).wrapping_add(0x30); }
        for index in 0..0x18 { memory.block[0x74c + index] = (index as u8).wrapping_add(0x60); }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_757a74(context, GuestAddress::new(DESTINATION), 0), Ok(GuestAddress::new(DESTINATION)));
        });
        assert_eq!(&memory.destination[0..0x28], &memory.block[0x680..0x6a8]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_758068(context, 0, GuestAddress::new(DESTINATION)), Ok(()));
        });
        assert_eq!(&memory.destination[0..0x18], &memory.block[0x74c..0x764]);
    }

    #[test]
    fn the_profile_slice_round_trips_behind_the_write_permission() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], resource: [0; 0x42b0], source: [0; 0x40], destination: [0; 0x40] };
        for index in 0..0x18 { memory.source[index] = (index as u8).wrapping_mul(5); }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_758098(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(&memory.block[0x74c..0x764], &[0u8; 0x18]);
        admitted(&mut memory, 15, 3);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_758098(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(&memory.block[0x74c..0x764], &memory.source[0..0x18]);
    }

    #[test]
    fn the_resource_copy_reads_the_bank_the_read_permission_selects() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], resource: [0; 0x42b0], source: [0; 0x40], destination: [0; 0x40] };
        admitted(&mut memory, 32, 3);
        memory.slots[0x08..0x0c].copy_from_slice(&0u32.to_le_bytes());
        memory.resource[0x2140..0x2154].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_758578(context, GuestAddress::new(DESTINATION), HANDLE), Ok(GuestAddress::new(DESTINATION)));
        });
        assert_eq!(&memory.destination[0..20], &memory.resource[0x2140..0x2154]);
    }

    #[test]
    fn the_resource_copy_falls_back_to_the_selected_bank() {
        let mut memory = Memory { slots: [0; 0x80], block: [0; 0x1000], resource: [0; 0x42b0], source: [0; 0x40], destination: [0; 0x40] };
        admitted(&mut memory, 32, 2);
        memory.slots[0x08..0x0c].copy_from_slice(&1u32.to_le_bytes());
        memory.resource[0x2158 + 0x2140..0x2158 + 0x2154].copy_from_slice(&[21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_758578(context, GuestAddress::new(DESTINATION), HANDLE), Ok(GuestAddress::new(DESTINATION)));
        });
        assert_eq!(&memory.destination[0..20], &memory.resource[0x2158 + 0x2140..0x2158 + 0x2154]);
    }
}
