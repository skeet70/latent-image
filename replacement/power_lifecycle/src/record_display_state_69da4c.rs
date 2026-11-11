//! Display state reads behind the service status and the display snapshot.
//!
//! Image-4 bodies 0x0069da4c-0x00744ee8.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::osal::CriticalKernel;
use crate::record_service_status_6b6ee8::{helper_6b6ee8, helper_6c4bec, helper_6c4c24, helper_6c65dc};

/// The image-4 body at 0x0069da4c.
pub fn helper_69da4c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, status: GuestAddress, value: GuestAddress) -> Result<i32, MemoryError> {
    context.with_guest_frame(ByteCount(4), StackAlignment::Word, |context, frame| {
        helper_6b6ee8(context, status, frame.base(), value)?;
        Ok(if context.read_u32(status)? == 14 { -254 } else { 0 })
    })
}

/// The image-4 body at 0x006c5b44.
pub fn helper_6c5b44<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_6c4c24(context)
}

/// The image-4 body at 0x006c5d8c.
///
/// The image-4 body at 0x006c4bec always returns one, so the stock comparison
/// against zero never takes its negative result.
pub fn helper_6c5d8c<M: GuestAddressSpace, E: CriticalKernel>(
    context: &mut RecordContext<M, E>,
    version: GuestAddress,
    serial: GuestAddress,
) -> Result<i32, MemoryError> {
    context.with_guest_frame(ByteCount(64), StackAlignment::Word, |context, frame| {
        helper_6c4bec(context, frame.base())?;
        let value = context.read_u8(frame.base().checked_add(8)?)?;
        context.write_u8(version, value)?;
        context.copy_bytes(serial, frame.base().checked_add(44)?, ByteCount(3))?;
        Ok(0)
    })
}

/// The image-4 body at 0x00744ee8.
pub fn helper_744ee8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<i32, MemoryError> {
    helper_6c65dc(context, destination)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0069da4c-0x00744ee8.
    use super::*;
    use crate::osal::{CpuCore, LockStamp, SavedInterrupts};
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::record_service_status_6b6ee8::{DISPLAY_STATE_COPY, DISPLAY_STATE_WORD, DISPLAY_VERSION, SERVICE_STATUS};

    const STACK: u32 = 0x0010_0000;
    const OUTPUT: u32 = 0x0080_0000;

    #[derive(Default)]
    struct Kernel;

    impl CriticalKernel for Kernel {
        fn core(&self) -> CpuCore { CpuCore::First }
        fn mask_interrupts(&mut self) -> SavedInterrupts { SavedInterrupts(0) }
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
        stack: [u8; 0x100],
        output: [u8; 0x40],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(SERVICE_STATUS, 0x10), &mut memory.status).unwrap(),
            MemoryRegion::new(descriptor(DISPLAY_STATE_COPY, 60), &mut memory.display_copy).unwrap(),
            MemoryRegion::new(descriptor(DISPLAY_STATE_WORD, 4), &mut memory.display_word).unwrap(),
            MemoryRegion::new(descriptor(DISPLAY_VERSION, 0x70), &mut memory.version).unwrap(),
            MemoryRegion::new(descriptor(STACK, 0x100), &mut memory.stack).unwrap(),
            MemoryRegion::new(descriptor(OUTPUT, 0x40), &mut memory.output).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x100)).unwrap();
        body(&mut RecordContext::with_stack(space, Kernel, stack))
    }

    fn memory() -> Memory {
        Memory {
            status: [0; 0x10],
            display_copy: [0; 60],
            display_word: [0; 4],
            version: [0; 0x70],
            stack: [0; 0x100],
            output: [0; 0x40],
        }
    }

    #[test]
    fn the_status_check_reports_fourteen_as_the_negative_result() {
        let mut memory = memory();
        memory.status[0..4].copy_from_slice(&14u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_69da4c(context, GuestAddress::new(SERVICE_STATUS), GuestAddress::new(OUTPUT)), Ok(-254));
        });
        memory.status[0..4].copy_from_slice(&13u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_69da4c(context, GuestAddress::new(SERVICE_STATUS), GuestAddress::new(OUTPUT)), Ok(0));
        });
    }

    #[test]
    fn the_display_word_and_snapshot_come_from_their_blocks() {
        let mut memory = memory();
        memory.display_word = 0x0102_0304u32.to_le_bytes();
        for index in 0..60 {
            memory.display_copy[index] = index as u8;
        }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_6c5b44(context), Ok(0x0102_0304));
            assert_eq!(helper_6c5d8c(context, GuestAddress::new(OUTPUT), GuestAddress::new(OUTPUT + 0x20)), Ok(0));
        });
        assert_eq!(memory.output[0], 8);
        assert_eq!(&memory.output[0x20..0x23], &[44, 45, 46]);
    }

    #[test]
    fn the_version_pack_returns_through_the_display_body() {
        let mut memory = memory();
        memory.version[0x62] = 0x20;
        memory.version[0x63] = 0x11;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_744ee8(context, GuestAddress::new(OUTPUT)), Ok(1));
        });
        assert_eq!(&memory.output[0..2], &2011u16.to_le_bytes());
    }
}
