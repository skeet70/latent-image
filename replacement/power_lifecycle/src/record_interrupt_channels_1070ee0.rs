//! The interrupt-channel release at 0x0107_0ee0.

use crate::osal::{interrupt_lock, interrupt_unlock, CriticalKernel};
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const CHANNELS: u32 = 0x014d_db10;
pub const RELEASE: u32 = 0xfff1_4804;
const RELEASE_STRIDE: u32 = 0x80;

pub const INVALID_CHANNEL: i32 = -29;
pub const NOT_ATTACHED: i32 = -19;

/// 0x01070ee0
pub fn helper_1070ee0<M: GuestAddressSpace, E: CriticalKernel>(context: &mut RecordContext<M, E>, channel: u32) -> Result<i32, MemoryError> {
    if channel > 3 {
        return Ok(INVALID_CHANNEL);
    }
    let slot = GuestAddress::new(CHANNELS.wrapping_add(channel.wrapping_mul(4)));
    let saved = interrupt_lock(&mut context.effects);
    let attached = context.read_u32(slot)? != 0;
    interrupt_unlock(&mut context.effects, saved);
    if !attached {
        return Ok(NOT_ATTACHED);
    }
    context.write_u32(GuestAddress::new(CHANNELS.wrapping_add(channel.wrapping_add(4).wrapping_mul(4))), 0)?;
    context.write_u32(slot, 0)?;
    let release = GuestAddress::new(RELEASE.wrapping_add(channel.wrapping_mul(RELEASE_STRIDE)));
    let value = context.read_u32(release)?;
    context.write_u32(release, value & !1)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x0107_0ee0.
    use super::*;
    use crate::osal::{CpuCore, LockStamp, SavedInterrupts};
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    #[derive(Default)]
    struct Kernel {
        masked: u32,
        nesting: u32,
    }

    impl CriticalKernel for Kernel {
        fn core(&self) -> CpuCore { CpuCore::First }
        fn mask_interrupts(&mut self) -> SavedInterrupts { self.masked += 1; SavedInterrupts(0) }
        fn restore_interrupts(&mut self, _saved: SavedInterrupts) {}
        fn observe_global_lock(&mut self) -> LockStamp { LockStamp(0) }
        fn try_global_lock(&mut self, _stamp: LockStamp) -> bool { true }
        fn release_global_lock(&mut self) {}
        fn nesting(&self) -> u32 { self.nesting }
        fn set_nesting(&mut self, depth: u32) { self.nesting = depth; }
    }

    struct Memory {
        channels: [u8; 32],
        release: [u8; 0x200],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CHANNELS, 32), &mut memory.channels).unwrap(),
            MemoryRegion::new(descriptor(RELEASE, 0x200), &mut memory.release).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, Kernel::default()))
    }

    fn memory() -> Memory {
        Memory { channels: [0; 32], release: [0xff; 0x200] }
    }

    fn channel(memory: &Memory, index: usize) -> u32 {
        u32::from_le_bytes(memory.channels[index * 4..index * 4 + 4].try_into().unwrap())
    }

    fn register(memory: &Memory, index: usize) -> u32 {
        let at = index * 0x80;
        u32::from_le_bytes(memory.release[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn a_channel_above_three_is_rejected_before_the_lock() {
        let mut memory = memory();
        let (result, masked) = with_memory(&mut memory, |context| (helper_1070ee0(context, 4), context.effects.masked));
        assert_eq!(result, Ok(INVALID_CHANNEL));
        assert_eq!(masked, 0);
        assert_eq!(register(&memory, 3), 0xffff_ffff);
    }

    #[test]
    fn an_unattached_channel_reports_after_unlocking() {
        let mut memory = memory();
        let (result, kernel) = with_memory(&mut memory, |context| {
            let result = helper_1070ee0(context, 2);
            (result, (context.effects.masked, context.effects.nesting))
        });
        assert_eq!(result, Ok(NOT_ATTACHED));
        assert_eq!(kernel, (1, 0));
        assert_eq!(register(&memory, 2), 0xffff_ffff);
    }

    #[test]
    fn an_attached_channel_clears_both_table_words_and_the_register_bit() {
        let mut memory = memory();
        memory.channels[4..8].copy_from_slice(&0x00ab_cdefu32.to_le_bytes());
        memory.channels[20..24].copy_from_slice(&0x0000_0102u32.to_le_bytes());
        memory.release[0x80..0x84].copy_from_slice(&0x0000_0003u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_1070ee0(context, 1), Ok(0)));
        assert_eq!(channel(&memory, 1), 0);
        assert_eq!(channel(&memory, 5), 0);
        assert_eq!(register(&memory, 1), 2);
        assert_eq!(channel(&memory, 2), 0);
        assert_eq!(register(&memory, 2), 0xffff_ffff);
    }
}
