//! Profile scaling and keyed halfword selection (image 5, 0x00ef33b4-0x00ef4b18).

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69fec8;
use crate::record_profile_accessors::offset;

const CONFIGURATION_POINTER: u32 = 0x0096_ca50;
const KEY_TABLE: u32 = 0x00ef_5048;

fn read_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress) -> Result<u32, MemoryError> {
    context.read_u16(address).map(u32::from)
}

fn signed_divide(numerator: u32, denominator: u32) -> u32 {
    (numerator as i32).wrapping_div(denominator as i32) as u32
}

fn bit(position: u32) -> u32 {
    let shift = position & 0xff;
    if shift < 32 { 1u32 << shift } else { 0 }
}

pub fn helper_ef33b4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, values: GuestAddress, scale: u32) -> Result<(), MemoryError> {
    for at in [0x00u32, 0x02, 0x04] {
        let value = u64::from(read_halfword(context, offset(values, at))?);
        let product = u64::from(scale).wrapping_mul(value);
        context.write_u16(offset(values, at), (product / 100) as u16)?;
    }
    Ok(())
}

fn primary_key(kind: u32) -> Option<u32> {
    Some(match kind {
        0 => 0x7ed7,
        1 => 0x7ed5,
        2 => 0x7ed3,
        3 => 0x7ed1,
        4 => 0x7ecf,
        5 => 0x7ecd,
        6 => 0x7ecb,
        7 => 0x7ec9,
        8 => 0x7ec7,
        10 => 0x7eb5,
        11 => 0x7eb7,
        12 => 0x7eb9,
        13 => 0x7ebb,
        14 => 0x7ebd,
        15 => 0x7ebf,
        16 => 0x7ec1,
        17 => 0x7ec3,
        18 => 0x7ec5,
        _ => return None,
    })
}

fn secondary_key(kind: u32) -> Option<u32> {
    Some(match kind {
        0 => 0x7efb,
        1 => 0x7ef9,
        2 => 0x7ef7,
        3 => 0x7ef5,
        4 => 0x7ef3,
        5 => 0x7ef1,
        6 => 0x7eef,
        7 => 0x7eed,
        8 => 0x7eeb,
        10 => 0x7ed9,
        11 => 0x7edb,
        12 => 0x7edd,
        13 => 0x7edf,
        14 => 0x7ee1,
        15 => 0x7ee3,
        16 => 0x7ee5,
        17 => 0x7ee7,
        18 => 0x7ee9,
        _ => return None,
    })
}

pub fn helper_ef3f04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, second: u32, output: GuestAddress) -> Result<(), MemoryError> {
    context.write_u16(offset(output, 4), 1024)?;
    let value = match primary_key(kind) {
        Some(key) => helper_69fec8(context, key)? as u16,
        None => 1024,
    };
    context.write_u16(output, value)?;
    let value = match secondary_key(second) {
        Some(key) => helper_69fec8(context, key)? as u16,
        None => 1024,
    };
    context.write_u16(offset(output, 2), value)
}

pub fn helper_ef4220<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, output: GuestAddress) -> Result<(), MemoryError> {
    let count = crate::record_profile_mapping::helper_74677c(context)? & 0xff;
    let mut index = value & 0xff;
    let limit = count.wrapping_sub(1);
    if (index as i32) > (limit as i32) { index = limit & 0xff; }
    let entry = GuestAddress::new(KEY_TABLE + index * 4);
    let first = read_halfword(context, entry)?;
    let first = helper_69fec8(context, first)? as u16;
    context.write_u16(output, first)?;
    let second = read_halfword(context, offset(entry, 2))?;
    let second = helper_69fec8(context, second)? as u16;
    context.write_u16(offset(output, 2), second)
}

fn pair_keys(kind: u32) -> Option<(u32, u32)> {
    Some(match kind {
        1 => (0x80d1, 0x80d3),
        2 => (0x80d5, 0x80d7),
        3 => (0x80e9, 0x80eb),
        4 => (0x80d9, 0x80db),
        5 => (0x80dd, 0x80df),
        6 => (0x80e1, 0x80e3),
        7..=13 => (0x80e5, 0x80e7),
        14 => (0x80ed, 0x80ef),
        _ => return None,
    })
}

pub fn helper_ef4274<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, output: GuestAddress) -> Result<(), MemoryError> {
    match pair_keys(kind) {
        Some((first, second)) => {
            let first = helper_69fec8(context, first)? as u16;
            context.write_u16(output, first)?;
            let second = helper_69fec8(context, second)? as u16;
            context.write_u16(offset(output, 2), second)
        }
        None => {
            context.write_u16(output, 1024)?;
            context.write_u16(offset(output, 2), 1024)
        }
    }
}

pub fn helper_ef4600<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    mode: u32,
    values: GuestAddress,
    flags: Option<GuestAddress>,
    scale: u32,
) -> Result<(), MemoryError> {
    let index = mode & 0xff;
    let (low_one, low_two, target_v0, target_v1, target_v2, target) = match mode {
        0 => {
            let target = (scale.wrapping_mul(helper_69fec8(context, 0x7938)?) as i32).wrapping_div(5) as u16;
            (helper_69fec8(context, 0x7930)?, helper_69fec8(context, 0x7934)?, helper_69fec8(context, 0x792a)?, helper_69fec8(context, 0x7928)?, helper_69fec8(context, 0x792c)?, u32::from(target))
        }
        1 => {
            let target = (scale.wrapping_mul(helper_69fec8(context, 0x7936)?) as i32).wrapping_div(5) as u16;
            (helper_69fec8(context, 0x7930)?, helper_69fec8(context, 0x7934)?, helper_69fec8(context, 0x7924)?, helper_69fec8(context, 0x7922)?, helper_69fec8(context, 0x7926)?, u32::from(target))
        }
        2 => {
            let target_v1 = helper_69fec8(context, 0x7cba)?;
            let target_v2 = helper_69fec8(context, 0x7cbe)?;
            let low_two = helper_69fec8(context, 0x7cb4)?;
            let target_v0 = helper_69fec8(context, 0x7cb8)?;
            let _ = helper_69fec8(context, 0x7938)?;
            (0, low_two, target_v0, target_v1, target_v2, 0)
        }
        _ => (0, 256, 256, 0x7fff, 0x7fff, 0),
    };
    let first_phase = matches!(mode, 0 | 1);
    let mut flag_word = 0u32;
    if first_phase {
        let v0 = read_halfword(context, values)?;
        if v0 < target_v0 {
            if v0 == 0 {
                let v1 = read_halfword(context, offset(values, 2))?;
                context.write_u16(offset(values, 2), v1.wrapping_mul(target_v0) as u16)?;
                let v2 = read_halfword(context, offset(values, 4))?;
                context.write_u16(offset(values, 4), v2.wrapping_mul(target_v0) as u16)?;
            } else {
                let v1 = read_halfword(context, offset(values, 2))?;
                context.write_u16(offset(values, 2), signed_divide(v1.wrapping_mul(target_v0), v0) as u16)?;
                let v2 = read_halfword(context, offset(values, 4))?;
                context.write_u16(offset(values, 4), signed_divide(v2.wrapping_mul(target_v0), v0) as u16)?;
            }
            context.write_u16(values, target_v0 as u16)?;
            flag_word = bit(3 * index);
        }
        let v1 = read_halfword(context, offset(values, 2))?;
        if v1 < target_v1 {
            if v1 == 0 {
                let v0 = read_halfword(context, values)?;
                context.write_u16(values, v0.wrapping_mul(target_v1) as u16)?;
                let v2 = read_halfword(context, offset(values, 4))?;
                context.write_u16(offset(values, 4), v2.wrapping_mul(target_v1) as u16)?;
            } else {
                let v0 = read_halfword(context, values)?;
                context.write_u16(values, signed_divide(v0.wrapping_mul(target_v1), v1) as u16)?;
                let v2 = read_halfword(context, offset(values, 4))?;
                context.write_u16(offset(values, 4), signed_divide(v2.wrapping_mul(target_v1), v1) as u16)?;
            }
            context.write_u16(offset(values, 2), target_v1 as u16)?;
            flag_word |= bit(3 * index + 1);
        }
        let v2 = read_halfword(context, offset(values, 4))?;
        if v2 < target_v2 {
            if v2 == 0 {
                let v0 = read_halfword(context, values)?;
                context.write_u16(values, v0.wrapping_mul(target_v2) as u16)?;
                let v1 = read_halfword(context, offset(values, 2))?;
                context.write_u16(offset(values, 2), v1.wrapping_mul(target_v2) as u16)?;
            } else {
                let v0 = read_halfword(context, values)?;
                context.write_u16(values, signed_divide(v0.wrapping_mul(target_v2), v2) as u16)?;
                let v1 = read_halfword(context, offset(values, 2))?;
                context.write_u16(offset(values, 2), signed_divide(v1.wrapping_mul(target_v2), v2) as u16)?;
            }
            context.write_u16(offset(values, 4), target_v2 as u16)?;
            flag_word |= bit(3 * index + 2);
        }
        if flag_word != 0 {
            let v0 = read_halfword(context, values)?;
            if v0 > target {
                let v1 = read_halfword(context, offset(values, 2))?;
                context.write_u16(offset(values, 2), signed_divide(v1.wrapping_mul(target), v0) as u16)?;
                let v2 = read_halfword(context, offset(values, 4))?;
                context.write_u16(offset(values, 4), signed_divide(v2.wrapping_mul(target), v0) as u16)?;
                context.write_u16(values, target as u16)?;
                flag_word |= bit(3 * index + 3);
            }
            let v1 = read_halfword(context, offset(values, 2))?;
            if v1 < target_v1 {
                context.write_u16(offset(values, 2), target_v1 as u16)?;
                flag_word |= bit(3 * index + 4);
            }
            let v2 = read_halfword(context, offset(values, 4))?;
            if v2 < target_v2 {
                context.write_u16(offset(values, 4), target_v2 as u16)?;
                flag_word |= bit(3 * index + 5);
            }
        }
    }
    let tail = |context: &mut RecordContext<M, E>| -> Result<(u32, u32), MemoryError> {
        let mut result = 0u32;
        let v0 = read_halfword(context, values)?;
        if v0 >= 0x8000 {
            context.write_u16(values, 0x7fff)?;
            result |= bit(3 * index);
        }
        let v1 = read_halfword(context, offset(values, 2))?;
        if v1 > low_one {
            context.write_u16(offset(values, 2), low_one as u16)?;
            result |= bit(3 * index + 1);
        }
        let v2 = read_halfword(context, offset(values, 4))?;
        if v2 > low_two {
            context.write_u16(offset(values, 4), low_two as u16)?;
            result |= bit(3 * index + 2);
        }
        Ok((result, flag_word))
    };
    let (result, flags_word) = if first_phase {
        tail(context)?
    } else {
        let mut result = 0u32;
        let mut low_flags = 0u32;
        let v0 = read_halfword(context, values)?;
        if v0 >= 0x8000 {
            context.write_u16(values, 0x7fff)?;
            result |= bit(3 * index);
        } else if v0 <= 255 {
            context.write_u16(values, 256)?;
            low_flags |= bit(3 * index);
        }
        let v1 = read_halfword(context, offset(values, 2))?;
        if v1 > target_v1 {
            context.write_u16(offset(values, 2), target_v1 as u16)?;
            result |= bit(3 * index + 1);
        } else if v1 < low_two {
            context.write_u16(offset(values, 2), low_two as u16)?;
            low_flags |= bit(3 * index + 1);
        }
        let v2 = read_halfword(context, offset(values, 4))?;
        if v2 > target_v2 {
            context.write_u16(offset(values, 4), target_v2 as u16)?;
            result |= bit(3 * index + 2);
        } else if v2 < target_v0 {
            context.write_u16(offset(values, 4), target_v0 as u16)?;
            low_flags |= bit(3 * index + 2);
        }
        (result, low_flags)
    };
    if let Some(address) = flags {
        context.write_u32(address, result)?;
        context.write_u32(offset(address, 4), flags_word)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ef33b4-0x00ef4b18.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use std::vec;

    const VALUES: u32 = 0x0010_0000;
    const OUTPUT: u32 = 0x0010_1000;
    const FLAGS: u32 = 0x0010_2000;
    const CONFIGURATION: u32 = 0x0200_0000;
    const CONFIGURATION_BYTES: u32 = 0x3e0f84;
    const KEY_TABLE_BYTES: u32 = 0x20;

    struct Memory {
        values: vec::Vec<u8>,
        output: vec::Vec<u8>,
        flags: vec::Vec<u8>,
        pointer: vec::Vec<u8>,
        configuration: vec::Vec<u8>,
        keys: vec::Vec<u8>,
    }

    impl Memory {
        fn new() -> Self {
            Self {
                values: vec![0u8; 8],
                output: vec![0u8; 8],
                flags: vec![0u8; 8],
                pointer: vec![0u8; 4],
                configuration: vec![0u8; CONFIGURATION_BYTES as usize],
                keys: vec![0u8; KEY_TABLE_BYTES as usize],
            }
        }
        fn configure(&mut self, key: u32, value: u16) {
            self.pointer.copy_from_slice(&CONFIGURATION.to_le_bytes());
            self.configuration[key as usize..key as usize + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn value(&self, at: u32) -> u16 { u16::from_le_bytes(self.values[at as usize..at as usize + 2].try_into().unwrap()) }
        fn set_value(&mut self, at: u32, value: u16) { self.values[at as usize..at as usize + 2].copy_from_slice(&value.to_le_bytes()); }
        fn run<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(VALUES, 8), &mut self.values[..]).unwrap(),
                MemoryRegion::new(descriptor(OUTPUT, 8), &mut self.output[..]).unwrap(),
                MemoryRegion::new(descriptor(FLAGS, 8), &mut self.flags[..]).unwrap(),
                MemoryRegion::new(descriptor(CONFIGURATION_POINTER, 4), &mut self.pointer[..]).unwrap(),
                MemoryRegion::new(descriptor(CONFIGURATION, CONFIGURATION_BYTES), &mut self.configuration[..]).unwrap(),
                MemoryRegion::new(descriptor(KEY_TABLE, KEY_TABLE_BYTES), &mut self.keys[..]).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }
    }

    #[test]
    fn scaling_divides_each_halfword_product_by_one_hundred() {
        let mut memory = Memory::new();
        memory.set_value(0, 500);
        memory.set_value(2, 999);
        memory.set_value(4, 65535);
        memory.run(|context| assert_eq!(helper_ef33b4(context, GuestAddress::new(VALUES), 50), Ok(())));
        assert_eq!(memory.value(0), 250);
        assert_eq!(memory.value(2), 499);
        assert_eq!(memory.value(4), (50u64 * 65535 / 100) as u16);
    }

    #[test]
    fn the_three_halfword_selection_uses_the_keyed_values() {
        let mut memory = Memory::new();
        memory.configure(0x7ed7, 0x1111);
        memory.configure(0x7efb, 0x2222);
        memory.run(|context| assert_eq!(helper_ef3f04(context, 0, 0, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..2], &0x1111u16.to_le_bytes());
        assert_eq!(&memory.output[2..4], &0x2222u16.to_le_bytes());
        assert_eq!(&memory.output[4..6], &1024u16.to_le_bytes());
        memory.run(|context| assert_eq!(helper_ef3f04(context, 9, 9, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..2], &1024u16.to_le_bytes());
        assert_eq!(&memory.output[2..4], &1024u16.to_le_bytes());
        memory.run(|context| assert_eq!(helper_ef3f04(context, 19, 0xffff_ffff, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..2], &1024u16.to_le_bytes());
        assert_eq!(&memory.output[2..4], &1024u16.to_le_bytes());
    }

    #[test]
    fn the_indexed_keys_come_from_the_split_count_table() {
        let mut memory = Memory::new();
        memory.configure(0x3e0f82, 4);
        memory.configure(0x1000, 0x0a0b);
        memory.configure(0x1002, 0x0c0d);
        memory.keys[4..6].copy_from_slice(&0x1000u16.to_le_bytes());
        memory.keys[6..8].copy_from_slice(&0x1002u16.to_le_bytes());
        memory.run(|context| assert_eq!(helper_ef4220(context, 1, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..2], &0x0a0bu16.to_le_bytes());
        assert_eq!(&memory.output[2..4], &0x0c0du16.to_le_bytes());
    }

    #[test]
    fn the_indexed_keys_clamp_to_the_split_count() {
        let mut memory = Memory::new();
        memory.configure(0x3e0f82, 2);
        memory.configure(0x1000, 0x0a0b);
        memory.keys[4..6].copy_from_slice(&0x1000u16.to_le_bytes());
        memory.run(|context| assert_eq!(helper_ef4220(context, 9, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..2], &0x0a0bu16.to_le_bytes());
    }

    #[test]
    fn the_kind_pairs_and_the_default() {
        let mut memory = Memory::new();
        memory.configure(0x80d1, 0x0101);
        memory.configure(0x80d3, 0x0202);
        memory.configure(0x80e5, 0x0303);
        memory.configure(0x80e7, 0x0404);
        memory.configure(0x80ed, 0x0505);
        memory.configure(0x80ef, 0x0606);
        memory.run(|context| assert_eq!(helper_ef4274(context, 1, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..4], &[0x01, 0x01, 0x02, 0x02]);
        memory.run(|context| assert_eq!(helper_ef4274(context, 9, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..4], &[0x03, 0x03, 0x04, 0x04]);
        memory.run(|context| assert_eq!(helper_ef4274(context, 14, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..4], &[0x05, 0x05, 0x06, 0x06]);
        memory.run(|context| assert_eq!(helper_ef4274(context, 15, GuestAddress::new(OUTPUT)), Ok(())));
        assert_eq!(&memory.output[0..4], &[0x00, 0x04, 0x00, 0x04]);
    }

    fn scaled() -> Memory {
        let mut memory = Memory::new();
        memory.configure(0x7930, 100);
        memory.configure(0x7934, 200);
        memory.configure(0x792a, 1000);
        memory.configure(0x7928, 2000);
        memory.configure(0x792c, 3000);
        memory.configure(0x7938, 10);
        memory
    }

    #[test]
    fn mode_zero_normalizes_then_clamps_to_the_target_and_bounds() {
        let mut memory = scaled();
        memory.set_value(0, 500);
        memory.set_value(2, 1000);
        memory.set_value(4, 1500);
        memory.run(|context| assert_eq!(helper_ef4600(context, 0, GuestAddress::new(VALUES), Some(GuestAddress::new(FLAGS)), 50), Ok(())));
        assert_eq!(memory.value(0), 100);
        assert_eq!(memory.value(2), 100);
        assert_eq!(memory.value(4), 200);
        assert_eq!(u32::from_le_bytes(memory.flags[0..4].try_into().unwrap()), 6);
        assert_eq!(u32::from_le_bytes(memory.flags[4..8].try_into().unwrap()), 57);
    }

    #[test]
    fn mode_one_multiplies_when_the_first_value_is_zero() {
        let mut memory = Memory::new();
        memory.configure(0x7930, 10);
        memory.configure(0x7934, 20);
        memory.configure(0x7924, 1000);
        memory.configure(0x7922, 2000);
        memory.configure(0x7926, 3000);
        memory.configure(0x7936, 10);
        memory.set_value(0, 0);
        memory.set_value(2, 3);
        memory.set_value(4, 4);
        memory.run(|context| assert_eq!(helper_ef4600(context, 1, GuestAddress::new(VALUES), Some(GuestAddress::new(FLAGS)), 50), Ok(())));
        assert_eq!(memory.value(0), 100);
        assert_eq!(memory.value(2), 10);
        assert_eq!(memory.value(4), 20);
        assert_eq!(u32::from_le_bytes(memory.flags[0..4].try_into().unwrap()), 48);
        assert_eq!(u32::from_le_bytes(memory.flags[4..8].try_into().unwrap()), 456);
    }

    #[test]
    fn mode_two_saturates_the_first_value_and_clamps_the_pair() {
        let mut memory = Memory::new();
        memory.configure(0x7cba, 1000);
        memory.configure(0x7cbe, 2000);
        memory.configure(0x7cb4, 100);
        memory.configure(0x7cb8, 50);
        memory.set_value(0, 0x9000);
        memory.set_value(2, 500);
        memory.set_value(4, 2500);
        memory.run(|context| assert_eq!(helper_ef4600(context, 2, GuestAddress::new(VALUES), Some(GuestAddress::new(FLAGS)), 7), Ok(())));
        assert_eq!(memory.value(0), 0x7fff);
        assert_eq!(memory.value(2), 500);
        assert_eq!(memory.value(4), 2000);
        assert_eq!(u32::from_le_bytes(memory.flags[0..4].try_into().unwrap()), 64 | 256);
        assert_eq!(u32::from_le_bytes(memory.flags[4..8].try_into().unwrap()), 0);
    }

    #[test]
    fn mode_two_raises_a_small_first_value_to_two_hundred_fifty_six() {
        let mut memory = Memory::new();
        memory.configure(0x7cba, 1000);
        memory.configure(0x7cbe, 2000);
        memory.configure(0x7cb4, 100);
        memory.configure(0x7cb8, 50);
        memory.set_value(0, 200);
        memory.set_value(2, 500);
        memory.set_value(4, 2500);
        memory.run(|context| assert_eq!(helper_ef4600(context, 2, GuestAddress::new(VALUES), None, 7), Ok(())));
        assert_eq!(memory.value(0), 256);
        assert_eq!(memory.value(2), 500);
        assert_eq!(memory.value(4), 2000);
    }

    #[test]
    fn other_modes_use_the_fixed_bounds() {
        let mut memory = Memory::new();
        memory.set_value(0, 1000);
        memory.set_value(2, 0x8000);
        memory.set_value(4, 0);
        memory.run(|context| assert_eq!(helper_ef4600(context, 3, GuestAddress::new(VALUES), Some(GuestAddress::new(FLAGS)), 7), Ok(())));
        assert_eq!(memory.value(0), 1000);
        assert_eq!(memory.value(2), 0x7fff);
        assert_eq!(memory.value(4), 256);
        assert_eq!(u32::from_le_bytes(memory.flags[0..4].try_into().unwrap()), 1024);
        assert_eq!(u32::from_le_bytes(memory.flags[4..8].try_into().unwrap()), 2048);
    }
}
