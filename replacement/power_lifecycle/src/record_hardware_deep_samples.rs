use super::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RegisterWordCount};
use super::record_hardware_deep_banks::{offset, BankSelector, helper_fe988c, helper_fe99f0, helper_fe9adc, helper_fe9c04};

pub fn helper_fef5e8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: GuestAddress) -> Result<u32, MemoryError> {
    const SAMPLE: GuestAddress = GuestAddress::new(0x016f_1028);
    let index = context.read_u16(request)?;
    let queue = if index == context.read_u16(SAMPLE.checked_add(748)?)? { GuestAddress::new(0x016f_1314) }
        else if index == context.read_u16(SAMPLE.checked_add(796)?)? { GuestAddress::new(0x016f_1344) }
        else { return Ok(0); };
    let destination = GuestAddress::new(context.read_u32(request.checked_add(4)?)?);
    let source = GuestAddress::new(context.read_u32(queue.checked_add(4)?)?);
    let width = u32::from(context.read_u8(SAMPLE.checked_add(0x34c)?)?).min(8);
    let height = u32::from(context.read_u8(SAMPLE.checked_add(0x34d)?)?).min(8);
    for row in 0..height {
        for plane in 0..4 {
            context.copy_bytes(destination.checked_add(plane * 256 + row * 32)?, source.checked_add(offset(BankSelector(3 + plane)) + row * 64)?, ByteCount(width * 4))?;
        }
    }
    let output = GuestAddress::new(context.read_u32(request.checked_add(8)?)?);
    helper_fe988c(context, output, source, GuestAddress::new(0x016f_1376))?;
    let output = GuestAddress::new(context.read_u32(request.checked_add(12)?)?);
    let count = u32::from(context.read_u8(SAMPLE.checked_add(0x352)?)?);
    let mode = context.read_u32(SAMPLE.checked_add(0x42c)?)?;
    helper_fe99f0(context, output, source, count, mode)?;
    let output = GuestAddress::new(context.read_u32(request.checked_add(16)?)?);
    helper_fe9adc(context, output, source, SAMPLE.checked_add(0x350)?)?;
    let output = GuestAddress::new(context.read_u32(request.checked_add(20)?)?);
    helper_fe9c04(context, output, source, SAMPLE.checked_add(0x350)?)?;
    context.copy_register_words(request.checked_add(24)?, queue.checked_add(8)?, RegisterWordCount::Two)?;
    context.copy_register_words(request.checked_add(32)?, queue.checked_add(16)?, RegisterWordCount::Two)?;
    context.copy_register_words(request.checked_add(40)?, queue.checked_add(32)?, RegisterWordCount::Two)?;
    context.copy_register_words(request.checked_add(48)?, queue.checked_add(24)?, RegisterWordCount::Two)?;
    let value = context.read_u32(queue.checked_add(40)?)?;
    context.write_u8(request.checked_add(56)?, value as u8)?;
    let value = context.read_u16(queue.checked_add(2)?)?;
    context.write_u16(request.checked_add(2)?, value)?;
    Ok(1)
}
