//! Profile-resource record accessors, image-4 bodies 0x0074_e154-0x0075_5b50.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_native_exports::stock_export;
use crate::record_profile_accessors::{can_read, copy_words, read_bank, resource_address, write_bank, ProfileField, ProfileHandle};

/// The image-4 body at 0x0074_e154.
pub fn helper_74e154<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(0))? {
        copy_words::<10>(context, resource_address(bank).checked_add(0x2118)?, source)?;
    }
    Ok(())
}

/// The image-4 body at 0x0075_4f3c.
pub fn helper_754f3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(13))? {
        copy_words::<4>(context, resource_address(bank).checked_add(0x20b8)?, source)?;
    }
    Ok(())
}

/// The image-4 body at 0x0075_520c.
pub fn helper_75520c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(13))?;
    copy_words::<4>(context, output, resource_address(bank).checked_add(0x20d8)?)?;
    Ok(output)
}

/// The image-4 body at 0x0075_5b0c.
pub fn helper_755b0c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    crate::record_profile_slotfields::helper_755b0c(context, handle)
}

fn slot_word<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, handle: u32, field: u32, index: u32, member: u32,
) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(field))?;
    let address = resource_address(bank).checked_add(0x16cu32.wrapping_add(index.wrapping_mul(0xa8)).wrapping_add(member))?;
    context.read_u32(address)
}

/// The image-4 body at 0x0075_38b0.
pub fn helper_7538b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
    slot_word(context, handle, 6, index, 0x40)
}

/// The image-4 body at 0x0075_3c20.
pub fn helper_753c20<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
    slot_word(context, handle, 7, index, 0x54)
}

/// The image-4 body at 0x0075_cc64.
pub fn helper_75cc64<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    context.read_u32(record.checked_add(0x40)?)
}

/// The image-4 body at 0x0075_cd54.
pub fn helper_75cd54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    context.read_u32(record.checked_add(0x54)?)
}

/// The image-4 body at 0x0075_d2fc.
pub fn helper_75d2fc<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<GuestAddress, MemoryError> {
    record.checked_add(0xc3c)
}

/// The image-4 body at 0x0075_ae08.
pub fn helper_75ae08<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    copy_words::<10>(context, destination, source)
}

stock_export!(
    xm1_power_record_helper_74e154,
    helper_74e154_abi,
    (),
    (handle: u32, source: u32) -> (),
    (handle: u32, source: GuestAddress),
    (handle, GuestAddress::new(source)),
    crate::record_profile_resource_words_74e154::helper_74e154,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_754f3c,
    helper_754f3c_abi,
    (),
    (handle: u32, source: u32) -> (),
    (handle: u32, source: GuestAddress),
    (handle, GuestAddress::new(source)),
    crate::record_profile_resource_words_74e154::helper_754f3c,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_75520c,
    helper_75520c_abi,
    (),
    (output: u32, handle: u32) -> u32,
    (output: GuestAddress, handle: u32),
    (GuestAddress::new(output), handle),
    crate::record_profile_resource_words_74e154::helper_75520c,
    |value| value.get()
);
stock_export!(
    xm1_power_record_helper_7538b0,
    helper_7538b0_abi,
    (),
    (handle: u32, index: u32) -> u32,
    (handle: u32, index: u32),
    (handle, index),
    crate::record_profile_resource_words_74e154::helper_7538b0,
    |value| value
);
stock_export!(
    xm1_power_record_helper_753c20,
    helper_753c20_abi,
    (),
    (handle: u32, index: u32) -> u32,
    (handle: u32, index: u32),
    (handle, index),
    crate::record_profile_resource_words_74e154::helper_753c20,
    |value| value
);
stock_export!(
    xm1_power_record_helper_755b0c,
    helper_755b0c_abi,
    (),
    (handle: u32) -> u32,
    (handle: u32),
    (handle),
    crate::record_profile_resource_words_74e154::helper_755b0c,
    |value| value
);

#[cfg(test)]
mod tests {
    //! Expected offsets are the image-4 loads and stores at 0x0074_e154-0x0075_5b50.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;
    const SLOT_BASE: u32 = 0x0091_5d38;
    const RESOURCE: u32 = 0x0096_ebd0;
    const SECOND_RESOURCE: u32 = 0x0097_0d28;

    struct Memory {
        slots: [u8; 0x80],
        first: [u8; 0x2158],
        second: [u8; 0x2158],
        source: [u8; 0x40],
        output: [u8; 0x40],
    }

    impl Memory {
        fn new() -> Self {
            let mut memory = Memory { slots: [0; 0x80], first: [0; 0x2158], second: [0; 0x2158], source: [0; 0x40], output: [0; 0x40] };
            for field in 0..43 { memory.slots[0x18 + field] = 3; }
            memory
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SLOT_BASE, 0x80), &mut self.slots).unwrap(),
                MemoryRegion::new(descriptor(RESOURCE, 0x2158), &mut self.first).unwrap(),
                MemoryRegion::new(descriptor(SECOND_RESOURCE, 0x2158), &mut self.second).unwrap(),
                MemoryRegion::new(descriptor(0x0010_0000, 0x40), &mut self.source).unwrap(),
                MemoryRegion::new(descriptor(0x0010_1000, 0x40), &mut self.output).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }
    }

    #[test]
    fn copies_follow_the_write_permission_of_their_field() {
        let mut memory = Memory::new();
        for index in 0..40 { memory.source[index] = index as u8 + 1; }
        memory.with_context(|context| {
            assert_eq!(helper_74e154(context, HANDLE, GuestAddress::new(0x0010_0000)), Ok(()));
            assert_eq!(helper_754f3c(context, HANDLE, GuestAddress::new(0x0010_0000)), Ok(()));
        });
        assert_eq!(&memory.first[0x2118..0x2140], &memory.source[..40]);
        assert_eq!(&memory.first[0x20b8..0x20c8], &memory.source[..16]);
        memory.slots[0x18] = 1;
        memory.with_context(|context| assert_eq!(helper_74e154(context, HANDLE, GuestAddress::new(0x0010_0000)), Ok(())));
        assert_eq!(&memory.first[0x2118..0x2140], &memory.source[..40]);
    }

    #[test]
    fn the_output_copy_uses_the_readable_bank() {
        let mut memory = Memory::new();
        for index in 0..4u32 { memory.first[0x20d8 + index as usize * 4..0x20dc + index as usize * 4].copy_from_slice(&(0x1000 + index).to_le_bytes()); }
        for index in 0..4u32 { memory.second[0x20d8 + index as usize * 4..0x20dc + index as usize * 4].copy_from_slice(&(0x2000 + index).to_le_bytes()); }
        memory.with_context(|context| assert_eq!(helper_75520c(context, GuestAddress::new(0x0010_1000), HANDLE), Ok(GuestAddress::new(0x0010_1000))));
        assert_eq!(&memory.output[..16], &memory.first[0x20d8..0x20e8]);
        memory.slots[0x18 + 13] = 2;
        memory.slots[0x08..0x0c].copy_from_slice(&1u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_75520c(context, GuestAddress::new(0x0010_1000), HANDLE), Ok(GuestAddress::new(0x0010_1000))));
        assert_eq!(&memory.output[..16], &memory.second[0x20d8..0x20e8]);
    }

    #[test]
    fn indexed_slot_words_are_read_at_their_members() {
        let mut memory = Memory::new();
        for index in 0..3u32 {
            let slot = 0x16c + index as usize * 0xa8;
            memory.first[slot + 0x40..slot + 0x44].copy_from_slice(&(0x3000 + index).to_le_bytes());
            memory.first[slot + 0x54..slot + 0x58].copy_from_slice(&(0x4000 + index).to_le_bytes());
        }
        memory.with_context(|context| {
            assert_eq!(helper_7538b0(context, HANDLE, 0), Ok(0x3000));
            assert_eq!(helper_7538b0(context, HANDLE, 2), Ok(0x3002));
            assert_eq!(helper_753c20(context, HANDLE, 1), Ok(0x4001));
        });
    }

    #[test]
    fn the_configuration_count_is_the_owned_slot_field() {
        let mut memory = Memory::new();
        memory.first[0xfc8..0xfcc].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_755b0c(context, HANDLE), Ok(0x1234_5678)));
    }

    #[test]
    fn record_word_helpers_read_and_copy_the_record() {
        let mut memory = Memory::new();
        for index in 0..10u32 { memory.source[index as usize * 4..index as usize * 4 + 4].copy_from_slice(&(0x500 + index).to_le_bytes()); }
        memory.first[0x40..0x44].copy_from_slice(&0x1111u32.to_le_bytes());
        memory.first[0x54..0x58].copy_from_slice(&0x2222u32.to_le_bytes());
        memory.with_context(|context| {
            assert_eq!(helper_75cc64(context, GuestAddress::new(RESOURCE)), Ok(0x1111));
            assert_eq!(helper_75cd54(context, GuestAddress::new(RESOURCE)), Ok(0x2222));
            assert_eq!(helper_75d2fc(context, GuestAddress::new(RESOURCE)), Ok(GuestAddress::new(RESOURCE + 0xc3c)));
            assert_eq!(helper_75ae08(context, GuestAddress::new(0x0010_1000), GuestAddress::new(0x0010_0000)), Ok(()));
        });
        assert_eq!(&memory.output[..40], &memory.source[..40]);
    }
}
