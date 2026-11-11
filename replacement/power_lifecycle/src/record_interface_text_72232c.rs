//! Image-4 byte compare, text copy/append and the adjusted-tick wait at
//! 0x0072232c-0x00743ce4.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_log::helper_721b04;
use crate::record_storage_service_6a7f28::StorageKernel;

pub fn helper_72232c<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, left: GuestAddress, right: GuestAddress, count: u32,
) -> Result<u32, MemoryError> {
    for index in 0..count {
        let first = context.read_u8(left.checked_add(index)?)?;
        let second = context.read_u8(right.checked_add(index)?)?;
        if first != second { return Ok(left.checked_add(index)?.get()); }
    }
    Ok(0)
}

pub fn helper_742250<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress,
) -> Result<u32, MemoryError> {
    helper_721b04(context, destination, source).map(GuestAddress::get)
}

pub fn helper_742260<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress,
) -> Result<u32, MemoryError> {
    let mut end = destination;
    while context.read_u8(end)? != 0 { end = end.checked_add(1)?; }
    helper_721b04(context, end, source).map(GuestAddress::get)
}

pub fn helper_743ce4<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>, ticks: u32) -> Result<(), MemoryError> {
    let scaled = (ticks / 10).wrapping_mul(10);
    let adjusted = if ticks == scaled { ticks.wrapping_add(1) } else { ticks };
    context.effects.delay_microseconds(adjusted & 0x7fff_ffff);
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0072232c, 0x00742250,
    //! 0x00742260 and 0x00743ce4.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::{KernelStatus, ServiceMessage, ShutdownRtos, TaskId};

    const LEFT: u32 = 0x0010_0000;
    const RIGHT: u32 = 0x0010_0100;

    #[derive(Default)]
    struct Kernel { delays: [u32; 4], delays_len: usize }

    impl ShutdownRtos for Kernel {
        fn current_task(&mut self) -> TaskId { TaskId(0) }
        fn post(&mut self, _message: ServiceMessage<'_>) -> Result<(), KernelStatus> { Ok(()) }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    impl StorageKernel for Kernel {
        fn wait_semaphore(&mut self, _id: u32, _timeout: u32) -> i32 { 0 }
        fn signal_semaphore(&mut self, _id: u32) -> i32 { 0 }
        fn kernel_command(&mut self, _category: u32, _code: u32) {}
        fn delay_microseconds(&mut self, duration: u32) {
            self.delays[self.delays_len] = duration;
            self.delays_len += 1;
        }
        fn invoke_callback(&mut self, _word: u32, _first: u32, _second: Option<u32>) {}
    }

    struct Memory { left: [u8; 0x40], right: [u8; 0x40] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(LEFT, 0x40), &mut memory.left).unwrap(),
            MemoryRegion::new(descriptor(RIGHT, 0x40), &mut memory.right).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, Kernel::default()))
    }

    #[test]
    fn the_compare_returns_the_first_differing_left_address_or_zero() {
        let mut memory = Memory { left: [0; 0x40], right: [0; 0x40] };
        memory.left[..4].copy_from_slice(b"abcd");
        memory.right[..4].copy_from_slice(b"abcd");
        with_memory(&mut memory, |context| {
            assert_eq!(helper_72232c(context, GuestAddress::new(LEFT), GuestAddress::new(RIGHT), 4), Ok(0));
        });
        memory.right[2] = b'x';
        with_memory(&mut memory, |context| {
            assert_eq!(helper_72232c(context, GuestAddress::new(LEFT), GuestAddress::new(RIGHT), 4), Ok(LEFT + 2));
            assert_eq!(helper_72232c(context, GuestAddress::new(LEFT), GuestAddress::new(RIGHT), 2), Ok(0));
        });
    }

    #[test]
    fn copy_and_append_return_the_written_terminator_address() {
        let mut memory = Memory { left: [0; 0x40], right: [0; 0x40] };
        memory.right[..4].copy_from_slice(b"one\0");
        let destination = GuestAddress::new(LEFT);
        let source = GuestAddress::new(RIGHT);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_742250(context, destination, source), Ok(LEFT + 3));
        });
        assert_eq!(&memory.left[..4], b"one\0");
        memory.right[..4].copy_from_slice(b"two\0");
        with_memory(&mut memory, |context| {
            assert_eq!(helper_742260(context, destination, source), Ok(LEFT + 6));
        });
        assert_eq!(&memory.left[..7], b"onetwo\0");
    }

    #[test]
    fn the_wait_records_the_adjusted_durations() {
        let mut memory = Memory { left: [0; 0x40], right: [0; 0x40] };
        with_memory(&mut memory, |context| {
            assert_eq!(helper_743ce4(context, 20), Ok(()));
            assert_eq!(helper_743ce4(context, 21), Ok(()));
            assert_eq!(helper_743ce4(context, 0x8000_0005), Ok(()));
            assert_eq!(context.effects.delays_len, 3);
            assert_eq!(context.effects.delays[0], 21);
            assert_eq!(context.effects.delays[1], 21);
            assert_eq!(context.effects.delays[2], 5);
        });
    }
}
