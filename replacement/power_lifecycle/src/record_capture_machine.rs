use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[path = "record_capture_machine_dispatch.rs"]
pub mod dispatch;

const CONTEXT: GuestAddress = GuestAddress::new(0x016f82fc);

pub fn helper_1002924<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    for offset in [0xb0, 0xc4, 0xc8, 0x19c, 0x1a4, 0x1a8, 0x1b0, 0x1b4, 0x1b8, 0x1bc, 0x1c8] {
        context.write_u32(CONTEXT.checked_add(offset)?, 0)?;
    }
    for offset in (0x140..0x180).step_by(4) {
        context.write_u32(CONTEXT.checked_add(offset)?, 0)?;
    }
    context.write_u8(CONTEXT.checked_add(0x199)?, 0)
}

pub fn helper_10028f4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, requested: ByteCount) -> Result<ByteCount, MemoryError> {
    let length = ByteCount(requested.0.min(0x240));
    context.copy_bytes(destination, CONTEXT, length)?;
    Ok(length)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureKind(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureRequestFlags(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureMode(pub u32);

pub fn helper_fea3a0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, kind: CaptureKind, request: CaptureRequestFlags, mode: CaptureMode, fallback: CaptureRequestFlags) -> Result<u32, MemoryError> {
    let use_request = fallback.0 & 2 != 0 || matches!(kind.0, 0x31 | 0x32);
    let enabled = if use_request { request.0 & 2 != 0 } else { true };
    Ok(u32::from(enabled && mode.0 != 3))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureDispatchError {
    Memory(MemoryError),
    State(dispatch::InvalidCaptureState),
}

impl From<MemoryError> for CaptureDispatchError {
    fn from(error: MemoryError) -> Self { Self::Memory(error) }
}

pub fn select_handler<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, event: dispatch::CaptureEvent) -> Result<Option<dispatch::CaptureHandler>, CaptureDispatchError> {
    context.write_u32(CONTEXT.checked_add(0xb4)?, event.0)?;
    let state = context.read_u32(CONTEXT.checked_add(0xb0)?)?;
    let state = dispatch::CaptureState::try_from(state).map_err(CaptureDispatchError::State)?;
    Ok(dispatch::route(state, event))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedCaptureValue(pub u32);

pub fn helper_edbad4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: SelectedCaptureValue) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(0x01698b4c), value.0)
}
