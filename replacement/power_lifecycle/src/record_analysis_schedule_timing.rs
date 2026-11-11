//! X-M1 1.50 timing tables, installed ef5448/ef5470/10032ec/1003840/1004ce0/1004d04/1004d68/1007e44 bytes.

use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_1003840<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(0x0101_e014u32.wrapping_add(bank.wrapping_mul(8))))
}

pub fn helper_10032ec<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selector: u32) -> Result<u32, MemoryError> {
    Ok(match selector { 2 => 54, 1 | 3 => 27, _ => 1 })
}

pub fn helper_10056f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u32, MemoryError> {
    super::record_configuration::helper_69f8fc(context, key)
}

pub fn helper_ef5448<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<u32, MemoryError> {
    Ok(context.read_u8(GuestAddress::new(0x00ef_6decu32.wrapping_add(bank.wrapping_mul(20))))? as u32)
}

pub fn helper_ef5470<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, output: GuestAddress, selector: u32) -> Result<(), MemoryError> {
    let index = context.read_u32(GuestAddress::new(0x00ef_6de8u32.wrapping_add(bank.wrapping_mul(20))))?;
    let table = GuestAddress::new(0x00ef_6aa4u32.wrapping_add(index.wrapping_mul(48)));
    let fourth = context.read_u16(table.checked_add(18)?)? as u32;
    context.write_u32(output.checked_add(8)?, fourth)?;
    let third = context.read_u16(table.checked_add(16)?)? as u32;
    context.write_u32(output.checked_add(12)?, third)?;
    let first = context.read_u16(table.checked_add(12)?)? as u32;
    context.write_u32(output, first)?;
    let second = context.read_u16(table.checked_add(14)?)? as u32;
    context.write_u32(output.checked_add(4)?, second)?;
    if selector == 0 && helper_ef5448(context, bank)? == 0 {
        context.write_u32(output, first >> 1)?;
        context.write_u32(output.checked_add(4)?, second.wrapping_mul(2))?;
        context.write_u32(output.checked_add(12)?, (third >> 1).wrapping_add(1))?;
        context.write_u32(output.checked_add(8)?, fourth.wrapping_mul(2))?;
    }
    Ok(())
}

pub fn helper_1004ce0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<u32, MemoryError> {
    let index = helper_1003840(context, bank)?;
    Ok(context.read_u16(GuestAddress::new(0x0101_dea4u32.wrapping_add(index.wrapping_mul(28))))? as u32)
}

pub fn helper_1004d04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, output: GuestAddress, selector: u32) -> Result<(), MemoryError> {
    let kind = helper_ef5448(context, bank)?;
    helper_ef5470(context, bank, output, selector)?;
    if kind == 5 {
        let index = helper_1003840(context, bank)?;
        let value = context.read_u16(GuestAddress::new(0x0101_dea6u32.wrapping_add(index.wrapping_mul(28))))? as u32;
        context.write_u32(output.checked_add(4)?, value)?;
        let count = context.read_u32(output.checked_add(8)?)?;
        context.write_u32(output.checked_add(8)?, count.wrapping_sub(1))?;
    }
    Ok(())
}

pub fn helper_1004d68<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    let index = helper_1003840(context, bank)?;
    let denominator = helper_10032ec(context, 2)?;
    let kind = helper_ef5448(context, bank)?;
    if kind > 5 || ((1u32 << kind) & 44) == 0 { return Ok(133); }
    let period = context.read_u32(GuestAddress::new(0x0101_de94u32.wrapping_add(index.wrapping_mul(28))))?;
    let intermediate = (period.wrapping_mul(10) / denominator).wrapping_mul(2);
    context.write_u32(output, intermediate)?;
    let rounded = (intermediate / 10).wrapping_add((intermediate % 10 > 4) as u32);
    context.write_u32(output, rounded)?;
    Ok(0)
}

pub fn helper_1007e44<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<u32, MemoryError> {
    let entry = GuestAddress::new(0x0102_2f54u32.wrapping_add(second.wrapping_add(first.wrapping_mul(6)).wrapping_mul(8)));
    let key = context.read_u32(entry)?;
    let high = helper_10056f0(context, key.wrapping_add(1))?;
    let low = helper_10056f0(context, key)?;
    let denominator = ((high << 8) | low) as u16 as u32;
    let key = context.read_u32(entry.checked_add(4)?)?;
    let high = helper_10056f0(context, key.wrapping_add(1))?;
    let low = helper_10056f0(context, key)?;
    let numerator = (((high << 8) | low) as u16 as u32).wrapping_mul(1000);
    let ratio = (numerator as i32).wrapping_div(if denominator == 0 { 1 } else { denominator as i32 }) as u32;
    Ok((ratio / 10).wrapping_add((ratio % 10 > 4) as u32))
}
