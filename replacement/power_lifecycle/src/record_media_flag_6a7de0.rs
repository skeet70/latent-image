//! Stock-shaped body for the flag byte the image-4 pair at 0x006a7de0 and
//! 0x006a7e68 toggles.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const FLAG: u32 = 0x409c_2826;

/// 0x006a7e68
pub fn helper_6a7e68<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(u32::from(context.read_u8(GuestAddress::new(FLAG))? & 1))
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_6a7e68,
    helper_6a7e68_abi,
    (),
    () -> u32,
    (),
    (),
    crate::record_media_flag_6a7de0::helper_6a7e68,
    |value| value
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 body at 0x006a7e68.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn with_flag<R>(value: u8, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut flag = [value];
        let descriptor = RegionDescriptor::new(GuestAddress::new(FLAG), ByteCount(1), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut flag).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn the_low_bit_of_the_flag_byte_is_the_result() {
        with_flag(0xfe, |context| assert_eq!(helper_6a7e68(context), Ok(0)));
        with_flag(0xff, |context| assert_eq!(helper_6a7e68(context), Ok(1)));
        with_flag(0x02, |context| assert_eq!(helper_6a7e68(context), Ok(0)));
        with_flag(0x03, |context| assert_eq!(helper_6a7e68(context), Ok(1)));
    }
}
