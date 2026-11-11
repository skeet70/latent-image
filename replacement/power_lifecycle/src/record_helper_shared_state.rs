//! Stock-shaped record helpers over the shared block at 0x0091_904c.
//!
//! Each body is the image-4 instruction sequence at the address in its name.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{can_write, copy_words, ProfileField, ProfileHandle};

pub const SHARED_BLOCK: u32 = 0x0091_904c;

pub fn shared_word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, at: u32) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(SHARED_BLOCK + at))
}

macro_rules! word_setter {
    ($name:ident, $at:literal, $field:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? { context.write_u32(GuestAddress::new(SHARED_BLOCK + $at), value)?; }
            Ok(())
        }
    };
}

macro_rules! halfword_setter {
    ($name:ident, $at:literal, $field:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? { context.write_u16(GuestAddress::new(SHARED_BLOCK + $at), value as u16)?; }
            Ok(())
        }
    };
}

macro_rules! byte_setter {
    ($name:ident, $at:literal, $field:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? { context.write_u8(GuestAddress::new(SHARED_BLOCK + $at), value as u8)?; }
            Ok(())
        }
    };
}

word_setter!(helper_7569c4, 0x13c, 20);
word_setter!(helper_756bbc, 0x158, 20);
word_setter!(helper_756c04, 0x15c, 20);
word_setter!(helper_756c4c, 0x160, 20);
word_setter!(helper_756c94, 0x164, 20);
word_setter!(helper_756d24, 0x16c, 20);
word_setter!(helper_756d6c, 0x170, 20);
halfword_setter!(helper_756fb8, 0x1fc, 26);
word_setter!(helper_757004, 0x200, 26);
halfword_setter!(helper_757050, 0x206, 26);
halfword_setter!(helper_7570a0, 0x208, 26);
halfword_setter!(helper_7570f0, 0x204, 26);
halfword_setter!(helper_757140, 0x20c, 26);
halfword_setter!(helper_757190, 0x212, 26);
halfword_setter!(helper_7571e0, 0x216, 26);
word_setter!(helper_75722c, 0x218, 26);
word_setter!(helper_757274, 0x230, 26);
halfword_setter!(helper_7572c0, 0x238, 26);
halfword_setter!(helper_757360, 0x234, 26);
byte_setter!(helper_7573ac, 0x236, 26);
word_setter!(helper_7573f4, 0x23c, 26);
word_setter!(helper_7583d0, 0x79c, 29);

pub fn helper_7589e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(35))? {
        let value = context.read_u32(source)?;
        context.write_u32(GuestAddress::new(SHARED_BLOCK + 0x83c), value)?;
    }
    Ok(())
}

pub fn helper_756878<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(17))? { copy_words::<12>(context, GuestAddress::new(0x0091_90fc), source)?; }
    Ok(())
}

pub fn helper_75a440<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    crate::record_profile_shared_accessors::helper_75a440(context, handle)
}

pub fn helper_75a61c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    crate::record_profile_shared_accessors::helper_75a61c(context, handle)
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::record_helper_native_memory::InstalledMemory;

    macro_rules! value_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name(handle: u32, value: u32) {
                let _ = super::$helper(&mut RecordContext::new(InstalledMemory, ()), handle, value);
            }
        };
    }
    macro_rules! source_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name(handle: u32, source: u32) {
                let _ = super::$helper(&mut RecordContext::new(InstalledMemory, ()), handle, GuestAddress::new(source));
            }
        };
    }
    macro_rules! getter_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name(handle: u32) -> u32 {
                super::$helper(&mut RecordContext::new(InstalledMemory, ()), handle).unwrap_or(0)
            }
        };
    }

    value_export!(xm1_power_record_helper_7569c4, helper_7569c4);
    value_export!(xm1_power_record_helper_756bbc, helper_756bbc);
    value_export!(xm1_power_record_helper_756c04, helper_756c04);
    value_export!(xm1_power_record_helper_756c4c, helper_756c4c);
    value_export!(xm1_power_record_helper_756c94, helper_756c94);
    value_export!(xm1_power_record_helper_756d24, helper_756d24);
    value_export!(xm1_power_record_helper_756d6c, helper_756d6c);
    value_export!(xm1_power_record_helper_756fb8, helper_756fb8);
    value_export!(xm1_power_record_helper_757004, helper_757004);
    value_export!(xm1_power_record_helper_757050, helper_757050);
    value_export!(xm1_power_record_helper_7570a0, helper_7570a0);
    value_export!(xm1_power_record_helper_7570f0, helper_7570f0);
    value_export!(xm1_power_record_helper_757140, helper_757140);
    value_export!(xm1_power_record_helper_757190, helper_757190);
    value_export!(xm1_power_record_helper_7571e0, helper_7571e0);
    value_export!(xm1_power_record_helper_75722c, helper_75722c);
    value_export!(xm1_power_record_helper_757274, helper_757274);
    value_export!(xm1_power_record_helper_7572c0, helper_7572c0);
    value_export!(xm1_power_record_helper_757360, helper_757360);
    value_export!(xm1_power_record_helper_7573ac, helper_7573ac);
    value_export!(xm1_power_record_helper_7573f4, helper_7573f4);
    value_export!(xm1_power_record_helper_7583d0, helper_7583d0);
    source_export!(xm1_power_record_helper_7589e0, helper_7589e0);
    source_export!(xm1_power_record_helper_756878, helper_756878);
    getter_export!(xm1_power_record_helper_75a440, helper_75a440);
    getter_export!(xm1_power_record_helper_75a61c, helper_75a61c);
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x00756878 and
    //! 0x007569c4-0x0075a61c.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;

    struct Memory {
        pointer: [u8; 4],
        slots: [u8; 0x80],
        block: [u8; 0x1000],
        source: [u8; 0x40],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0091_5d38, 0x80), &mut memory.slots).unwrap(),
            MemoryRegion::new(descriptor(0x0091_904c, 0x1000), &mut memory.block).unwrap(),
            MemoryRegion::new(descriptor(0x0010_0000, 0x40), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(0x0010_1000, 4), &mut memory.pointer).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn admitted(memory: &mut Memory) {
        memory.slots[0x08..0x0c].copy_from_slice(&0u32.to_le_bytes());
        memory.slots[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        memory.slots[0x14..0x18].copy_from_slice(&0u32.to_le_bytes());
        for field in 0..43 { memory.slots[0x18 + field] = 3; }
    }

    #[test]
    fn word_setters_store_only_for_admitted_fields() {
        let mut memory = Memory { pointer: [0; 4], slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x40] };
        admitted(&mut memory);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_7569c4(context, HANDLE, 0x1122_3344), Ok(()));
            assert_eq!(helper_756bbc(context, HANDLE, 0x5566_7788), Ok(()));
            assert_eq!(helper_756c04(context, HANDLE, 0x99aa_bbcc), Ok(()));
            assert_eq!(helper_756c4c(context, HANDLE, 0xddee_ff00), Ok(()));
            assert_eq!(helper_756c94(context, HANDLE, 1), Ok(()));
            assert_eq!(helper_756d24(context, HANDLE, 2), Ok(()));
            assert_eq!(helper_756d6c(context, HANDLE, 3), Ok(()));
            assert_eq!(helper_757004(context, HANDLE, 4), Ok(()));
            assert_eq!(helper_75722c(context, HANDLE, 5), Ok(()));
            assert_eq!(helper_757274(context, HANDLE, 6), Ok(()));
            assert_eq!(helper_7573f4(context, HANDLE, 7), Ok(()));
            assert_eq!(helper_7583d0(context, HANDLE, 8), Ok(()));
            assert_eq!(helper_756fb8(context, HANDLE, 0x1_2345), Ok(()));
            assert_eq!(helper_757050(context, HANDLE, 0x6789), Ok(()));
            assert_eq!(helper_7570a0(context, HANDLE, 0xabcd), Ok(()));
            assert_eq!(helper_7570f0(context, HANDLE, 0xef01), Ok(()));
            assert_eq!(helper_757140(context, HANDLE, 0x2345), Ok(()));
            assert_eq!(helper_757190(context, HANDLE, 0x6789), Ok(()));
            assert_eq!(helper_7571e0(context, HANDLE, 0xabcd), Ok(()));
            assert_eq!(helper_7572c0(context, HANDLE, 0xef01), Ok(()));
            assert_eq!(helper_757360(context, HANDLE, 0x1357), Ok(()));
            assert_eq!(helper_7573ac(context, HANDLE, 0x1ff), Ok(()));
        });
        assert_eq!(&memory.block[0x13c..0x140], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&memory.block[0x158..0x15c], &0x5566_7788u32.to_le_bytes());
        assert_eq!(&memory.block[0x15c..0x160], &0x99aa_bbccu32.to_le_bytes());
        assert_eq!(&memory.block[0x160..0x164], &0xddee_ff00u32.to_le_bytes());
        assert_eq!(&memory.block[0x164..0x168], &1u32.to_le_bytes());
        assert_eq!(&memory.block[0x16c..0x170], &2u32.to_le_bytes());
        assert_eq!(&memory.block[0x170..0x174], &3u32.to_le_bytes());
        assert_eq!(&memory.block[0x200..0x204], &4u32.to_le_bytes());
        assert_eq!(&memory.block[0x218..0x21c], &5u32.to_le_bytes());
        assert_eq!(&memory.block[0x230..0x234], &6u32.to_le_bytes());
        assert_eq!(&memory.block[0x23c..0x240], &7u32.to_le_bytes());
        assert_eq!(&memory.block[0x79c..0x7a0], &8u32.to_le_bytes());
        assert_eq!(&memory.block[0x1fc..0x1fe], &0x2345u16.to_le_bytes());
        assert_eq!(&memory.block[0x206..0x208], &0x6789u16.to_le_bytes());
        assert_eq!(&memory.block[0x208..0x20a], &0xabcdu16.to_le_bytes());
        assert_eq!(&memory.block[0x204..0x206], &0xef01u16.to_le_bytes());
        assert_eq!(&memory.block[0x20c..0x20e], &0x2345u16.to_le_bytes());
        assert_eq!(&memory.block[0x212..0x214], &0x6789u16.to_le_bytes());
        assert_eq!(&memory.block[0x216..0x218], &0xabcdu16.to_le_bytes());
        assert_eq!(&memory.block[0x238..0x23a], &0xef01u16.to_le_bytes());
        assert_eq!(&memory.block[0x234..0x236], &0x1357u16.to_le_bytes());
        assert_eq!(memory.block[0x236], 0xff);
    }

    #[test]
    fn setters_ignore_a_handle_without_the_write_permission() {
        let mut memory = Memory { pointer: [0; 4], slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x40] };
        admitted(&mut memory);
        memory.slots[0x18 + 26] = 1;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_7569c4(context, HANDLE, 0x1122_3344), Ok(()));
            assert_eq!(helper_757004(context, HANDLE, 0x1122_3344), Ok(()));
        });
        assert_eq!(&memory.block[0x13c..0x140], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&memory.block[0x200..0x204], &0u32.to_le_bytes());
    }

    #[test]
    fn pointer_and_block_copies_follow_the_admitted_slot() {
        let mut memory = Memory { pointer: [0; 4], slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x40] };
        admitted(&mut memory);
        for index in 0..12 { memory.source[index * 4..index * 4 + 4].copy_from_slice(&(0x1000 + index as u32).to_le_bytes()); }
        memory.pointer = 0x0a0b_0c0du32.to_le_bytes();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756878(context, HANDLE, GuestAddress::new(0x0010_0000)), Ok(()));
            assert_eq!(helper_7589e0(context, HANDLE, GuestAddress::new(0x0010_1000)), Ok(()));
        });
        for index in 0..12 { assert_eq!(&memory.block[0xb0 + index * 4..0xb4 + index * 4], &(0x1000 + index as u32).to_le_bytes()); }
        assert_eq!(&memory.block[0x83c..0x840], &0x0a0b_0c0du32.to_le_bytes());
    }

    #[test]
    fn getters_read_the_shared_block() {
        let mut memory = Memory { pointer: [0; 4], slots: [0; 0x80], block: [0; 0x1000], source: [0; 0x40] };
        memory.block[0x972..0x974].copy_from_slice(&0xfffeu16.to_le_bytes());
        memory.block[0x9d0..0x9d4].copy_from_slice(&0x8091_a0b0u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_75a440(context, HANDLE), Ok(0xffff_fffe));
            assert_eq!(helper_75a61c(context, HANDLE), Ok(0x8091_a0b0));
        });
    }
}
