//! Code selection over the image-5 jump table at 0x00dabb4c (0x00dabb38).

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_dabb38<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, kind: u32) -> Result<u32, MemoryError> {
    Ok(match kind {
        0..=4 => 0x1de7,
        5 => 0x1de5,
        6..=13 => 0x1de6,
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body and table at 0x00dabb38-0x00dabb88.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn select(kind: u32) -> u32 {
        let mut bytes = [0u8; 4];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0010_0000), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut bytes).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        helper_dabb38(&mut RecordContext::new(space, ()), kind).unwrap()
    }

    #[test]
    fn the_table_splits_at_five_and_six() {
        assert_eq!(select(0), 0x1de7);
        assert_eq!(select(4), 0x1de7);
        assert_eq!(select(5), 0x1de5);
        assert_eq!(select(6), 0x1de6);
        assert_eq!(select(13), 0x1de6);
        assert_eq!(select(14), 0);
        assert_eq!(select(0xffff_ffff), 0);
    }
}
