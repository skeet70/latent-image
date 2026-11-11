//! Bit remapping (image 5, 0x00d9e840).

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_d9e840<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, mask: u32) -> Result<u32, MemoryError> {
    let mut value = mask & 1;
    if mask & 4 != 0 { value |= 2; }
    if mask & 0x10 != 0 { value |= 4; }
    if mask & 0x20 != 0 { value |= 0x38; }
    if mask & 0x40 != 0 { value |= 0x100; }
    if mask & 2 != 0 { value |= 0x40; }
    if mask & 8 != 0 { value |= 0x80; }
    Ok(value)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00d9e840.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn map(mask: u32) -> u32 {
        let mut bytes = [0u8; 4];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0010_0000), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut bytes).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        helper_d9e840(&mut RecordContext::new(space, ()), mask).unwrap()
    }

    #[test]
    fn each_source_bit_maps_to_its_destination_bits() {
        assert_eq!(map(0), 0);
        assert_eq!(map(1), 1);
        assert_eq!(map(2), 0x40);
        assert_eq!(map(4), 2);
        assert_eq!(map(8), 0x80);
        assert_eq!(map(0x10), 4);
        assert_eq!(map(0x20), 0x38);
        assert_eq!(map(0x40), 0x100);
    }

    #[test]
    fn the_mappings_combine() {
        assert_eq!(map(0x7f), 1 | 0x40 | 2 | 0x80 | 4 | 0x38 | 0x100);
        assert_eq!(map(0x80), 0);
    }
}
