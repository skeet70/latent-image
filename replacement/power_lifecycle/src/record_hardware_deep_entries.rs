use super::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RegisterWordCount};
use super::record_hardware_deep_banks::{offset, BankSelector};
use super::record_hardware_deep_calibration::{entry_parameters, helper_edb08c};
use super::record_hardware_deep_correction::helper_fe9d30;
use super::record_deep_geometry::calculate_entry_report;

pub fn helper_fec494<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, grid: GuestAddress, regions: GuestAddress, black: GuestAddress, dimensions: GuestAddress, region_dimensions: GuestAddress, black_dimensions: GuestAddress, first_report: GuestAddress, second_report: GuestAddress, token: GuestAddress, adjusted: GuestAddress) -> Result<u32, MemoryError> {
    const STATE: GuestAddress = GuestAddress::new(0x016f_0d88);
    let queue = if u32::from(context.read_u8(STATE.checked_add(12)?)?) == index { STATE.checked_add(12)? }
        else if u32::from(context.read_u8(STATE.checked_add(120)?)?) == index { STATE.checked_add(120)? }
        else { return Ok(0); };
    let bank = GuestAddress::new(context.read_u32(queue.checked_add(4)?)?);
    for plane in 0..4 {
        context.copy_bytes(grid.checked_add(plane * 1024)?, bank.checked_add(offset(BankSelector(3 + plane)))?, ByteCount(1024))?;
    }
    for channel in 0..4 {
        context.copy_bytes(black.checked_add(channel * 4)?, bank.checked_add(offset(BankSelector(13 + channel)))?, ByteCount(4))?;
    }
    let mut sum = 0u64;
    for channel in 0..4 { sum += u64::from(context.read_u32(black.checked_add(channel * 4)?)?); }
    let average = (sum >> 2) as u32;
    for channel in 0..4 { context.write_u32(black.checked_add(channel * 4)?, average)?; }
    if regions.get() != 0 && region_dimensions.get() != 0 {
        let source = bank.checked_add(offset(BankSelector(7)))?;
        for region in 0..10 {
            let destination = regions.checked_add(region * 32)?;
            let source = source.checked_add(region * 32)?;
            context.copy_register_words(destination, source, RegisterWordCount::Two)?;
            let second_low = context.read_u32(source.checked_add(8)?)?;
            let second_high = context.read_u32(source.checked_add(12)?)?;
            context.write_u32(destination.checked_add(8)?, second_low)?;
            context.write_u32(destination.checked_add(12)?, second_high)?;
            context.copy_register_words(destination.checked_add(16)?, source.checked_add(16)?, RegisterWordCount::Two)?;
            context.write_u32(destination.checked_add(24)?, second_low)?;
            context.write_u32(destination.checked_add(28)?, second_high)?;
            context.copy_register_words(region_dimensions.checked_add(region * 8)?, queue.checked_add(24 + region * 8)?, RegisterWordCount::Two)?;
        }
    }
    context.copy_register_words(dimensions, queue.checked_add(8)?, RegisterWordCount::Two)?;
    context.copy_register_words(black_dimensions, queue.checked_add(16)?, RegisterWordCount::Two)?;
    let value = context.read_u32(queue.checked_add(104)?)?;
    context.write_u32(token, value)?;
    let mut parameters = entry_parameters(context, index)?;
    parameters[16..20].fill(0);
    let mut compact = [0u8; 12];
    compact[0..4].copy_from_slice(&context.read_u32(STATE.checked_add(284)?)?.to_le_bytes());
    compact[4..6].copy_from_slice(&context.read_u16(STATE.checked_add(288)?)?.to_le_bytes());
    let mut report = [0u8; 208];
    calculate_entry_report(context, &parameters, &compact, &mut report)?;
    for (destination, report_offset) in [(first_report, 8usize), (second_report, 16)] {
        for word in 0..2 {
            let offset = report_offset + word * 4;
            let value = u32::from_le_bytes([report[offset], report[offset + 1], report[offset + 2], report[offset + 3]]);
            context.write_u32(destination.checked_add(word as u32 * 4)?, value)?;
        }
    }
    let entry = STATE.checked_add(index.checked_mul(20).ok_or(MemoryError::AddressOverflow)?)?.checked_add(296)?;
    let gain = context.read_u16(entry.checked_add(14)?)? as i16 as i32;
    let profile = context.read_u32(entry.checked_add(4)?)?;
    helper_edb08c(context, gain, profile, 0, adjusted)?;
    helper_fe9d30(context, grid, dimensions, regions, region_dimensions, black, black_dimensions, profile, gain)?;
    Ok(1)
}
