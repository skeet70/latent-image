//! Configuration-keyed selection buckets, index tables and halfword totals, image-5 entries 0x00f1a3cc-0x00f1bca8.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const THRESHOLD_BASE: u32 = 0x1904;
const THRESHOLD_STEP: u32 = 2;
/// The table at 0x00f1_ed80 holds these six keys, three bytes apart.
const BUCKET_KEYS: [u32; 6] = [0x52d9, 0x52dc, 0x52df, 0x52e2, 0x52e5, 0x52e8];
const MODE_KEY: u32 = 0x52ec;

const ROW_TABLE: u32 = 0x016b_13c0;
const ROW_TABLE_COLUMN_FALLBACK: u32 = 0x016a_e448;
const ROW_FALLBACK_TABLE: u32 = 0x016a_edc8;
const ROW_FALLBACK_BOTH: u32 = 0x016a_d7a0;
const ROW_SIZE: u32 = 0x2f4;
const COLUMN_SIZE: u32 = 0xfc;
const LAYER_SIZE: u32 = 0x54;
const FALLBACK_COLUMN_SIZE: u32 = 0x8dc;

const WIDTH: u32 = 12;
const HEIGHT: u32 = 14;
const DEPTH: u32 = 16;
const TAIL: u32 = 26;

pub fn helper_f1a3cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    match crate::record_configuration::helper_69ff7c(context, MODE_KEY)? {
        100 => Ok(0),
        200 => Ok(1),
        300 => Ok(2),
        _ => Ok(7),
    }
}

fn bucket(index: u32, thresholds: [i32; 5]) -> u32 {
    let index = index as i32;
    if thresholds[0] > index { return 0; }
    if index < thresholds[1] { return 1; }
    if index < thresholds[2] { return 2; }
    if index < thresholds[3] { return 3; }
    if index >= thresholds[4] { 5 } else { 4 }
}

pub fn helper_f1a408<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, low: u32, high: u32, index: u32) -> Result<u32, MemoryError> {
    let mut thresholds = [0; 5];
    for (position, threshold) in thresholds.iter_mut().enumerate() {
        *threshold = crate::record_profile_globals::helper_772f28(context, 2 + position as u32)? as i32;
    }
    let key = BUCKET_KEYS[bucket(index, thresholds) as usize];
    let value = (u64::from(high) << 32) | u64::from(low);
    let below = |context: &mut RecordContext<M, E>, at: u32| -> Result<bool, MemoryError> {
        Ok(value < u64::from(crate::record_configuration::helper_69f8fc(context, at)?))
    };
    if below(context, key)? { return Ok(7); }
    if below(context, key + 1)? { return Ok(0); }
    if below(context, key + 2)? { return Ok(1); }
    Ok(2)
}

pub fn helper_f1ae20<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, row: u32, column: u32, fallback_row: u32, fallback_column: u32, layer: u32) -> Result<GuestAddress, MemoryError> {
    let layer_offset = layer.wrapping_mul(LAYER_SIZE);
    let address = if row <= 5 {
        if column <= 2 {
            ROW_TABLE.wrapping_add(row * ROW_SIZE + column * COLUMN_SIZE + layer_offset)
        } else {
            ROW_TABLE_COLUMN_FALLBACK.wrapping_add(row * COLUMN_SIZE + layer_offset)
        }
    } else {
        let fallback_row = fallback_row.min(2);
        let fallback_column = fallback_column.min(2);
        if column <= 2 {
            ROW_FALLBACK_TABLE.wrapping_add(column * FALLBACK_COLUMN_SIZE + fallback_row * ROW_SIZE + layer_offset + fallback_column * COLUMN_SIZE)
        } else {
            ROW_FALLBACK_BOTH.wrapping_add(layer_offset + fallback_column * COLUMN_SIZE + fallback_row * ROW_SIZE)
        }
    };
    Ok(GuestAddress::new(address))
}

pub fn helper_f1bca8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    let mut total = u32::from(context.read_u16(record.checked_add(WIDTH)?)?);
    total = total.wrapping_add(u32::from(context.read_u16(record.checked_add(HEIGHT)?)?));
    total = total.wrapping_add(u32::from(context.read_u16(record.checked_add(DEPTH)?)?));
    total = total.wrapping_add(u32::from(context.read_u16(record.checked_add(TAIL)?)?));
    Ok(total as u16 as i16 as i32 as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const POINTER: u32 = 0x0096_ca50;
    const CONFIGURATION: u32 = 0x0098_0000;
    const RECORD: u32 = 0x0169_d29c;

    struct Memory {
        pointer: [u8; 4],
        configuration: [u8; 0x6000],
        record: [u8; 0x40],
    }

    impl Memory {
        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(POINTER, 4), &mut self.pointer).unwrap(),
                MemoryRegion::new(descriptor(CONFIGURATION, 0x6000), &mut self.configuration).unwrap(),
                MemoryRegion::new(descriptor(RECORD, 0x40), &mut self.record).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }

        fn new() -> Self {
            let mut memory = Memory { pointer: [0; 4], configuration: [0; 0x6000], record: [0; 0x40] };
            memory.pointer.copy_from_slice(&CONFIGURATION.to_le_bytes());
            memory
        }

        fn set_byte(&mut self, at: u32, value: u8) { self.configuration[at as usize] = value; }
        fn set_halfword(&mut self, at: u32, value: u16) { self.configuration[at as usize..at as usize + 2].copy_from_slice(&value.to_le_bytes()); }
    }

    #[test]
    fn the_mode_key_maps_three_values_and_everything_else_to_seven() {
        let mut memory = Memory::new();
        for (value, expected) in [(100u16, 0u32), (200, 1), (300, 2), (0, 7), (99, 7), (301, 7)] {
            memory.set_halfword(MODE_KEY, value);
            memory.with_context(|context| assert_eq!(helper_f1a3cc(context), Ok(expected), "mode {value}"));
        }
    }

    #[test]
    fn the_index_ladder_clamps_into_six_buckets() {
        assert_eq!(bucket(5, [10, 20, 30, 40, 50]), 0);
        assert_eq!(bucket(15, [10, 20, 30, 40, 50]), 1);
        assert_eq!(bucket(25, [10, 20, 30, 40, 50]), 2);
        assert_eq!(bucket(35, [10, 20, 30, 40, 50]), 3);
        assert_eq!(bucket(45, [10, 20, 30, 40, 50]), 4);
        assert_eq!(bucket(55, [10, 20, 30, 40, 50]), 5);
        assert_eq!(bucket(10, [10, 20, 30, 40, 50]), 1);
        assert_eq!(bucket(20, [10, 20, 30, 40, 50]), 2);
    }

    #[test]
    fn the_bucket_bytes_classify_the_wide_value() {
        let mut memory = Memory::new();
        for index in 0..5u32 { memory.set_halfword(THRESHOLD_BASE + index * THRESHOLD_STEP, [10, 20, 30, 40, 50][index as usize]); }
        for key in BUCKET_KEYS {
            memory.set_byte(key, 10);
            memory.set_byte(key + 1, 20);
            memory.set_byte(key + 2, 30);
        }
        for (low, high, index, expected) in [(5u32, 0u32, 5u32, 7u32), (15, 0, 5, 0), (25, 0, 5, 1), (35, 0, 5, 2), (0, 1, 5, 2), (5, 1, 5, 2)] {
            memory.with_context(|context| assert_eq!(helper_f1a408(context, low, high, index), Ok(expected), "value {high}:{low}"));
        }
        memory.with_context(|context| assert_eq!(helper_f1a408(context, 15, 0, 15), Ok(0)));
        memory.with_context(|context| assert_eq!(helper_f1a408(context, 35, 0, 55), Ok(2)));
        memory.with_context(|context| assert_eq!(helper_f1a408(context, 15, 0, 55), Ok(0)));
        memory.with_context(|context| assert_eq!(helper_f1a408(context, 25, 0, 55), Ok(1)));
    }

    #[test]
    fn the_index_tables_take_the_four_branches() {
        let mut memory = Memory::new();
        memory.with_context(|context| {
            assert_eq!(helper_f1ae20(context, 1, 2, 0, 0, 1), Ok(GuestAddress::new(0x016b_13c0 + 0x2f4 + 2 * 0xfc + 0x54)));
            assert_eq!(helper_f1ae20(context, 5, 3, 0, 0, 0), Ok(GuestAddress::new(0x016a_e448 + 5 * 0xfc)));
            assert_eq!(helper_f1ae20(context, 6, 2, 1, 2, 0), Ok(GuestAddress::new(0x016a_edc8 + 2 * 0x8dc + 0x2f4 + 2 * 0xfc)));
            assert_eq!(helper_f1ae20(context, 6, 3, 1, 2, 1), Ok(GuestAddress::new(0x016a_d7a0 + 0x54 + 2 * 0xfc + 0x2f4)));
            assert_eq!(helper_f1ae20(context, 6, 3, 7, 9, 0), Ok(GuestAddress::new(0x016a_d7a0 + 2 * 0xfc + 2 * 0x2f4)));
        });
    }

    #[test]
    fn the_total_adds_four_halfwords_and_keeps_sixteen_bits() {
        let mut memory = Memory::new();
        for (at, value) in [(12u32, 0x0102u16), (14, 0x0304), (16, 0x0506), (26, 0x0708)] {
            memory.record[at as usize..at as usize + 2].copy_from_slice(&value.to_le_bytes());
        }
        memory.with_context(|context| assert_eq!(helper_f1bca8(context, GuestAddress::new(RECORD)), Ok(0x1014)));
        memory.record[12..14].copy_from_slice(&0x7fffu16.to_le_bytes());
        memory.record[14..16].copy_from_slice(&0x7fffu16.to_le_bytes());
        memory.record[16..18].copy_from_slice(&1u16.to_le_bytes());
        memory.record[26..28].copy_from_slice(&1u16.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f1bca8(context, GuestAddress::new(RECORD)), Ok(0)));
        memory.record[12..14].copy_from_slice(&0xffffu16.to_le_bytes());
        memory.record[14..16].copy_from_slice(&0u16.to_le_bytes());
        memory.record[16..18].copy_from_slice(&0u16.to_le_bytes());
        memory.record[26..28].copy_from_slice(&0u16.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f1bca8(context, GuestAddress::new(RECORD)), Ok(0xffff_ffff)));
    }
}
