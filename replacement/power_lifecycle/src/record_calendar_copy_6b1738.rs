//! Calendar snapshot copy, image-4 body 0x006b1738.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::osal::{interrupt_lock, interrupt_unlock, CriticalKernel};

pub const CALENDAR: u32 = 0x409c34b8;
pub const CALENDAR_BYTES: u32 = 6;

/// The image-4 body at 0x006b1738.
pub fn helper_6b1738<M: GuestAddressSpace, E: CriticalKernel>(context: &mut RecordContext<M, E>, fields: GuestAddress) -> Result<u32, MemoryError> {
    let saved = interrupt_lock(&mut context.effects);
    let copied = context.copy_bytes(fields, GuestAddress::new(CALENDAR), ByteCount(CALENDAR_BYTES));
    interrupt_unlock(&mut context.effects, saved);
    copied?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osal::{CpuCore, LockStamp, SavedInterrupts};
    use crate::record_address_space::{
        GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const FIELDS: u32 = 0x0010_0000;

    #[derive(Default)]
    struct TestKernel {
        saved: Vec<u32>,
        restored: Vec<u32>,
    }

    impl CriticalKernel for TestKernel {
        fn core(&self) -> CpuCore { CpuCore::First }
        fn mask_interrupts(&mut self) -> SavedInterrupts { self.saved.push(0x80); SavedInterrupts(0x80) }
        fn restore_interrupts(&mut self, saved: SavedInterrupts) { self.restored.push(saved.0); }
        fn observe_global_lock(&mut self) -> LockStamp { LockStamp(0x11) }
        fn try_global_lock(&mut self, _: LockStamp) -> bool { true }
        fn release_global_lock(&mut self) {}
        fn nesting(&self) -> u32 { 0 }
        fn set_nesting(&mut self, _: u32) {}
    }

    struct Memory {
        calendar: [u8; 6],
        fields: [u8; 6],
    }

    impl Memory {
        fn new() -> Self {
            let mut calendar = [0u8; 6];
            calendar.copy_from_slice(&[26, 11, 10, 12, 34, 56]);
            Self { calendar, fields: [0; 6] }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, TestKernel>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(CALENDAR, 6), &mut self.calendar).unwrap(),
                MemoryRegion::new(descriptor(FIELDS, 6), &mut self.fields).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, TestKernel::default()))
        }
    }

    #[test]
    fn calendar_copy_reads_six_bytes_under_the_kernel_lock() {
        let mut memory = Memory::new();
        let (saved, restored) = memory.with_context(|context| {
            assert_eq!(helper_6b1738(context, GuestAddress::new(FIELDS)), Ok(0));
            (context.effects.saved.clone(), context.effects.restored.clone())
        });
        assert_eq!(memory.fields, [26, 11, 10, 12, 34, 56]);
        assert_eq!(saved, vec![0x80]);
        assert_eq!(restored, vec![0x80]);
    }
}
