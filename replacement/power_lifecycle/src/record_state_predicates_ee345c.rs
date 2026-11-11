//! Predicates over the state words at 0x0169_8a50 and 0x0169_964c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const BLOCK: u32 = 0x0169_8a50;
const FLAGS: u32 = 0x0169_964c;

fn word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(BLOCK + offset))
}

fn clear_of<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offsets: &[u32]) -> Result<bool, MemoryError> {
    for offset in offsets {
        if word(context, *offset)? != 0 { return Ok(false); }
    }
    Ok(true)
}

fn zero_flag(value: u32) -> u32 { u32::from(value == 0) }

pub fn helper_ee345c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if !clear_of(context, &[0x04, 0x7a4, 0x100, 0x300, 0x500, 0x6b8])? { return Ok(0); }
    Ok(zero_flag(word(context, 0x7a0)?))
}

pub fn helper_ee3514<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if !clear_of(context, &[0x100, 0x7a4, 0x300, 0x500, 0x6b8])? { return Ok(0); }
    if word(context, 0xf4)? != 1 { return Ok(1); }
    Ok(zero_flag(word(context, 0x04)?))
}

pub fn helper_ee360c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if word(context, 0x500)? != 0 { return Ok(0); }
    Ok(zero_flag(word(context, 0x6b8)?))
}

pub fn helper_ee3be0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if !matches!(word(context, 0x84c)?, 4 | 1) { return Ok(0); }
    if !matches!(word(context, 0x500)?, 4 | 0) { return Ok(0); }
    Ok(u32::from(matches!(word(context, 0x6b8)?, 4 | 0)))
}

pub fn helper_ee5a04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if !clear_of(context, &[0x04, 0x7a4, 0x100, 0x300, 0x500])? { return Ok(0); }
    Ok(zero_flag(word(context, 0x6b8)?))
}

pub fn helper_ee5aa4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u32(GuestAddress::new(FLAGS + 0x04))? == 0 { return Ok(0); }
    if !clear_of(context, &[0x100, 0x7a4, 0x300, 0x500, 0x6b8])? { return Ok(0); }
    if word(context, 0xf4)? != 1 { return Ok(1); }
    Ok(zero_flag(word(context, 0x04)?))
}

pub fn helper_ee5bb4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u32(GuestAddress::new(FLAGS + 0x14))? == 0 { return Ok(0); }
    if !clear_of(context, &[0x100, 0x300, 0x500])? { return Ok(0); }
    Ok(zero_flag(word(context, 0x6b8)?))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ee345c-0x00ee5c38.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory { block: [u8; 0x860], flags: [u8; 0x18] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(BLOCK, 0x860), &mut memory.block).unwrap(),
            MemoryRegion::new(descriptor(FLAGS, 0x18), &mut memory.flags).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn set(memory: &mut Memory, offset: u32, value: u32) {
        memory.block[offset as usize..offset as usize + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn the_clear_predicates_read_every_guard_word() {
        let mut memory = Memory { block: [0; 0x860], flags: [0; 0x18] };
        set(&mut memory, 0x7a0, 0);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee345c(context), Ok(1));
            assert_eq!(helper_ee360c(context), Ok(1));
            assert_eq!(helper_ee5a04(context), Ok(1));
        });
        set(&mut memory, 0x500, 1);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee345c(context), Ok(0));
            assert_eq!(helper_ee360c(context), Ok(0));
            assert_eq!(helper_ee5a04(context), Ok(0));
        });
    }

    #[test]
    fn the_flag_block_gates_the_two_predicates() {
        let mut memory = Memory { block: [0; 0x860], flags: [0; 0x18] };
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee5aa4(context), Ok(0));
            assert_eq!(helper_ee5bb4(context), Ok(0));
        });
        memory.flags[0x04..0x08].copy_from_slice(&1u32.to_le_bytes());
        memory.flags[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee5aa4(context), Ok(1));
            assert_eq!(helper_ee5bb4(context), Ok(1));
        });
        set(&mut memory, 0xf4, 1);
        with_memory(&mut memory, |context| assert_eq!(helper_ee5aa4(context), Ok(1)));
        set(&mut memory, 0x04, 7);
        with_memory(&mut memory, |context| assert_eq!(helper_ee5aa4(context), Ok(0)));
        set(&mut memory, 0xf4, 2);
        with_memory(&mut memory, |context| assert_eq!(helper_ee5aa4(context), Ok(1)));
    }

    #[test]
    fn the_three_word_predicate_accepts_four_or_zero() {
        let mut memory = Memory { block: [0; 0x860], flags: [0; 0x18] };
        set(&mut memory, 0x84c, 4);
        set(&mut memory, 0x500, 0);
        set(&mut memory, 0x6b8, 4);
        with_memory(&mut memory, |context| assert_eq!(helper_ee3be0(context), Ok(1)));
        set(&mut memory, 0x84c, 1);
        set(&mut memory, 0x6b8, 0);
        with_memory(&mut memory, |context| assert_eq!(helper_ee3be0(context), Ok(1)));
        set(&mut memory, 0x84c, 2);
        with_memory(&mut memory, |context| assert_eq!(helper_ee3be0(context), Ok(0)));
        set(&mut memory, 0x84c, 1);
        set(&mut memory, 0x500, 2);
        with_memory(&mut memory, |context| assert_eq!(helper_ee3be0(context), Ok(0)));
        set(&mut memory, 0x500, 4);
        set(&mut memory, 0x6b8, 2);
        with_memory(&mut memory, |context| assert_eq!(helper_ee3be0(context), Ok(0)));
    }
}
