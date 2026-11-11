use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RecordError};
use crate::shutdown::KernelStatus;

pub trait ImageCache {
    fn clean_cache(&mut self, address: GuestAddress, length: ByteCount) -> Result<(), KernelStatus>;
}

pub fn helper_1036a70<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    if output.get() == 0 { return Ok(()); }
    context.write_u32(output, 0x5354_4643)?;
    context.write_u32(output.checked_add(0x104)?, 0x4c41_4643)?;
    let wrapped = context.read_u32(GuestAddress::new(0x0170_32d8))? != 0;
    for index in 0u32..64 {
        let cursor = context.read_u32(GuestAddress::new(0x0170_32d4))?;
        let destination = if wrapped {
            if cursor > index { index.wrapping_add(64u32.wrapping_sub(cursor)) }
            else { index.wrapping_sub(cursor) }
        } else { index };
        let value = if wrapped || cursor > index {
            u32::from(context.read_u8(GuestAddress::new(0x0170_32dc + index))?)
        } else { u32::MAX };
        context.write_u32(output.checked_add(4)?.checked_add(destination.wrapping_mul(4))?, value)?;
    }
    Ok(())
}

pub fn helper_103b044<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    helper_1036a70(context, output)
}

pub struct PreviewConfiguration {
    pub handle: u32,
    pub table: GuestAddress,
    pub kind: u32,
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
    pub first: u32,
    pub second: u32,
}

pub fn helper_1042e9c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, configuration: PreviewConfiguration) -> Result<(), MemoryError> {
    const PREVIEW: u32 = 0x0170_5784;
    context.write_u32(GuestAddress::new(PREVIEW), configuration.handle)?;
    context.write_u8(GuestAddress::new(PREVIEW + 0x116), configuration.kind.wrapping_shl(2) as u8)?;
    for (index, value) in [configuration.width, configuration.height, configuration.x,
        configuration.y, configuration.first, configuration.second].into_iter().enumerate() {
        context.write_u16(GuestAddress::new(PREVIEW + 0x124 + index as u32 * 2), value as u16)?;
    }
    for index in 0..14 {
        let byte = context.read_u8(configuration.table.checked_add(index)?)?;
        context.write_u8(GuestAddress::new(PREVIEW + 0x104 + index), byte)?;
    }
    Ok(())
}

pub fn helper_10517f8<M: GuestAddressSpace, E: ImageCache>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<u32, RecordError> {
    if output.get() == 0 { return Ok(132); }
    helper_10452e8(context, output)?;
    Ok(0)
}

pub fn helper_10452e8<M: GuestAddressSpace, E: ImageCache>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), RecordError> {
    if crate::record_configuration::helper_69f8fc(context, 0xa8)? & 0x80 == 0 { return Ok(()); }
    context.write_u32(output, 0x5354_4348)?;
    context.copy_bytes(output.checked_add(4)?, GuestAddress::new(0x0170_58e4), ByteCount(52))?;
    let mut cursor = u32::from(context.read_u8(GuestAddress::new(0x4171_bc28))?);
    for index in 0..60 {
        context.copy_bytes(output.checked_add(56 + index * 68)?,
            GuestAddress::new(0x0170_58e0).checked_add(cursor * 68)?.checked_add(56)?, ByteCount(68))?;
        cursor += 1;
        if cursor > 59 { cursor = 0; }
    }
    context.write_u32(output.checked_add(0x1028)?, 0x4c41_4348)?;
    context.effects.clean_cache(GuestAddress::new(0x0170_58e0), ByteCount(4160))?;
    Ok(())
}
