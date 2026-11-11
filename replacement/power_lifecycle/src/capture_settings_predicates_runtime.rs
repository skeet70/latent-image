//! X-M1 1.50 predicate state, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::capture_settings_predicates::CaptureSettings;

const RECORDING: GuestAddress = GuestAddress::new(0x0152_6a78);

pub(crate) fn selection_active<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, settings: CaptureSettings) -> Result<bool, MemoryError> {
    if !matches!(settings.word::<0x4c>(context)?, 4..=7) { return Ok(false); }
    if settings.word::<0xbc>(context)? != 0xa4 { return Ok(false); }
    if settings.word::<0xf8>(context)? != 1 { return Ok(true); }
    Ok(!matches!(settings.word::<0xa4>(context)?, 1 | 2))
}
pub(crate) fn output_state<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(RECORDING.checked_add(0x9578)?)
}
pub(crate) fn configuration_mode<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    crate::record_configuration::helper_69f8fc(context, 0xa902)
}
pub(crate) fn adjust_mode(value: u32, mode: u32) -> u32 { if mode == 6 { 0 } else { value } }

fn output_flags(memory: &mut impl GuestAddressSpace) -> Result<u16, MemoryError> {
    let disabled = memory.read_u32(GuestAddress::new(0x0095_5b20))?;
    let bank = if disabled != 0 { 0 } else { u32::from(memory.read_u32(GuestAddress::new(0x0095_5b08))? == 1) };
    memory.read_u16(GuestAddress::new(0x0096_b9bc).checked_add(bank * 0x1a4 + 0x54)?)
}
pub(crate) fn output_disabled<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<bool, MemoryError> {
    if output_flags(context)? & 1 != 0 { return Ok(true); }
    if output_flags(context)? & 2 != 0 { return Ok(true); }
    Ok(context.read_u32(RECORDING.checked_add(0xc204)?)? == 1)
}
pub(crate) fn output_class<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(0x00dd_1ab8))
}
pub(crate) fn auxiliary_state<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(RECORDING.checked_add(0x957c)?)
}
pub(crate) fn auxiliary_pending<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(RECORDING.checked_add(0x63fc)?)
}
pub(crate) fn bytes_differ(memory: &mut impl GuestAddressSpace, first: GuestAddress, second: GuestAddress, count: u32) -> Result<bool, MemoryError> {
    for offset in 0..count {
        if memory.read_u8(first.checked_add(offset)?)? != memory.read_u8(second.checked_add(offset)?)? { return Ok(true); }
    }
    Ok(false)
}
pub(crate) fn capture_flags<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let source = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3540)?)?);
    context.read_u32(source)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePredicateError {
    Memory(MemoryError),
    InvalidAuxiliaryState(u16),
}
impl From<MemoryError> for CapturePredicateError {
    fn from(error: MemoryError) -> Self { Self::Memory(error) }
}

#[derive(Clone, Copy)]
#[repr(u16)]
enum AuxiliaryState { Idle, Starting, Waiting, Ready, Alternate, Stopping }
impl AuxiliaryState {
    fn parse(value: u16) -> Result<Self, CapturePredicateError> {
        match value {
            0 => Ok(Self::Idle), 1 => Ok(Self::Starting), 2 => Ok(Self::Waiting),
            3 => Ok(Self::Ready), 4 => Ok(Self::Alternate), 5 => Ok(Self::Stopping),
            _ => Err(CapturePredicateError::InvalidAuxiliaryState(value)),
        }
    }
}

pub fn helper_dec93c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, CapturePredicateError> {
    context.write_u32(RECORDING.checked_add(0xb488)?, 0)?;
    context.write_u32(RECORDING.checked_add(0xb48c)?, 0)?;
    context.write_u16(GuestAddress::new(0x0154_57f6), 0)?;
    let state = AuxiliaryState::parse(context.read_u16(GuestAddress::new(0x0154_58b6))?)?;
    context.write_u32(RECORDING.checked_add(0xb490)?, state as u32)?;
    let response = context.read_u32(RECORDING.checked_add(0xb490)?)?;
    let _result = context.read_u32(RECORDING.checked_add(0xb494)?)?;
    Ok(response)
}

pub(crate) fn capture_mode<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, CapturePredicateError> {
    helper_dec93c(context)
}
