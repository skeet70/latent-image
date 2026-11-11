//! Entries 0x00740a10 and 0x00740a34 of the image-4 cluster at 0x007405dc.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

/// The block the image-4 entries at 0x007405dc-0x00740e74 share.
pub const POWER_CONTROL_BLOCK: u32 = 0x0091_4db8;

fn state_word() -> GuestAddress { GuestAddress::new(POWER_CONTROL_BLOCK) }

fn store_state<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u32(state_word(), value)
}

pub fn helper_740a10<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { store_state(context, 0) }

pub fn helper_740a34<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { store_state(context, 1) }

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_740a10, helper_740a10_abi, (), () -> (), (), (), crate::record_power_control_7405dc::helper_740a10, |_| ());
    stock_export!(xm1_power_record_helper_740a34, helper_740a34_abi, (), () -> (), (), (), crate::record_power_control_7405dc::helper_740a34, |_| ());
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x00740a10 and 0x00740a34.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const BLOCK: u32 = 0x0091_4db8;

    fn with_memory<R>(block: &mut [u8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor(BLOCK, block.len() as u32), block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn the_entries_store_their_own_state_word() {
        let mut block = [0u8; 0x48];
        block[0..4].copy_from_slice(&7u32.to_le_bytes());
        with_memory(&mut block, |context| {
            assert_eq!(helper_740a10(context), Ok(()));
        });
        assert_eq!(&block[0..4], &0u32.to_le_bytes());
        with_memory(&mut block, |context| {
            assert_eq!(helper_740a34(context), Ok(()));
        });
        assert_eq!(&block[0..4], &1u32.to_le_bytes());
    }

    #[test]
    fn the_entries_touch_no_other_word() {
        let mut block = [0xaa; 0x48];
        with_memory(&mut block, |context| assert_eq!(helper_740a34(context), Ok(())));
        assert_eq!(&block[0..4], &1u32.to_le_bytes());
        assert_eq!(&block[4..], &[0xaa; 0x44][..]);
    }
}
