//! X-M1 1.50 packed comparison and conditional canonical publication.

use crate::capture_profile_bindings::*;
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_735d54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, input: GuestAddress) -> Result<u32, MemoryError> {
    let previous = c75b290(context, profile)?.to_le_bytes();
    for (offset, byte) in previous.into_iter().enumerate() {
        if byte != context.read_u8(input.checked_add(offset as u32)?)? { return c75b2c0(context, profile, input); }
    }
    Ok(0)
}

pub fn helper_735dac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, input: GuestAddress, comparison: GuestAddress) -> Result<u32, MemoryError> {
    c75b830(context, comparison, profile)?;
    for offset in 0..8 {
        let previous = context.read_u8(comparison.checked_add(offset)?)?;
        if previous != context.read_u8(input.checked_add(offset)?)? { return c75b854(context, profile, input); }
    }
    Ok(0)
}

pub fn helper_735df8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, input: GuestAddress, comparison: GuestAddress) -> Result<u32, MemoryError> {
    c75b9e8(context, comparison, profile)?;
    for offset in 0..6 {
        let previous = context.read_u8(comparison.checked_add(offset)?)?;
        if previous != context.read_u8(input.checked_add(offset)?)? { return c75ba08(context, profile, input); }
    }
    Ok(0)
}

pub fn helper_73611c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, input: GuestAddress) -> Result<u32, MemoryError> {
    let previous = (c75be78(context, profile)? as u16).to_le_bytes();
    for (offset, byte) in previous.into_iter().enumerate() {
        if byte != context.read_u8(input.checked_add(offset as u32)?)? { return c75bea8(context, profile, input); }
    }
    Ok(0)
}
