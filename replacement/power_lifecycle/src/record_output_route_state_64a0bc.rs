//! Output route channel table at 0x009c_1e84, image-4 bodies 0x0064a0bc and 0x0064a224.
//!
//! One channel is the ceiling, the thirty-eight requests, and the level last
//! written to the routing register.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const CHANNELS: u32 = 0x009c_1e84;
pub const CHANNEL_STRIDE: u32 = 0xa0;
pub const CHANNEL_COUNT: u32 = 11;
pub const SELECTS: u32 = 0x7a20d4;
pub const SELECTS_STRIDE: u32 = 12;
pub const REGISTERS: u32 = 0xfff8_8000;
pub const REGISTER_COUNT: u32 = 10;
pub const STATUS_INVALID: i32 = -30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RouteChannel(pub u32);

impl RouteChannel {
    pub const fn base(self) -> u32 { CHANNELS.wrapping_add(self.0.wrapping_mul(CHANNEL_STRIDE)) }
    pub const fn ceiling(self) -> GuestAddress { GuestAddress::new(self.base()) }
    pub const fn request(self, index: u32) -> GuestAddress { GuestAddress::new(self.base().wrapping_add(4 + index * 4)) }
    pub const fn selected(self) -> GuestAddress { GuestAddress::new(self.base().wrapping_add(0x9c)) }
    pub const fn register(self) -> GuestAddress { GuestAddress::new(REGISTERS.wrapping_add(self.0.wrapping_mul(4))) }
    pub const fn select(self, level: u32) -> GuestAddress {
        GuestAddress::new(SELECTS.wrapping_add(self.0.wrapping_mul(SELECTS_STRIDE * 4).wrapping_add(level.wrapping_mul(4))))
    }
}

pub trait RouteInterrupts {
    fn save_irq(&mut self) -> u32;
    fn restore_irq(&mut self, saved: u32);
}

fn write_selected<M: GuestAddressSpace, E: RouteInterrupts>(
    context: &mut RecordContext<M, E>,
    channel: RouteChannel,
    selection: u32,
) -> Result<(), MemoryError> {
    let saved = context.effects.save_irq();
    let register = channel.register();
    let previous = context.read_u32(register)?;
    let value = if channel.0 == 9 {
        (previous & 0x8fff_ffff) | selection.wrapping_shl(28)
    } else {
        (previous & !7) | selection
    };
    context.write_u32(register, value)?;
    context.effects.restore_irq(saved);
    Ok(())
}

/// The image-4 body at 0x0064a0bc.
pub fn helper_64a0bc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: RouteChannel) -> Result<u32, MemoryError> {
    let mut resolved = context.read_u32(channel.request(0))?;
    for index in 1..38 {
        let requested = context.read_u32(channel.request(index))?;
        if requested > resolved { resolved = requested; }
    }
    let ceiling = context.read_u32(channel.ceiling())?;
    Ok(if ceiling < resolved { ceiling } else { resolved })
}

/// The image-4 body at 0x0064a224.
pub fn helper_64a224<M: GuestAddressSpace, E: RouteInterrupts>(
    context: &mut RecordContext<M, E>,
    channel: RouteChannel,
    level: u32,
) -> Result<i32, MemoryError> {
    let selection = context.read_u32(channel.select(level))?;
    if selection == u32::MAX { return Ok(STATUS_INVALID); }
    if context.read_u32(channel.selected())? == level { return Ok(0); }
    if channel.0 >= REGISTER_COUNT { return Ok(STATUS_INVALID); }
    write_selected(context, channel, selection)?;
    context.write_u32(channel.selected(), level)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osal::{interrupt_lock, interrupt_unlock, CpuCore, CriticalKernel, LockStamp, SavedInterrupts};
    use crate::record_address_space::{
        ByteCount, GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    struct Memory {
        channels: [u8; CHANNEL_COUNT as usize * CHANNEL_STRIDE as usize],
        selects: [u8; CHANNEL_COUNT as usize * SELECTS_STRIDE as usize * 4],
        registers: [u8; REGISTER_COUNT as usize * 4],
        stack: [u8; 0x40],
    }

    impl Memory {
        fn new() -> Self {
            Self {
                channels: [0; CHANNEL_COUNT as usize * CHANNEL_STRIDE as usize],
                selects: [0xff; CHANNEL_COUNT as usize * SELECTS_STRIDE as usize * 4],
                registers: [0; REGISTER_COUNT as usize * 4],
                stack: [0; 0x40],
            }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, TestKernel>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(CHANNELS, self.channels.len() as u32), &mut self.channels).unwrap(),
                MemoryRegion::new(descriptor(SELECTS, self.selects.len() as u32), &mut self.selects).unwrap(),
                MemoryRegion::new(descriptor(REGISTERS, self.registers.len() as u32), &mut self.registers).unwrap(),
                MemoryRegion::new(descriptor(0x0010_0000, 0x40), &mut self.stack).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(0x0010_0000), ByteCount(0x40)).unwrap();
            body(&mut RecordContext::with_stack(space, TestKernel::default(), stack))
        }

        fn word(&self, base: u32, offset: u32) -> u32 {
            let at = (base - CHANNELS) as usize + offset as usize;
            u32::from_le_bytes(self.channels[at..at + 4].try_into().unwrap())
        }

        fn set_word(&mut self, base: u32, offset: u32, value: u32) {
            let at = (base - CHANNELS) as usize + offset as usize;
            self.channels[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn set_select(&mut self, channel: u32, level: u32, value: u32) {
            let at = (channel * SELECTS_STRIDE + level) as usize * 4;
            self.selects[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn register(&self, channel: u32) -> u32 {
            let at = channel as usize * 4;
            u32::from_le_bytes(self.registers[at..at + 4].try_into().unwrap())
        }

        fn set_register(&mut self, channel: u32, value: u32) {
            let at = channel as usize * 4;
            self.registers[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    #[derive(Default)]
    struct TestKernel {
        saved: Vec<u32>,
        restored: Vec<u32>,
        locks: u32,
        unlocks: u32,
    }

    impl CriticalKernel for TestKernel {
        fn core(&self) -> CpuCore { CpuCore::First }
        fn mask_interrupts(&mut self) -> SavedInterrupts { self.saved.push(0x40); SavedInterrupts(0x40) }
        fn restore_interrupts(&mut self, saved: SavedInterrupts) { self.restored.push(saved.0); }
        fn observe_global_lock(&mut self) -> LockStamp { LockStamp(0x11) }
        fn try_global_lock(&mut self, _: LockStamp) -> bool { true }
        fn release_global_lock(&mut self) {}
        fn nesting(&self) -> u32 { 0 }
        fn set_nesting(&mut self, _: u32) {}
    }

    impl RouteInterrupts for TestKernel {
        fn save_irq(&mut self) -> u32 { self.locks += 1; 0x55 }
        fn restore_irq(&mut self, saved: u32) { self.unlocks += 1; self.restored.push(saved); }
    }

    #[test]
    fn resolve_takes_the_highest_request_under_the_ceiling() {
        let mut memory = Memory::new();
        let channel = RouteChannel(3);
        let base = channel.base();
        memory.set_word(base, 0, 6);
        for index in 0..38u32 { memory.set_word(base, 4 + index * 4, index); }
        memory.with_context(|context| assert_eq!(helper_64a0bc(context, channel), Ok(6)));
        memory.set_word(base, 0, 100);
        memory.with_context(|context| assert_eq!(helper_64a0bc(context, channel), Ok(37)));
        memory.set_word(base, 4 + 7 * 4, 99);
        memory.with_context(|context| assert_eq!(helper_64a0bc(context, channel), Ok(99)));
        memory.set_word(base, 0, 0);
        memory.with_context(|context| assert_eq!(helper_64a0bc(context, channel), Ok(0)));
    }

    #[test]
    fn apply_rejects_an_unselected_level_without_writing() {
        let mut memory = Memory::new();
        let channel = RouteChannel(2);
        memory.set_word(channel.base(), 0x9c, 4);
        memory.with_context(|context| assert_eq!(helper_64a224(context, channel, 5), Ok(-30)));
        assert_eq!(memory.register(2), 0);
        assert_eq!(memory.word(channel.base(), 0x9c), 4);
    }

    #[test]
    fn apply_skips_a_level_the_register_already_holds() {
        let mut memory = Memory::new();
        let channel = RouteChannel(1);
        memory.set_select(1, 3, 5);
        memory.set_word(channel.base(), 0x9c, 3);
        memory.with_context(|context| assert_eq!(helper_64a224(context, channel, 3), Ok(0)));
        assert_eq!(memory.register(1), 0);
    }

    #[test]
    fn apply_replaces_the_low_three_bits_of_a_low_channel_register() {
        let mut memory = Memory::new();
        let channel = RouteChannel(4);
        memory.set_select(4, 7, 5);
        memory.set_register(4, 0x1234_ffff);
        memory.with_context(|context| {
            assert_eq!(helper_64a224(context, channel, 7), Ok(0));
            assert_eq!(context.effects.locks, 1);
            assert_eq!(context.effects.unlocks, 1);
        });
        assert_eq!(memory.register(4), 0x1234_fffd);
        assert_eq!(memory.word(channel.base(), 0x9c), 7);
    }

    #[test]
    fn apply_replaces_the_top_nibble_of_channel_nine() {
        let mut memory = Memory::new();
        let channel = RouteChannel(9);
        memory.set_select(9, 2, 3);
        memory.set_register(9, 0x9abc_def0);
        memory.with_context(|context| assert_eq!(helper_64a224(context, channel, 2), Ok(0)));
        assert_eq!(memory.register(9), 0xbabc_def0);
        assert_eq!(memory.word(channel.base(), 0x9c), 2);
    }

    #[test]
    fn apply_rejects_channels_past_the_register_block() {
        let mut memory = Memory::new();
        let channel = RouteChannel(10);
        memory.set_select(10, 1, 4);
        memory.with_context(|context| assert_eq!(helper_64a224(context, channel, 1), Ok(-30)));
        assert_eq!(memory.word(channel.base(), 0x9c), 0);
    }

    #[test]
    fn osal_lock_and_unlock_remain_the_critical_section_mechanism() {
        let mut kernel = TestKernel::default();
        let saved = interrupt_lock(&mut kernel);
        interrupt_unlock(&mut kernel, saved);
        assert_eq!(kernel.saved, vec![0x40]);
        assert_eq!(kernel.restored, vec![0x40]);
    }
}
