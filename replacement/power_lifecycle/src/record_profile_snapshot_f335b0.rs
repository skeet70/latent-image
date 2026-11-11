//! Profile record publication from one selected record, image-5 entry 0x00f335b0.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};

pub const SELECTED_HANDLE: u32 = 0x016b_fe48;
const RECORD_BYTES: u32 = 76;

fn selected<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(SELECTED_HANDLE))
}

fn signed_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress, at: u32) -> Result<u32, MemoryError> {
    context.read_u16(source.checked_add(at)?).map(|value| value as i16 as i32 as u32)
}

pub fn helper_f335b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> {
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 156)?;
    crate::record_shared_block_writers_757494::helper_7575d4(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 0)?;
    crate::record_shared_block_writers_757494::helper_757494(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 22)?;
    crate::record_shared_block_writers_757494::helper_75789c(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 24)?;
    crate::record_shared_block_writers_757494::helper_7578ec(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 26)?;
    crate::record_shared_block_writers_757494::helper_75798c(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 28)?;
    crate::record_shared_block_writers_757494::helper_7579dc(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 30)?;
    crate::record_shared_block_writers_757494::helper_758a30(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 32)?;
    crate::record_shared_block_writers_757494::helper_758a80(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 34)?;
    crate::record_shared_block_writers_757494::helper_758ad0(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 36)?;
    crate::record_shared_block_writers_757494::helper_758b20(context, handle, value)?;
    let handle = selected(context)?;
    let value = signed_halfword(context, source, 38)?;
    crate::record_shared_block_writers_757494::helper_758b70(context, handle, value)?;
    let handle = selected(context)?;
    let value = u32::from(context.read_u16(source.checked_add(40)?)?);
    crate::record_shared_block_writers_757494::helper_758bc0(context, handle, value)?;
    let handle = selected(context)?;
    let value = context.read_u32(source.checked_add(0x2c)?)?;
    crate::record_shared_block_writers_757494::helper_758c0c(context, handle, value)?;
    if crate::record_profile_accessors::helper_7503ec(context, handle)? != 1 { return Ok(()); }
    context.with_guest_frame(ByteCount(RECORD_BYTES), StackAlignment::Word, |context, first| {
        let handle = selected(context)?;
        crate::record_profile_shared_cpu::helper_7588e8(context, first.base())?;
        context.with_guest_frame(ByteCount(RECORD_BYTES), StackAlignment::Word, |context, second| {
            context.copy_bytes(second.base(), first.base(), ByteCount(RECORD_BYTES))?;
            for (at, from) in [(44u32, 180u32), (46, 182), (48, 184), (74, 48)] {
                let value = context.read_u16(source.checked_add(from)?)?;
                context.write_u16(second.base().checked_add(at)?, value)?;
            }
            crate::record_task43_selection_timing_state::helper_75890c(context, handle, second.base())
        })
    })
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(
        xm1_power_record_helper_f335b0,
        helper_f335b0_abi,
        (),
        (source: u32) -> (),
        (source: GuestAddress),
        (GuestAddress::new(source)),
        crate::record_profile_snapshot_f335b0::helper_f335b0,
        |_| ()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SLOT: u32 = 0x0091_5d44;
    const SHARED: u32 = 0x0091_904c;
    const SOURCE: u32 = 0x016b_f374;
    const STACK: u32 = 0x0010_0000;
    const WRITABLE: u32 = (63 << 18) | (1 << 17) | (0x123 << 4);
    const FIELD: u32 = 33;

    struct Memory {
        slots: [u8; 16 * 0x38],
        selected_bank: [u8; 4],
        resource: [u8; 0x2158],
        shared: [u8; 0xa00],
        selected: [u8; 4],
        source: [u8; 0x100],
        stack: [u8; 0x200],
    }

    impl Memory {
        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SLOT, 16 * 0x38), &mut self.slots).unwrap(),
                MemoryRegion::new(descriptor(0x0091_5d40, 4), &mut self.selected_bank).unwrap(),
                MemoryRegion::new(descriptor(0x0096_ebd0, 0x2158), &mut self.resource).unwrap(),
                MemoryRegion::new(descriptor(SHARED, 0xa00), &mut self.shared).unwrap(),
                MemoryRegion::new(descriptor(SELECTED_HANDLE, 4), &mut self.selected).unwrap(),
                MemoryRegion::new(descriptor(SOURCE, 0x100), &mut self.source).unwrap(),
                MemoryRegion::new(descriptor(STACK, 0x200), &mut self.stack).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x200)).unwrap();
            body(&mut RecordContext::with_stack(space, (), stack))
        }
    }

    fn memory() -> Memory {
        let mut memory = Memory { slots: [0; 16 * 0x38], selected_bank: [0; 4], resource: [0; 0x2158], shared: [0; 0xa00], selected: [0; 4], source: [0; 0x100], stack: [0; 0x200] };
        memory.slots[..4].copy_from_slice(&0x123u32.to_le_bytes());
        memory.slots[12 + 27] = 2;
        memory.slots[12 + FIELD as usize] = 2;
        memory.selected.copy_from_slice(&WRITABLE.to_le_bytes());
        for at in [0u32, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40, 48, 156, 180, 182, 184] {
            memory.source[at as usize..at as usize + 2].copy_from_slice(&(0x100 + at as u16).to_le_bytes());
        }
        memory.source[0x2c..0x30].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        memory.resource[0x63] = 1;
        memory.shared[0x7a0] = 0x55;
        memory
    }

    #[test]
    fn the_twelve_fields_reach_the_shared_block() {
        let mut memory = memory();
        memory.with_context(|context| assert_eq!(helper_f335b0(context, GuestAddress::new(SOURCE)), Ok(())));
        let half = |at: u32| u16::from_le_bytes(memory.shared[at as usize..at as usize + 2].try_into().unwrap());
        assert_eq!(half(0x24c), 0x100 + 156);
        assert_eq!(half(0x244), 0x100);
        assert_eq!(half(0x262), 0x100 + 22);
        assert_eq!(half(0x264), 0x100 + 24);
        assert_eq!(half(0x268), 0x100 + 26);
        assert_eq!(half(0x26a), 0x100 + 28);
        assert_eq!(half(0x6f8), 0x100 + 30);
        assert_eq!(half(0x6fa), 0x100 + 32);
        assert_eq!(half(0x6fc), 0x100 + 34);
        assert_eq!(half(0x6fe), 0x100 + 36);
        assert_eq!(half(0x700), 0x100 + 38);
        assert_eq!(half(0x702), 0x100 + 40);
        assert_eq!(u32::from_le_bytes(memory.shared[0x704..0x708].try_into().unwrap()), 0x1122_3344);
        assert_eq!(memory.shared[0x7a0], 0x55);
        for (at, from) in [(0x7a0 + 44usize, 180u32), (0x7a0 + 46, 182), (0x7a0 + 48, 184), (0x7a0 + 74, 48)] {
            assert_eq!(&memory.shared[at..at + 2], &(0x100 + from as u16).to_le_bytes());
        }
    }

    #[test]
    fn the_record_publishes_only_under_field_thirty_three() {
        let mut memory = memory();
        memory.slots[12 + 27] = 0;
        memory.slots[12 + FIELD as usize] = 0;
        memory.with_context(|context| assert_eq!(helper_f335b0(context, GuestAddress::new(SOURCE)), Ok(())));
        assert_eq!(memory.shared[0x24c..0x24e], [0, 0]);
        assert_eq!(memory.shared[0x704..0x708], [0, 0, 0, 0]);
        assert_eq!(memory.shared[0x7a0 + 44], 0);
    }
}
