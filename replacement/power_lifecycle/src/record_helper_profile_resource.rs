//! Stock-shaped record helpers over a profile resource block.
//!
//! A resource block sits at 0x0096_ebd0 + 0x2158 * bank; each body is the
//! image-4 instruction sequence at the address in its name.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{can_read, can_write, handle_bank, offset, read_bank, resource_address, ProfileField, ProfileHandle};

macro_rules! resource_word_setter {
    ($name:ident, $field:literal, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? {
                let bank = handle_bank(context, ProfileHandle(handle))?;
                context.write_u32(offset(resource_address(bank), $at), value)?;
            }
            Ok(())
        }
    };
}

macro_rules! resource_slot_word_setter {
    ($name:ident, $field:literal, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? {
                let bank = handle_bank(context, ProfileHandle(handle))?;
                context.write_u32(slot(resource_address(bank), index, $at), value)?;
            }
            Ok(())
        }
    };
}

macro_rules! resource_slot_halfword_setter {
    ($name:ident, $field:literal, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? {
                let bank = handle_bank(context, ProfileHandle(handle))?;
                context.write_u16(slot(resource_address(bank), index, $at), value as u16)?;
            }
            Ok(())
        }
    };
}

macro_rules! resource_slot_byte_setter {
    ($name:ident, $field:literal, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? {
                let bank = handle_bank(context, ProfileHandle(handle))?;
                context.write_u8(slot(resource_address(bank), index, $at), value as u8)?;
            }
            Ok(())
        }
    };
}

fn slot(resource: GuestAddress, index: u32, at: u32) -> GuestAddress {
    offset(resource, 0x16cu32.wrapping_add(index.wrapping_mul(0xa8)).wrapping_add(at))
}

pub fn helper_74e1f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(2))? {
        let bank = handle_bank(context, ProfileHandle(handle))?;
        context.copy_bytes(offset(resource_address(bank), 0xc8), source, ByteCount(124))?;
    }
    Ok(())
}

pub fn helper_74edf4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    crate::record_profile_accessors::helper_74edf4(context, handle)
}

fn signed_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, field: u32, at: u32) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(field))?;
    context.read_u16(offset(resource_address(bank), at)).map(|value| value as i16 as i32 as u32)
}

pub fn helper_751914<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> { signed_halfword(context, handle, 1, 0xb4) }
pub fn helper_7519ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> { signed_halfword(context, handle, 1, 0xb6) }

resource_word_setter!(helper_7527ac, 4, 0x160);
resource_word_setter!(helper_752844, 4, 0x164);
resource_word_setter!(helper_7528dc, 4, 0x168);

pub fn helper_752990<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
    if !can_read(context, ProfileHandle(handle), ProfileField(5))? { return Ok(0); }
    let bank = handle_bank(context, ProfileHandle(handle))?;
    context.read_u16(slot(resource_address(bank), index, 0)).map(|value| value as i16 as i32 as u32)
}

resource_slot_halfword_setter!(helper_7529e8, 5, 0);
resource_slot_halfword_setter!(helper_753018, 5, 0x12);
resource_slot_halfword_setter!(helper_7530c8, 5, 0x14);
resource_slot_halfword_setter!(helper_753228, 5, 0x18);
resource_slot_byte_setter!(helper_7532d8, 5, 0x2c);
resource_slot_word_setter!(helper_753388, 5, 0x1c);
resource_slot_word_setter!(helper_753438, 5, 0x20);
resource_slot_word_setter!(helper_7534e8, 5, 0x24);
resource_slot_halfword_setter!(helper_753598, 5, 0x16);
resource_slot_word_setter!(helper_753648, 5, 0x28);
resource_slot_word_setter!(helper_7539b8, 7, 0x44);
resource_slot_word_setter!(helper_753a68, 7, 0x48);
resource_slot_word_setter!(helper_753b18, 7, 0x4c);
resource_slot_halfword_setter!(helper_753f38, 8, 0x84);
resource_slot_halfword_setter!(helper_753fe8, 8, 0x86);
resource_slot_word_setter!(helper_7542a8, 10, 0xa4);
resource_slot_word_setter!(helper_7544b8, 10, 0x9c);
resource_slot_word_setter!(helper_754568, 10, 0xa0);

resource_word_setter!(helper_754dec, 13, 0x20b0);
resource_word_setter!(helper_754e8c, 13, 0x20b4);
resource_word_setter!(helper_754fdc, 13, 0x20c8);
resource_word_setter!(helper_75507c, 13, 0x20cc);
resource_word_setter!(helper_75511c, 13, 0x20d0);
resource_word_setter!(helper_7551bc, 13, 0x20d4);
resource_word_setter!(helper_7553bc, 13, 0x20f0);
resource_word_setter!(helper_75545c, 13, 0x20f4);
resource_word_setter!(helper_75559c, 13, 0x20fc);
resource_word_setter!(helper_75578c, 31, 0x2114);

pub fn helper_75564c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(13))? {
        let bank = handle_bank(context, ProfileHandle(handle))?;
        crate::record_profile_accessors::copy_words::<4>(context, offset(resource_address(bank), 0x2100), source)?;
    }
    Ok(())
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::record_helper_native_memory::InstalledMemory;

    macro_rules! word_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name(handle: u32, value: u32) {
                let _ = super::$helper(&mut RecordContext::new(InstalledMemory, ()), handle, value);
            }
        };
    }
    macro_rules! slot_word_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name(handle: u32, index: u32, value: u32) {
                let _ = super::$helper(&mut RecordContext::new(InstalledMemory, ()), handle, index, value);
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
    macro_rules! slot_getter_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name(handle: u32, index: u32) -> u32 {
                super::$helper(&mut RecordContext::new(InstalledMemory, ()), handle, index).unwrap_or(0)
            }
        };
    }

    source_export!(xm1_power_record_helper_74e1f0, helper_74e1f0);
    getter_export!(xm1_power_record_helper_74edf4, helper_74edf4);
    getter_export!(xm1_power_record_helper_751914, helper_751914);
    getter_export!(xm1_power_record_helper_7519ac, helper_7519ac);
    word_export!(xm1_power_record_helper_7527ac, helper_7527ac);
    word_export!(xm1_power_record_helper_752844, helper_752844);
    word_export!(xm1_power_record_helper_7528dc, helper_7528dc);
    slot_getter_export!(xm1_power_record_helper_752990, helper_752990);
    slot_word_export!(xm1_power_record_helper_7529e8, helper_7529e8);
    slot_word_export!(xm1_power_record_helper_753018, helper_753018);
    slot_word_export!(xm1_power_record_helper_7530c8, helper_7530c8);
    slot_word_export!(xm1_power_record_helper_753228, helper_753228);
    slot_word_export!(xm1_power_record_helper_7532d8, helper_7532d8);
    slot_word_export!(xm1_power_record_helper_753388, helper_753388);
    slot_word_export!(xm1_power_record_helper_753438, helper_753438);
    slot_word_export!(xm1_power_record_helper_7534e8, helper_7534e8);
    slot_word_export!(xm1_power_record_helper_753598, helper_753598);
    slot_word_export!(xm1_power_record_helper_753648, helper_753648);
    slot_word_export!(xm1_power_record_helper_7539b8, helper_7539b8);
    slot_word_export!(xm1_power_record_helper_753a68, helper_753a68);
    slot_word_export!(xm1_power_record_helper_753b18, helper_753b18);
    slot_word_export!(xm1_power_record_helper_753f38, helper_753f38);
    slot_word_export!(xm1_power_record_helper_753fe8, helper_753fe8);
    slot_word_export!(xm1_power_record_helper_7542a8, helper_7542a8);
    slot_word_export!(xm1_power_record_helper_7544b8, helper_7544b8);
    slot_word_export!(xm1_power_record_helper_754568, helper_754568);
    word_export!(xm1_power_record_helper_754dec, helper_754dec);
    word_export!(xm1_power_record_helper_754e8c, helper_754e8c);
    word_export!(xm1_power_record_helper_754fdc, helper_754fdc);
    word_export!(xm1_power_record_helper_75507c, helper_75507c);
    word_export!(xm1_power_record_helper_75511c, helper_75511c);
    word_export!(xm1_power_record_helper_7551bc, helper_7551bc);
    word_export!(xm1_power_record_helper_7553bc, helper_7553bc);
    word_export!(xm1_power_record_helper_75545c, helper_75545c);
    word_export!(xm1_power_record_helper_75559c, helper_75559c);
    source_export!(xm1_power_record_helper_75564c, helper_75564c);
    word_export!(xm1_power_record_helper_75578c, helper_75578c);
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0074e1f0-0x0075578c.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;
    const SOURCE: u32 = 0x0010_0000;

    struct Memory {
        slots: [u8; 0x80],
        resource: [u8; 0x42b0],
        source: [u8; 0x100],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0091_5d38, 0x80), &mut memory.slots).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ebd0, 0x42b0), &mut memory.resource).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 0x100), &mut memory.source).unwrap(),
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

    fn word(memory: &Memory, at: usize) -> u32 { u32::from_le_bytes(memory.resource[at..at + 4].try_into().unwrap()) }
    fn halfword(memory: &Memory, at: usize) -> u16 { u16::from_le_bytes(memory.resource[at..at + 2].try_into().unwrap()) }

    #[test]
    fn resource_setters_store_at_the_stock_offsets() {
        let mut memory = Memory { slots: [0; 0x80], resource: [0; 0x42b0], source: [0; 0x100] };
        admitted(&mut memory);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_7527ac(context, HANDLE, 0x11), Ok(()));
            assert_eq!(helper_752844(context, HANDLE, 0x22), Ok(()));
            assert_eq!(helper_7528dc(context, HANDLE, 0x33), Ok(()));
            assert_eq!(helper_7529e8(context, HANDLE, 0, 0x4401), Ok(()));
            assert_eq!(helper_753018(context, HANDLE, 0, 0x4402), Ok(()));
            assert_eq!(helper_7530c8(context, HANDLE, 0, 0x4403), Ok(()));
            assert_eq!(helper_753228(context, HANDLE, 0, 0x4404), Ok(()));
            assert_eq!(helper_7532d8(context, HANDLE, 0, 0x4405), Ok(()));
            assert_eq!(helper_753388(context, HANDLE, 0, 0x4406), Ok(()));
            assert_eq!(helper_753438(context, HANDLE, 0, 0x4407), Ok(()));
            assert_eq!(helper_7534e8(context, HANDLE, 0, 0x4408), Ok(()));
            assert_eq!(helper_753598(context, HANDLE, 0, 0x4409), Ok(()));
            assert_eq!(helper_753648(context, HANDLE, 0, 0x440a), Ok(()));
            assert_eq!(helper_7539b8(context, HANDLE, 0, 0x440b), Ok(()));
            assert_eq!(helper_753a68(context, HANDLE, 0, 0x440c), Ok(()));
            assert_eq!(helper_753b18(context, HANDLE, 0, 0x440d), Ok(()));
            assert_eq!(helper_753f38(context, HANDLE, 0, 0x440e), Ok(()));
            assert_eq!(helper_753fe8(context, HANDLE, 0, 0x440f), Ok(()));
            assert_eq!(helper_7542a8(context, HANDLE, 0, 0x4410), Ok(()));
            assert_eq!(helper_7544b8(context, HANDLE, 0, 0x4411), Ok(()));
            assert_eq!(helper_754568(context, HANDLE, 0, 0x4412), Ok(()));
            assert_eq!(helper_7529e8(context, HANDLE, 2, 0x4413), Ok(()));
            assert_eq!(helper_754dec(context, HANDLE, 0x5501), Ok(()));
            assert_eq!(helper_754e8c(context, HANDLE, 0x5502), Ok(()));
            assert_eq!(helper_754fdc(context, HANDLE, 0x5503), Ok(()));
            assert_eq!(helper_75507c(context, HANDLE, 0x5504), Ok(()));
            assert_eq!(helper_75511c(context, HANDLE, 0x5505), Ok(()));
            assert_eq!(helper_7551bc(context, HANDLE, 0x5506), Ok(()));
            assert_eq!(helper_7553bc(context, HANDLE, 0x5507), Ok(()));
            assert_eq!(helper_75545c(context, HANDLE, 0x5508), Ok(()));
            assert_eq!(helper_75559c(context, HANDLE, 0x5509), Ok(()));
            assert_eq!(helper_75578c(context, HANDLE, 0x550a), Ok(()));
        });
        assert_eq!(word(&memory, 0x160), 0x11);
        assert_eq!(word(&memory, 0x164), 0x22);
        assert_eq!(word(&memory, 0x168), 0x33);
        assert_eq!(halfword(&memory, 0x16c), 0x4401);
        assert_eq!(halfword(&memory, 0x16c + 0x12), 0x4402);
        assert_eq!(halfword(&memory, 0x16c + 0x14), 0x4403);
        assert_eq!(halfword(&memory, 0x16c + 0x18), 0x4404);
        assert_eq!(memory.resource[0x16c + 0x2c], 0x05);
        assert_eq!(word(&memory, 0x16c + 0x1c), 0x4406);
        assert_eq!(word(&memory, 0x16c + 0x20), 0x4407);
        assert_eq!(word(&memory, 0x16c + 0x24), 0x4408);
        assert_eq!(halfword(&memory, 0x16c + 0x16), 0x4409);
        assert_eq!(word(&memory, 0x16c + 0x28), 0x440a);
        assert_eq!(word(&memory, 0x16c + 0x44), 0x440b);
        assert_eq!(word(&memory, 0x16c + 0x48), 0x440c);
        assert_eq!(word(&memory, 0x16c + 0x4c), 0x440d);
        assert_eq!(halfword(&memory, 0x16c + 0x84), 0x440e);
        assert_eq!(halfword(&memory, 0x16c + 0x86), 0x440f);
        assert_eq!(word(&memory, 0x16c + 0xa4), 0x4410);
        assert_eq!(word(&memory, 0x16c + 0x9c), 0x4411);
        assert_eq!(word(&memory, 0x16c + 0xa0), 0x4412);
        assert_eq!(halfword(&memory, 0x16c + 2 * 0xa8), 0x4413);
        assert_eq!(word(&memory, 0x20b0), 0x5501);
        assert_eq!(word(&memory, 0x20b4), 0x5502);
        assert_eq!(word(&memory, 0x20c8), 0x5503);
        assert_eq!(word(&memory, 0x20cc), 0x5504);
        assert_eq!(word(&memory, 0x20d0), 0x5505);
        assert_eq!(word(&memory, 0x20d4), 0x5506);
        assert_eq!(word(&memory, 0x20f0), 0x5507);
        assert_eq!(word(&memory, 0x20f4), 0x5508);
        assert_eq!(word(&memory, 0x20fc), 0x5509);
        assert_eq!(word(&memory, 0x2114), 0x550a);
    }

    #[test]
    fn resource_setters_ignore_a_handle_without_the_write_permission() {
        let mut memory = Memory { slots: [0; 0x80], resource: [0; 0x42b0], source: [0; 0x100] };
        admitted(&mut memory);
        for field in 0..43 { memory.slots[0x18 + field] = 1; }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_7527ac(context, HANDLE, 0x11), Ok(()));
            assert_eq!(helper_7529e8(context, HANDLE, 0, 0x4401), Ok(()));
            assert_eq!(helper_754dec(context, HANDLE, 0x5501), Ok(()));
            assert_eq!(helper_74e1f0(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(word(&memory, 0x160), 0);
        assert_eq!(halfword(&memory, 0x16c), 0);
        assert_eq!(word(&memory, 0x20b0), 0);
        assert_eq!(memory.resource[0xc8], 0);
    }

    #[test]
    fn copies_follow_the_selected_bank_and_source() {
        let mut memory = Memory { slots: [0; 0x80], resource: [0; 0x42b0], source: [0; 0x100] };
        admitted(&mut memory);
        for index in 0..124 { memory.source[index] = (index as u8).wrapping_mul(3); }
        for index in 0..16 { memory.source[0x40 + index] = 0xa0 + index as u8; }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_74e1f0(context, HANDLE, GuestAddress::new(SOURCE)), Ok(()));
            assert_eq!(helper_75564c(context, HANDLE, GuestAddress::new(SOURCE + 0x40)), Ok(()));
        });
        assert_eq!(&memory.resource[0xc8..0xc8 + 124], &memory.source[0..124]);
        assert_eq!(&memory.resource[0x2100..0x2110], &memory.source[0x40..0x50]);
    }

    #[test]
    fn resource_getters_read_the_selected_bank() {
        let mut memory = Memory { slots: [0; 0x80], resource: [0; 0x42b0], source: [0; 0x100] };
        admitted(&mut memory);
        memory.resource[0x37] = 0x81;
        memory.resource[0xb4..0xb6].copy_from_slice(&0x8001u16.to_le_bytes());
        memory.resource[0xb6..0xb8].copy_from_slice(&0x7ffeu16.to_le_bytes());
        memory.resource[0x16c..0x16e].copy_from_slice(&0xfff0u16.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_74edf4(context, HANDLE), Ok(0x81));
            assert_eq!(helper_751914(context, HANDLE), Ok(0xffff_8001));
            assert_eq!(helper_7519ac(context, HANDLE), Ok(0x7ffe));
            assert_eq!(helper_752990(context, HANDLE, 0), Ok(0xffff_fff0));
        });
    }

    #[test]
    fn resource_slot_getter_returns_zero_without_the_read_permission() {
        let mut memory = Memory { slots: [0; 0x80], resource: [0; 0x42b0], source: [0; 0x100] };
        admitted(&mut memory);
        memory.slots[0x18 + 5] = 2;
        memory.resource[0x16c..0x16e].copy_from_slice(&0xfff0u16.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_752990(context, HANDLE, 0), Ok(0)));
    }

    #[test]
    fn a_second_bank_selects_its_own_resource_block() {
        let mut memory = Memory { slots: [0; 0x80], resource: [0; 0x42b0], source: [0; 0x100] };
        admitted(&mut memory);
        memory.slots[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_7527ac(context, HANDLE, 0x77), Ok(())));
        assert_eq!(word(&memory, 0x160), 0);
        assert_eq!(word(&memory, 0x2158 + 0x160), 0x77);
    }
}
