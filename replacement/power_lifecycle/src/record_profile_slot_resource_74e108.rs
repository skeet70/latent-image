//! The bank resource a profile handle names: image-4 bodies 0x0074_e108 and
//! 0x0075_3750.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{can_read, can_write, copy_words, handle_bank, offset, read_bank, resource_address, selected_bank, ProfileField, ProfileHandle};
use crate::record_profile_slot_table_756720::helper_75cc04;

/// Word offset of the record 0x0074_e108 stores.
pub const RESOURCE_RECORD: u32 = 352;
/// Byte stride between the indexed records.
pub const INDEX_STRIDE: u32 = 0xa8;
/// Byte offset of the indexed records.
pub const INDEX_BASE: u32 = 364;

/// The image-4 body at 0x0074_e108.
pub fn helper_74e108<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(0))? { return Ok(()); }
    let bank = handle_bank(context, ProfileHandle(handle))?;
    copy_words::<3>(context, offset(resource_address(bank), RESOURCE_RECORD), source)
}

/// The image-4 body at 0x0075_3750.
pub fn helper_753750<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(6))?;
    let base = offset(resource_address(bank), index.wrapping_mul(INDEX_STRIDE).wrapping_add(INDEX_BASE));
    helper_75cc04(context, base).map(|value| value as u32)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(
        xm1_power_record_helper_74e108, helper_74e108_abi, (),
        (handle: u32, source: u32) -> (),
        (handle: u32, source: GuestAddress),
        (handle, GuestAddress::new(source)),
        crate::record_profile_slot_resource_74e108::helper_74e108,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_753750, helper_753750_abi, (),
        (handle: u32, index: u32) -> u32,
        (handle: u32, index: u32),
        (handle, index),
        crate::record_profile_slot_resource_74e108::helper_753750,
        |value| value
    );
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0074_e108 and 0x0075_3750.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;
    const SOURCE: u32 = 0x0010_0000;

    struct Memory {
        slots: [u8; 0x80],
        resources: [u8; 0x42b0],
        source: [u8; 0x10],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0091_5d38, 0x80), &mut memory.slots).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ebd0, 0x42b0), &mut memory.resources).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 0x10), &mut memory.source).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    fn admitted(memory: &mut Memory, bank: u32, field: usize, permission: u8) {
        memory.slots[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        memory.slots[0x14..0x18].copy_from_slice(&bank.to_le_bytes());
        memory.slots[0x18 + field] = permission;
    }

    #[test]
    fn the_record_copy_needs_the_field_zero_write_permission() {
        let mut memory = Memory { slots: [0; 0x80], resources: [0; 0x42b0], source: [0x11; 0x10] };
        admitted(&mut memory, 1, 0, 3);
        with_memory(&mut memory, |context| assert_eq!(helper_74e108(context, HANDLE, GuestAddress::new(SOURCE)), Ok(())));
        let at = 0x2158 + RESOURCE_RECORD as usize;
        assert_eq!(&memory.resources[at..at + 12], &[0x11u8; 12]);
        assert_eq!(&memory.resources[..12], &[0u8; 12]);
    }

    #[test]
    fn the_record_copy_skips_a_handle_without_the_permission() {
        let mut memory = Memory { slots: [0; 0x80], resources: [0; 0x42b0], source: [0x11; 0x10] };
        admitted(&mut memory, 1, 0, 1);
        with_memory(&mut memory, |context| assert_eq!(helper_74e108(context, HANDLE, GuestAddress::new(SOURCE)), Ok(())));
        assert!(memory.resources.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn the_index_read_follows_the_bank_the_field_six_read_permission_selects() {
        let mut memory = Memory { slots: [0; 0x80], resources: [0; 0x42b0], source: [0; 0x10] };
        admitted(&mut memory, 1, 6, 3);
        let at = 0x2158 + 2 * INDEX_STRIDE as usize + INDEX_BASE as usize + 56;
        memory.resources[at..at + 2].copy_from_slice(&0x8003u16.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_753750(context, HANDLE, 2), Ok(0xffff_8003)));
        memory.resources[at..at + 2].copy_from_slice(&0x7fffu16.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_753750(context, HANDLE, 2), Ok(0x7fff)));
        admitted(&mut memory, 0, 6, 1);
        memory.slots[8..12].copy_from_slice(&1u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_753750(context, HANDLE, 2), Ok(0)));
    }

    #[test]
    fn the_index_read_uses_the_selected_bank_without_the_permission() {
        let mut memory = Memory { slots: [0; 0x80], resources: [0; 0x42b0], source: [0; 0x10] };
        memory.slots[8..12].copy_from_slice(&1u32.to_le_bytes());
        let at = 0x2158 + INDEX_BASE as usize + 56;
        memory.resources[at..at + 2].copy_from_slice(&0xfffeu16.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_753750(context, HANDLE, 0), Ok(0xffff_fffe)));
    }
}
