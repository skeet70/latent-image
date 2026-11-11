//! Profile pixel counts into two caller halfwords (image 5, 0x00edbed8).

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_image_geometry_query::profiles::{helper_ef5ab8, helper_ef5cc4};

pub fn helper_edbed8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, first: GuestAddress, second: GuestAddress) -> Result<(), MemoryError> {
    let value = helper_ef5ab8(context, profile, 1)?;
    context.write_u16(first, value as u16)?;
    let value = helper_ef5cc4(context, profile, 1)?;
    context.write_u16(second, value as u16)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00edbed8 and the profile
    //! tables it reaches through 0x00ef5ab8 and 0x00ef5cc4.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const ROWS: u32 = 0x00ef_6de4;
    const FIELDS: u32 = 0x00ef_6aa4;
    const FIRST: u32 = 0x0010_0000;
    const SECOND: u32 = 0x0010_1000;

    struct Memory { rows: [u8; 8], fields: [u8; 0x60], first: [u8; 4], second: [u8; 4] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(ROWS, 8), &mut memory.rows).unwrap(),
            MemoryRegion::new(descriptor(FIELDS, 0x60), &mut memory.fields).unwrap(),
            MemoryRegion::new(descriptor(FIRST, 4), &mut memory.first).unwrap(),
            MemoryRegion::new(descriptor(SECOND, 4), &mut memory.second).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn both_halfwords_come_from_the_profile_fields() {
        let mut memory = Memory { rows: [0; 8], fields: [0; 0x60], first: [0xff; 4], second: [0xff; 4] };
        memory.fields[0x24] = 4;
        memory.fields[0x25] = 3;
        memory.fields[0x28] = 1;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_edbed8(context, 0, GuestAddress::new(FIRST), GuestAddress::new(SECOND)), Ok(()));
        });
        assert_eq!(&memory.first[..2], &8u16.to_le_bytes());
        assert_eq!(&memory.first[2..], &[0xff; 2]);
        assert_eq!(&memory.second[..2], &3u16.to_le_bytes());
        assert_eq!(&memory.second[2..], &[0xff; 2]);
    }
}
