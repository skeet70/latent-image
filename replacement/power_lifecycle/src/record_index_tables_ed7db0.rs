//! Word lookups in the 16-byte records at 0x00ed_7e08 (0x00ed7db0 and 0x00ed7ddc).

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const TABLE: u32 = 0x00ed_7e08;
const ENTRY_BYTES: u32 = 16;
const LAST: u32 = 35;

pub fn helper_ed7db0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    if index > LAST { return Ok(14); }
    context.read_u32(GuestAddress::new(TABLE + index * ENTRY_BYTES + 4))
}

pub fn helper_ed7ddc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    if index > LAST { return Ok(1); }
    context.read_u32(GuestAddress::new(TABLE + index * ENTRY_BYTES + 8))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ed7db0 and 0x00ed7ddc.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const RECORDS: u32 = TABLE;
    const RECORD_BYTES: u32 = 36 * 16;

    struct Memory { records: [u8; RECORD_BYTES as usize] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(RECORDS), ByteCount(RECORD_BYTES), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut memory.records).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn the_lookups_read_their_record_words() {
        let mut memory = Memory { records: [0; RECORD_BYTES as usize] };
        memory.records[0 * 16 + 4..0 * 16 + 8].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        memory.records[0 * 16 + 8..0 * 16 + 12].copy_from_slice(&0x0506_0708u32.to_le_bytes());
        memory.records[35 * 16 + 4..35 * 16 + 8].copy_from_slice(&0x1112_1314u32.to_le_bytes());
        memory.records[35 * 16 + 8..35 * 16 + 12].copy_from_slice(&0x1516_1718u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ed7db0(context, 0), Ok(0x0102_0304));
            assert_eq!(helper_ed7ddc(context, 0), Ok(0x0506_0708));
            assert_eq!(helper_ed7db0(context, 35), Ok(0x1112_1314));
            assert_eq!(helper_ed7ddc(context, 35), Ok(0x1516_1718));
        });
    }

    #[test]
    fn indexes_past_the_last_record_return_the_defaults() {
        let mut memory = Memory { records: [0; RECORD_BYTES as usize] };
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ed7db0(context, 36), Ok(14));
            assert_eq!(helper_ed7ddc(context, 36), Ok(1));
            assert_eq!(helper_ed7db0(context, 0xffff_ffff), Ok(14));
            assert_eq!(helper_ed7ddc(context, 0xffff_ffff), Ok(1));
        });
    }
}
