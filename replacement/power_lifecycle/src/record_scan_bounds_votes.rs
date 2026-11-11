//! X-M1 1.50 scan-bound votes, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{helper_74e5c0, helper_74e7f0, helper_74f1ac, helper_750584, helper_75071c, ProfileHandle};
use crate::record_scan_bounds_state::{byte, half, signed_byte, state, SettingKey};
use crate::record_scan_bounds_modes::helper_f4acd4;

fn response_changed<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileHandle, kind: u32) -> Result<bool, MemoryError> {
    if byte(context, SettingKey(0x75fa))? == 0 { return Ok(false); }
    if kind == 1 { return Ok(context.read_u32(state(0x34))? != 0); }
    let current = context.read_u16(state(0x30))?;
    let previous = context.read_u16(state(0x32))?;
    let difference = current.wrapping_sub(previous) as i16;
    let key = if helper_74e5c0(context, profile.0)? == 1 { 0x75ff } else { 0x75fc };
    Ok(difference as i32 > (byte(context, SettingKey(key))? * 10) as i16 as i32)
}
fn distance_below<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileHandle) -> Result<bool, MemoryError> {
    if byte(context, SettingKey(0x75fb))? == 0 { return Ok(false); }
    let category = helper_74e5c0(context, profile.0)?;
    let distance = context.read_u16(state(0x28))? as i16 as i32;
    let key = if category == 1 { 0x7600 } else { 0x75fd };
    Ok(distance < (byte(context, SettingKey(key))? * 10) as i16 as i32)
}
pub fn helper_f4a368<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> { Ok(response_changed(context, ProfileHandle(profile), kind)? as u32) }
pub fn helper_f4a3fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<u32, MemoryError> { Ok(distance_below(context, ProfileHandle(profile))? as u32) }
pub fn helper_f4a464<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    if context.read_u32(state(0x20))? == 0 || context.read_u32(state(0x24))? == 0 { return Ok(0); }
    let profile = ProfileHandle(profile);
    Ok(if byte(context, SettingKey(0x75f9))? != 0 {
        if distance_below(context, profile)? { 0xe } else if response_changed(context, profile, kind)? { 4 } else { 1 }
    } else if response_changed(context, profile, kind)? { 4 }
    else if distance_below(context, profile)? { 0xe } else { 1 })
}
fn category_allowed<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<bool, MemoryError> {
    let elapsed = context.read_u32(state(0x4c))?;
    let key = if elapsed < half(context, SettingKey(0x75e9))? { 0x75ec } else { 0x75eb };
    Ok(match byte(context, SettingKey(key))? {
        0 => true,
        1 => context.read_u32(state(0x3c))? == 0,
        _ => matches!(context.read_u32(state(0x3c))?, 0 | 2 | 3),
    })
}
fn profile_allowed<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileHandle) -> Result<bool, MemoryError> {
    if helper_74e5c0(context, profile.0)? == 1 { return Ok(false); }
    if matches!(helper_74f1ac(context, profile.0)?, 2 | 3 | 7) { return Ok(false); }
    Ok(helper_75071c(context, profile.0)? != 1 && helper_74e7f0(context, profile.0)? == 0)
}
pub fn helper_f4b26c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    if !profile_allowed(context, ProfileHandle(profile))? { return Ok(0); }
    let blocked = if kind == 1 {
        context.read_u32(state(0x40))? > 1 || (context.read_u16(state(0x44))? as i16) < context.read_u16(state(0x48))? as i16
    } else { !category_allowed(context)? || (context.read_u16(state(0x44))? as i16) < context.read_u16(state(0x48))? as i16 };
    if blocked {
        let position = context.read_u16(state(0x2a))? as i16;
        if context.read_u16(state(0x5e))? as i16 <= position && position <= context.read_u16(state(0x5c))? as i16 { helper_f4acd4(context, profile)?; }
    }
    Ok(0)
}
pub fn helper_f4b49c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, _kind: u32, selection: u32, flags: u16) -> Result<u16, MemoryError> {
    if helper_750584(context, profile)? == 0 || byte(context, SettingKey(0x78d0))? == 0 || !profile_allowed(context, ProfileHandle(profile))? { return Ok(flags); }
    if !matches!(selection, 0 | 0xb | 0x30) { return Ok(flags); }
    let (position, lower, upper) = if selection == 0xb {
        (context.read_u16(state(0x2c))? as i16, context.read_u16(state(0x62))? as i16, context.read_u16(state(0x60))? as i16)
    } else { (context.read_u16(state(0x2a))? as i16, context.read_u16(state(0x5e))? as i16, context.read_u16(state(0x5c))? as i16) };
    if lower <= position && position <= upper {
        if helper_f4acd4(context, profile)? != 0 && context.read_u32(state(0x8c))? == 1 { return Ok(flags); }
        return Ok(flags | 2);
    }
    Ok(flags)
}
pub fn helper_f4b5b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    if (kind != 1 && !category_allowed(context)?) || (context.read_u16(state(0x44))? as i16) < context.read_u16(state(0x48))? as i16 { return Ok(0); }
    let key = if helper_74e5c0(context, profile)? == 1 { 0x7603 } else { 0x75f7 };
    let distance = context.read_u16(state(0x28))? as i16 as i32;
    Ok(if distance >= signed_byte(context, SettingKey(key))?.wrapping_mul(10) { 0x30 } else { 0 })
}
pub fn helper_f4b7b4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    let category = helper_74e5c0(context, profile)?;
    let distance = context.read_u16(state(0x28))? as i16 as i32;
    let key = if category == 1 { 0x7602 } else { 0x75f2 };
    if distance > signed_byte(context, SettingKey(key))?.wrapping_mul(10) { return Ok(0); }
    if kind == 1 {
        if byte(context, SettingKey(0x75f5))? != 0 && (context.read_u32(state(0x40))? > 1 || (context.read_u16(state(0x44))? as i16) > context.read_u16(state(0x4a))? as i16) { return Ok(0); }
        if byte(context, SettingKey(0x75f3))? != 0 && (byte(context, SettingKey(0x75f3))? == 0 || context.read_u32(state(0x58))? == 0) { return Ok(0); }
    } else if byte(context, SettingKey(0x75f4))? == 0 { return Ok(0); }
    if byte(context, SettingKey(0x75f5))? == 0 { return Ok(0xb); }
    Ok(if category_allowed(context)? && (context.read_u16(state(0x44))? as i16) <= context.read_u16(state(0x4a))? as i16 { 0xb } else { 0 })
}
pub fn helper_f4ba3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    if kind == 1 {
        if context.read_u32(state(0x40))? > 1 || (context.read_u16(state(0x44))? as i16) > context.read_u16(state(0x4a))? as i16 { return Ok(0); }
    } else if !category_allowed(context)? || (context.read_u16(state(0x44))? as i16) > context.read_u16(state(0x4a))? as i16 { return Ok(0); }
    let key = if helper_74e5c0(context, profile)? == 1 { 0x7601 } else { 0x75f0 };
    let distance = context.read_u16(state(0x28))? as i16 as i32;
    Ok(if distance >= signed_byte(context, SettingKey(key))?.wrapping_mul(10) { 5 } else { 0 })
}
pub fn helper_f4bc58<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, _kind: u32, selection: u32) -> Result<u32, MemoryError> {
    if !matches!(selection, 1 | 4) { return Ok(selection); }
    let key = if helper_74e5c0(context, profile)? == 1 { 0x7601 } else { 0x75f0 };
    let distance = context.read_u16(state(0x28))? as i16 as i32;
    Ok(if distance >= signed_byte(context, SettingKey(key))?.wrapping_mul(10) { if selection == 1 { 8 } else { 9 } } else { selection })
}
