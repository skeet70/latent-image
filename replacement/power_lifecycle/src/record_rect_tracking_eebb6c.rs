//! Stock-shaped record helpers over the 0x68-byte record arrays the image-5
//! bodies at these addresses walk.
//!
//! Each body is the instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_image_geometry_query::profiles::{helper_ef5ab8, helper_ef5cc4, helper_ef5d44};

/// Stride of the record array the bodies at 0x00eebc30-0x00eebd30 walk.
const RECORD_STRIDE: u32 = 0x68;

/// Image-5 lookup table the body at 0x00eebc08 indexes.
const LOOKUP_TABLE: u32 = 0x00ee_e480;

/// Profile word the body at 0x00eec3c8 queries beside its own profile.
const SECONDARY_PROFILE: u32 = 0x4171_778c;

fn record(records: GuestAddress, index: u32) -> Result<GuestAddress, MemoryError> {
    records.checked_add(index.wrapping_mul(RECORD_STRIDE))
}

/// 0x00eebb6c
pub fn helper_eebb6c<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, code: u32) -> Result<u32, MemoryError> {
    Ok(match code { 51 => 1, 52 => 2, _ => 0 })
}

/// 0x00eebb98
pub fn helper_eebb98<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    let high = context.read_u32(source.checked_add(4)?)?;
    let low = context.read_u32(source)?;
    context.write_u16(destination, high as u16)?;
    context.write_u16(destination.checked_add(2)?, low as u16)?;
    Ok(())
}

/// 0x00eebbbc
pub fn helper_eebbbc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    let first = context.read_u32(source)?;
    let second = context.read_u32(source.checked_add(4)?)?;
    context.write_u16(destination, first as u16)?;
    context.write_u16(destination.checked_add(2)?, second as u16)?;
    helper_eebb98(context, destination.checked_add(4)?, source.checked_add(8)?)
}

/// 0x00eebc08
pub fn helper_eebc08<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    if index > 8 { return Ok(3); }
    context.read_u32(GuestAddress::new(LOOKUP_TABLE).checked_add(index * 4)?)
}

/// 0x00eebc30
pub fn helper_eebc30<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, records: GuestAddress, delta: GuestAddress) -> Result<(), MemoryError> {
    let count = context.read_u32(records)?;
    for index in 0..count {
        let at = record(records, index)?;
        let horizontal = context.read_u32(delta)?;
        let vertical = context.read_u32(delta.checked_add(4)?)?;
        for (field, value) in [(4u32, horizontal), (8, vertical), (0x14, horizontal), (0x18, vertical)] {
            let previous = context.read_u32(at.checked_add(field)?)?;
            context.write_u32(at.checked_add(field)?, previous.wrapping_add(value))?;
        }
    }
    Ok(())
}

/// 0x00eebca4
pub fn helper_eebca4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, records: GuestAddress, first: GuestAddress, second: GuestAddress) -> Result<(), MemoryError> {
    let horizontal = (context.read_u32(first.checked_add(4)?)? as i32).wrapping_sub(context.read_u32(second.checked_add(0xc)?)? as i32) / 2;
    let vertical = (context.read_u32(first)? as i32).wrapping_sub(context.read_u32(second.checked_add(8)?)? as i32) / 2;
    let count = context.read_u32(records)?;
    for index in 0..count {
        let at = record(records, index)?;
        for (field, value) in [(4u32, horizontal), (8, vertical), (0x14, horizontal), (0x18, vertical)] {
            let previous = context.read_u32(at.checked_add(field)?)?;
            context.write_u32(at.checked_add(field)?, previous.wrapping_add(value as u32))?;
        }
    }
    Ok(())
}

/// 0x00eebd30
pub fn helper_eebd30<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, records: GuestAddress, numerator: GuestAddress, multiplier: GuestAddress) -> Result<(), MemoryError> {
    let divisor = context.read_u32(numerator.checked_add(4)?)?;
    let count = context.read_u32(records)?;
    if divisor == 0 || count == 0 { return Ok(()); }
    for index in 0..count {
        let at = record(records, index)?.checked_add(0x26)?;
        let value = context.read_u16(at)?;
        let scaled = context.read_u32(multiplier.checked_add(4)?)?.wrapping_mul(u32::from(value)) / divisor;
        context.write_u16(at, scaled as u16)?;
    }
    Ok(())
}

/// 0x00eebd8c
pub fn helper_eebd8c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: GuestAddress, second: GuestAddress) -> Result<(), MemoryError> {
    let first_left = context.read_u32(first)?;
    let first_top = context.read_u32(first.checked_add(4)?)?;
    let first_right = first_left.wrapping_add(context.read_u32(first.checked_add(0xc)?)?);
    let first_bottom = first_top.wrapping_add(context.read_u32(first.checked_add(8)?)?);
    let second_left = context.read_u32(second)?;
    let second_top = context.read_u32(second.checked_add(4)?)?;
    let second_width = context.read_u32(second.checked_add(0xc)?)?;
    let second_height = context.read_u32(second.checked_add(8)?)?;
    let second_right = second_width.wrapping_add(second_left);
    let second_bottom = second_top.wrapping_add(second_height);
    if first_left > second_left { context.write_u32(second, first_left)?; }
    if second_top < first_top { context.write_u32(second.checked_add(4)?, first_top)?; }
    let left = context.read_u32(second)?;
    let width = if second_right <= first_right { second_right.wrapping_sub(left) } else { first_right.wrapping_sub(left) };
    let top = context.read_u32(second.checked_add(4)?)?;
    let height = if second_bottom <= first_bottom { second_bottom.wrapping_sub(top) } else { first_bottom.wrapping_sub(top) };
    if second_width >= width {
        context.write_u32(second.checked_add(0xc)?, width)?;
    } else {
        let first_width = context.read_u32(first.checked_add(0xc)?)?;
        context.write_u32(second.checked_add(0xc)?, first_width)?;
        let first_left = context.read_u32(first)?;
        context.write_u32(second, first_left)?;
    }
    let stored_height = context.read_u32(second.checked_add(8)?)?;
    if height <= stored_height {
        context.write_u32(second.checked_add(8)?, height)?;
    } else {
        let first_height = context.read_u32(first.checked_add(8)?)?;
        context.write_u32(second.checked_add(8)?, first_height)?;
        let first_top = context.read_u32(first.checked_add(4)?)?;
        context.write_u32(second.checked_add(4)?, first_top)?;
    }
    let left = context.read_u32(second)?;
    let top = context.read_u32(second.checked_add(4)?)?;
    let first_left = context.read_u32(first)?;
    let first_top = context.read_u32(first.checked_add(4)?)?;
    context.write_u32(second, left.wrapping_sub(first_left))?;
    context.write_u32(second.checked_add(4)?, top.wrapping_sub(first_top))
}

/// 0x00eebe58
pub fn helper_eebe58<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(0) }

/// 0x00eebe70
pub fn helper_eebe70<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(1) }

/// 0x00eec300
pub fn helper_eec300<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, rectangle: GuestAddress, limit: GuestAddress) -> Result<(), MemoryError> {
    let horizontal = helper_ef5ab8(context, profile, 1)?;
    let vertical = helper_ef5cc4(context, profile, 1)?;
    let width = context.read_u32(rectangle.checked_add(0xc)?)?;
    let scaled = horizontal.wrapping_mul(width.wrapping_mul(3)) / vertical.wrapping_mul(4);
    if scaled < context.read_u32(limit.checked_add(8)?)? { context.write_u32(limit.checked_add(8)?, scaled)?; }
    if width < context.read_u32(limit.checked_add(0xc)?)? { context.write_u32(limit.checked_add(0xc)?, width)?; }
    context.write_u32(rectangle.checked_add(4)?, 0)?;
    context.write_u32(rectangle, 0)?;
    context.write_u32(rectangle.checked_add(8)?, scaled)?;
    let height = context.read_u32(limit.checked_add(8)?)?;
    context.write_u32(limit.checked_add(4)?, scaled.wrapping_sub(height) >> 1)?;
    let left = context.read_u32(rectangle.checked_add(0xc)?)?;
    let right = context.read_u32(limit.checked_add(0xc)?)?;
    context.write_u32(limit, left.wrapping_sub(right) >> 1)
}

/// 0x00eec3c8
pub fn helper_eec3c8<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, profile: u32, parameters: GuestAddress, size: GuestAddress, output: GuestAddress, fifth: GuestAddress,
) -> Result<(), MemoryError> {
    for offset in [0, 4, 8, 0xc] { let _ = context.read_u32(parameters.checked_add(offset)?)?; }
    let rectangle = helper_ef5d44(context, profile, 1)?;
    let horizontal = helper_ef5ab8(context, profile, 1)?;
    let vertical = helper_ef5cc4(context, profile, 1)?;
    let secondary = context.read_u32(GuestAddress::new(SECONDARY_PROFILE))?;
    let secondary_rectangle = helper_ef5d44(context, secondary, 1)?;
    let secondary_horizontal = helper_ef5ab8(context, secondary, 1)?;
    let limit = context.read_u32(size.checked_add(4)?)?;
    let used = rectangle[3].wrapping_mul(horizontal).wrapping_sub(secondary_rectangle[3].wrapping_mul(secondary_horizontal)) / horizontal;
    let available = limit - used.min(limit);
    let scaled = available.wrapping_mul(horizontal.wrapping_mul(3)) / vertical.wrapping_mul(4);
    if context.read_u32(parameters.checked_add(0xc)?)? == 0x0001_0001 {
        let height = u32::from(context.read_u16(parameters.checked_add(0xe)?)?).wrapping_mul(vertical);
        let width = horizontal.wrapping_mul(u32::from(context.read_u16(parameters.checked_add(0xc)?)?));
        context.write_u32(fifth.checked_add(0xc)?, scaled.wrapping_mul(height) / width)?;
        context.write_u32(fifth.checked_add(8)?, scaled)?;
    } else {
        context.write_u32(fifth.checked_add(0xc)?, available)?;
        let height = u32::from(context.read_u16(parameters.checked_add(0xc)?)?).wrapping_mul(horizontal);
        let width = u32::from(context.read_u16(parameters.checked_add(0xe)?)?).wrapping_mul(vertical);
        context.write_u32(fifth.checked_add(8)?, available.wrapping_mul(height) / width)?;
    }
    let bounded = available.min(limit);
    let scaled = scaled.min(context.read_u32(size)?);
    if bounded < context.read_u32(fifth.checked_add(0xc)?)? { context.write_u32(fifth.checked_add(0xc)?, bounded)?; }
    if scaled < context.read_u32(fifth.checked_add(8)?)? { context.write_u32(fifth.checked_add(8)?, scaled)?; }
    context.write_u32(output.checked_add(8)?, scaled)?;
    context.write_u32(output.checked_add(0xc)?, bounded)?;
    let height = context.read_u32(size)?;
    context.write_u32(output.checked_add(4)?, height.wrapping_sub(scaled) >> 1)?;
    let width = context.read_u32(size.checked_add(4)?)?;
    context.write_u32(output, width.wrapping_sub(bounded) >> 1)?;
    let top = context.read_u32(fifth.checked_add(8)?)?;
    context.write_u32(fifth.checked_add(4)?, scaled.wrapping_sub(top) >> 1)?;
    let right = context.read_u32(output.checked_add(0xc)?)?;
    let left = context.read_u32(fifth.checked_add(0xc)?)?;
    context.write_u32(fifth, right.wrapping_sub(left) >> 1)
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_eebb6c,
    helper_eebb6c_abi,
    (),
    (code: u32) -> u32,
    (code: u32),
    (code),
    crate::record_rect_tracking_eebb6c::helper_eebb6c,
    |value| value
);
stock_export!(
    xm1_power_record_helper_eebb98,
    helper_eebb98_abi,
    (),
    (destination: u32, source: u32) -> (),
    (destination: GuestAddress, source: GuestAddress),
    (GuestAddress::new(destination), GuestAddress::new(source)),
    crate::record_rect_tracking_eebb6c::helper_eebb98,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_eebbbc,
    helper_eebbbc_abi,
    (),
    (destination: u32, source: u32) -> (),
    (destination: GuestAddress, source: GuestAddress),
    (GuestAddress::new(destination), GuestAddress::new(source)),
    crate::record_rect_tracking_eebb6c::helper_eebbbc,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_eebc08,
    helper_eebc08_abi,
    (),
    (index: u32) -> u32,
    (index: u32),
    (index),
    crate::record_rect_tracking_eebb6c::helper_eebc08,
    |value| value
);
stock_export!(
    xm1_power_record_helper_eebc30,
    helper_eebc30_abi,
    (),
    (records: u32, delta: u32) -> (),
    (records: GuestAddress, delta: GuestAddress),
    (GuestAddress::new(records), GuestAddress::new(delta)),
    crate::record_rect_tracking_eebb6c::helper_eebc30,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_eebca4,
    helper_eebca4_abi,
    (),
    (records: u32, first: u32, second: u32) -> (),
    (records: GuestAddress, first: GuestAddress, second: GuestAddress),
    (GuestAddress::new(records), GuestAddress::new(first), GuestAddress::new(second)),
    crate::record_rect_tracking_eebb6c::helper_eebca4,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_eebd30,
    helper_eebd30_abi,
    (),
    (records: u32, numerator: u32, multiplier: u32) -> (),
    (records: GuestAddress, numerator: GuestAddress, multiplier: GuestAddress),
    (GuestAddress::new(records), GuestAddress::new(numerator), GuestAddress::new(multiplier)),
    crate::record_rect_tracking_eebb6c::helper_eebd30,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_eebd8c,
    helper_eebd8c_abi,
    (),
    (first: u32, second: u32) -> (),
    (first: GuestAddress, second: GuestAddress),
    (GuestAddress::new(first), GuestAddress::new(second)),
    crate::record_rect_tracking_eebb6c::helper_eebd8c,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_eebe58,
    helper_eebe58_abi,
    (),
    () -> u32,
    (),
    (),
    crate::record_rect_tracking_eebb6c::helper_eebe58,
    |value| value
);
stock_export!(
    xm1_power_record_helper_eebe70,
    helper_eebe70_abi,
    (),
    () -> u32,
    (),
    (),
    crate::record_rect_tracking_eebb6c::helper_eebe70,
    |value| value
);
stock_export!(
    xm1_power_record_helper_eec300,
    helper_eec300_abi,
    (),
    (profile: u32, rectangle: u32, limit: u32) -> (),
    (profile: u32, rectangle: GuestAddress, limit: GuestAddress),
    (profile, GuestAddress::new(rectangle), GuestAddress::new(limit)),
    crate::record_rect_tracking_eebb6c::helper_eec300,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_eec3c8,
    helper_eec3c8_abi,
    (),
    (profile: u32, parameters: u32, size: u32, output: u32, fifth: u32) -> (),
    (profile: u32, parameters: GuestAddress, size: GuestAddress, output: GuestAddress, fifth: GuestAddress),
    (profile, GuestAddress::new(parameters), GuestAddress::new(size), GuestAddress::new(output), GuestAddress::new(fifth)),
    crate::record_rect_tracking_eebb6c::helper_eec3c8,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at
    //! 0x00eebb6c-0x00eec3c8.
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const RECORDS: u32 = 0x0010_0000;
    const SCRATCH: u32 = 0x0020_0000;
    const RECTANGLE: u32 = 0x0030_0000;
    const LIMIT: u32 = 0x0030_1000;
    const OUTPUT: u32 = 0x0030_2000;
    const FIFTH: u32 = 0x0030_3000;
    const PROFILE_ROW: u32 = 0x00ef_6de4;
    const PROFILE_TABLE: u32 = 0x00ef_6aa4;
    const PROFILE_RECTANGLE: u32 = 0x00ef_71bc;
    const ACTIVE_RECTANGLE: u32 = 0x00ef_6994;

    struct Memory {
        records: [u8; 0x200],
        scratch: [u8; 0x100],
        rectangle: [u8; 0x20],
        limit: [u8; 0x20],
        output: [u8; 0x20],
        fifth: [u8; 0x20],
        table: [u8; 0x24],
        row: [u8; 0x30],
        profile_table: [u8; 0x60],
        profile_rectangle: [u8; 0x20],
        active: [u8; 0x20],
        secondary: [u8; 4],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(RECORDS, 0x200), &mut memory.records).unwrap(),
            MemoryRegion::new(descriptor(SCRATCH, 0x100), &mut memory.scratch).unwrap(),
            MemoryRegion::new(descriptor(RECTANGLE, 0x20), &mut memory.rectangle).unwrap(),
            MemoryRegion::new(descriptor(LIMIT, 0x20), &mut memory.limit).unwrap(),
            MemoryRegion::new(descriptor(OUTPUT, 0x20), &mut memory.output).unwrap(),
            MemoryRegion::new(descriptor(FIFTH, 0x20), &mut memory.fifth).unwrap(),
            MemoryRegion::new(descriptor(LOOKUP_TABLE, 0x24), &mut memory.table).unwrap(),
            MemoryRegion::new(descriptor(PROFILE_ROW, 0x30), &mut memory.row).unwrap(),
            MemoryRegion::new(descriptor(PROFILE_TABLE, 0x60), &mut memory.profile_table).unwrap(),
            MemoryRegion::new(descriptor(PROFILE_RECTANGLE, 0x20), &mut memory.profile_rectangle).unwrap(),
            MemoryRegion::new(descriptor(ACTIVE_RECTANGLE, 0x20), &mut memory.active).unwrap(),
            MemoryRegion::new(descriptor(SECONDARY_PROFILE, 4), &mut memory.secondary).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn memory() -> Memory {
        Memory {
            records: [0; 0x200], scratch: [0; 0x100], rectangle: [0; 0x20], limit: [0; 0x20],
            output: [0; 0x20], fifth: [0; 0x20], table: [0; 0x24], row: [0; 0x30],
            profile_table: [0; 0x60], profile_rectangle: [0; 0x20], active: [0; 0x20], secondary: [0; 4],
        }
    }

    /// Both profiles resolve to row 0.
    fn profiles(memory: &mut Memory) {
        memory.profile_table[0x24] = 5;
        memory.profile_table[0x25] = 3;
        memory.profile_table[0x26] = 0;
        memory.profile_table[0x27] = 0;
        memory.profile_table[0x28] = 0;
        memory.secondary.copy_from_slice(&1u32.to_le_bytes());
    }

    fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }

    fn put(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }

    #[test]
    fn code_mapping_and_lookup_table() {
        let mut memory = memory();
        for index in 0..9 { put(&mut memory.table, index * 4, index as u32); }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebb6c(context, 51), Ok(1));
            assert_eq!(helper_eebb6c(context, 52), Ok(2));
            assert_eq!(helper_eebb6c(context, 50), Ok(0));
            assert_eq!(helper_eebb6c(context, 53), Ok(0));
            assert_eq!(helper_eebc08(context, 0), Ok(0));
            assert_eq!(helper_eebc08(context, 8), Ok(8));
            assert_eq!(helper_eebc08(context, 9), Ok(3));
            assert_eq!(helper_eebc08(context, u32::MAX), Ok(3));
        });
    }

    #[test]
    fn record_copy_swaps_the_halfword_order() {
        let mut memory = memory();
        for index in 0..4 { put(&mut memory.scratch, index * 4, 0x1111_0000 + index as u32); }
        with_memory(&mut memory, |context| {
            let source = GuestAddress::new(SCRATCH);
            let destination = GuestAddress::new(RECTANGLE);
            assert_eq!(helper_eebb98(context, destination, source), Ok(()));
            assert_eq!(context.read_u16(destination), Ok(0x0001));
            assert_eq!(context.read_u16(destination.checked_add(2).unwrap()), Ok(0x0000));

            assert_eq!(helper_eebbbc(context, destination, source), Ok(()));
            assert_eq!(context.read_u16(destination), Ok(0x0000));
            assert_eq!(context.read_u16(destination.checked_add(2).unwrap()), Ok(0x0001));
            assert_eq!(context.read_u16(destination.checked_add(4).unwrap()), Ok(0x0003));
            assert_eq!(context.read_u16(destination.checked_add(6).unwrap()), Ok(0x0002));
        });
    }

    #[test]
    fn deltas_move_both_pairs_of_record_words() {
        let mut memory = memory();
        put(&mut memory.records, 0, 2);
        for index in 0..2 {
            let base = index as usize * 0x68;
            for field in [4usize, 8, 0x14, 0x18] { put(&mut memory.records, base + field, 100); }
        }
        put(&mut memory.scratch, 0, 7);
        put(&mut memory.scratch, 4, 0xffff_fffd);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebc30(context, GuestAddress::new(RECORDS), GuestAddress::new(SCRATCH)), Ok(()));
            for index in 0..2 {
                let base = GuestAddress::new(RECORDS).checked_add(index * 0x68).unwrap();
                assert_eq!(context.read_u32(base.checked_add(4).unwrap()), Ok(107));
                assert_eq!(context.read_u32(base.checked_add(8).unwrap()), Ok(97));
                assert_eq!(context.read_u32(base.checked_add(0x14).unwrap()), Ok(107));
                assert_eq!(context.read_u32(base.checked_add(0x18).unwrap()), Ok(97));
            }
        });
    }

    #[test]
    fn centred_delta_halves_the_difference_toward_zero() {
        let mut memory = memory();
        put(&mut memory.records, 0, 1);
        for field in [4usize, 8, 0x14, 0x18] { put(&mut memory.records, field, 1000); }
        put(&mut memory.scratch, 0, 30);
        put(&mut memory.scratch, 4, 30);
        put(&mut memory.scratch, 0x10, 20);
        put(&mut memory.scratch, 0x18, 25);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebca4(context, GuestAddress::new(RECORDS), GuestAddress::new(SCRATCH), GuestAddress::new(SCRATCH + 0x10)), Ok(()));
            assert_eq!(context.read_u32(GuestAddress::new(RECORDS + 4)), Ok(1015));
            assert_eq!(context.read_u32(GuestAddress::new(RECORDS + 8)), Ok(1002));
            assert_eq!(context.read_u32(GuestAddress::new(RECORDS + 0x14)), Ok(1015));
            assert_eq!(context.read_u32(GuestAddress::new(RECORDS + 0x18)), Ok(1002));
        });
    }

    #[test]
    fn scaling_divides_each_record_halfword() {
        let mut memory = memory();
        put(&mut memory.records, 0, 2);
        memory.records[0x26..0x28].copy_from_slice(&10u16.to_le_bytes());
        memory.records[0x68 + 0x26..0x68 + 0x28].copy_from_slice(&11u16.to_le_bytes());
        put(&mut memory.scratch, 4, 4);
        put(&mut memory.scratch, 0x10 + 4, 3);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebd30(context, GuestAddress::new(RECORDS), GuestAddress::new(SCRATCH), GuestAddress::new(SCRATCH + 0x10)), Ok(()));
            assert_eq!(context.read_u16(GuestAddress::new(RECORDS + 0x26)), Ok(7));
            assert_eq!(context.read_u16(GuestAddress::new(RECORDS + 0x68 + 0x26)), Ok(8));
        });
    }

    #[test]
    fn intersection_writes_the_clipped_rectangle_relative_to_the_first() {
        let mut memory = memory();
        for (index, value) in [0u32, 0, 100, 100].into_iter().enumerate() { put(&mut memory.scratch, index * 4, value); }
        for (index, value) in [10u32, 20, 200, 200].into_iter().enumerate() { put(&mut memory.scratch, 0x40 + index * 4, value); }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebd8c(context, GuestAddress::new(SCRATCH), GuestAddress::new(SCRATCH + 0x40)), Ok(()));
        });
        assert_eq!([word(&memory.scratch, 0x40), word(&memory.scratch, 0x44), word(&memory.scratch, 0x48), word(&memory.scratch, 0x4c)], [10, 20, 80, 90]);
    }

    #[test]
    fn disjoint_intersection_takes_the_first_extent() {
        let mut memory = memory();
        for (index, value) in [10u32, 20, 60, 50].into_iter().enumerate() { put(&mut memory.scratch, index * 4, value); }
        for (index, value) in [0u32, 0, 10, 10].into_iter().enumerate() { put(&mut memory.scratch, 0x40 + index * 4, value); }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebd8c(context, GuestAddress::new(SCRATCH), GuestAddress::new(SCRATCH + 0x40)), Ok(()));
        });
        assert_eq!([word(&memory.scratch, 0x40), word(&memory.scratch, 0x44), word(&memory.scratch, 0x48), word(&memory.scratch, 0x4c)], [0, 0, 60, 0]);
    }

    #[test]
    fn constant_bodies_return_zero_and_one() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebe58(context), Ok(0));
            assert_eq!(helper_eebe70(context), Ok(1));
        });
    }

    #[test]
    fn fitted_rectangle_clamps_to_the_limit_block() {
        let mut memory = memory();
        profiles(&mut memory);
        put(&mut memory.rectangle, 0xc, 120);
        put(&mut memory.limit, 8, 100);
        put(&mut memory.limit, 0xc, 100);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eec300(context, 0, GuestAddress::new(RECTANGLE), GuestAddress::new(LIMIT)), Ok(()));
        });
        assert_eq!(word(&memory.rectangle, 0), 0);
        assert_eq!(word(&memory.rectangle, 4), 0);
        assert_eq!(word(&memory.rectangle, 8), 150);
        assert_eq!(word(&memory.limit, 8), 100);
        assert_eq!(word(&memory.limit, 0xc), 100);
        assert_eq!(word(&memory.limit, 4), 25);
        assert_eq!(word(&memory.limit, 0), 10);
    }

    fn scaled_memory() -> Memory {
        let mut memory = memory();
        profiles(&mut memory);
        put(&mut memory.limit, 0, 1000);
        put(&mut memory.limit, 4, 600);
        memory
    }

    #[test]
    fn exact_size_parameters_scale_the_tall_side() {
        let mut memory = scaled_memory();
        put(&mut memory.scratch, 0xc, 0x0001_0001);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eec3c8(context, 0, GuestAddress::new(SCRATCH), GuestAddress::new(LIMIT), GuestAddress::new(OUTPUT), GuestAddress::new(FIFTH)), Ok(()));
        });
        assert_eq!(word(&memory.fifth, 8), 750);
        assert_eq!(word(&memory.fifth, 0xc), 450);
        assert_eq!(word(&memory.output, 8), 750);
        assert_eq!(word(&memory.output, 0xc), 600);
        assert_eq!(word(&memory.output, 4), 125);
        assert_eq!(word(&memory.output, 0), 0);
        assert_eq!(word(&memory.fifth, 4), 0);
        assert_eq!(word(&memory.fifth, 0), 75);
    }

    #[test]
    fn other_parameters_scale_the_wide_side() {
        let mut memory = scaled_memory();
        put(&mut memory.scratch, 0xc, 0);
        memory.scratch[0xc..0xe].copy_from_slice(&4u16.to_le_bytes());
        memory.scratch[0xe..0x10].copy_from_slice(&2u16.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eec3c8(context, 0, GuestAddress::new(SCRATCH), GuestAddress::new(LIMIT), GuestAddress::new(OUTPUT), GuestAddress::new(FIFTH)), Ok(()));
        });
        assert_eq!(word(&memory.fifth, 8), 750);
        assert_eq!(word(&memory.fifth, 0xc), 600);
        assert_eq!(word(&memory.output, 8), 750);
        assert_eq!(word(&memory.output, 0xc), 600);
        assert_eq!(word(&memory.output, 4), 125);
        assert_eq!(word(&memory.fifth, 4), 0);
        assert_eq!(word(&memory.fifth, 0), 0);
    }
}
