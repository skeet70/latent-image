//! Value classification over the installed image-4 tables.
//!
//! Each body is the image-4 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_mapping::helper_746864;

const BYTE_TABLE: u32 = 0x4081_84bc;
const WORD_TABLE: u32 = 0x4081_82bc;
const HALFWORD_TABLE: u32 = 0x4081_8238;

/// The image-4 division by ten at 0x0073fef8-0x0073ff0c: the high word of the
/// signed magic product is shifted logically rather than arithmetically.
fn divide_by_ten(value: u32) -> u32 {
    let product = (0x6666_6667u32 as i32 as i64).wrapping_mul(value as i32 as i64);
    let high = (product >> 32) as u32;
    let sign = (value as i32 >> 31) as u32;
    high.wrapping_shr(2).wrapping_sub(sign) & 0xff
}

pub fn helper_73fef0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    let index = divide_by_ten(value);
    if index <= 31 { return Ok(6); }
    let mut slot = 1u32;
    loop {
        let threshold = context.read_u8(GuestAddress::new(BYTE_TABLE + slot))?;
        if index < u32::from(threshold) { break; }
        slot = (slot + 1) & 0xff;
        if slot == 30 { break; }
    }
    Ok((slot + 6) & 0xff)
}

pub fn helper_73ff58<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    if value <= 524 {
        let lower = context.read_u32(GuestAddress::new(WORD_TABLE))?;
        let midpoint = 525u32.wrapping_add(lower) >> 1;
        return Ok(if value < midpoint { 0 } else { 1 });
    }
    let mut index = 2u32;
    loop {
        let bound = context.read_u32(GuestAddress::new(WORD_TABLE + index * 4))?;
        if bound > value {
            let lower = context.read_u32(GuestAddress::new(WORD_TABLE + (index - 1) * 4))?;
            let midpoint = bound.wrapping_add(lower) >> 1;
            return Ok(if value < midpoint { index - 1 } else { index });
        }
        index = (index + 1) & 0xff;
        if index & 0x80 != 0 { return Ok(127); }
    }
}

pub fn helper_73ffe4<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(match value {
        46 => 2,
        47 => 3,
        45 => 1,
        _ => 0,
    })
}

pub fn helper_740190<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, value: u32) -> Result<u32, MemoryError> {
    let class = helper_746864(context, value, bank)?;
    if class <= 103 { return Ok(0); }
    let mut index = 1u32;
    while index != 66 {
        let threshold = u32::from(context.read_u16(GuestAddress::new(HALFWORD_TABLE + index * 2))?);
        if class < threshold { return Ok(index); }
        index = (index + 1) & 0xff;
    }
    Ok(66)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0073fef0-0x007401e8.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory { bytes: [u8; 0x30], words: [u8; 0x200], halfwords: [u8; 0x84] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(BYTE_TABLE, 0x30), &mut memory.bytes).unwrap(),
            MemoryRegion::new(descriptor(WORD_TABLE, 0x200), &mut memory.words).unwrap(),
            MemoryRegion::new(descriptor(HALFWORD_TABLE, 0x84), &mut memory.halfwords).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn word(memory: &mut Memory, index: u32, value: u32) {
        memory.words[index as usize * 4..index as usize * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn byte_table_classification_returns_the_slot_plus_six() {
        let mut memory = Memory { bytes: [0; 0x30], words: [0; 0x200], halfwords: [0; 0x84] };
        memory.bytes[1] = 45;
        memory.bytes[2] = 50;
        with_memory(&mut memory, |context| {
            // 400/10 = 40 sits below the slot-1 threshold, 460/10 = 46 below slot 2.
            assert_eq!(helper_73fef0(context, 400), Ok(7));
            assert_eq!(helper_73fef0(context, 460), Ok(8));
            // 300/10 = 30 is at the cutoff, so no threshold is read.
            assert_eq!(helper_73fef0(context, 300), Ok(6));
            // 2550/10 = 255 exceeds every threshold and the scan ends at slot 30.
            assert_eq!(helper_73fef0(context, 2550), Ok(36));
        });
    }

    #[test]
    fn word_table_bisection_returns_the_bracket() {
        let mut memory = Memory { bytes: [0; 0x30], words: [0; 0x200], halfwords: [0; 0x84] };
        word(&mut memory, 0, 500);
        word(&mut memory, 1, 600);
        word(&mut memory, 2, 700);
        word(&mut memory, 3, 900);
        word(&mut memory, 4, 1000);
        word(&mut memory, 5, 2000);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_73ff58(context, 100), Ok(0));
            assert_eq!(helper_73ff58(context, 524), Ok(1));
            assert_eq!(helper_73ff58(context, 525), Ok(1));
            assert_eq!(helper_73ff58(context, 700), Ok(2));
            assert_eq!(helper_73ff58(context, 899), Ok(3));
            assert_eq!(helper_73ff58(context, 900), Ok(3));
        });
    }

    #[test]
    fn word_table_bisection_saturates_at_one_hundred_twenty_seven() {
        let mut memory = Memory { bytes: [0; 0x30], words: [0; 0x200], halfwords: [0; 0x84] };
        word(&mut memory, 2, 700);
        with_memory(&mut memory, |context| assert_eq!(helper_73ff58(context, 100_000), Ok(127)));
    }

    #[test]
    fn code_mapping_reads_the_three_known_codes() {
        let mut memory = Memory { bytes: [0; 0x30], words: [0; 0x200], halfwords: [0; 0x84] };
        with_memory(&mut memory, |context| {
            assert_eq!(helper_73ffe4(context, 45), Ok(1));
            assert_eq!(helper_73ffe4(context, 46), Ok(2));
            assert_eq!(helper_73ffe4(context, 47), Ok(3));
            assert_eq!(helper_73ffe4(context, 44), Ok(0));
            assert_eq!(helper_73ffe4(context, 48), Ok(0));
        });
    }

    /// The 0x00746864 call reads key 0x003f4600 + index for the class selector
    /// and the halfword at key (0x001f1298 + selector) * 2 for the class.
    fn bisect(class: u16, thresholds: &[(u32, u16)]) -> u32 {
        use std::vec;
        let mut halfwords = vec![0u8; 0x84];
        let mut configuration = vec![0u8; 0x3f_4610];
        configuration[0x3e0f82] = 1;
        configuration[0x3f4600] = 2;
        let key = (0x1f1298u32 + 2) * 2;
        configuration[key as usize..key as usize + 2].copy_from_slice(&class.to_le_bytes());
        let mut pointer = vec![0u8; 4];
        pointer.copy_from_slice(&0x0100_0000u32.to_le_bytes());
        let mut bytes = vec![0u8; 0x30];
        for (index, value) in thresholds {
            halfwords[*index as usize * 2..*index as usize * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(HALFWORD_TABLE, 0x84), &mut halfwords[..]).unwrap(),
            MemoryRegion::new(descriptor(0x0100_0000, 0x3f_4610), &mut configuration[..]).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut pointer[..]).unwrap(),
            MemoryRegion::new(descriptor(BYTE_TABLE, 0x30), &mut bytes[..]).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        helper_740190(&mut context, 0, 0).unwrap()
    }

    #[test]
    fn halfword_table_bisection_brackets_the_converted_class() {
        // 0x1234 = 4660 is above the 103 cutoff and between the two thresholds.
        assert_eq!(bisect(0x1234, &[(1, 4096), (2, 8192)]), 2);
        assert_eq!(bisect(0x1234, &[(1, 8192), (2, 16384)]), 1);
        assert_eq!(bisect(0x1234, &[]), 66);
    }

    #[test]
    fn a_class_at_or_below_one_hundred_three_maps_to_zero() {
        assert_eq!(bisect(103, &[(1, 0)]), 0);
    }
}
