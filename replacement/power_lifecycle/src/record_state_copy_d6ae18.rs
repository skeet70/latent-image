//! The 0x5cf4-byte state block at +0xbd468 and its source copy.
//!
//! Each body is the image-5 instruction sequence at the address in its name.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::offset;

const STATE_OFFSET: u32 = 0xbd468;
const STATE_BYTES: u32 = 0x5cf4;
const HEAD_MAGIC: u32 = 0x5354_5943;
const TAIL_MAGIC: u32 = 0x4c41_5943;
const TAIL_OFFSET: u32 = 0x5cf0;
const SAVED_WORDS: u32 = 11;
const SAVED_FIRST: u32 = 0x4c;
const CONSTANT_HEAD: u32 = 0xd6_ba44;
const CONSTANT_TAIL: u32 = 0xd6_ba4c;
const CONSTANT_BYTES: u32 = 4;
const RECORD_BYTES: u32 = 580;

fn fill_zero<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, length: u32) -> Result<(), MemoryError> {
    for index in 0..length {
        context.write_u8(destination.checked_add(index)?, 0)?;
    }
    Ok(())
}

/// The image-5 division by ten at 0x00d6af38-0x00d6af40 and 0x00d6af94-0x00d6afa4.
fn divide_by_ten(value: u32) -> u32 {
    (value as i32).wrapping_div(10) as u32
}

pub fn helper_d6ae18<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, state: GuestAddress, reset: u32, source: GuestAddress) -> Result<(), MemoryError> {
    let block = offset(state, STATE_OFFSET);
    if reset == 0 {
        let mut saved = [0u32; SAVED_WORDS as usize];
        for (index, value) in saved.iter_mut().enumerate() {
            *value = context.read_u32(offset(block, SAVED_FIRST + index as u32 * 4))?;
        }
        fill_zero(context, block, STATE_BYTES)?;
        for (index, value) in saved.into_iter().enumerate() {
            context.write_u32(offset(block, SAVED_FIRST + index as u32 * 4), value)?;
        }
        context.write_u32(block, HEAD_MAGIC)?;
        context.write_u32(offset(block, TAIL_OFFSET), TAIL_MAGIC)?;
    }
    let first = u32::from(context.read_u8(offset(source, 0x11b))?);
    context.write_u32(offset(block, 0x0c), first)?;
    let value = context.read_u32(offset(source, 0xf4))?;
    context.write_u32(offset(block, 0x10), value)?;
    let value = context.read_u32(offset(source, 0x138))?;
    context.write_u32(offset(block, 0x14), value)?;
    let value = context.read_u32(offset(source, 0xf0))?;
    context.write_u32(offset(block, 0x18), value)?;
    let value = divide_by_ten(context.read_u32(offset(source, 0x130))?);
    context.write_u32(offset(block, 0x1c), value)?;
    let value = u32::from(context.read_u8(offset(source, 0x199))?);
    context.write_u32(offset(block, 0x78), value)?;
    let value = context.read_u32(offset(source, 0x1bc))?;
    context.write_u32(offset(block, 0x2c), value)?;
    let value = u32::from(context.read_u16(offset(source, 0x1a0))?);
    context.write_u32(offset(block, 0x30), value)?;
    let value = u32::from(context.read_u8(offset(source, 0x1c4))?);
    context.write_u32(offset(block, 0x34), value)?;
    let value = u32::from(context.read_u8(offset(source, 0x1c5))?);
    context.write_u32(offset(block, 0x38), value)?;
    let value = u32::from(context.read_u8(offset(source, 0x1c6))?);
    context.write_u32(offset(block, 0x3c), value)?;
    let scaled = context.read_u32(offset(source, 0x188))?;
    if scaled != 0 {
        context.write_u32(offset(block, 0x20), divide_by_ten(scaled))?;
    }
    let value = u32::from(context.read_u8(offset(source, 0x178))?);
    if value != 0 {
        context.write_u32(offset(block, 0x24), value)?;
    }
    let value = u32::from(context.read_u8(offset(source, 0x190))?);
    if value != 0 {
        context.write_u32(offset(block, 0x28), value)?;
    }
    Ok(())
}

pub fn helper_d6afe4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(destination, GuestAddress::new(CONSTANT_HEAD), ByteCount(CONSTANT_BYTES))?;
    context.copy_bytes(offset(destination, CONSTANT_BYTES), GuestAddress::new(CONSTANT_TAIL), ByteCount(CONSTANT_BYTES))?;
    context.copy_bytes(offset(destination, 8), source, ByteCount(RECORD_BYTES))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00d6ae18 and 0x00d6afe4.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use std::vec;

    const STATE: u32 = 0x0010_0000;
    const SOURCE: u32 = 0x0020_0000;
    const DESTINATION: u32 = 0x0030_0000;
    const CONSTANTS: u32 = 0x00d6_ba44;

    fn region<'a>(base: u32, length: u32, bytes: &'a mut [u8]) -> MemoryRegion<'a> {
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap(), bytes).unwrap()
    }

    #[test]
    fn the_reset_zeroes_the_block_keeps_eleven_words_and_stamps_the_magics() {
        let mut state = vec![0xffu8; 0xc4000];
        let mut source = vec![0u8; 0x300];
        let block = STATE_OFFSET as usize;
        for index in 0..SAVED_WORDS as usize {
            state[block + 0x4c + index * 4..block + 0x50 + index * 4].copy_from_slice(&(0x100 + index as u32).to_le_bytes());
        }
        source[0x11b] = 0x21;
        source[0xf4..0xf8].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        source[0x138..0x13c].copy_from_slice(&0x0506_0708u32.to_le_bytes());
        source[0xf0..0xf4].copy_from_slice(&0x090a_0b0cu32.to_le_bytes());
        source[0x130..0x134].copy_from_slice(&304u32.to_le_bytes());
        source[0x199] = 0x31;
        source[0x1bc..0x1c0].copy_from_slice(&0x0d0e_0f10u32.to_le_bytes());
        source[0x1a0..0x1a2].copy_from_slice(&0x1234u16.to_le_bytes());
        source[0x1c4] = 0x41;
        source[0x1c5] = 0x42;
        source[0x1c6] = 0x43;
        source[0x188..0x18c].copy_from_slice(&250u32.to_le_bytes());
        source[0x178] = 7;
        source[0x190] = 9;
        let mut destination = vec![0u8; 0x300];
        let mut constants = vec![0u8; 12];
        constants[..4].copy_from_slice(&0xa1a2_a3a4u32.to_le_bytes());
        constants[8..12].copy_from_slice(&0xb1b2_b3b4u32.to_le_bytes());
        let mut regions = [
            region(STATE, 0xc4000, &mut state),
            region(SOURCE, 0x300, &mut source),
            region(DESTINATION, 0x300, &mut destination),
            region(CONSTANTS, 12, &mut constants),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_d6ae18(&mut context, GuestAddress::new(STATE), 0, GuestAddress::new(SOURCE)), Ok(()));
        assert_eq!(helper_d6afe4(&mut context, GuestAddress::new(DESTINATION), GuestAddress::new(SOURCE)), Ok(()));
        let word = |at: usize| u32::from_le_bytes(state[block + at..block + at + 4].try_into().unwrap());
        assert_eq!(word(0x00), HEAD_MAGIC);
        assert_eq!(word(TAIL_OFFSET as usize), TAIL_MAGIC);
        for index in 0..SAVED_WORDS as usize { assert_eq!(word(0x4c + index * 4), 0x100 + index as u32); }
        assert_eq!(state[block + 0x40], 0);
        assert_eq!(state[block + STATE_BYTES as usize], 0xff);
        assert_eq!(word(0x0c), 0x21);
        assert_eq!(word(0x10), 0x0102_0304);
        assert_eq!(word(0x14), 0x0506_0708);
        assert_eq!(word(0x18), 0x090a_0b0c);
        assert_eq!(word(0x1c), 30);
        assert_eq!(word(0x78), 0x31);
        assert_eq!(word(0x2c), 0x0d0e_0f10);
        assert_eq!(word(0x30), 0x1234);
        assert_eq!(word(0x34), 0x41);
        assert_eq!(word(0x38), 0x42);
        assert_eq!(word(0x3c), 0x43);
        assert_eq!(word(0x20), 25);
        assert_eq!(word(0x24), 7);
        assert_eq!(word(0x28), 9);
        assert_eq!(&destination[0..4], &0xa1a2_a3a4u32.to_le_bytes());
        assert_eq!(&destination[4..8], &0xb1b2_b3b4u32.to_le_bytes());
        assert_eq!(&destination[8..8 + RECORD_BYTES as usize], &source[..RECORD_BYTES as usize]);
        assert_eq!(destination[8 + RECORD_BYTES as usize], 0);
    }

    #[test]
    fn a_nonzero_reset_keeps_the_block_and_copies_the_fields() {
        let mut state = vec![0xffu8; 0xc4000];
        let mut source = vec![0u8; 0x300];
        let mut destination = vec![0u8; 0x300];
        let mut constants = vec![0u8; 12];
        source[0x11b] = 0x21;
        source[0x188] = 0;
        source[0x178] = 0;
        source[0x190] = 0;
        let mut regions = [
            region(STATE, 0xc4000, &mut state),
            region(SOURCE, 0x300, &mut source),
            region(DESTINATION, 0x300, &mut destination),
            region(CONSTANTS, 12, &mut constants),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_d6ae18(&mut context, GuestAddress::new(STATE), 1, GuestAddress::new(SOURCE)), Ok(()));
        let block = STATE_OFFSET as usize;
        assert_eq!(&state[block..block + 4], &[0xff; 4]);
        assert_eq!(&state[block + 0x20..block + 0x24], &[0xff; 4]);
        assert_eq!(&state[block + 0x24..block + 0x28], &[0xff; 4]);
        assert_eq!(&state[block + 0x28..block + 0x2c], &[0xff; 4]);
        assert_eq!(&state[block + 0x0c..block + 0x10], &0x21u32.to_le_bytes());
    }
}
