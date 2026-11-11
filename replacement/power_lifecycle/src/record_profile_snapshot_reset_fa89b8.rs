//! Stock-shaped body at 0x00fa89b8, which clears the profile snapshot prefix.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_profile_shared_accessors::{helper_757ff0, helper_758028};

const PREFIX_BYTES: u32 = 40;

/// 0x00fa89b8
pub fn helper_fa89b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(48), StackAlignment::Word, |context, frame| {
        let scratch = frame.base();
        helper_757ff0(context, handle, scratch)?;
        for offset in 0..PREFIX_BYTES {
            context.write_u8(scratch.checked_add(offset)?, 0)?;
        }
        context.write_u16(scratch.checked_add(PREFIX_BYTES)?, 0)?;
        helper_758028(context, handle, scratch)
    })
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_fa89b8,
    helper_fa89b8_abi,
    (),
    (handle: u32) -> (),
    (handle: u32),
    (handle),
    crate::record_profile_snapshot_reset_fa89b8::helper_fa89b8,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00fa89b8.
    use super::*;
    use crate::record_address_space::{
        GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const SHARED: u32 = 0x0091_98b0;
    const SLOTS: u32 = 0x0091_5d44;
    const STACK: u32 = 0x0020_0000;
    const HANDLE: u32 = 0x00fe_0000;

    #[test]
    fn the_first_forty_two_bytes_are_cleared_and_the_rest_survive() {
        let mut shared = [0u8; 48];
        for (index, byte) in shared.iter_mut().enumerate() { *byte = index as u8 + 1; }
        let mut slots = [0u8; 0x38];
        slots[12 + 14] = 2;
        let mut stack = [0u8; 64];
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(SHARED, 48), &mut shared).unwrap(),
            MemoryRegion::new(descriptor(SLOTS, 0x38), &mut slots).unwrap(),
            MemoryRegion::new(descriptor(STACK, 64), &mut stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let guest_stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(64)).unwrap();
        let mut context = RecordContext::with_stack(space, (), guest_stack);
        assert_eq!(helper_fa89b8(&mut context, HANDLE), Ok(()));
        assert_eq!(&shared[..42], &[0u8; 42]);
        assert_eq!(&shared[42..], &[43u8, 44, 45, 46, 47, 48]);
    }

    #[test]
    fn a_handle_without_the_write_permission_leaves_the_snapshot_alone() {
        let mut shared = [0xa5u8; 48];
        let mut slots = [0u8; 0x38];
        slots[12 + 14] = 1;
        let mut stack = [0u8; 64];
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(SHARED, 48), &mut shared).unwrap(),
            MemoryRegion::new(descriptor(SLOTS, 0x38), &mut slots).unwrap(),
            MemoryRegion::new(descriptor(STACK, 64), &mut stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let guest_stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(64)).unwrap();
        let mut context = RecordContext::with_stack(space, (), guest_stack);
        assert_eq!(helper_fa89b8(&mut context, HANDLE), Ok(()));
        assert_eq!(shared, [0xa5; 48]);
    }
}
