use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_103b054<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    helper_103eefc(context, output)
}

pub fn helper_103eefc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    if output.get() == 0 { return Ok(()); }
    if crate::record_configuration::helper_69f8fc(context, 0xa8)? & 0x80 == 0 { return Ok(()); }
    let source = GuestAddress::new(context.read_u32(GuestAddress::new(0x0170_3ad4))?);
    let destination = output.checked_add(0x000d_bf58)?;
    for offset in (0x250..0x264).step_by(4) {
        copy_word(context, source, destination, offset)?;
    }
    for index in 0..256 {
        for base in [0x264, 0x664, 0xa64] {
            copy_word(context, source, destination, base + index * 4)?;
        }
    }
    for index in 0..64 {
        for base in [0xe64, 0xf64, 0x1064, 0x1164] {
            copy_word(context, source, destination, base + index * 4)?;
        }
    }
    for offset in (0x1264..0x12bc).step_by(4) {
        copy_word(context, source, destination, offset)?;
    }
    for offset in (0x12bc..0x14bc).step_by(4) {
        copy_word(context, source, destination, offset)?;
    }
    Ok(())
}

fn copy_word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress, destination: GuestAddress, offset: u32) -> Result<(), MemoryError> {
    let value = context.read_u32(source.checked_add(offset)?)?;
    context.write_u32(destination.checked_add(offset)?, value)
}
