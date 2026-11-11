//! Data-cache range publication, image-4 entry 0x00740e74.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_scan_publication_workspace::CachePublication;

const LINE: u32 = 31;
const NON_CACHEABLE: u32 = 0x4000_001f;

pub fn helper_740e74<M: GuestAddressSpace, E: CachePublication>(context: &mut RecordContext<M, E>, address: u32, length: u32) -> Result<(), MemoryError> {
    let aligned = length.wrapping_add(address & LINE).wrapping_add(LINE) & !LINE;
    context.effects.clean_data_cache(GuestAddress::new(address & !NON_CACHEABLE), ByteCount(aligned))
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(
        xm1_power_record_helper_740e74,
        helper_740e74_abi,
        crate::record_native_boundary::NativeCachePublication,
        (address: u32, length: u32) -> (),
        (address: u32, length: u32),
        (address, length),
        crate::record_cache_range_740e74::helper_740e74,
        |_| (),
        bounds CachePublication
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use std::vec::Vec;

    #[derive(Default)]
    struct Cache {
        ranges: Vec<(u32, u32)>,
    }

    impl CachePublication for Cache {
        fn clean_data_cache(&mut self, address: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
            self.ranges.push((address.get(), length.0));
            Ok(())
        }
    }

    fn clean(address: u32, length: u32) -> (u32, u32) {
        let mut region = [0u8; 4];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x1000), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut region).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, Cache::default());
        assert_eq!(helper_740e74(&mut context, address, length), Ok(()));
        context.effects.ranges[0]
    }

    #[test]
    fn the_range_starts_on_a_line_and_covers_the_offset_head() {
        assert_eq!(clean(0x016e_9000, 0x100), (0x016e_9000, 0x100));
        assert_eq!(clean(0x016e_9004, 0x100), (0x016e_9000, 0x120));
        assert_eq!(clean(0x016e_901f, 1), (0x016e_9000, 0x20));
        assert_eq!(clean(0x016e_9000, 0), (0x016e_9000, 0));
    }

    #[test]
    fn the_non_cacheable_bit_does_not_reach_the_range() {
        assert_eq!(clean(0x416e_9000, 0x20), (0x016e_9000, 0x20));
        assert_eq!(clean(0x4000_0010, 0x10), (0x0000_0000, 0x20));
    }
}
