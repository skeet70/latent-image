//! Service status, display state and pipeline counters.
//!
//! Image-4 bodies 0x006b6ee8-0x0076a5fc and image-5 bodies 0x00ef9ca8-0x00ff6fc0.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::osal::{interrupt_lock, interrupt_unlock, CriticalKernel};

/// The published status triple the image-4 body at 0x006b6ee8 copies out.
pub const SERVICE_STATUS: u32 = 0x4089_ea5c;

/// The image-4 body at 0x006b6ee8.
pub fn helper_6b6ee8<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    status: GuestAddress,
    detail: GuestAddress,
    value: GuestAddress,
) -> Result<i32, MemoryError> {
    let status_word = context.read_u32(GuestAddress::new(SERVICE_STATUS))?;
    let detail_word = context.read_u32(GuestAddress::new(SERVICE_STATUS + 8))?;
    let value_word = context.read_u32(GuestAddress::new(SERVICE_STATUS + 0xc))?;
    context.write_u32(status, status_word)?;
    context.write_u32(detail, detail_word)?;
    context.write_u32(value, value_word)?;
    Ok(0)
}

/// The display state block the image-4 body at 0x006c4bec copies.
pub const DISPLAY_STATE_COPY: u32 = 0x0096_b37c;
/// The length that body copies.
pub const DISPLAY_STATE_COPY_LENGTH: u32 = 60;

/// The image-4 body at 0x006c4bec.
pub fn helper_6c4bec<M: GuestAddressSpace, E: CriticalKernel>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<i32, MemoryError> {
    let saved = interrupt_lock(&mut context.effects);
    let copied = context.copy_bytes(destination, GuestAddress::new(DISPLAY_STATE_COPY), ByteCount(DISPLAY_STATE_COPY_LENGTH));
    interrupt_unlock(&mut context.effects, saved);
    copied?;
    Ok(1)
}

/// The word the image-4 body at 0x006c4c24 returns.
pub const DISPLAY_STATE_WORD: u32 = 0x0095_e1dc;

/// The image-4 body at 0x006c4c24.
pub fn helper_6c4c24<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(DISPLAY_STATE_WORD))
}

/// The two packed version bytes the image-4 body at 0x006c65dc reads.
pub const DISPLAY_VERSION: u32 = 0x0093_c544;

/// The image-4 body at 0x006c65dc.
///
/// The stored halfword is the four decimal digits of the two bytes whatever
/// their nibbles hold; the result reports whether both bytes are decimal.
pub fn helper_6c65dc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<i32, MemoryError> {
    let first = context.read_u8(GuestAddress::new(DISPLAY_VERSION + 0x62))?;
    let second = context.read_u8(GuestAddress::new(DISPLAY_VERSION + 0x63))?;
    let decimal = (first & 0xf0) < 0xa0 && (first & 0x0f) < 10 && (second & 0xf0) < 0xa0 && (second & 0x0f) < 10;
    let packed = u16::from(first >> 4) * 1000 + u16::from(first & 0x0f) * 100 + u16::from(second >> 4) * 10 + u16::from(second & 0x0f);
    context.write_u16(destination, packed)?;
    Ok(i32::from(decimal))
}

/// The image-4 body at 0x00757470.
pub fn helper_757470<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    context.read_u16(GuestAddress::new(SHARED_BLOCK + 0x244)).map(|value| i32::from(value as i16))
}

/// The image-4 body at 0x0076a5fc.
pub fn helper_76a5fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    let _ = context;
    Ok(value)
}

/// The image-5 body at 0x00ef9ca8.
pub fn helper_ef9ca8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let _ = context;
    Ok(0x0169_d128)
}

/// The word the image-5 body at 0x00f1ed5c returns.
pub const PIPELINE_SNAPSHOT_WORD: u32 = 0x016b_3b54;

/// The image-5 body at 0x00f1ed5c.
pub fn helper_f1ed5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(PIPELINE_SNAPSHOT_WORD))
}

/// The selection byte the image-5 body at 0x00f251c4 writes.
pub const SELECTION_VALUE_CHANGED: u32 = 0x016b_6584;

/// The image-5 body at 0x00f251c4.
pub fn helper_f251c4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u8(GuestAddress::new(SELECTION_VALUE_CHANGED), value as u8)
}

/// The image-5 body at 0x00f8083c.
pub fn helper_f8083c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let _ = context;
    Ok(())
}

/// The counter the image-5 body at 0x00ff6fc0 publishes.
pub const PIPELINE_SNAPSHOT_COUNTER: u32 = 0x016f_672c;

/// The image-5 body at 0x00ff6fc0.
pub fn helper_ff6fc0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    let value = context.read_u32(GuestAddress::new(PIPELINE_SNAPSHOT_COUNTER))?;
    context.write_u32(destination, value)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x006b6ee8-0x0076a5fc and
    //! the image-5 bodies at 0x00ef9ca8-0x00ff6fc0.
    use super::*;
    use crate::osal::{CpuCore, LockStamp, SavedInterrupts};

    const CONFIG_POINTER: u32 = 0x0096_ca50;

    #[derive(Default)]
    struct Kernel {
        masked: u32,
    }

    impl CriticalKernel for Kernel {
        fn core(&self) -> CpuCore { CpuCore::First }
        fn mask_interrupts(&mut self) -> SavedInterrupts { self.masked += 1; SavedInterrupts(self.masked) }
        fn restore_interrupts(&mut self, _: SavedInterrupts) {}
        fn observe_global_lock(&mut self) -> LockStamp { LockStamp(0x11) }
        fn try_global_lock(&mut self, _: LockStamp) -> bool { true }
        fn release_global_lock(&mut self) {}
        fn nesting(&self) -> u32 { 1 }
        fn set_nesting(&mut self, _: u32) {}
    }

    struct Memory {
        status: [u8; 0x10],
        display_copy: [u8; 60],
        display_word: [u8; 4],
        version: [u8; 0x70],
        shared: [u8; 0x300],
        pipeline: [u8; 4],
        selection: [u8; 4],
        counter: [u8; 4],
        output: [u8; 0x80],
    }

    impl Memory {
        fn new() -> Self {
            Self {
                status: [0; 0x10],
                display_copy: [0; 60],
                display_word: [0; 4],
                version: [0; 0x70],
                shared: [0; 0x300],
                pipeline: [0; 4],
                selection: [0; 4],
                counter: [0; 4],
                output: [0; 0x80],
            }
        }
    }

    fn descriptor(base: u32, length: u32) -> crate::record_address_space::RegionDescriptor {
        crate::record_address_space::RegionDescriptor::new(
            GuestAddress::new(base),
            ByteCount(length),
            crate::record_address_space::RegionDomain::Shared,
            crate::record_address_space::RegionPermission::ReadWrite,
        )
        .unwrap()
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<crate::record_address_space::HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion};
        let mut regions = [
            MemoryRegion::new(descriptor(SERVICE_STATUS, 0x10), &mut memory.status).unwrap(),
            MemoryRegion::new(descriptor(DISPLAY_STATE_COPY, 60), &mut memory.display_copy).unwrap(),
            MemoryRegion::new(descriptor(DISPLAY_STATE_WORD, 4), &mut memory.display_word).unwrap(),
            MemoryRegion::new(descriptor(DISPLAY_VERSION, 0x70), &mut memory.version).unwrap(),
            MemoryRegion::new(descriptor(SHARED_BLOCK, 0x300), &mut memory.shared).unwrap(),
            MemoryRegion::new(descriptor(PIPELINE_SNAPSHOT_WORD, 4), &mut memory.pipeline).unwrap(),
            MemoryRegion::new(descriptor(SELECTION_VALUE_CHANGED, 4), &mut memory.selection).unwrap(),
            MemoryRegion::new(descriptor(PIPELINE_SNAPSHOT_COUNTER, 4), &mut memory.counter).unwrap(),
            MemoryRegion::new(descriptor(0x0030_0000, 0x80), &mut memory.output).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, Kernel::default()))
    }

    #[test]
    fn the_status_triplet_copies_three_published_words() {
        let mut memory = Memory::new();
        memory.status[0..4].copy_from_slice(&0x1111_1111u32.to_le_bytes());
        memory.status[8..12].copy_from_slice(&0x2222_2222u32.to_le_bytes());
        memory.status[12..16].copy_from_slice(&0x3333_3333u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            let output = GuestAddress::new(0x0030_0000);
            assert_eq!(helper_6b6ee8(context, output, output.checked_add(4).unwrap(), output.checked_add(8).unwrap()), Ok(0));
        });
        assert_eq!(&memory.output[0..4], &0x1111_1111u32.to_le_bytes());
        assert_eq!(&memory.output[4..8], &0x2222_2222u32.to_le_bytes());
        assert_eq!(&memory.output[8..12], &0x3333_3333u32.to_le_bytes());
    }

    #[test]
    fn the_display_copy_runs_inside_the_interrupt_mask() {
        let mut memory = Memory::new();
        let mut expected = [0u8; 60];
        for index in 0..60 {
            memory.display_copy[index] = index as u8 + 1;
            expected[index] = index as u8 + 1;
        }
        let mut masked = 0;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_6c4bec(context, GuestAddress::new(0x0030_0000)), Ok(1));
            masked = context.effects.masked;
        });
        assert_eq!(masked, 1);
        assert_eq!(&memory.output[0..60], &expected);
    }

    #[test]
    fn the_display_word_and_version_digits_are_read_from_their_blocks() {
        let mut memory = Memory::new();
        memory.display_word = 0x0506_0708u32.to_le_bytes();
        memory.version[0x62] = 0x12;
        memory.version[0x63] = 0x34;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_6c4c24(context), Ok(0x0506_0708));
            assert_eq!(helper_6c65dc(context, GuestAddress::new(0x0030_0010)), Ok(1));
        });
        assert_eq!(&memory.output[0x10..0x12], &1234u16.to_le_bytes());
    }

    #[test]
    fn a_non_decimal_nibble_clears_the_version_result_but_still_packs() {
        let mut memory = Memory::new();
        memory.version[0x62] = 0x1a;
        memory.version[0x63] = 0x34;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_6c65dc(context, GuestAddress::new(0x0030_0010)), Ok(0));
        });
        assert_eq!(&memory.output[0x10..0x12], &2034u16.to_le_bytes());
    }

    #[test]
    fn the_shared_halfword_is_sign_extended() {
        let mut memory = Memory::new();
        memory.shared[0x244..0x246].copy_from_slice(&0xfffeu16.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_757470(context), Ok(-2)));
    }

    #[test]
    fn the_identity_and_empty_bodies_return_their_input() {
        let mut memory = Memory::new();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_76a5fc(context, 0x1234_5678), Ok(0x1234_5678));
            assert_eq!(helper_f8083c(context), Ok(()));
            assert_eq!(helper_ef9ca8(context), Ok(0x0169_d128));
        });
    }

    #[test]
    fn the_pipeline_words_follow_their_published_addresses() {
        let mut memory = Memory::new();
        memory.pipeline = 0x0a0b_0c0du32.to_le_bytes();
        memory.counter = 0x0102_0304u32.to_le_bytes();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_f1ed5c(context), Ok(0x0a0b_0c0d));
            assert_eq!(helper_ff6fc0(context, GuestAddress::new(0x0030_0020)), Ok(()));
        });
        assert_eq!(&memory.output[0x20..0x24], &0x0102_0304u32.to_le_bytes());
    }

    #[test]
    fn the_selection_store_keeps_the_low_byte() {
        let mut memory = Memory::new();
        with_memory(&mut memory, |context| assert_eq!(helper_f251c4(context, 0x1ff), Ok(())));
        assert_eq!(memory.selection[0], 0xff);
    }
}
