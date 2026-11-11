//! Entry 0x00ef6790 of the image-5 cluster at 0x00ef6790.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_ef6790<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(1) }

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_ef6790, helper_ef6790_abi, (), () -> u32, (), (), crate::record_frontend_ef6790::helper_ef6790, |value| value);
}

#[cfg(test)]
mod tests {
    //! The expected value comes from the image-5 body at 0x00ef6790.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn the_entry_returns_one() {
        let mut block = [0u8; 8];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0091_904c), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_ef6790(&mut context), Ok(1));
        assert_eq!(block, [0u8; 8]);
    }
}
