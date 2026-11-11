use core::mem::MaybeUninit;
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecimalWidth(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecimalStyle { ZeroTerminated, ZeroFixed, SpaceTerminated, SpaceFixed, UnpaddedTerminated, UnpaddedFixed }
impl DecimalStyle {
    pub const fn from_abi(flags: u32) -> Self {
        match flags { 0 => Self::ZeroTerminated, 1 => Self::ZeroFixed, 2 => Self::SpaceTerminated, 3 => Self::SpaceFixed, other if other & 1 == 0 => Self::UnpaddedTerminated, _ => Self::UnpaddedFixed }
    }
    fn fill(self) -> Option<u8> {
        match self { Self::ZeroTerminated | Self::ZeroFixed => Some(b'0'), Self::SpaceTerminated | Self::SpaceFixed => Some(b' '), Self::UnpaddedTerminated | Self::UnpaddedFixed => None }
    }
    fn terminated(self) -> bool { matches!(self, Self::ZeroTerminated | Self::SpaceTerminated | Self::UnpaddedTerminated) }
}
pub fn helper_721b04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mut destination: GuestAddress, mut source: GuestAddress) -> Result<GuestAddress, MemoryError> {
    loop {
        let value = context.read_u8(source)?;
        context.write_u8(destination, value)?;
        if value == 0 { return Ok(destination); }
        source = source.checked_add(1)?;
        destination = destination.checked_add(1)?;
    }
}
pub fn helper_722218<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, mut value: u32, width: DecimalWidth, style: DecimalStyle) -> Result<i32, MemoryError> {
    let mut digits = [MaybeUninit::<u8>::uninit(); 10];
    let mut length = 0usize;
    while value != 0 {
        let quotient = value / 10;
        digits[length].write(b'0' + (value - quotient * 10) as u8);
        value = quotient;
        length += 1;
    }
    if width.0 < length as u32 { return Ok(-1); }
    let padding = if let Some(fill) = style.fill() {
        for offset in 0..width.0 { context.write_u8(destination.checked_add(offset)?, fill)?; }
        width.0 - length as u32
    } else { 0 };
    for offset in 0..length {
        let value = unsafe { digits[length - offset - 1].assume_init() };
        context.write_u8(destination.checked_add(padding + offset as u32)?, value)?;
    }
    if style.terminated() { context.write_u8(destination.checked_add(padding + length as u32)?, 0)?; }
    Ok(length as i32)
}
