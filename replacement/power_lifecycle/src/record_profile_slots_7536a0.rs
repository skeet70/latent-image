//! Slot and list entry access inside one profile resource, image-4 entries 0x007536a0-0x00756068.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{read_bank, resource_address, ProfileBank, ProfileField, ProfileHandle};

const SLOT_ENTRY: u32 = 0xa8;
const SLOT_BASE: u32 = 0x16c;
const LIST_BASE: u32 = 0x187c;

fn entry<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, field: u32, index: u32) -> Result<GuestAddress, MemoryError> {
    let bank: ProfileBank = read_bank(context, ProfileHandle(handle), ProfileField(field))?;
    Ok(resource_address(bank).checked_add(SLOT_BASE)?.checked_add(index.wrapping_mul(SLOT_ENTRY))?)
}

fn list<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank: ProfileBank = read_bank(context, ProfileHandle(handle), ProfileField(30))?;
    let record = resource_address(bank).checked_add(LIST_BASE)?;
    crate::record_profile_fields_75cbd4::helper_75d394(context, record)
}

pub fn helper_7536a0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
    let entry = entry(context, handle, 6, index)?;
    crate::record_profile_fields_75cbd4::helper_75cbd4(context, entry)
}

pub fn helper_753800<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<u32, MemoryError> {
    let entry = entry(context, handle, 6, index)?;
    crate::record_profile_fields_75cbd4::helper_75cc34(context, entry)
}

pub fn helper_755f5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<u32, MemoryError> {
    let list = list(context, handle)?;
    crate::record_profile_aux_configuration::helper_75d57c(context, list, value)
}

pub fn helper_75600c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32, source: GuestAddress) -> Result<u32, MemoryError> {
    let list = list(context, handle)?;
    let index = crate::record_profile_aux_configuration::AuxiliaryIndex::new(index).ok_or(MemoryError::Unmapped(list))?;
    crate::record_profile_aux_configuration::helper_75d5b4(context, list, index, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SLOT: u32 = 0x0091_5d44;
    const SELECTED: u32 = 0x0091_5d40;
    const RESOURCE: u32 = 0x0096_ebd0;
    const SOURCE: u32 = 0x0080_0000;
    const HANDLE: u32 = u32::MAX;

    struct Memory {
        slots: [u8; 16 * 0x38],
        selected: [u8; 4],
        resource: [u8; 0x2158],
        source: [u8; 0x68],
    }

    impl Memory {
        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SLOT, 16 * 0x38), &mut self.slots).unwrap(),
                MemoryRegion::new(descriptor(SELECTED, 4), &mut self.selected).unwrap(),
                MemoryRegion::new(descriptor(RESOURCE, 0x2158), &mut self.resource).unwrap(),
                MemoryRegion::new(descriptor(SOURCE, 0x68), &mut self.source).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }

        fn word(&self, at: u32) -> u32 { u32::from_le_bytes(self.resource[at as usize..at as usize + 4].try_into().unwrap()) }
    }

    #[test]
    fn the_slot_readers_take_the_two_entry_words() {
        let mut memory = Memory { slots: [0; 16 * 0x38], selected: [0; 4], resource: [0; 0x2158], source: [0; 0x68] };
        memory.resource[0x16c + 0xa8 + 0x34..0x16c + 0xa8 + 0x38].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        memory.resource[0x16c + 0xa8 + 0x3c..0x16c + 0xa8 + 0x40].copy_from_slice(&0x5566_7788u32.to_le_bytes());
        memory.with_context(|context| {
            assert_eq!(helper_7536a0(context, HANDLE, 1), Ok(0x1122_3344));
            assert_eq!(helper_753800(context, HANDLE, 1), Ok(0x5566_7788));
            assert_eq!(helper_7536a0(context, HANDLE, 0), Ok(0));
        });
    }

    #[test]
    fn the_list_writer_stores_the_word_after_the_entry_advance() {
        let mut memory = Memory { slots: [0; 16 * 0x38], selected: [0; 4], resource: [0; 0x2158], source: [0; 0x68] };
        memory.with_context(|context| assert_eq!(helper_755f5c(context, HANDLE, 0x0102_0304), Ok(RESOURCE + 0x1c90)));
        assert_eq!(memory.word(0x1c90), 0x0102_0304);
    }

    #[test]
    fn the_list_copy_takes_the_indexed_entry() {
        let mut memory = Memory { slots: [0; 16 * 0x38], selected: [0; 4], resource: [0; 0x2158], source: [0; 0x68] };
        for index in 0..0x68u32 { memory.source[index as usize] = index as u8; }
        let destination = memory.with_context(|context| helper_75600c(context, HANDLE, 2, GuestAddress::new(SOURCE)));
        assert_eq!(destination, Ok(RESOURCE + 0x1c90 + 4 + 2 * 0x68));
        assert_eq!(&memory.resource[0x1c90 + 4 + 2 * 0x68..0x1c90 + 4 + 3 * 0x68], &memory.source[..]);
    }
}
