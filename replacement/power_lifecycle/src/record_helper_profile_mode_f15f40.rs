//! Image-5 helper.
//!
//! The cluster's other two functions, 0x00f1_5f40 and 0x00f1_62e4, are not
//! written: 0x00f1_5f40 calls 0x00f1_582c and 0x00f1_5d5c, which have no body in
//! this tree.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_native_exports::stock_export;

pub const CONTEXT: u32 = 0x016a_049c;

/// Inverts the profile header byte at 0x30 for the selected context.
pub fn helper_f1661c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let handle = context.read_u32(GuestAddress::new(CONTEXT))?;
    let mode = crate::record_profile_accessors::helper_74e5c0(context, handle)?;
    Ok(if mode <= 1 { 1 - mode } else { 0 })
}

stock_export!(
    xm1_power_record_helper_f1661c,
    helper_f1661c_abi,
    (),
    () -> i32,
    (),
    (),
    crate::record_helper_profile_mode_f15f40::helper_f1661c,
    |value| value as i32
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f1_661c.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const REGISTRY: u32 = 0x0091_5d40;
    const RESOURCE: u32 = 0x0096_ebd0;

    struct Memory {
        context: [u8; 4],
        registry: [u8; 4],
        resource: [u8; 0x40],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONTEXT, 4), &mut memory.context).unwrap(),
            MemoryRegion::new(descriptor(REGISTRY, 4), &mut memory.registry).unwrap(),
            MemoryRegion::new(descriptor(RESOURCE, 0x40), &mut memory.resource).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    #[test]
    fn inverts_modes_zero_and_one_and_rejects_the_rest() {
        let mut memory = Memory { context: u32::MAX.to_le_bytes(), registry: [0; 4], resource: [0; 0x40] };
        for (mode, expected) in [(0u8, 1u32), (1, 0), (2, 0), (0x80, 0)] {
            memory.resource[0x30] = mode;
            with_memory(&mut memory, |context| assert_eq!(helper_f1661c(context), Ok(expected)));
        }
    }
}
