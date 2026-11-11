//! Forwarders to the owned profile accessors at 0x00752714 and 0x0074a214.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_75a200<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    crate::record_profile_slotfields::helper_752714(context, handle)
}

pub fn helper_76da28<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: u32) -> Result<u32, MemoryError> {
    crate::record_profile_globals::helper_74a214(context, resource)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0075a200 and 0x0076da28,
    //! which are the tail calls to 0x00752714 and 0x0074a214.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const REGISTRY: u32 = 0x0091_5d40;
    const RESOURCES: u32 = 0x0096_ebd0;
    const CONFIGURATION_BASE: u32 = 0x0200_0000;
    const HANDLE: u32 = (63 << 18) | (5 << 4);

    struct Memory { registry: [u8; 0x40], resources: [u8; 0x4400], pointer: [u8; 4], configuration: std::vec::Vec<u8> }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(REGISTRY, 0x40), &mut memory.registry).unwrap(),
            MemoryRegion::new(descriptor(RESOURCES, 0x4400), &mut memory.resources).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION_BASE, 0x3e0f82), &mut memory.configuration[..]).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn readable(memory: &mut Memory) {
        memory.registry[4..8].copy_from_slice(&5u32.to_le_bytes());
        memory.registry[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        for field in [0, 4] { memory.registry[16 + field] = 1; }
    }

    fn memory() -> Memory {
        Memory { registry: [0; 0x40], resources: [0; 0x4400], pointer: [0; 4], configuration: std::vec![0; 0x3e0f82] }
    }

    #[test]
    fn the_slot_field_selects_a_halfword_or_a_word() {
        let mut memory = memory();
        readable(&mut memory);
        memory.resources[0x69] = 1;
        memory.resources[0x2c..0x2e].copy_from_slice(&0xbeefu16.to_le_bytes());
        memory.resources[0x160..0x164].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_75a200(context, HANDLE), Ok(0xbeef)));
        memory.resources[0x69] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_75a200(context, HANDLE), Ok(0xdead_beef)));
    }

    #[test]
    fn the_resource_predicate_uses_the_channel_count() {
        let mut memory = memory();
        memory.pointer.copy_from_slice(&CONFIGURATION_BASE.to_le_bytes());
        memory.configuration[0x3e0f80] = 10;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_76da28(context, 5), Ok(1));
            assert_eq!(helper_76da28(context, 9), Ok(1));
            assert_eq!(helper_76da28(context, 10), Ok(0));
            assert_eq!(helper_76da28(context, 0xffff_ffff), Ok(0));
        });
    }
}
