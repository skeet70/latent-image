//! Channel-pair mapping comparisons, image-5 entries 0x00f5fa24 and 0x00f5fa7c.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};

const PAIR_WORDS: u32 = 2;
const FIRST_INDEX: u32 = 2;
const SECOND_INDEX: u32 = 4;

pub fn helper_f5fa24<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, bank: u32, value: u32) -> Result<u32, MemoryError> {
    context.with_guest_frame(ByteCount(PAIR_WORDS * 4), StackAlignment::Word, |context, frame| {
        crate::record_profile_shared_accessors::helper_75a4ac(context, handle, FIRST_INDEX, frame.base())?;
        let source = context.read_u32(frame.base())?;
        let second = context.read_u32(frame.base().checked_add(4)?)?;
        let mapped = crate::record_profile_mapping::helper_74714c(context, source, second, bank)?;
        let left = crate::record_profile_mapping::helper_746864(context, mapped, bank)?;
        let right = crate::record_profile_mapping::helper_746864(context, value, bank)?;
        Ok(u32::from(left != right))
    })
}

pub fn helper_f5fa7c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, bank: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    let mapped = context.with_guest_frame(ByteCount(PAIR_WORDS * 4), StackAlignment::Word, |context, frame| {
        crate::record_profile_shared_accessors::helper_75a4ac(context, handle, FIRST_INDEX, frame.base())?;
        let source = context.read_u32(frame.base())?;
        let second = context.read_u32(frame.base().checked_add(4)?)?;
        let mapped = crate::record_profile_mapping::helper_74714c(context, source, second, bank)?;
        context.write_u32(output, mapped)?;
        Ok::<u32, MemoryError>(mapped)
    })?;
    let left = crate::record_profile_mapping::helper_746864(context, mapped, bank)?;
    context.with_guest_frame(ByteCount(PAIR_WORDS * 4), StackAlignment::Word, |context, frame| {
        crate::record_profile_shared_accessors::helper_75a4ac(context, handle, SECOND_INDEX, frame.base())?;
        let source = context.read_u32(frame.base())?;
        let value = context.read_u32(frame.base().checked_add(4)?)?;
        let right = crate::record_profile_mapping::helper_746864(context, value, source)?;
        Ok(u32::from(left != right))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SHARED: u32 = 0x0091_904c;
    const POINTER: u32 = 0x0096_ca50;
    const STACK: u32 = 0x0010_0000;
    const OUTPUT: u32 = 0x0080_0000;
    const HANDLE: u32 = u32::MAX;

    struct Memory {
        shared: [u8; 0xa00],
        pointer: [u8; 4],
        gate: [u8; 1],
        split: [u8; 1],
        entries: [u8; 2],
        limits: [u8; 4],
        stack: [u8; 0x100],
        output: [u8; 4],
    }

    impl Memory {
        fn new() -> Self {
            let mut memory = Memory { shared: [0; 0xa00], pointer: [0; 4], gate: [1], split: [3], entries: [0, 1], limits: [0; 4], stack: [0; 0x100], output: [0; 4] };
            memory.limits[..2].copy_from_slice(&0x10u16.to_le_bytes());
            memory.limits[2..].copy_from_slice(&0x20u16.to_le_bytes());
            memory
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SHARED, 0xa00), &mut self.shared).unwrap(),
                MemoryRegion::new(descriptor(POINTER, 4), &mut self.pointer).unwrap(),
                MemoryRegion::new(descriptor(0x3e0009, 1), &mut self.gate).unwrap(),
                MemoryRegion::new(descriptor(0x3e0f82, 1), &mut self.split).unwrap(),
                MemoryRegion::new(descriptor(0x3f4600, 2), &mut self.entries).unwrap(),
                MemoryRegion::new(descriptor(0x3e2530, 4), &mut self.limits).unwrap(),
                MemoryRegion::new(descriptor(STACK, 0x100), &mut self.stack).unwrap(),
                MemoryRegion::new(descriptor(OUTPUT, 4), &mut self.output).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x100)).unwrap();
            body(&mut RecordContext::with_stack(space, (), stack))
        }
    }

    #[test]
    fn the_pair_comparison_reports_a_difference() {
        let mut memory = Memory::new();
        memory.with_context(|context| assert_eq!(helper_f5fa24(context, HANDLE, 0, 0), Ok(0)));
        memory.with_context(|context| assert_eq!(helper_f5fa24(context, HANDLE, 0, 1), Ok(1)));
    }

    #[test]
    fn the_second_comparison_stores_the_mapping_and_compares_the_indexed_pair() {
        let mut memory = Memory::new();
        memory.with_context(|context| assert_eq!(helper_f5fa7c(context, HANDLE, 0, GuestAddress::new(OUTPUT)), Ok(0)));
        assert_eq!(u32::from_le_bytes(memory.output), 0);
        memory.shared[0x998..0x99c].copy_from_slice(&1u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f5fa7c(context, HANDLE, 0, GuestAddress::new(OUTPUT)), Ok(1)));
    }
}
