//! The forwarder at 0x00fa_7a6c to the shared-block word at 0x0091_9288.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

/// 0x00fa7a6c
pub fn helper_fa7a6c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    crate::record_profile_shared_accessors::helper_7573d4(context, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    #[test]
    fn the_shared_block_word_is_returned() {
        let mut block = [0u8; 0x1000];
        block[0x23c..0x240].copy_from_slice(&0x8091_a0b0u32.to_le_bytes());
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0091_904c), ByteCount(0x1000), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_fa7a6c(&mut context, 0), Ok(0x8091_a0b0));
    }
}
