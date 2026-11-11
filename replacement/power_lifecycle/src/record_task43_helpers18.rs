//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; stock f2afc8..f2e67c.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub(crate) const DISPLAY: GuestAddress = GuestAddress::new(0x016b_751c);
pub(crate) const CHANNEL: GuestAddress = GuestAddress::new(0x016b_7864);
pub(crate) const COMMAND: GuestAddress = GuestAddress::new(0x0169_d09c);
pub(crate) const REQUEST: GuestAddress = GuestAddress::new(0x0169_d370);

macro_rules! setter {
    ($name:ident, $base:expr, $offset:expr, $method:ident, $ty:ty) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: $ty) -> Result<(), MemoryError> {
            context.$method($base.checked_add($offset)?, value)
        }
    };
}
macro_rules! getter {
    ($name:ident, $base:expr, $offset:expr, $method:ident, $ty:ty) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<$ty, MemoryError> {
            Ok(context.$method($base.checked_add($offset)?)? as $ty)
        }
    };
}

setter!(helper_f2afc8, DISPLAY, 0, write_u32, u32);
setter!(helper_f2cb3c, CHANNEL, 0, write_u32, u32);
setter!(helper_f2d6fc, CHANNEL, 0x7c, write_u32, u32);
setter!(helper_f2d71c, CHANNEL, 0x84, write_u32, u32);
setter!(helper_f2d73c, CHANNEL, 0x80, write_u32, u32);
setter!(helper_f2d75c, CHANNEL, 0x88, write_u32, u32);
getter!(helper_f2c148, DISPLAY, 0x144, read_u32, u32);
getter!(helper_f2c168, DISPLAY, 0x14c, read_u32, u32);
getter!(helper_f2c1f0, DISPLAY, 0x150, read_u32, u32);
getter!(helper_f2c328, DISPLAY, 0x194, read_u32, u32);
getter!(helper_f2c3a0, DISPLAY, 0x168, read_u8, u8);
getter!(helper_f2c878, DISPLAY, 0x18c, read_u8, u8);
getter!(helper_f2d6dc, CHANNEL, 0x78, read_u32, i32);
getter!(helper_f2d77c, CHANNEL, 0x7c, read_u32, i32);
getter!(helper_f2e65c, REQUEST, 0x10, read_u32, i32);

macro_rules! signed_half_getter {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
            Ok(context.read_u16(DISPLAY.checked_add($offset)?)? as i16 as i32)
        }
    };
}
signed_half_getter!(helper_f2c210, 0x14a);
signed_half_getter!(helper_f2c234, 0x148);
signed_half_getter!(helper_f2c2bc, 0x160);
signed_half_getter!(helper_f2c2e0, 0x162);
signed_half_getter!(helper_f2c304, 0x166);
signed_half_getter!(helper_f2c348, 0x198);
signed_half_getter!(helper_f2c37c, 0x164);

pub fn helper_f2be34<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u8(DISPLAY.checked_add(0x10)?)? == 1))
}
pub fn helper_f2d094<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u8(CHANNEL.checked_add(4)?, 1)
}
pub fn helper_f2d0b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u8(CHANNEL.checked_add(4)?)? == 1))
}
pub fn helper_f2d0e4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u8(CHANNEL.checked_add(4)?, 0)
}
pub fn helper_f2d130<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u8(CHANNEL.checked_add(5)?, 1)
}
pub fn helper_f2d154<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u8(CHANNEL.checked_add(5)?)? == 1))
}
pub fn helper_f2d180<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u8(CHANNEL.checked_add(5)?, 0)
}
pub fn helper_f2cb5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u32(COMMAND.checked_add(4)?)? & 1 != 0 || context.read_u32(COMMAND.checked_add(8)?)? == 3))
}
pub fn helper_f2cb90<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u32(COMMAND.checked_add(4)?)? & 0x19e != 0))
}
pub fn helper_f2d79c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok((context.read_u32(COMMAND.checked_add(4)?)? >> 9) & 1)
}
pub fn helper_f2dcf4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(if helper_f2d79c(context)? != 0 { 1 } else { ((context.read_u32(COMMAND.checked_add(4)?)? >> 5) & 1) as i32 })
}
pub fn helper_f2ddd4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(helper_f2dcf4(context)? != 0 && context.read_u32(CHANNEL.checked_add(0x84)?)? == 0))
}
pub fn helper_f2d68c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.copy_bytes(CHANNEL.checked_add(0x94)?, REQUEST.checked_add(0x14)?, ByteCount(12))?;
    for (source, destination) in [(0x94, 0x18), (0x98, 0x1c), (0x9c, 0x24)] {
        let output = GuestAddress::new(context.read_u32(COMMAND.checked_add(0x38)?)?);
        let value = context.read_u32(CHANNEL.checked_add(source)?)?;
        context.write_u32(output.checked_add(destination)?, value)?;
    }
    Ok(())
}
pub fn helper_f2c3c0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<GuestAddress, MemoryError> {
    context.copy_bytes(destination, DISPLAY.checked_add(0x16c)?, ByteCount(24))?;
    Ok(destination)
}
pub fn helper_f2c8ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u32(DISPLAY.checked_add(0x154)?)? != 0))
}

// f2c898..f2c8ac has no body effects.
pub fn helper_f2c898<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }
