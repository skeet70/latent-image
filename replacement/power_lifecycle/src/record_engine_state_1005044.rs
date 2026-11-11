//! Stock-shaped body at 0x01005044.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

const ENGINE_STATE: u32 = 0x016f_8f30;

/// 0x01005044
pub fn helper_1005044<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(ENGINE_STATE)
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_1005044,
    helper_1005044_abi,
    (),
    () -> u32,
    (),
    (),
    crate::record_engine_state_1005044::helper_1005044,
    |value| value
);

#[cfg(test)]
mod tests {
    //! Expected value comes from the image-5 body at 0x01005044.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn the_body_returns_the_state_address() {
        let mut block = [0u8; 4];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0010_0000), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_1005044(&mut context), Ok(0x016f_8f30));
    }
}
