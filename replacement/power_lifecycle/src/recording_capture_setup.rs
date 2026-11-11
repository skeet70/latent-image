use crate::record_address_space::GuestAddressSpace;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureOutputKind { Primary, Secondary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureOutputMode { Still, Movie }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureOutputParameters {
    pub first: u16,
    pub second: u16,
    pub third: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationByteKey(pub u32);

pub trait CaptureOutputSource {
    fn configuration_byte(&mut self, key: ConfigurationByteKey) -> u8;
    fn parameter_bank(&mut self, bank: CaptureParameterBank) -> CaptureOutputParameters;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CaptureParameterBank { StillPrimary = 0, StillSecondary = 1, MoviePrimary = 2, MovieSecondary = 3 }

pub fn output_parameters(source: &mut impl CaptureOutputSource, mode: CaptureOutputMode, kind: CaptureOutputKind) -> CaptureOutputParameters {
    if source.configuration_byte(ConfigurationByteKey(0xa902)) == 2 && mode == CaptureOutputMode::Still {
        let mut half = |high, low| {
            let high = source.configuration_byte(ConfigurationByteKey(high));
            let low = source.configuration_byte(ConfigurationByteKey(low));
            u16::from_le_bytes([low, high])
        };
        return CaptureOutputParameters { first: half(0xa904, 0xa903), second: half(0xa906, 0xa905), third: half(0xa908, 0xa907) };
    }
    source.parameter_bank(match (mode, kind) {
        (CaptureOutputMode::Still, CaptureOutputKind::Primary) => CaptureParameterBank::StillPrimary,
        (CaptureOutputMode::Still, CaptureOutputKind::Secondary) => CaptureParameterBank::StillSecondary,
        (CaptureOutputMode::Movie, CaptureOutputKind::Primary) => CaptureParameterBank::MoviePrimary,
        (CaptureOutputMode::Movie, CaptureOutputKind::Secondary) => CaptureParameterBank::MovieSecondary,
    })
}

pub fn initialize_capture_profile(destination: &mut [u8; 0x28], residue: u32) {
    let words: [u32; 10] = [0x42e4_0000, 0x0098_ee00, residue, 0x5ffb_0dc0, 0x42e4_0000, 0x01b4_8600, 0x5fbb_a360, 0, 0x4498_de00, 0x01fa_f400];
    for (slot, value) in destination.chunks_exact_mut(4).zip(words) { slot.copy_from_slice(&value.to_le_bytes()); }
}

pub fn prepare_capture_packet(primary: &mut [u8; 0x90], payload: &mut [u8; 0xa8], callback: u32, argument: u32, secondary: u32, limits: u32, profile: u32, special_value: i16) {
    payload[8..0x98].copy_from_slice(primary);
    let value = if u16::from_le_bytes([primary[0x30], primary[0x31]]) == 0x1800 { special_value } else { i16::from_le_bytes([primary[0x22], primary[0x23]]) };
    payload[0x2a..0x2c].copy_from_slice(&value.to_le_bytes());
    let mask = u32::from_le_bytes(payload[0x88..0x8c].try_into().unwrap()) | 0x0040_0000;
    payload[0x88..0x8c].copy_from_slice(&mask.to_le_bytes());
    primary[0x80..0x90].fill(0);
    for (offset, value) in [(0, callback), (4, argument), (0x98, secondary), (0x9c, limits), (0xa0, profile)] {
        payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

pub fn helper_737854<M: crate::record_address_space::GuestAddressSpace, E>(_context: &mut crate::record_address_space::RecordContext<M, E>, first: u32, second: u32) -> Result<u32, crate::record_address_space::MemoryError> {
    Ok(if second != 0 { 0 } else { match first { 0 => 1, 1 => 2, _ => 3 } })
}

pub fn helper_72daac<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<u32, crate::record_address_space::MemoryError> {
    context.read_u32(crate::record_address_space::GuestAddress::new(0x0091_68d8))
}

pub fn helper_72dafc<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<u32, crate::record_address_space::MemoryError> {
    context.read_u32(crate::record_address_space::GuestAddress::new(0x0091_68f4))
}

pub fn helper_72db7c<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<u32, crate::record_address_space::MemoryError> {
    context.read_u32(crate::record_address_space::GuestAddress::new(0x0091_68e0))
}

pub fn helper_eb02f8<M: crate::record_address_space::GuestAddressSpace, E>(
    context: &mut crate::record_address_space::RecordContext<M, E>, mode: u32, kind: u32,
    first: crate::record_address_space::GuestAddress, second: crate::record_address_space::GuestAddress, third: crate::record_address_space::GuestAddress,
) -> Result<(), crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    if crate::record_configuration::helper_69f8fc(context, 0xa902)? == 2 && mode == 1 {
        for (destination, high, low) in [(first, 0xa904, 0xa903), (second, 0xa906, 0xa905), (third, 0xa908, 0xa907)] {
            if destination.get() != 0 {
                let high = crate::record_configuration::helper_69f8fc(context, high)?;
                let low = crate::record_configuration::helper_69f8fc(context, low)?;
                context.write_u16(destination, (low | high << 8) as u16)?;
            }
        }
        return Ok(());
    }
    let bank = if mode == 1 { u32::from(kind != 0) } else if kind == 0 { 2 } else { 3 };
    for (destination, offset) in [(first, 0), (second, 2), (third, 4)] {
        let value = context.read_u16(GuestAddress::new(0x409c_2944 + bank * 6 + offset))?;
        context.write_u16(destination, value)?;
    }
    Ok(())
}

pub fn helper_7389b8<M: crate::record_address_space::GuestAddressSpace, E>(
    context: &mut crate::record_address_space::RecordContext<M, E>, descriptor: crate::record_address_space::GuestAddress, limit: u32,
) -> Result<(), crate::record_address_space::MemoryError> {
    if context.read_u32(descriptor)? != limit { context.write_u32(descriptor, limit)?; }
    let second = descriptor.checked_add(4)?;
    if context.read_u32(second)? != 16 { context.write_u32(second, 16)?; }
    let third = descriptor.checked_add(8)?;
    let previous = context.read_u32(third)?;
    if previous != crate::record_configuration::helper_69f8fc(context, 0x254)? {
        let value = crate::record_configuration::helper_69f8fc(context, 0x254)?;
        context.write_u32(third, value)?;
    }
    Ok(())
}
