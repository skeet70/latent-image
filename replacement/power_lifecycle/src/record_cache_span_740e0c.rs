//! Image-4 cache span and image-5 quarter-plane gathers, bodies 0x0074_0e0c-0x00ff_6110.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

/// The buffer pointer word both gathers offset.
const IMAGE_BASE_WORD: u32 = 0x016f_2e50 + 0x3db4;

/// The whole-cache clean and invalidate stock enters at 0x0004_2634.
pub trait DataCache {
    fn clean_invalidate_all(&mut self);
}

/// The image-4 body at 0x0074_0e0c.
pub fn helper_740e0c(address: u32) -> u32 {
    address & !0x4000_0000
}

/// The image-4 body at 0x0074_0e3c.
pub fn helper_740e3c(address: u32, length: u32, cache: &mut impl DataCache) -> u32 {
    let end = (address & 31).wrapping_add(length);
    let end = if end & 31 != 0 { (end & !31).wrapping_add(32) } else { end };
    let start = if end <= 16384 { address & !31 } else { 0 };
    cache.clean_invalidate_all();
    start & !0x4000_001f
}

fn copy<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: u32, source: u32, bytes: u32,
) -> Result<(), MemoryError> {
    context.copy_bytes(GuestAddress::new(destination), GuestAddress::new(source), ByteCount(bytes))
}

/// The image-5 body at 0x00ff_5fc4.
pub fn helper_ff5fc4<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: u32, bytes: u32, cache: &mut impl DataCache,
) -> Result<(), MemoryError> {
    use crate::record_address_space::StackAlignment;
    helper_740e3c(0, 0, cache);
    let target = helper_740e0c(destination);
    context.with_guest_frame(ByteCount(4), StackAlignment::Word, |context, frame| {
        crate::record_hardware_deep_banks::helper_edae88(context, 3, frame.base())?;
        let offset = context.read_u32(frame.base())?;
        let base = context.read_u32(GuestAddress::new(IMAGE_BASE_WORD))?;
        let source = helper_740e0c(base).wrapping_add(offset);
        copy(context, target, source, bytes)?;
        let quarter = bytes >> 2;
        copy(context, target.wrapping_add(quarter.wrapping_mul(3)), source.wrapping_add(quarter), quarter)?;
        Ok::<_, MemoryError>(())
    })?;
    helper_740e3c(0, 0, cache);
    Ok(())
}

/// The image-5 body at 0x00ff_6050.
pub fn helper_ff6050<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: u32, bytes: u32,
) -> Result<(), MemoryError> {
    use crate::record_address_space::StackAlignment;
    let quarter = bytes >> 2;
    for (index, selector) in [13u32, 14, 15, 16].into_iter().enumerate() {
        context.with_guest_frame(ByteCount(4), StackAlignment::Word, |context, frame| {
            crate::record_hardware_deep_banks::helper_edae88(context, selector, frame.base())?;
            let offset = context.read_u32(frame.base())?;
            let base = context.read_u32(GuestAddress::new(IMAGE_BASE_WORD))?;
            copy(context, destination.wrapping_add(quarter.wrapping_mul(index as u32)), base.wrapping_add(offset), quarter)
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Expected spans and copies are the image-4/5 instructions at 0x0074_0e0c-0x00ff_6110.
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Cache { calls: u32 }

    impl DataCache for Cache {
        fn clean_invalidate_all(&mut self) { self.calls += 1; }
    }

    #[test]
    fn the_address_helper_clears_the_uncached_bit() {
        assert_eq!(helper_740e0c(0x4010_0000), 0x0010_0000);
        assert_eq!(helper_740e0c(0x0010_0000), 0x0010_0000);
    }

    #[test]
    fn the_span_helper_rounds_the_end_up_and_aligns_the_start() {
        let mut cache = Cache { calls: 0 };
        assert_eq!(helper_740e3c(0x0010_0010, 0x20, &mut cache), 0x0010_0000);
        assert_eq!(cache.calls, 1);
        assert_eq!(helper_740e3c(0x4010_0010, 1, &mut cache), 0x0010_0000);
        assert_eq!(cache.calls, 2);
        assert_eq!(helper_740e3c(0x0010_0000, 0x4000, &mut cache), 0x0010_0000);
        assert_eq!(helper_740e3c(0x0010_0000, 0x4001, &mut cache), 0);
        assert_eq!(cache.calls, 4);
    }

    /// The bank offsets `0x00ed_ae88` reports for selector 3 and for 13..16.
    const OFFSET_3: usize = 2128;
    const OFFSET_13: usize = 8592;

    #[test]
    fn the_two_plane_copy_moves_the_quarter_plane_again() {
        let mut offset = [0u8; 4];
        let mut base = [0u8; 4];
        let mut source = [0u8; 0x3000];
        let mut destination = [0u8; 0x40];
        let mut stack_bytes = [0u8; 0x400];
        for index in 0..0x3000 { source[index] = (index as u8) ^ 0x5a; }
        offset.copy_from_slice(&0u32.to_le_bytes());
        base.copy_from_slice(&0x0010_1000u32.to_le_bytes());
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0010_0000, 4), &mut offset).unwrap(),
            MemoryRegion::new(descriptor(IMAGE_BASE_WORD, 4), &mut base).unwrap(),
            MemoryRegion::new(descriptor(0x0010_1000, 0x3000), &mut source).unwrap(),
            MemoryRegion::new(descriptor(0x0010_6000, 0x40), &mut destination).unwrap(),
            MemoryRegion::new(descriptor(0x0010_8000, 0x400), &mut stack_bytes).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(0x0010_8000), ByteCount(0x400)).unwrap();
        let mut context = RecordContext::with_stack(space, (), stack);
        let mut cache = Cache { calls: 0 };
        assert_eq!(helper_ff5fc4(&mut context, 0x0010_6000, 0x20, &mut cache), Ok(()));
        assert_eq!(cache.calls, 2);
        assert_eq!(&destination[..0x18], &source[OFFSET_3..OFFSET_3 + 0x18]);
        assert_eq!(&destination[0x18..0x20], &source[OFFSET_3 + 8..OFFSET_3 + 0x10]);
    }

    #[test]
    fn the_quarter_plane_gather_writes_each_selector_run() {
        let mut offsets = [0u8; 4];
        offsets.copy_from_slice(&0u32.to_le_bytes());
        let mut base = [0u8; 4];
        base.copy_from_slice(&0x0010_1000u32.to_le_bytes());
        let mut source = [0u8; 0x3000];
        for index in 0..0x3000 { source[index] = (index as u8) ^ 0x5a; }
        let mut destination = [0u8; 0x10];
        let mut stack_bytes = [0u8; 0x400];
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0010_0000, 4), &mut offsets).unwrap(),
            MemoryRegion::new(descriptor(IMAGE_BASE_WORD, 4), &mut base).unwrap(),
            MemoryRegion::new(descriptor(0x0010_1000, 0x3000), &mut source).unwrap(),
            MemoryRegion::new(descriptor(0x0010_6000, 0x10), &mut destination).unwrap(),
            MemoryRegion::new(descriptor(0x0010_8000, 0x400), &mut stack_bytes).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(0x0010_8000), ByteCount(0x400)).unwrap();
        let mut context = RecordContext::with_stack(space, (), stack);
        assert_eq!(helper_ff6050(&mut context, 0x0010_6000, 0x10), Ok(()));
        assert_eq!(&destination[..], &source[OFFSET_13..OFFSET_13 + 0x10]);
    }
}
