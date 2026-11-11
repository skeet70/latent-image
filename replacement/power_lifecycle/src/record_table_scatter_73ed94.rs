//! Table-driven word scatter (image 4, 0x0073ed94).
//!
//! Each 8-byte entry is a destination offset followed by a source offset; the
//! body copies the word at `source + source offset` to `destination +
//! destination offset` for every entry.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const ENTRY_BYTES: u32 = 8;

pub fn helper_73ed94<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    destination: GuestAddress,
    source: GuestAddress,
    table: GuestAddress,
    count: u32,
) -> Result<(), MemoryError> {
    let mut index = 0;
    while index < count {
        let entry = table.checked_add(index.wrapping_mul(ENTRY_BYTES))?;
        let source_offset = context.read_u32(entry.checked_add(4)?)?;
        let value = context.read_u32(source.checked_add(source_offset)?)?;
        let destination_offset = context.read_u32(entry)?;
        context.write_u32(destination.checked_add(destination_offset)?, value)?;
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 body at 0x0073ed94.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const DESTINATION: u32 = 0x0010_0000;
    const SOURCE: u32 = 0x0010_1000;
    const TABLE: u32 = 0x0010_2000;

    struct Memory { destination: [u8; 16], source: [u8; 16], table: [u8; 16] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(DESTINATION, 16), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 16), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(TABLE, 16), &mut memory.table).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn entry(memory: &mut Memory, index: usize, destination: u32, source: u32) {
        memory.table[index * 8..index * 8 + 4].copy_from_slice(&destination.to_le_bytes());
        memory.table[index * 8 + 4..index * 8 + 8].copy_from_slice(&source.to_le_bytes());
    }

    #[test]
    fn copies_each_entry_word_to_its_destination_offset() {
        let mut memory = Memory { destination: [0; 16], source: [0; 16], table: [0; 16] };
        for index in 0..4 { memory.source[index * 4..index * 4 + 4].copy_from_slice(&(0xa0b0_c0d0 + index as u32).to_le_bytes()); }
        entry(&mut memory, 0, 12, 0);
        entry(&mut memory, 1, 0, 8);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_73ed94(context, GuestAddress::new(DESTINATION), GuestAddress::new(SOURCE), GuestAddress::new(TABLE), 2), Ok(()));
        });
        assert_eq!(&memory.destination[12..16], &0xa0b0_c0d0u32.to_le_bytes());
        assert_eq!(&memory.destination[0..4], &0xa0b0_c0d2u32.to_le_bytes());
        assert_eq!(&memory.destination[4..12], &[0; 8]);
    }

    #[test]
    fn a_zero_count_copies_nothing() {
        let mut memory = Memory { destination: [0; 16], source: [0; 16], table: [0; 16] };
        entry(&mut memory, 0, 0, 0);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_73ed94(context, GuestAddress::new(DESTINATION), GuestAddress::new(SOURCE), GuestAddress::new(TABLE), 0), Ok(()));
        });
        assert_eq!(&memory.destination[..], &[0; 16]);
    }
}
