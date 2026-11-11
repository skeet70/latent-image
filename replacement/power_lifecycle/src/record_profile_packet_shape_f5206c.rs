//! Two-word packet shape at 0x00f5_206c.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_native_exports::stock_export;

/// The image-5 body at 0x00f5_206c.
pub fn helper_f5206c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let kind = crate::record_profile_accessors::helper_74fa2c(context, handle)?;
    let (first, second) = match kind {
        2 | 5 => context.with_guest_frame(ByteCount(8), StackAlignment::Word, |context, frame| {
            crate::record_profile_accessors::helper_74fab4(context, frame.base(), handle)?;
            Ok((context.read_u32(frame.base())?, context.read_u32(frame.base().checked_add(4)?)?))
        })?,
        0 => {
            let halfword = crate::record_profile_accessors::helper_750b48(context, handle)? as u16;
            (u32::from(halfword as u8), u32::from(halfword >> 8))
        }
        _ => (3, 3),
    };
    context.write_u32(destination, first)?;
    context.write_u32(destination.checked_add(4)?, second)?;
    Ok(destination)
}

stock_export!(
    xm1_power_record_helper_f5206c,
    helper_f5206c_abi,
    (),
    (output: u32, request: u32) -> (),
    (destination: GuestAddress, handle: u32),
    (GuestAddress::new(output), request),
    crate::record_profile_packet_shape_f5206c::helper_f5206c,
    |_| ()
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;

    struct Memory {
        slot: [u8; 0x38],
        resource: [u8; 0x2158],
        output: [u8; 8],
        stack: [u8; 0x20],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0091_5d44, 0x38), &mut memory.slot).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ebd0, 0x2158), &mut memory.resource).unwrap(),
            MemoryRegion::new(descriptor(0x0010_0000, 8), &mut memory.output).unwrap(),
            MemoryRegion::new(descriptor(0x0020_0000, 0x20), &mut memory.stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(0x0020_0000), ByteCount(0x20)).unwrap();
        body(&mut RecordContext::with_stack(space, (), stack))
    }

    fn memory() -> Memory {
        let mut memory = Memory { slot: [0; 0x38], resource: [0; 0x2158], output: [0; 8], stack: [0; 0x20] };
        for field in 0..43 { memory.slot[12 + field] = 3; }
        memory
    }

    fn set_kind(memory: &mut Memory, kind: u8) {
        memory.resource[0x47] = kind;
    }

    fn pair(memory: &Memory) -> (u32, u32) {
        (u32::from_le_bytes(memory.output[..4].try_into().unwrap()), u32::from_le_bytes(memory.output[4..].try_into().unwrap()))
    }

    #[test]
    fn kinds_two_and_five_take_the_resource_pair() {
        for kind in [2u8, 5] {
            let mut memory = memory();
            set_kind(&mut memory, kind);
            memory.resource[4..8].copy_from_slice(&0x0a0b_0c0du32.to_le_bytes());
            memory.resource[8..12].copy_from_slice(&0x0e0f_1011u32.to_le_bytes());
            with_memory(&mut memory, |context| assert_eq!(helper_f5206c(context, GuestAddress::new(0x0010_0000), HANDLE), Ok(GuestAddress::new(0x0010_0000))));
            assert_eq!(pair(&memory), (0x0a0b_0c0d, 0x0e0f_1011));
        }
    }

    #[test]
    fn kind_zero_splits_the_header_halfword() {
        let mut memory = memory();
        set_kind(&mut memory, 0);
        memory.resource[12..14].copy_from_slice(&0x89abu16.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f5206c(context, GuestAddress::new(0x0010_0000), HANDLE), Ok(GuestAddress::new(0x0010_0000))));
        assert_eq!(pair(&memory), (0xab, 0x89));
    }

    #[test]
    fn every_other_kind_writes_three() {
        for kind in [1u8, 3, 4, 6, 0xff] {
            let mut memory = memory();
            set_kind(&mut memory, kind);
            memory.output = [0xff; 8];
            with_memory(&mut memory, |context| assert_eq!(helper_f5206c(context, GuestAddress::new(0x0010_0000), HANDLE), Ok(GuestAddress::new(0x0010_0000))));
            assert_eq!(pair(&memory), (3, 3));
        }
    }
}
