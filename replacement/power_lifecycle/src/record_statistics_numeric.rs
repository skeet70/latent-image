use super::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};

#[derive(Clone, Copy)]
pub struct Hundredths(pub i32);

pub fn helper_ee98a0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    if value == 0 { return Ok(0x84); }
    let exponent = 31 - value.leading_zeros();
    let normalized = if exponent > 8 { value >> (exponent - 8) } else { value << (8 - exponent) };
    let fraction = context.read_u8(GuestAddress::new(0x00ee_9ef4 + normalized - 256))?;
    context.write_u16(output, (u32::from(fraction) + exponent * 100) as u16)?;
    Ok(0)
}
pub fn helper_ee9928<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    context.with_guest_frame(ByteCount(24), StackAlignment::Doubleword, |context, frame| {
        let scratch = frame.at(ByteCount(6), ByteCount(2))?;
        helper_ee98a0(context, value, scratch)?;
        let converted = context.read_u16(scratch)?;
        let result = ((u64::from(converted) * 100 * 0x3159_721f) >> 38) as u32;
        context.write_u32(output, result)?;
        Ok(0)
    })
}
pub fn helper_ee9ab0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: i32) -> Result<u32, MemoryError> {
    let value = Hundredths(value);
    let quotient = value.0 / 100;
    let exponent = quotient as i16;
    let remainder = value.0.wrapping_sub(quotient.wrapping_mul(100)).wrapping_add(100) as i16;
    let fraction = u32::from(context.read_u8(GuestAddress::new(0x00ee_9dc8u32.wrapping_add(remainder as i32 as u32)))?);
    Ok(if exponent < 0 {
        let shift = (-(i32::from(exponent))) as u32;
        let base = if shift < 32 { 0x10000u32 >> shift } else { 0 };
        base.wrapping_mul(fraction) >> 16
    } else {
        let shift = exponent as u32;
        let base = if shift < 32 { 1u32 << shift } else { 0 };
        base.wrapping_mul(fraction)
    })
}
