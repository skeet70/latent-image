//! Configuration-derived reads and band classifications.
//!
//! Image-4 bodies 0x0069fffc-0x00747290 and image-5 bodies 0x00f73bc8-0x00fa7f80.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::{helper_69f8fc, helper_69fec8};
use crate::record_native_exports::stock_export;

/// The image-4 body at 0x0069fffc.
///
/// The stock body sign-extends the byte through a halfword, which leaves a
/// value in 0..=255 unchanged.
pub fn helper_69fffc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<i32, MemoryError> {
    Ok(helper_69f8fc(context, key)? as i32)
}

/// The word the image-4 body at 0x007407fc returns.
pub const HARDWARE_STATE: u32 = 0x40a6_c7e8;

/// The image-4 body at 0x007407fc.
pub fn helper_7407fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(HARDWARE_STATE))
}

const BAND_BYTE_LOW: u32 = 0x3e1a_b1;
const BAND_BYTE_MIDDLE: u32 = 0x3e1a_b2;
const BAND_BYTE_HIGH: u32 = 0x3e1a_b3;
const BAND_HALF_LOW: u32 = 0x3e1a_b5;
const BAND_HALF_MIDDLE: u32 = 0x3e1a_b7;
const BAND_HALF_HIGH: u32 = 0x3e1a_b9;

/// The image-4 body at 0x00747234.
pub fn helper_747234<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<i32, MemoryError> {
    let position = index.wrapping_add(1) as i32;
    if position <= helper_69f8fc(context, BAND_BYTE_LOW)? as i32 {
        return Ok(0);
    }
    if position <= helper_69f8fc(context, BAND_BYTE_MIDDLE)? as i32 {
        return Ok(1);
    }
    Ok(if position > helper_69f8fc(context, BAND_BYTE_HIGH)? as i32 { 3 } else { 2 })
}

/// The image-4 body at 0x00747290.
pub fn helper_747290<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<i32, MemoryError> {
    if value <= helper_69fec8(context, BAND_HALF_LOW)? {
        return Ok(0);
    }
    if value <= helper_69fec8(context, BAND_HALF_MIDDLE)? {
        return Ok(1);
    }
    Ok(if helper_69fec8(context, BAND_HALF_HIGH)? < value { 3 } else { 2 })
}

/// The selector word the image-5 body at 0x00fa7f80 gates on.
pub const SNAPSHOT_SELECTOR: u32 = 0x016d_8708;
const SNAPSHOT_FLAG_KEY: u32 = 0x0e4b;

/// The image-5 body at 0x00fa7f80.
pub fn helper_fa7f80<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u32(GuestAddress::new(SNAPSHOT_SELECTOR + 8))? != 1 {
        return Ok(0);
    }
    Ok(u32::from(helper_69f8fc(context, SNAPSHOT_FLAG_KEY)? != 0))
}

/// The image-5 body at 0x00f73bc8.
pub fn helper_f73bc8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u32, MemoryError> {
    Ok(match helper_69f8fc(context, key)? {
        4 => 2,
        2 => 1,
        _ => 0,
    })
}

/// The word rows the image-5 body at 0x00f73bf4 indexes.
pub const SELECTION_TABLE: u32 = 0x00f8_331c;

/// The image-5 body at 0x00f73bf4.
pub fn helper_f73bf4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32, index: u32) -> Result<u32, MemoryError> {
    let row = helper_69f8fc(context, key)?.wrapping_mul(3).wrapping_add(index);
    context.read_u32(GuestAddress::new(SELECTION_TABLE.wrapping_add(row.wrapping_mul(4))))
}

stock_export!(
    xm1_power_record_helper_69fffc,
    helper_69fffc_abi,
    (),
    (key: u32) -> i32,
    (key: u32),
    (key),
    crate::record_configuration_classification_69fffc::helper_69fffc,
    |value| value
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0069fffc-0x00747290 and
    //! the image-5 bodies at 0x00f73bc8-0x00fa7f80.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;

    struct Memory {
        pointer: [u8; 4],
        config: [u8; 0x20],
        hardware: [u8; 4],
        selector: [u8; 0x10],
        table: [u8; 0x20],
    }

    impl Memory {
        fn new() -> Self {
            Self { pointer: [0; 4], config: [0; 0x20], hardware: [0; 4], selector: [0; 0x10], table: [0; 0x20] }
        }
    }

    fn descriptor(base: u32, length: u32) -> RegionDescriptor {
        RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, 0x20), &mut memory.config).unwrap(),
            MemoryRegion::new(descriptor(HARDWARE_STATE, 4), &mut memory.hardware).unwrap(),
            MemoryRegion::new(descriptor(SNAPSHOT_SELECTOR, 0x10), &mut memory.selector).unwrap(),
            MemoryRegion::new(descriptor(SELECTION_TABLE, 0x20), &mut memory.table).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn publish(memory: &mut Memory, key: u32) {
        memory.pointer = CONFIG_DATA.wrapping_sub(key).to_le_bytes();
    }

    #[test]
    fn the_byte_wrapper_returns_the_configuration_byte() {
        let mut memory = Memory::new();
        memory.config[3] = 0x9a;
        publish(&mut memory, 0);
        with_memory(&mut memory, |context| assert_eq!(helper_69fffc(context, 3), Ok(0x9a)));
    }

    #[test]
    fn the_hardware_state_is_the_word_at_its_published_address() {
        let mut memory = Memory::new();
        memory.hardware = 0x0403_0201u32.to_le_bytes();
        with_memory(&mut memory, |context| assert_eq!(helper_7407fc(context), Ok(0x0403_0201)));
    }

    #[test]
    fn the_byte_bands_split_the_position_above_the_index() {
        let mut memory = Memory::new();
        publish(&mut memory, BAND_BYTE_LOW);
        memory.config[0] = 3;
        memory.config[1] = 6;
        memory.config[2] = 9;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_747234(context, 1), Ok(0));
            assert_eq!(helper_747234(context, 2), Ok(0));
            assert_eq!(helper_747234(context, 5), Ok(1));
            assert_eq!(helper_747234(context, 6), Ok(2));
            assert_eq!(helper_747234(context, 8), Ok(2));
            assert_eq!(helper_747234(context, 9), Ok(3));
        });
    }

    #[test]
    fn the_halfword_bands_split_the_value() {
        let mut memory = Memory::new();
        publish(&mut memory, BAND_HALF_LOW);
        memory.config[0..2].copy_from_slice(&0x0100u16.to_le_bytes());
        memory.config[2..4].copy_from_slice(&0x0200u16.to_le_bytes());
        memory.config[4..6].copy_from_slice(&0x0300u16.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_747290(context, 0x00ff), Ok(0));
            assert_eq!(helper_747290(context, 0x0100), Ok(0));
            assert_eq!(helper_747290(context, 0x0101), Ok(1));
            assert_eq!(helper_747290(context, 0x0200), Ok(1));
            assert_eq!(helper_747290(context, 0x0201), Ok(2));
            assert_eq!(helper_747290(context, 0x0300), Ok(2));
            assert_eq!(helper_747290(context, 0x0301), Ok(3));
        });
    }

    #[test]
    fn the_snapshot_flag_needs_the_selector() {
        let mut memory = Memory::new();
        publish(&mut memory, SNAPSHOT_FLAG_KEY);
        memory.config[0] = 7;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_fa7f80(context), Ok(0));
            context.write_u32(GuestAddress::new(SNAPSHOT_SELECTOR + 8), 1).unwrap();
            assert_eq!(helper_fa7f80(context), Ok(1));
            context.write_u32(GuestAddress::new(SNAPSHOT_SELECTOR + 8), 2).unwrap();
            assert_eq!(helper_fa7f80(context), Ok(0));
        });
    }

    #[test]
    fn the_selection_codes_map_four_and_two() {
        let mut memory = Memory::new();
        publish(&mut memory, 5);
        memory.config[0] = 4;
        memory.config[1] = 2;
        memory.config[2] = 3;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_f73bc8(context, 5), Ok(2));
            assert_eq!(helper_f73bc8(context, 6), Ok(1));
            assert_eq!(helper_f73bc8(context, 7), Ok(0));
        });
    }

    #[test]
    fn the_selection_rows_are_three_words_per_selection() {
        let mut memory = Memory::new();
        publish(&mut memory, 9);
        memory.config[0] = 1;
        for index in 0..8 {
            memory.table[index * 4..index * 4 + 4].copy_from_slice(&(0x7000 + index as u32).to_le_bytes());
        }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_f73bf4(context, 9, 0), Ok(0x7003));
            assert_eq!(helper_f73bf4(context, 9, 2), Ok(0x7005));
        });
    }
}
