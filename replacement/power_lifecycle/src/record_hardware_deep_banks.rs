use super::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy)]
pub(super) struct BankSelector(pub u32);

pub(super) fn offset(selector: BankSelector) -> u32 {
    match selector.0 {
        1 => 1024, 2 => 2048, 3 => 2128, 4 | 6 => 3152, 5 => 4176,
        7 => 6224, 8 | 10 => 6232, 9 => 6240, 11 => 6544, 12 => 7568,
        13..=21 => 8592 + (selector.0 - 13) * 4,
        22..=37 => 8688 + (selector.0 - 22) * 64,
        _ => 0,
    }
}

pub fn statistics_offset(selector: u32) -> u32 { offset(BankSelector(selector)) }

pub fn helper_ee04f0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, base: u32, selector: u32) -> Result<u32, MemoryError> {
    Ok(base.wrapping_add(offset(BankSelector(selector))))
}

pub fn helper_edae88<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selector: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    context.write_u32(output, offset(BankSelector(selector)))?;
    Ok(0)
}

fn dimensions<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, window: GuestAddress, mutable: bool) -> Result<(u32, u32), MemoryError> {
    let width = u32::from(context.read_u8(window)?);
    let height = u32::from(context.read_u8(window.checked_add(1)?)?);
    if mutable && height > 8 { context.write_u8(window.checked_add(1)?, 8)?; }
    if mutable && width > 8 { context.write_u8(window, 8)?; }
    Ok((width.min(8), height.min(8)))
}

pub fn helper_fe988c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, source: GuestAddress, window: GuestAddress) -> Result<u32, MemoryError> {
    if output.get() == 0 || source.get() == 0 { return Ok(132); }
    let (width, height) = if window.get() == 0 { (8, 8) } else { dimensions(context, window, false)? };
    for row in 0..height {
        context.copy_bytes(output.checked_add(256 + row * 32)?, source.checked_add(offset(BankSelector(1)) + row * 64)?, ByteCount(width * 4))?;
        context.copy_bytes(output.checked_add(row * 32)?, source.checked_add(row * 64)?, ByteCount(width * 4))?;
    }
    Ok(0)
}

pub fn helper_fe99f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, source: GuestAddress, count: u32, mode: u32) -> Result<u32, MemoryError> {
    if output.get() == 0 || source.get() == 0 { return Ok(132); }
    if count != 0 {
        let samples = (count.wrapping_sub(1) as u8 as u32) + 1;
        for index in 0..samples {
            let displacement = if mode == 0 || mode == 2 { count.wrapping_sub(index + 1).wrapping_mul(4) } else { index * 4 };
            let value = context.read_u32(source.checked_add(offset(BankSelector(2)) + index * 8)?)?;
            context.write_u32(output.checked_add(displacement)?, value)?;
        }
    }
    Ok(0)
}

pub fn helper_fe9adc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, source: GuestAddress, window: GuestAddress) -> Result<u32, MemoryError> {
    if output.get() == 0 || source.get() == 0 { return Ok(132); }
    let (width, height) = dimensions(context, window, true)?;
    let source = source.checked_add(offset(BankSelector(11)))?;
    for row in 0..height {
        for column in 0..width {
            let input = source.checked_add(row * 64 + column * 4)?;
            let destination = output.checked_add(row * 16 + column * 2)?;
            let high = (context.read_u32(input)? >> 16) as u16;
            context.write_u16(destination, high)?;
            let high = (context.read_u32(input)? >> 16) as u16;
            context.write_u16(destination.checked_add(128)?, high)?;
            let low = context.read_u32(input)? as u16;
            context.write_u16(destination.checked_add(256)?, low)?;
            let low = context.read_u32(input)? as u16;
            context.write_u16(destination.checked_add(384)?, low)?;
        }
    }
    Ok(0)
}

pub fn helper_fe9c04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, source: GuestAddress, window: GuestAddress) -> Result<u32, MemoryError> {
    if output.get() == 0 || source.get() == 0 { return Ok(132); }
    let (width, height) = dimensions(context, window, true)?;
    let source = source.checked_add(offset(BankSelector(12)))?;
    for row in 0..height {
        for column in 0..width {
            let input = source.checked_add(row * 64 + column * 4)?;
            let destination = output.checked_add(row * 32 + column * 4)?;
            let value = context.read_u32(input)?;
            context.write_u32(destination, value)?;
            let value = context.read_u32(input)?;
            context.write_u32(destination.checked_add(256)?, value)?;
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn reads_word_banks_through_the_admitted_address_space() {
        let mut source = [0u8; 2048];
        source[0..4].copy_from_slice(&0x12345678u32.to_le_bytes());
        source[1024..1028].copy_from_slice(&0x87654321u32.to_le_bytes());
        let mut destination = [0xa5u8; 512];
        let mut window = [1u8, 1];
        let region = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(region(0x1000, 2048), &mut source).unwrap(),
            MemoryRegion::new(region(0x2000, 512), &mut destination).unwrap(),
            MemoryRegion::new(region(0x3000, 2), &mut window).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, ());
        assert_eq!(helper_fe988c(&mut context, GuestAddress::new(0x2000), GuestAddress::new(0x1000), GuestAddress::new(0x3000)), Ok(0));
        assert_eq!(context.read_u32(GuestAddress::new(0x2000)), Ok(0x12345678));
        assert_eq!(context.read_u32(GuestAddress::new(0x2100)), Ok(0x87654321));
        assert_eq!(context.read_u32(GuestAddress::new(0x2004)), Ok(0xa5a5a5a5));
        assert_eq!(helper_fe9adc(&mut context, GuestAddress::new(0), GuestAddress::new(1), GuestAddress::new(u32::MAX)), Ok(132));
        assert_eq!(helper_ee04f0(&mut context, 0x1000, 37), Ok(0x35b0));
        assert_eq!(helper_ee04f0(&mut context, 0x1000, 38), Ok(0x1000));
    }
}
