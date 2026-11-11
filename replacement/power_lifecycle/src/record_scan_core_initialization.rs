//! Byte-derived initialization and sample lifetime operations at f8cc9c..f8d8c8.

use super::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use super::record_scan_core_memory::{ScanMemoryAddress, SAMPLE_STATE};

const GUARD: u32 = 0xfdfd_fdfd;

fn guard_rows(memory: &mut impl GuestAddressSpace, base: GuestAddress, rows: u32, stride: u32) -> Result<(), MemoryError> {
    for row in 0..rows { base.checked_add(row * stride)?.set_word(GUARD, memory)?; }
    Ok(())
}

pub fn helper_f8cc9c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: GuestAddress) -> Result<i32, MemoryError> {
    if request.get() == 0 { return Ok(0x84); }
    for offset in [0, 4, 8, 12, 32] {
        if request.checked_add(offset)?.word(context)? == 0 { return Ok(0x84); }
    }
    SAMPLE_STATE.set_word(0, context)?;
    for (input, output, bytes) in [(8, 0x134, 0x98), (12, 0x1cc, 0xc4), (16, 0x290, 0x6c), (20, 0x2fc, 0x18), (24, 0x314, 4), (28, 0x318, 0x90), (32, 0x3a8, 0x218)] {
        let source = request.checked_add(input)?.address(context)?;
        SAMPLE_STATE.checked_add(output)?.copy_from(source, ByteCount(bytes), context)?;
    }
    SAMPLE_STATE.checked_add(9)?.fill(0, ByteCount(0xc4), context)?;
    let scratch = request.address(context)?;
    SAMPLE_STATE.checked_add(4)?.set_byte(0, context)?;
    SAMPLE_STATE.checked_add(5)?.set_byte(0, context)?;
    SAMPLE_STATE.checked_add(0x130)?.set_word(3, context)?;
    SAMPLE_STATE.checked_add(6)?.set_byte(0, context)?;
    SAMPLE_STATE.checked_add(0x5c0)?.set_word(scratch.get(), context)?;
    let work = request.checked_add(4)?.word(context)?;
    let work = GuestAddress::new(if work & 3 == 0 { work } else { (work & !3).wrapping_add(4) });
    SAMPLE_STATE.checked_add(0x6cc)?.set_word(work.get(), context)?;
    for (field, offset) in [(0x690, 4), (0x694, 0x1d0a4), (0x698, 0x21f64), (0x69c, 0x181e4), (0x6a0, 0x336f4), (0x6a4, 0x346b4), (0x6a8, 0x35984), (0x6b4, 0x406d4), (0x6bc, 0x26e24), (0x6c0, 0x29194), (0x6c4, 0x2d874), (0x6c8, 0x2ea2c), (0x6b8, 0x2ec24), (0x6d0, 0x44c84), (0x6ac, 0x491a0)] {
        SAMPLE_STATE.checked_add(field)?.set_word(scratch.checked_add(offset)?.get(), context)?;
    }
    for bank in 0..2 { guard_rows(context, scratch.checked_add(4 + bank * 0xc0f0)?, 49, 0x3f0)?; }
    for row in 0..10 {
        scratch.checked_add(0x181e4 + row * 0x3f0)?.set_word(GUARD, context)?;
        scratch.checked_add(0x181e4 + 0x2760 + row * 0x3f0)?.set_word(GUARD, context)?;
    }
    for bank in 0..2 {
        for group in 0..2 {
            for row in 0..5 {
                let offset = bank * 0x2760 + group * 0x13b0 + row * 0x3f0;
                scratch.checked_add(0x1d0a4 + offset)?.set_word(GUARD, context)?;
                scratch.checked_add(0x21f64 + offset)?.set_word(GUARD, context)?;
            }
        }
    }
    guard_rows(context, scratch.checked_add(0x406d4)?, 10, 0x3f0)?;
    for group in 0..3 { guard_rows(context, scratch.checked_add(0x35984 + group * 0xfc0)?, 4, 0x3f0)?; }
    for offset in [0x388c4, 0x389c4, 0x38ac4, 0x38bc4] { scratch.checked_add(offset)?.set_byte(0xfd, context)?; }
    for bank in 0..2 {
        for row in 0..9 {
            scratch.checked_add(0x26e24 + bank * 0x11b8 + row * 0x1f8)?.set_half(0xfdfd, context)?;
            scratch.checked_add(0x29194 + bank * 0x2370 + row * 0x3f0)?.set_word(GUARD, context)?;
        }
    }
    for row in 0..9 { scratch.checked_add(0x2d874 + row * 0x1f8)?.set_half(0xfdfd, context)?; }
    scratch.checked_add(0x2ea2c)?.set_half(0xfdfd, context)?;
    for row in 0..4 {
        scratch.checked_add(0x336f4 + row * 0x3f0)?.set_word(GUARD, context)?;
        scratch.checked_add(0x346b4 + row * 0x3f0)?.set_word(GUARD, context)?;
    }
    for row in 1..8 { scratch.checked_add(0x2ec24 + row * 0x3f0)?.set_word(GUARD, context)?; }
    guard_rows(context, scratch.checked_add(0x2ec24 + 0x1f80)?, 10, 0x3f0)?;
    scratch.checked_add(0x2ec24 + 0x46e0)?.set_word(GUARD, context)?;
    for bank in 0..2 {
        guard_rows(context, scratch.checked_add(0x491a0 + bank * 0x3720)?, 14, 0x3f0)?;
        scratch.checked_add(0x4fff0 + bank * 0x10)?.fill(0, ByteCount(14), context)?;
        scratch.checked_add(0x4fffe + bank * 0x10)?.set_byte(0xfd, context)?;
    }
    scratch.checked_add(0x50010)?.set_word(0, context)?;
    for index in 0..9 { work.checked_add(0x1554 + index * 0x18)?.copy_from(GuestAddress::new(0x00f9_5ca0), ByteCount(0x18), context)?; }
    for (offset, value, bytes) in [(0, 0, 0x2a8), (0x6a4, 0, 0x2a8), (0x2a8, 0xff, 0x2a8), (0x94c, 0xff, 0x2a8), (0x550, 0, 0xaa), (0x5fa, 0, 0xaa), (0xbf4, 0, 0xaa), (0xc9e, 0, 0xaa)] {
        work.checked_add(offset)?.fill(value, ByteCount(bytes), context)?;
    }
    scratch.checked_add(0x459e4)?.fill(0, ByteCount(0x3cc), context)?;
    scratch.checked_add(0x44c84)?.fill(0, ByteCount(0xa04), context)?;
    for index in 0..3 {
        work.checked_add(0x162c + index * 4)?.set_byte(1, context)?;
        work.checked_add(0x162d + index * 4)?.set_byte(2, context)?;
        work.checked_add(0x162e + index * 4)?.set_byte(1, context)?;
        work.checked_add(0x162f + index * 4)?.set_byte(2, context)?;
        for offset in [0x1638, 0x163c, 0x1640, 0x1644] { work.checked_add(offset + index * 0x10)?.set_word(0, context)?; }
    }
    work.checked_add(0x1088)?.fill(0, ByteCount(0x4cc), context)?;
    scratch.checked_add(0x48d3c)?.fill(0, ByteCount(0x2dc), context)?;
    let mut first_selected = 64;
    for index in 0..64 {
        if SAMPLE_STATE.checked_add(0x134 + index)?.byte(context)? & 4 != 0 { first_selected = index; break; }
    }
    SAMPLE_STATE.checked_add(7)?.set_byte((first_selected >> 3) as u8, context)?;
    let mut preceding = 0;
    for index in 0..64 {
        let flags = SAMPLE_STATE.checked_add(0x134 + index)?.byte(context)?;
        if preceding < 9 && flags & 0x80 != 0 {
            if flags & 4 != 0 { break; }
            preceding += 1;
        }
    }
    SAMPLE_STATE.checked_add(8)?.set_byte(preceding, context)?;
    Ok(0)
}

pub fn helper_f8d754<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    SAMPLE_STATE.set_word(1, context)
}

pub fn helper_f8a780<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(u32::from(SAMPLE_STATE.checked_add(4)?.byte(context)?))
}

pub fn helper_f8d778<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<i32, MemoryError> {
    if output.get() == 0 { return Ok(0x84); }
    output.set_byte(SAMPLE_STATE.checked_add(4)?.byte(context)?, context)?;
    Ok(0)
}

pub fn helper_f8d7a0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> { Ok(0) }

pub fn helper_f8d7b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, positions: GuestAddress, index: u32) -> Result<i32, MemoryError> {
    if u32::from(SAMPLE_STATE.checked_add(4)?.byte(context)?) <= index { return Ok(0x84); }
    let output = SAMPLE_STATE.checked_add(0x6b8)?.address(context)?;
    let mode = SAMPLE_STATE.checked_add(0x178)?.word(context)?;
    let count = u32::from(SAMPLE_STATE.checked_add(0x17c)?.byte(context)?);
    for row in 0..7 {
        if SAMPLE_STATE.checked_add(0x134 + row * 8)?.byte(context)? & 1 != 0 {
            let position = positions.checked_add(row * 2)?.half(context)? as i16 as i32 as u32;
            output.checked_add((index + (row + 1) * 252) * 4)?.set_word(position, context)?;
        }
    }
    for row in 0..count {
        let position = positions.checked_add(14 + row * 2)?.half(context)? as i16 as i32 as u32;
        output.checked_add(0x1f80 + row * 0x3f0 + index * 4)?.set_word(position, context)?;
    }
    if mode == 1 {
        let position = positions.checked_add(14)?.half(context)? as i16 as i32 as u32;
        output.checked_add((index + 0x11b8) * 4)?.set_word(position, context)?;
    }
    Ok(0)
}
