//! Channel-pair resolution for mode seven, image-5 entry 0x00f1d6a0.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};

const MODE_SEVEN: u32 = 7;
const SNAPSHOT_WORDS: u32 = 12;
const ENTRY_OFFSET: u32 = 44;
const KIND_MASK: u32 = 0xff;

pub fn helper_f1d6a0<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, handle: u32, mode: u32, flag: u32, value: u32, kind: u32,
    entry: GuestAddress, mapped: GuestAddress, limit: GuestAddress,
) -> Result<u32, MemoryError> {
    context.write_u32(entry, 0)?;
    context.write_u32(mapped, 0)?;
    context.write_u16(limit, 0)?;
    if mode != MODE_SEVEN || kind & KIND_MASK != 0 { return Ok(0); }
    let snapshot = context.with_guest_frame(ByteCount(SNAPSHOT_WORDS * 4), StackAlignment::Word, |context, frame| {
        crate::record_shared_block_756720::helper_756840(context, handle, frame.base())?;
        context.read_u32(frame.base().checked_add(ENTRY_OFFSET)?)
    })?;
    let result = if flag == 0 {
        let second = crate::record_channel_comparison_f5fa24::helper_f5fa7c(context, handle, snapshot, mapped)?;
        if second | flag == 0 { 2 } else { 1 }
    } else {
        let first = crate::record_channel_comparison_f5fa24::helper_f5fa24(context, handle, snapshot, value)?;
        crate::record_channel_comparison_f5fa24::helper_f5fa7c(context, handle, snapshot, mapped)?;
        if first != 0 { 0 } else { 1 }
    };
    context.write_u32(entry, snapshot)?;
    let stored = context.read_u32(mapped)?;
    let bounded = crate::record_profile_mapping::helper_746a80(context, stored, snapshot)?;
    context.write_u16(limit, bounded as u16)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SHARED: u32 = 0x0091_904c;
    const POINTER: u32 = 0x0096_ca50;
    const STACK: u32 = 0x0010_0000;
    const ENTRY: u32 = 0x0080_0000;
    const MAPPED: u32 = 0x0080_0010;
    const LIMIT: u32 = 0x0080_0020;
    const HANDLE: u32 = u32::MAX;

    struct Memory {
        shared: [u8; 0xa00],
        pointer: [u8; 4],
        gate: [u8; 1],
        split: [u8; 1],
        entries: [u8; 2],
        limits: [u8; 4],
        stack: [u8; 0x100],
        out: [u8; 0x24],
    }

    impl Memory {
        fn new() -> Self {
            let mut memory = Memory {
                shared: [0; 0xa00], pointer: [0; 4], gate: [1], split: [3], entries: [0, 1], limits: [0; 4],
                stack: [0; 0x100], out: [0; 0x24],
            };
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
                MemoryRegion::new(descriptor(ENTRY, 0x24), &mut self.out).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x100)).unwrap();
            body(&mut RecordContext::with_stack(space, (), stack))
        }

        fn word(&self, at: u32) -> u32 { u32::from_le_bytes(self.out[at as usize..at as usize + 4].try_into().unwrap()) }
        fn half(&self, at: u32) -> u16 { u16::from_le_bytes(self.out[at as usize..at as usize + 2].try_into().unwrap()) }
    }

    fn run(memory: &mut Memory, mode: u32, flag: u32, value: u32, kind: u32) -> Result<u32, MemoryError> {
        memory.with_context(|context| helper_f1d6a0(
            context, HANDLE, mode, flag, value, kind, GuestAddress::new(ENTRY), GuestAddress::new(MAPPED), GuestAddress::new(LIMIT),
        ))
    }

    #[test]
    fn another_mode_or_kind_clears_the_three_outputs_and_returns_zero() {
        let mut memory = Memory::new();
        memory.out[..4].copy_from_slice(&9u32.to_le_bytes());
        assert_eq!(run(&mut memory, 6, 0, 0, 0), Ok(0));
        assert_eq!(memory.word(0), 0);
        assert_eq!(memory.word(0x10), 0);
        assert_eq!(memory.half(0x20), 0);
        memory.out[..4].copy_from_slice(&9u32.to_le_bytes());
        assert_eq!(run(&mut memory, 7, 0, 0, 1), Ok(0));
        assert_eq!(memory.word(0), 0);
    }

    #[test]
    fn mode_seven_stores_the_snapshot_entry_and_the_bounded_value() {
        let mut memory = Memory::new();
        memory.shared[0xb0 + 44..0xb0 + 48].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(run(&mut memory, 7, 0, 0, 0), Ok(2));
        assert_eq!(memory.word(0), 0);
        assert_eq!(memory.word(0x10), 0);
        assert_eq!(memory.half(0x20), 0);
    }

    #[test]
    fn a_nonzero_flag_answers_with_the_first_comparison() {
        let mut memory = Memory::new();
        assert_eq!(run(&mut memory, 7, 1, 0, 0), Ok(1));
        assert_eq!(memory.half(0x20), 0);
        assert_eq!(run(&mut memory, 7, 1, 1, 0), Ok(0));
    }
}
