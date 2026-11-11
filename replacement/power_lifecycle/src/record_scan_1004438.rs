//! Entry 0x01004438 of the image-5 cluster at 0x01004438.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_1004438<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_1004438, helper_1004438_abi, (), () -> (), (), (), crate::record_scan_1004438::helper_1004438, |_| ());
}

#[cfg(test)]
mod tests {
    //! The body at 0x01004438 is a prologue and epilogue around no operation.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn the_entry_changes_nothing() {
        let mut block = [0x5au8; 8];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0091_904c), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_1004438(&mut context), Ok(()));
        assert_eq!(block, [0x5au8; 8]);
    }
}
