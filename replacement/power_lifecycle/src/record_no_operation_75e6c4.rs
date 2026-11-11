//! Stock-shaped body at 0x0075e6c4.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

/// 0x0075e6c4
pub fn helper_75e6c4<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_75e6c4,
    helper_75e6c4_abi,
    (),
    () -> (),
    (),
    (),
    crate::record_no_operation_75e6c4::helper_75e6c4,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! The image-4 body at 0x0075e6c4 is a bare return.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn the_body_returns_without_writing() {
        let mut block = [0x5au8; 8];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0010_0000), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_75e6c4(&mut context), Ok(()));
        assert_eq!(context.read_u32(GuestAddress::new(0x0010_0000)), Ok(0x5a5a_5a5a));
    }
}
