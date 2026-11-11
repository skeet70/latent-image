//! X-M1 1.50 actual738800 projection through the shared admitted address space.

use crate::capture_profile_bindings::*;
use crate::capture_profile_primary::CaptureSettings;
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy)]
pub struct CaptureWork { address: GuestAddress }

impl CaptureWork {
    pub fn new(address: GuestAddress) -> Result<Self, MemoryError> {
        address.checked_add(359)?;
        Ok(Self { address })
    }
    fn selection<M: GuestAddressSpace, E>(&self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        context.read_u32(self.address.checked_add(0x164)?)
    }
}

#[derive(Clone, Copy)]
pub struct SecondaryScratch { pub values: GuestAddress }

pub fn project_secondary<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, settings: CaptureSettings, work: CaptureWork, scratch: SecondaryScratch) -> Result<u32, MemoryError> {
    let p = profile;
    let values = scratch.values;
    macro_rules! call {
        ($helper:ident) => { $helper(context)? };
        ($helper:ident, $first:expr) => {{ let first = $first; $helper(context, first)? }};
        ($helper:ident, $first:expr, $second:expr) => {{ let first = $first; let second = $second; $helper(context, first, second)? }};
        ($helper:ident, $first:expr, $second:expr, $third:expr) => {{ let first = $first; let second = $second; let third = $third; $helper(context, first, second, third)? }};
        ($helper:ident, $first:expr, $second:expr, $third:expr, $fourth:expr) => {{ let first = $first; let second = $second; let third = $third; let fourth = $fourth; $helper(context, first, second, third, fourth)? }};
    }
    macro_rules! update {
        ($get:ident, $set:ident, $value:expr) => {{
            let previous = $get(context, p)?;
            let value = $value;
            if previous != value { $set(context, p, value)?; }
        }};
    }
    update!(c75d668, c75d680, call!(c737854, settings.word(context, 0x3f)?, settings.word(context, 0x40)?));
    call!(c7378fc, values);
    call!(c735fc0, p, values);
    update!(c75d710, c75d728, call!(c73790c, settings.word(context, 0x40)?, settings.word(context, 0x41)?));
    update!(c75d740, c75d758, call!(c737968, settings.word(context, 0xec)?));
    update!(c75d770, c75d788, call!(c737988));
    update!(c75d7a0, c75d7b8, call!(c7379a0, settings.word(context, 0xec)?, settings.word(context, 0)?, settings.word(context, 0x17)?));
    call!(c737aa8, values, settings.word(context, 0x3a)?, settings.word(context, 0x3b)?, work.selection(context)?);
    update!(c75d890, c75d8a8, context.read_u32(values)?);
    update!(c75d8c0, c75d8d8, context.read_u32(values.checked_add(4)?)?);
    update!(c75d8f0, c75d908, context.read_u32(values.checked_add(8)?)?);
    call!(c7360a4, p, values.checked_add(12)?);
    context.write_u32(values, 0)?;
    call!(c7360f0, p, values);
    let previous = call!(c75d698, p);
    let value = call!(c73788c, settings.word(context, 0x32)?, settings.word(context, 0x3f)?);
    Ok(if previous != value { call!(c75d6b0, p, value) } else { value })
}

pub fn helper_738800<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, settings: GuestAddress, work: GuestAddress, scratch: SecondaryScratch) -> Result<u32, MemoryError> {
    project_secondary(context, profile, CaptureSettings::new(settings)?, CaptureWork::new(work)?, scratch)
}

pub fn helper_7378cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<u32, MemoryError> {
    context.write_u32(output, 0)?;
    context.write_u32(output.checked_add(4)?, 0)?;
    context.write_u32(output.checked_add(8)?, 480)?;
    context.write_u32(output.checked_add(12)?, 640)?;
    Ok(output.get())
}

pub fn helper_7378fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<u32, MemoryError> {
    helper_7378cc(context, output)
}

pub fn helper_737968<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok((value == 1) as u32)
}

pub fn helper_737988<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(0)
}
