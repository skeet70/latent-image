//! Driver handle table, image-4 bodies 0x0065722c, 0x00657384 and 0x0065744c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const SERIAL: u32 = 0x409c2804;
pub const STATUS_INVALID_INDEX: i32 = -30;
pub const STATUS_SLOT_BUSY: i32 = -20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandleTable {
    pub base: GuestAddress,
    pub capacity: u32,
}

impl HandleTable {
    pub const fn slot(self, index: u32) -> GuestAddress { GuestAddress::new(self.base.get().wrapping_add(index.wrapping_mul(4))) }
}

fn serial<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(SERIAL))
}

/// The image-4 body at 0x0065722c.
pub fn helper_65722c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, table: HandleTable) -> Result<i32, MemoryError> {
    if index >= table.capacity { return Ok(STATUS_INVALID_INDEX); }
    if context.read_u32(table.slot(index))? != 0 { return Ok(STATUS_SLOT_BUSY); }
    let handle = loop {
        let mut handle = serial(context)?.wrapping_add(1);
        context.write_u32(GuestAddress::new(SERIAL), handle)?;
        if handle == 0 {
            handle = serial(context)?.wrapping_add(1);
            context.write_u32(GuestAddress::new(SERIAL), handle)?;
        }
        let mut scan = 0;
        while scan < table.capacity && context.read_u32(table.slot(scan))? != handle { scan += 1; }
        if scan == table.capacity { break handle; }
    };
    context.write_u32(table.slot(index), handle)?;
    Ok(0)
}

/// The image-4 body at 0x00657384.
pub fn helper_657384<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, table: HandleTable, channel: GuestAddress) -> Result<i32, MemoryError> {
    if handle == 0 { return Ok(STATUS_INVALID_INDEX); }
    let mut index = 0;
    while index < table.capacity && context.read_u32(table.slot(index))? != handle { index += 1; }
    if index == table.capacity { return Ok(STATUS_INVALID_INDEX); }
    context.write_u32(channel, index)?;
    Ok(0)
}

/// The image-4 body at 0x0065744c.
pub fn helper_65744c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, table: HandleTable) -> Result<i32, MemoryError> {
    if index >= table.capacity { return Ok(STATUS_INVALID_INDEX); }
    context.write_u32(table.slot(index), 0)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const TABLE: u32 = 0x0091_8bd8;

    struct Memory {
        serial: [u8; 4],
        handles: [u8; 8 * 4],
        channel: [u8; 4],
    }

    impl Memory {
        fn new(serial: u32) -> Self {
            let mut memory = Self { serial: serial.to_le_bytes(), handles: [0; 32], channel: [0; 4] };
            for (index, value) in [1u32, 2, 3, 0, 7, 0, 0, 0].into_iter().enumerate() {
                memory.handles[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            memory
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(SERIAL, 4), &mut self.serial).unwrap(),
                MemoryRegion::new(descriptor(TABLE, 32), &mut self.handles).unwrap(),
                MemoryRegion::new(descriptor(0x1000, 4), &mut self.channel).unwrap(),
            ];
            body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
        }

        fn handle(&self, index: usize) -> u32 { u32::from_le_bytes(self.handles[index * 4..index * 4 + 4].try_into().unwrap()) }
        fn serial(&self) -> u32 { u32::from_le_bytes(self.serial) }
        fn channel(&self) -> u32 { u32::from_le_bytes(self.channel) }
    }

    fn table() -> HandleTable { HandleTable { base: GuestAddress::new(TABLE), capacity: 8 } }

    #[test]
    fn allocate_rejects_a_slot_past_the_table() {
        let mut memory = Memory::new(0);
        memory.with_context(|context| assert_eq!(helper_65722c(context, 8, table()), Ok(-30)));
        assert_eq!(memory.serial(), 0);
        assert_eq!(memory.handle(0), 1);
    }

    #[test]
    fn allocate_rejects_a_busy_slot() {
        let mut memory = Memory::new(0);
        memory.with_context(|context| assert_eq!(helper_65722c(context, 0, table()), Ok(-20)));
        assert_eq!(memory.serial(), 0);
    }

    #[test]
    fn allocate_takes_the_next_free_serial() {
        let mut memory = Memory::new(4);
        memory.with_context(|context| assert_eq!(helper_65722c(context, 3, table()), Ok(0)));
        assert_eq!(memory.serial(), 5);
        assert_eq!(memory.handle(3), 5);
    }

    #[test]
    fn allocate_skips_a_serial_that_is_already_held() {
        let mut memory = Memory::new(4);
        memory.handles[16..20].copy_from_slice(&5u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_65722c(context, 5, table()), Ok(0)));
        assert_eq!(memory.serial(), 6);
        assert_eq!(memory.handle(5), 6);
    }

    #[test]
    fn allocate_skips_zero_after_wrapping() {
        let mut memory = Memory::new(u32::MAX);
        memory.with_context(|context| assert_eq!(helper_65722c(context, 3, table()), Ok(0)));
        assert_eq!(memory.serial(), 4);
        assert_eq!(memory.handle(3), 4);
    }

    #[test]
    fn find_reports_the_slot_of_a_live_handle() {
        let mut memory = Memory::new(0);
        memory.with_context(|context| {
            assert_eq!(helper_657384(context, 7, table(), GuestAddress::new(0x1000)), Ok(0));
            assert_eq!(context.read_u32(GuestAddress::new(0x1000)), Ok(4));
        });
    }

    #[test]
    fn find_rejects_zero_and_unknown_handles_without_writing() {
        let mut memory = Memory::new(0);
        memory.channel.copy_from_slice(&0x1234_5678u32.to_le_bytes());
        memory.with_context(|context| {
            assert_eq!(helper_657384(context, 0, table(), GuestAddress::new(0x1000)), Ok(-30));
            assert_eq!(helper_657384(context, 99, table(), GuestAddress::new(0x1000)), Ok(-30));
        });
        assert_eq!(memory.channel(), 0x1234_5678);
    }

    #[test]
    fn clear_zeroes_one_slot_and_rejects_a_slot_past_the_table() {
        let mut memory = Memory::new(0);
        memory.with_context(|context| {
            assert_eq!(helper_65744c(context, 2, table()), Ok(0));
            assert_eq!(helper_65744c(context, 8, table()), Ok(-30));
        });
        assert_eq!(memory.handle(2), 0);
        assert_eq!(memory.handle(1), 2);
        assert_eq!(memory.handle(4), 7);
    }
}
