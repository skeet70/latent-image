//! Byte-derived f8ca48/f8ca94/f8d8c8 extrema refresh.

use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use super::record_scan_core_memory::{ScanMemoryAddress, SAMPLE_STATE};

#[derive(Clone, Copy)]
enum Extremum { Maximum, Minimum }

fn refresh(memory: &mut impl GuestAddressSpace, count: u32, values: GuestAddress, index: GuestAddress, output: GuestAddress, extremum: Extremum) -> Result<(), MemoryError> {
    index.set_byte(0, memory)?;
    output.set_word(match extremum { Extremum::Maximum => 0, Extremum::Minimum => u32::MAX }, memory)?;
    for position in 0..count {
        let value = values.checked_add(position * 4)?.word(memory)?;
        let previous = output.word(memory)?;
        if match extremum { Extremum::Maximum => previous < value, Extremum::Minimum => previous > value } {
            output.set_word(value, memory)?;
            index.set_byte(position as u8, memory)?;
        }
    }
    Ok(())
}

pub fn helper_f8ca48<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, count: u32, values: GuestAddress, index: GuestAddress, output: GuestAddress) -> Result<(), MemoryError> {
    refresh(context, count, values, index, output, Extremum::Maximum)
}

pub fn helper_f8ca94<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, count: u32, values: GuestAddress, index: GuestAddress, output: GuestAddress) -> Result<(), MemoryError> {
    refresh(context, count, values, index, output, Extremum::Minimum)
}

fn refresh_row(memory: &mut impl GuestAddressSpace, count: u32, values: GuestAddress, work: GuestAddress, maximum_index: u32, minimum_index: u32, maximum: u32, minimum: u32) -> Result<(), MemoryError> {
    if count <= u32::from(work.checked_add(maximum_index)?.byte(memory)?) {
        refresh(memory, count, values, work.checked_add(maximum_index)?, work.checked_add(maximum)?, Extremum::Maximum)?;
    }
    if count <= u32::from(work.checked_add(minimum_index)?.byte(memory)?) {
        refresh(memory, count, values, work.checked_add(minimum_index)?, work.checked_add(minimum)?, Extremum::Minimum)?;
    }
    Ok(())
}

pub fn helper_f8d8c8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, count: u32) -> Result<i32, MemoryError> {
    if count == 0 || count > u32::from(SAMPLE_STATE.checked_add(4)?.byte(context)?) { return Ok(0x84); }
    let work = SAMPLE_STATE.checked_add(0x6cc)?.address(context)?;
    let cells = SAMPLE_STATE.checked_add(0x690)?.address(context)?;
    for bank in 0..2 {
        for row in 0..49 {
            let index = bank * 49 + row;
            refresh_row(context, count, cells.checked_add(bank * 0xc0f0 + row * 0x3f0)?, work, 0x550 + index, 0x5fa + index, index * 4, 0x2a8 + index * 4)?;
        }
    }
    let groups = SAMPLE_STATE.checked_add(0x694)?.address(context)?;
    for bank in 0..2 {
        for group in 0..2 {
            for row in 0..5 {
                let index = bank * 10 + group * 5 + row;
                refresh_row(context, count, groups.checked_add(bank * 0x2760 + group * 0x13b0 + row * 0x3f0)?, work, 0x5b2 + index, 0x65c + index, 0x188 + index * 4, 0x430 + index * 4)?;
            }
        }
    }
    let lines = SAMPLE_STATE.checked_add(0x69c)?.address(context)?;
    for bank in 0..2 {
        for row in 0..10 {
            let index = bank * 10 + row;
            refresh_row(context, count, lines.checked_add(bank * 0x2760 + row * 0x3f0)?, work, 0x5ca + index, 0x674 + index, 0x1e8 + index * 4, 0x490 + index * 4)?;
        }
    }
    SAMPLE_STATE.checked_add(4)?.set_byte(count as u8, context)?;
    if count < u32::from(SAMPLE_STATE.checked_add(5)?.byte(context)?) { SAMPLE_STATE.checked_add(5)?.set_byte(count as u8, context)?; }
    Ok(0)
}
