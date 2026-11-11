//! Entry 0x0103af30 of the image-5 cluster at 0x0103af30.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const HALFWORD_170380A: GuestAddress = GuestAddress::new(0x0170_380a);

pub fn helper_103af30<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u16(HALFWORD_170380A, value as u16)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_103af30, helper_103af30_abi, (), (value: u32) -> (), (value: u32), (value), crate::record_frontend_103af30::helper_103af30, |_| ());
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x0103af30.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn the_entry_stores_the_low_halfword_at_its_own_address() {
        let mut words = [0u8; 8];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0170_3808), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut words).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_103af30(&mut context, 0x1234_5678), Ok(()));
        assert_eq!(&words[0..2], &[0, 0]);
        assert_eq!(&words[2..4], &0x5678u16.to_le_bytes());
        assert_eq!(&words[4..8], &[0, 0, 0, 0]);
    }
}
