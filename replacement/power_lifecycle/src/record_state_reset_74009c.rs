//! State reset and per-entry initialization in the image-4 tables.
//!
//! Each body is the image-4 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69fec8;

const COMPARE_BLOCK: u32 = 0x0091_4db8;
const LOG_BLOCK: u32 = 0x40a6_c7e8;
const ENTRY_TABLE: u32 = 0x0095_c880;
const ENTRY_BYTES: u32 = 64;
const ENTRY_HEAD: u32 = 0x98;
const SCALE_KEY: u32 = 0x136b;

pub fn helper_74009c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let value = context.read_u32(GuestAddress::new(COMPARE_BLOCK + 0x20))?;
    let scaled = helper_69fec8(context, SCALE_KEY)?.wrapping_mul(10);
    if value != scaled { return Ok(1); }
    Ok(u32::from(context.read_u32(GuestAddress::new(COMPARE_BLOCK + 0x24))?.wrapping_sub(1) != 0))
}

pub fn helper_7405a0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(LOG_BLOCK), 0)?;
    for offset in (0x34..=0x40).step_by(4) {
        context.write_u32(GuestAddress::new(COMPARE_BLOCK + offset), 0)?;
    }
    Ok(())
}

pub fn helper_740eb0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<(), MemoryError> {
    let base = ENTRY_TABLE.wrapping_add(index.wrapping_shl(6)).wrapping_add(ENTRY_HEAD);
    context.write_u8(GuestAddress::new(base), 0)?;
    for offset in (4..24).step_by(4) {
        context.write_u32(GuestAddress::new(base + offset), 0)?;
    }
    for pair in 0..5 {
        context.write_u32(GuestAddress::new(base + 24 + pair * 8), 0)?;
        context.write_u32(GuestAddress::new(base + 28 + pair * 8), 3)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0074009c-0x00740f08.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIGURATION_BASE: u32 = 0x0200_0000;

    struct Memory { block: [u8; 0x48], log: [u8; 4], pointer: [u8; 4], configuration: [u8; 0x1400], entries: [u8; 0x100] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(COMPARE_BLOCK, 0x48), &mut memory.block).unwrap(),
            MemoryRegion::new(descriptor(LOG_BLOCK, 4), &mut memory.log).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION_BASE, 0x1400), &mut memory.configuration).unwrap(),
            MemoryRegion::new(descriptor(ENTRY_TABLE, 0x100), &mut memory.entries).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn configured(memory: &mut Memory, halfword: u16) {
        memory.pointer.copy_from_slice(&CONFIGURATION_BASE.to_le_bytes());
        memory.configuration[0x136b..0x136d].copy_from_slice(&halfword.to_le_bytes());
    }

    #[test]
    fn the_scaled_halfword_matches_only_at_the_expected_word() {
        let mut memory = Memory { block: [0; 0x48], log: [0; 4], pointer: [0; 4], configuration: [0; 0x1400], entries: [0; 0x100] };
        configured(&mut memory, 2);
        memory.block[0x20..0x24].copy_from_slice(&20u32.to_le_bytes());
        memory.block[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_74009c(context), Ok(0)));
        memory.block[0x24..0x28].copy_from_slice(&2u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_74009c(context), Ok(1)));
        memory.block[0x20..0x24].copy_from_slice(&21u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_74009c(context), Ok(1)));
    }

    #[test]
    fn the_reset_clears_the_log_and_four_block_words() {
        let mut memory = Memory { block: [0xff; 0x48], log: [0xff; 4], pointer: [0; 4], configuration: [0; 0x1400], entries: [0; 0x100] };
        with_memory(&mut memory, |context| assert_eq!(helper_7405a0(context), Ok(())));
        assert_eq!(&memory.log[..], &[0; 4]);
        assert_eq!(&memory.block[0x34..0x44], &[0; 0x10]);
        assert_eq!(&memory.block[0x30..0x34], &[0xff; 4]);
        assert_eq!(&memory.block[0x44..], &[0xff; 4]);
    }

    #[test]
    fn entry_initialization_writes_one_aligned_record() {
        let mut memory = Memory { block: [0; 0x48], log: [0; 4], pointer: [0; 4], configuration: [0; 0x1400], entries: [0xff; 0x100] };
        with_memory(&mut memory, |context| assert_eq!(helper_740eb0(context, 0), Ok(())));
        let base = 0x98;
        assert_eq!(memory.entries[base], 0);
        assert_eq!(&memory.entries[base + 4..base + 24], &[0; 20]);
        for pair in 0..5 {
            assert_eq!(&memory.entries[base + 24 + pair * 8..base + 28 + pair * 8], &[0; 4]);
            assert_eq!(&memory.entries[base + 28 + pair * 8..base + 32 + pair * 8], &3u32.to_le_bytes());
        }
        assert_eq!(&memory.entries[..base], &[0xff; 0x98]);
        assert_eq!(&memory.entries[base + 64..], &[0xff; 0x28]);
    }
}
