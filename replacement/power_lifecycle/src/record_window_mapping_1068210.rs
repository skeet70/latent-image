//! The window mapping at 0x0106_8210.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const BLOCK: u32 = 0x014d_d534;
const RAW: u32 = 0x5c4;
const SCALED: u32 = 0x5cc;
pub const LIMITS: u32 = 0x014d_8758;
pub const LEVEL_TABLE: u32 = 0x014d_ab80;
pub const LEVEL_INDEX: u32 = 0x4171_65b8;

const HORIZONTAL_GAIN: u32 = 0x0106_f350;
const HORIZONTAL_HALF: u32 = 0x014d_8754;
const HORIZONTAL_BASE: u32 = 0x014d_a32c;
const VERTICAL_GAIN: u32 = 0x0106_f398;
const VERTICAL_HALF: u32 = 0x014d_88c0;
const VERTICAL_BASE: u32 = 0x014d_ab78;
const HORIZONTAL_DIVISOR: u32 = 0x014d_a330;
const VERTICAL_DIVISOR: u32 = 0x014d_889c;

const HORIZONTAL_ORIGIN: u32 = 0x6640;
const VERTICAL_ORIGIN: u32 = 0x4c80;
const WIDTH_CEILING: u32 = 0xcc00;
const HEIGHT_CEILING: u32 = 0x9880;

/// Stock divides through 0x0004_d978, which traps on a zero divisor.
fn divide(numerator: u32, divisor: u32) -> u32 {
    numerator.checked_div(divisor).unwrap_or(0)
}

fn hundredths(value: u32) -> u16 {
    ((u64::from(value) * 0x51eb_851f) >> 37) as u16
}

fn mapped(base: u32, product: u32) -> u32 {
    let product = product as i32;
    let adjusted = if product < 0 { product.wrapping_add(127) } else { product };
    base.wrapping_add((adjusted >> 7) as u32)
}

fn halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress) -> Result<u32, MemoryError> {
    context.read_u16(address).map(u32::from)
}

fn halfword_at<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, base: GuestAddress, offset: u32) -> Result<u32, MemoryError> {
    halfword(context, base.checked_add(offset)?)
}

fn byte_at(context: &mut impl GuestAddressSpace, base: u32, offset: u32) -> Result<u32, MemoryError> {
    context.read_u8(GuestAddress::new(base.wrapping_add(offset))).map(u32::from)
}

fn word_at(context: &mut impl GuestAddressSpace, base: u32) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(base))
}

fn table_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, entry: u32, field: u32, offset: u32) -> Result<u32, MemoryError> {
    byte_at(context, BLOCK.wrapping_add(entry.wrapping_mul(4)).wrapping_add(field), offset)
}

fn write_halfword(context: &mut impl GuestAddressSpace, at: u32, value: u16) -> Result<(), MemoryError> {
    context.write_u16(GuestAddress::new(BLOCK.wrapping_add(at)), value)
}

fn fill<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, entry: u32, field: u32, scale: u32) -> Result<(), MemoryError> {
    let mut values = [0u32; 4];
    for (offset, value) in values.iter_mut().enumerate() {
        *value = table_byte(context, entry, field, offset as u32)?;
        write_halfword(context, RAW.wrapping_add(offset as u32 * 2), *value as u16)?;
    }
    for (offset, value) in values.into_iter().enumerate() {
        write_halfword(context, SCALED.wrapping_add(offset as u32 * 2), hundredths(value.wrapping_mul(scale)))?;
    }
    Ok(())
}

/// 0x01068210
pub fn helper_1068210<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    mode: u32,
    record: GuestAddress,
    window: GuestAddress,
    destination: GuestAddress,
    index: u32,
    kind: u8,
) -> Result<u32, MemoryError> {
    let width = halfword_at(context, record, 0)?;
    let height = halfword_at(context, record, 2)?;
    let top = halfword_at(context, record, 0x0a)?;
    let limits = GuestAddress::new(LIMITS.wrapping_add(index.wrapping_add(u32::from(kind) * 2).wrapping_mul(16)));
    let width_limit = halfword_at(context, limits, 8)?;
    if width < width_limit {
        return Ok(1);
    }
    let height_limit = halfword_at(context, limits, 0x0a)?;
    if height < height_limit {
        return Ok(1);
    }
    let width_extent = halfword_at(context, limits, 0x0c)?;
    if width.wrapping_add(width_extent) > WIDTH_CEILING {
        return Ok(1);
    }
    let height_extent = halfword_at(context, limits, 0x0e)?;
    if height.wrapping_add(height_extent) > HEIGHT_CEILING {
        return Ok(1);
    }
    let horizontal = width.wrapping_sub(width_limit) & 0xffff;
    let vertical = height.wrapping_sub(height_limit) & 0xffff;
    let horizontal_high = width_extent.wrapping_add(width_limit);
    let vertical_high = height_extent.wrapping_add(height_limit);
    let horizontal_gain = byte_at(context, HORIZONTAL_GAIN, 0)?.wrapping_mul(halfword(context, GuestAddress::new(HORIZONTAL_HALF))?);
    let horizontal_base = word_at(context, HORIZONTAL_BASE)?;
    let low_x = mapped(horizontal_base, horizontal.wrapping_sub(HORIZONTAL_ORIGIN).wrapping_mul(horizontal_gain));
    let high_x = mapped(
        horizontal_base,
        horizontal.wrapping_add(horizontal_high & 0xffff).wrapping_sub(HORIZONTAL_ORIGIN).wrapping_mul(horizontal_gain),
    );
    let vertical_gain = byte_at(context, VERTICAL_GAIN, 0)?.wrapping_mul(halfword(context, GuestAddress::new(VERTICAL_HALF))?);
    let vertical_base = word_at(context, VERTICAL_BASE)?;
    let low_y = mapped(vertical_base, vertical.wrapping_sub(VERTICAL_ORIGIN).wrapping_mul(vertical_gain));
    let high_y = mapped(
        vertical_base,
        vertical.wrapping_add(vertical_high & 0xffff).wrapping_sub(VERTICAL_ORIGIN).wrapping_mul(vertical_gain),
    );
    let horizontal_span = halfword_at(context, window, 8)?.wrapping_sub(1);
    let horizontal_divisor = halfword(context, GuestAddress::new(HORIZONTAL_DIVISOR))?;
    let vertical_span = halfword_at(context, window, 0x0a)?.wrapping_sub(1);
    let vertical_divisor = halfword(context, GuestAddress::new(VERTICAL_DIVISOR))?;
    let scaled_low_x = divide(low_x.wrapping_mul(horizontal_span), horizontal_divisor) >> 8;
    let scaled_low_y = divide(low_y.wrapping_mul(vertical_span), vertical_divisor) >> 8;
    let scaled_high_x = divide(high_x.wrapping_mul(horizontal_span), horizontal_divisor) >> 8;
    let scaled_high_y = divide(high_y.wrapping_mul(vertical_span), vertical_divisor) >> 8;
    let left = halfword_at(context, window, 0)?;
    if scaled_low_x < left {
        return Ok(1);
    }
    if scaled_high_x > left.wrapping_add(halfword_at(context, window, 4)?).wrapping_sub(1) {
        return Ok(1);
    }
    let top_bound = halfword_at(context, window, 2)?;
    if scaled_low_y < top_bound {
        return Ok(1);
    }
    if scaled_high_y > top_bound.wrapping_add(halfword_at(context, window, 6)?).wrapping_sub(1) {
        return Ok(1);
    }
    let offset = top.wrapping_sub(128);
    let scale = offset.wrapping_add(offset >> 31) >> 1 & 0xffff;
    let row = byte_at(context, record.get(), 0x10)?;
    let column = byte_at(context, record.get(), 0x11)?;
    match mode {
        0 => fill(context, row.wrapping_mul(12).wrapping_add(column), 0xb4, scale)?,
        1 => {
            let bias = word_at(context, LEVEL_INDEX)?.wrapping_mul(36);
            fill(context, row.wrapping_mul(12).wrapping_add(column).wrapping_add(bias), 0x144, scale)?
        }
        2 => fill(context, row.wrapping_mul(12).wrapping_add(column), 0x2f4, scale)?,
        3 | 4 => {
            let index = word_at(context, LEVEL_INDEX)?;
            let level = byte_at(context, LEVEL_TABLE.wrapping_add(index), 0x0b)?;
            let value = hundredths(level.wrapping_mul(scale));
            for offset in 0..4 {
                write_halfword(context, SCALED.wrapping_add(offset * 2), value)?;
            }
        }
        5 | 6 | 7 | 8 => {
            let index = word_at(context, LEVEL_INDEX)?;
            let level = byte_at(context, LEVEL_TABLE.wrapping_add(index), 0x0b)?;
            let value = u32::from(hundredths(level.wrapping_mul(scale)));
            let field = match mode {
                5 => 0x384,
                6 => 0x414,
                7 => 0x4a4,
                _ => 0x534,
            };
            fill(context, row.wrapping_mul(12).wrapping_add(column), field, value)?
        }
        _ => {}
    }
    let first = halfword(context, GuestAddress::new(BLOCK.wrapping_add(SCALED)))?;
    let second = halfword(context, GuestAddress::new(BLOCK.wrapping_add(SCALED + 2)))?;
    let third = halfword(context, GuestAddress::new(BLOCK.wrapping_add(SCALED + 4)))?;
    let fourth = halfword(context, GuestAddress::new(BLOCK.wrapping_add(SCALED + 6)))?;
    let result = [
        width.wrapping_sub(first) as u16,
        height.wrapping_sub(second) as u16,
        first.wrapping_add(third) as u16,
        second.wrapping_add(fourth) as u16,
    ];
    for (index, value) in result.into_iter().enumerate() {
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() {
            context.write_u8(destination.checked_add(index as u32 * 2 + offset as u32)?, byte)?;
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x0106_8210.
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const RECORD: u32 = 0x0020_0000;
    const WINDOW: u32 = 0x0020_1000;
    const DESTINATION: u32 = 0x0020_2000;

    struct Memory {
        record: [u8; 0x14],
        window: [u8; 0x10],
        destination: [u8; 8],
        limits: [u8; 0x100],
        horizontal_half: [u8; 2],
        horizontal_base: [u8; 4],
        horizontal_divisor: [u8; 4],
        horizontal_gain: [u8; 1],
        vertical_half: [u8; 2],
        vertical_divisor: [u8; 4],
        vertical_gain: [u8; 1],
        vertical: [u8; 0x20],
        index: [u8; 4],
        block: [u8; 0x600],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(RECORD, 0x14), &mut memory.record).unwrap(),
            MemoryRegion::new(descriptor(WINDOW, 0x10), &mut memory.window).unwrap(),
            MemoryRegion::new(descriptor(DESTINATION, 8), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(LIMITS, 0x100), &mut memory.limits).unwrap(),
            MemoryRegion::new(descriptor(HORIZONTAL_HALF, 2), &mut memory.horizontal_half).unwrap(),
            MemoryRegion::new(descriptor(HORIZONTAL_BASE, 4), &mut memory.horizontal_base).unwrap(),
            MemoryRegion::new(descriptor(HORIZONTAL_DIVISOR, 4), &mut memory.horizontal_divisor).unwrap(),
            MemoryRegion::new(descriptor(HORIZONTAL_GAIN, 1), &mut memory.horizontal_gain).unwrap(),
            MemoryRegion::new(descriptor(VERTICAL_HALF, 2), &mut memory.vertical_half).unwrap(),
            MemoryRegion::new(descriptor(VERTICAL_DIVISOR, 4), &mut memory.vertical_divisor).unwrap(),
            MemoryRegion::new(descriptor(VERTICAL_GAIN, 1), &mut memory.vertical_gain).unwrap(),
            MemoryRegion::new(descriptor(VERTICAL_BASE, 0x20), &mut memory.vertical).unwrap(),
            MemoryRegion::new(descriptor(LEVEL_INDEX, 4), &mut memory.index).unwrap(),
            MemoryRegion::new(descriptor(BLOCK, 0x600), &mut memory.block).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn sample() -> Memory {
        let mut memory = Memory {
            record: [0; 0x14],
            window: [0; 0x10],
            destination: [0; 8],
            limits: [0; 0x100],
            horizontal_half: [0x10, 0],
            horizontal_base: [0; 4],
            horizontal_divisor: [0; 4],
            horizontal_gain: [0x08],
            vertical_half: [0x10, 0],
            vertical_divisor: [0; 4],
            vertical_gain: [0x08],
            vertical: [0; 0x20],
            index: [0; 4],
            block: [0; 0x600],
        };
        let record = &mut memory.record;
        record[0..2].copy_from_slice(&0x66c0u16.to_le_bytes());
        record[2..4].copy_from_slice(&0x4d80u16.to_le_bytes());
        record[0x0a..0x0c].copy_from_slice(&0x0100u16.to_le_bytes());
        let window = &mut memory.window;
        window[4..6].copy_from_slice(&0x0200u16.to_le_bytes());
        window[6..8].copy_from_slice(&0x0200u16.to_le_bytes());
        window[8..10].copy_from_slice(&0x0400u16.to_le_bytes());
        window[0x0a..0x0c].copy_from_slice(&0x0400u16.to_le_bytes());
        memory.limits[0x0c..0x0e].copy_from_slice(&0x0200u16.to_le_bytes());
        memory.limits[0x0e..0x10].copy_from_slice(&0x0100u16.to_le_bytes());
        memory.horizontal_divisor[0..2].copy_from_slice(&0x0800u16.to_le_bytes());
        memory.vertical_divisor[0..2].copy_from_slice(&0x0800u16.to_le_bytes());
        memory.block[0xb4..0xb8].copy_from_slice(&[100, 50, 200, 10]);
        memory.block[0x384..0x388].copy_from_slice(&[100, 50, 200, 10]);
        memory.vertical[0x08 + 0x0b] = 100;
        memory
    }

    fn call(memory: &mut Memory, mode: u32) -> u32 {
        with_memory(memory, |context| {
            helper_1068210(
                context,
                mode,
                GuestAddress::new(RECORD),
                GuestAddress::new(WINDOW),
                GuestAddress::new(DESTINATION),
                0,
                0,
            )
            .unwrap()
        })
    }

    fn destination(memory: &Memory) -> [u16; 4] {
        core::array::from_fn(|index| u16::from_le_bytes(memory.destination[index * 2..index * 2 + 2].try_into().unwrap()))
    }

    fn block_halfwords(memory: &Memory, at: usize) -> [u16; 4] {
        core::array::from_fn(|index| u16::from_le_bytes(memory.block[at + index * 2..at + 2 + index * 2].try_into().unwrap()))
    }

    #[test]
    fn the_first_table_maps_four_bytes_and_scales_them() {
        let mut memory = sample();
        assert_eq!(call(&mut memory, 0), 0);
        assert_eq!(destination(&memory), [0x6680, 0x4d60, 192, 38]);
        assert_eq!(block_halfwords(&memory, 0x5c4), [100, 50, 200, 10]);
        assert_eq!(block_halfwords(&memory, 0x5cc), [64, 32, 128, 6]);
    }

    #[test]
    fn the_level_only_modes_scale_the_level_byte_into_all_four_words() {
        let mut memory = sample();
        assert_eq!(call(&mut memory, 3), 0);
        assert_eq!(destination(&memory), [0x6680, 0x4d40, 128, 128]);
        assert_eq!(block_halfwords(&memory, 0x5c4), [0, 0, 0, 0]);
        let mut memory = sample();
        assert_eq!(call(&mut memory, 4), 0);
        assert_eq!(destination(&memory), [0x6680, 0x4d40, 128, 128]);
    }

    #[test]
    fn the_level_scaled_modes_scale_the_table_bytes_by_the_level() {
        let mut memory = sample();
        assert_eq!(call(&mut memory, 5), 0);
        assert_eq!(destination(&memory), [0x6680, 0x4d60, 192, 38]);
        assert_eq!(block_halfwords(&memory, 0x5cc), [64, 32, 128, 6]);
        let mut memory = sample();
        assert_eq!(call(&mut memory, 6), 0);
        assert_eq!(block_halfwords(&memory, 0x5c4), [0, 0, 0, 0]);
        assert_eq!(block_halfwords(&memory, 0x5cc), [0, 0, 0, 0]);
    }

    #[test]
    fn an_unlisted_mode_keeps_the_words_it_finds() {
        let mut memory = sample();
        assert_eq!(call(&mut memory, 9), 0);
        assert_eq!(destination(&memory), [0x66c0, 0x4d80, 0, 0]);
        assert_eq!(block_halfwords(&memory, 0x5c4), [0, 0, 0, 0]);
        assert_eq!(block_halfwords(&memory, 0x5cc), [0, 0, 0, 0]);
    }

    #[test]
    fn the_input_bounds_reject_before_any_table_write() {
        let mut memory = sample();
        memory.limits[8..10].copy_from_slice(&0x7000u16.to_le_bytes());
        assert_eq!(call(&mut memory, 0), 1);
        assert_eq!(destination(&memory), [0; 4]);
        assert_eq!(block_halfwords(&memory, 0x5c4), [0, 0, 0, 0]);
        assert_eq!(block_halfwords(&memory, 0x5cc), [0, 0, 0, 0]);
    }
}
