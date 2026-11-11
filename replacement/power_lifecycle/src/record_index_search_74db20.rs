//! Search for a value in the 30-word table at 0x4081_8dc0 (image 4, 0x0074db20).

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const TABLE: u32 = 0x4081_8dc0;
const EXCLUDED: u32 = 0x9c4;
const ENTRIES: u32 = 30;

pub fn helper_74db20<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    if value == EXCLUDED { return Ok(0); }
    let mut index = 1u32;
    while index <= ENTRIES {
        if context.read_u32(GuestAddress::new(TABLE + index * 4))? == value { return Ok(index); }
        index += 1;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 body at 0x0074db20.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory { table: [u8; 0x80] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(TABLE), ByteCount(0x80), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut memory.table).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn finds_the_first_matching_slot() {
        let mut memory = Memory { table: [0; 0x80] };
        memory.table[1 * 4..1 * 4 + 4].copy_from_slice(&0x1234u32.to_le_bytes());
        memory.table[3 * 4..3 * 4 + 4].copy_from_slice(&0x5678u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_74db20(context, 0x1234), Ok(1));
            assert_eq!(helper_74db20(context, 0x5678), Ok(3));
            assert_eq!(helper_74db20(context, 0x9999), Ok(0));
        });
    }

    #[test]
    fn the_excluded_value_and_the_last_slot() {
        let mut memory = Memory { table: [0; 0x80] };
        memory.table[30 * 4..30 * 4 + 4].copy_from_slice(&0x9c4u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_74db20(context, 0x9c4), Ok(0));
            assert_eq!(helper_74db20(context, 0x9c4 + 1), Ok(0));
        });
        memory.table[30 * 4..30 * 4 + 4].copy_from_slice(&0x1111u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_74db20(context, 0x1111), Ok(30)));
    }
}
