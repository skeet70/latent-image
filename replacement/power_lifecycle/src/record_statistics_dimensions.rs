use super::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};

#[derive(Clone, Copy)]
pub struct Ratio(pub u32);
#[derive(Clone, Copy)]
pub struct DimensionIndex(pub u32);

pub fn helper_6495b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    if kind > 11 { return Ok((-30i32) as u32); }
    let value = context.read_u32(GuestAddress::new(0xfff8_a030 + kind * 4))?;
    context.write_u16(output, value as u16)?;
    Ok(0)
}
fn dimension<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: DimensionIndex, ratio: Ratio, table: GuestAddress) -> Result<u32, MemoryError> {
    let column = match ratio.0 { 0..=2 => ratio.0, _ => 3 };
    context.read_u32(table.checked_add(index.0.wrapping_mul(16).wrapping_add(column * 4))?)
}
pub fn helper_734540<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, ratio: u32) -> Result<u32, MemoryError> {
    dimension(context, DimensionIndex(index), Ratio(ratio), GuestAddress::new(0x0081_6f60))
}
pub fn helper_73469c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, ratio: u32) -> Result<u32, MemoryError> {
    dimension(context, DimensionIndex(index), Ratio(ratio), GuestAddress::new(0x0081_63a8))
}
pub fn helper_739500<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, dimensions: GuestAddress) -> Result<u32, MemoryError> {
    let first = context.read_u16(dimensions)?;
    let second = context.read_u16(dimensions.checked_add(2)?)?;
    Ok(match (first, second) { (2, 3) => 1, (9, 16) => 2, (1, 1) => 3, _ => 0 })
}
pub fn helper_73946c(mode: u32) -> u32 {
    match mode { 2 => 1, 3 => 2, 4 => 4, 5 => 5, 6 => 3, 7 => 6, 8 => 9, 9 => 8, 10 => 7, _ => 0 }
}
pub fn helper_73958c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32, ratio: u32) -> Result<u32, MemoryError> {
    context.write_u32(GuestAddress::new(0x0091_34ec), helper_73946c(mode))?;
    let selected = context.read_u32(GuestAddress::new(0x0091_34ec))?;
    let index = context.read_u32(GuestAddress::new(0x0091_34ec))?;
    let result = if selected != 255 { context.read_u32(GuestAddress::new(0x0081_7478).checked_add(ratio.wrapping_add(index.wrapping_mul(4)).wrapping_mul(4))?)? } else { index };
    context.write_u32(GuestAddress::new(0x0091_34f0), result)?;
    let _observed = context.read_u32(GuestAddress::new(0x0091_34f0))?;
    context.read_u32(GuestAddress::new(0x0091_34f0))
}
pub fn helper_741ee0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    context.with_guest_frame(ByteCount(24), StackAlignment::Doubleword, |context, frame| {
        if kind > 8 { context.write_u8(output, 0)?; return Ok(0x84); }
        let mapped = context.read_u8(GuestAddress::new(0x0081_ac28 + kind))?;
        if mapped == 255 { context.write_u8(output, 0)?; return Ok(0x11); }
        let scratch = frame.at(ByteCount(6), ByteCount(2))?;
        let result = helper_6495b8(context, u32::from(mapped), scratch)?;
        if result == 0 {
            let status = context.read_u16(scratch)?;
            context.write_u8(output, (status >> 2) as u8)?;
            Ok(0)
        } else { context.write_u8(output, 0)?; Ok(0x81) }
    })
}
