//! Entry 0x00fa7b84 of the image-5 cluster at 0x00fa7b84.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_fa7b84<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, alternate: u32, offset: i32) -> Result<u32, MemoryError> {
    Ok(if offset > 99 { u32::from(alternate != 1) } else if alternate == 1 { 2 } else { 0 })
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_fa7b84, helper_fa7b84_abi, (), (alternate: u32, offset: i32) -> u32, (alternate: u32, offset: i32), (alternate, offset), crate::record_numeric_fa7b84::helper_fa7b84, |value| value);
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00fa7b84.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn with_memory<R>(block: &mut [u8; 8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0091_904c), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn an_offset_above_ninety_nine_reports_whether_the_alternate_is_one() {
        let mut block = [0u8; 8];
        with_memory(&mut block, |context| {
            assert_eq!(helper_fa7b84(context, 0, 100), Ok(1));
            assert_eq!(helper_fa7b84(context, 2, 100), Ok(1));
            assert_eq!(helper_fa7b84(context, 1, 100), Ok(0));
            assert_eq!(helper_fa7b84(context, 1, i32::MAX), Ok(0));
        });
    }

    #[test]
    fn an_offset_up_to_ninety_nine_maps_a_one_alternate_to_two() {
        let mut block = [0u8; 8];
        with_memory(&mut block, |context| {
            assert_eq!(helper_fa7b84(context, 1, 99), Ok(2));
            assert_eq!(helper_fa7b84(context, 1, 0), Ok(2));
            assert_eq!(helper_fa7b84(context, 1, -1), Ok(2));
            assert_eq!(helper_fa7b84(context, 0, 99), Ok(0));
            assert_eq!(helper_fa7b84(context, 2, -100), Ok(0));
        });
    }
}
