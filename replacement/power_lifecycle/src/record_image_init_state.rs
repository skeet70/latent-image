//! X-M1 1.50 shared image initialization state.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; actual image-5 bytes.

use crate::record_address_space::{CallbackError, CallbackId, CallbackRegistry, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RecordError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageInitError { Memory(MemoryError), Callback(CallbackError), Runtime(RecordError), DivideByZero }
impl From<MemoryError> for ImageInitError { fn from(error: MemoryError) -> Self { Self::Memory(error) } }
impl From<CallbackError> for ImageInitError { fn from(error: CallbackError) -> Self { Self::Callback(error) } }
impl From<RecordError> for ImageInitError { fn from(error: RecordError) -> Self { Self::Runtime(error) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticResidue(pub u16);

pub(crate) fn word(memory: &mut impl GuestAddressSpace, address: usize) -> Result<u32, ImageInitError> { Ok(memory.read_u32(GuestAddress::new(address as u32))?) }
pub(crate) fn put(memory: &mut impl GuestAddressSpace, address: usize, value: u32) -> Result<(), ImageInitError> { Ok(memory.write_u32(GuestAddress::new(address as u32), value)?) }
pub(crate) fn put_byte(memory: &mut impl GuestAddressSpace, address: usize, value: u8) -> Result<(), ImageInitError> { Ok(memory.write_u8(GuestAddress::new(address as u32), value)?) }
pub(crate) fn put_half(memory: &mut impl GuestAddressSpace, address: usize, value: u16) -> Result<(), ImageInitError> { Ok(memory.write_u16(GuestAddress::new(address as u32), value)?) }
pub(crate) fn clear(memory: &mut impl GuestAddressSpace, address: usize, bytes: usize) -> Result<(), ImageInitError> {
    for offset in 0..bytes { put_byte(memory, address + offset, 0)?; }
    Ok(())
}

pub fn helper_f4c384<M: GuestAddressSpace, E>(context: &mut RecordContext<M,E>) -> Result<(), ImageInitError> {
    put(context, 0x016c2370, 1)?;
    for address in 0x016c901c..0x016c9148 { put_byte(context, address, 0xe0)?; }
    Ok(())
}
pub fn helper_f4c3b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M,E>) -> Result<(), ImageInitError> { put(context, 0x016c2370, 0) }
pub fn helper_10546d0<M: GuestAddressSpace, E>(context: &mut RecordContext<M,E>) -> Result<(), ImageInitError> { put(context, 0x01706a3c, 1) }
pub fn helper_10546e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M,E>) -> Result<(), ImageInitError> { put(context, 0x01706a3c, 0) }
pub fn helper_1050f8c<M: GuestAddressSpace, E>(_: &mut RecordContext<M,E>) -> Result<u32, ImageInitError> { Ok(0) }
pub fn helper_1050fa4<M: GuestAddressSpace, E>(_: &mut RecordContext<M,E>) -> Result<u32, ImageInitError> { Ok(0) }
pub fn helper_1051fb4<M: GuestAddressSpace, E>(_: &mut RecordContext<M,E>) -> Result<u32, ImageInitError> { Ok(0) }
pub fn helper_1051fcc<M: GuestAddressSpace, E>(_: &mut RecordContext<M,E>) -> Result<u32, ImageInitError> { Ok(0) }

pub fn helper_fea418<M: GuestAddressSpace, E>(context: &mut RecordContext<M,E>, window: GuestAddress) -> Result<(), ImageInitError> {
    let primary = context.read_u32(window)?;
    if primary != 0 {
        put(context, 0x016f8024, primary)?;
        put(context, 0x016f803c, primary.wrapping_add(0x63d800))?;
        put(context, 0x016f8054, primary.wrapping_add(0xc7b000))?;
    }
    let secondary = context.read_u32(window.checked_add(4)?)?;
    if secondary != 0 {
        put(context, 0x016f8070, secondary)?;
        put(context, 0x016f8088, secondary.wrapping_add(0xa71c00))?;
        put(context, 0x016f80a0, secondary.wrapping_add(0x14e3800))?;
    }
    // 75e6f0 has no effects; 75e6d8 returns one without reading its arguments.
    put(context, 0x016f08c0, 1)
}
pub fn helper_fea428<M: GuestAddressSpace, E>(context: &mut RecordContext<M,E>) -> Result<(), ImageInitError> { put(context, 0x016f08c0, 0) }

pub fn helper_1039610<M: GuestAddressSpace, E, const N: usize>(context: &mut RecordContext<M,E>, callback: Option<CallbackId>, registry: &CallbackRegistry<RecordContext<M,E>,N>) -> Result<u32, ImageInitError> {
    put(context, 0x016fec34 + 0x445c, 0)?;
    if let Some(callback) = callback { registry.invoke_status(callback, context, 0)?; }
    Ok(0)
}
