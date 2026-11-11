//! X-M1 1.50 scan-mode decisions, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::helper_74e5c0;
use crate::record_scan_bounds_state::{byte, state, RateFamily, SettingKey};

pub fn helper_f49fd8(mode: u32) -> u32 {
    let bit = mode.wrapping_sub(0x12);
    (bit <= 24 && (0x01ff_03bf_u32 >> bit) & 1 != 0) as u32
}
pub fn helper_f4a0e4(value: u32) -> u32 {
    match value { 2 => 0x30, 3 => 1, 4 => 4, 5 => 0xe, 6 => 5, 7 => 0xb, _ => 0 }
}
pub fn helper_f4a154(previous: u32, proposed: u32) -> u32 {
    if previous == proposed || helper_f49fd8(proposed) == 0 { return proposed; }
    match (previous, proposed) {
        (1 | 8, 0x12 | 0x13) => 0x13, (1 | 8, 0x15 | 0x16) => 0x16,
        (1 | 8, 0x19 | 0x1a) => 0x1a, (1 | 8, 0x22 | 0x23) => 0x23,
        (1 | 8, 0x25 | 0x26) => 0x26, (1 | 8, 0x28 | 0x29) => 0x29,
        (4 | 9, 0x12 | 0x14) => 0x14, (4 | 9, 0x15 | 0x17) => 0x17,
        (4 | 9, 0x19 | 0x1b) => 0x1b, (4 | 9, 0x22 | 0x24) => 0x24,
        (4 | 9, 0x25 | 0x27) => 0x27, (4 | 9, 0x28 | 0x2a) => 0x2a,
        _ => proposed,
    }
}
macro_rules! profile_setting {
    ($name:ident, $key:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<u32, MemoryError> {
            if helper_74e5c0(context, profile)? == 1 { Ok(0) } else { byte(context, SettingKey($key)) }
        }
    };
}
profile_setting!(helper_f4a554, 0x7684);
profile_setting!(helper_f4acd4, 0x76dc);
profile_setting!(helper_f4ad54, 0x76da);

pub fn helper_f4a520<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, kind: u32) -> Result<u32, MemoryError> {
    if helper_74e5c0(context, profile)? == 1 { Ok(0) }
    else { byte(context, SettingKey(if kind == 0 { 0x7683 } else { 0x76b0 })) }
}
pub fn helper_f4a57c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32) -> Result<u32, MemoryError> {
    let key = match mode { 0 => 0x7685, 5 => 0x7686, 1 => 0x7687, 4 => 0x7688, 8 => 0x7689, 9 => 0x768a, 0xb => 0x768b, 0xe => 0x768c, 0x30 => 0x768d, _ => return Ok(0) };
    Ok((byte(context, SettingKey(key))? != 0) as u32)
}
pub fn helper_f4acfc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32) -> Result<u32, MemoryError> {
    Ok(if byte(context, SettingKey(0x76dd))? != 0 && context.read_u32(state(0x8c))? == 1 {
        match mode { 0xb => 0xc, 0xe => 0xf, _ => mode }
    } else { mode })
}
pub fn helper_f4a848<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, previous: u32, proposed: u32, kind: u32) -> Result<u32, MemoryError> {
    let mut quality_filter = 0;
    let mut distance_filter = 0;
    let result = if let Some(family) = RateFamily::parse(previous) {
        let ordinal = family.ordinal();
        if byte(context, SettingKey(0x7697 + ordinal))? == 0 { proposed }
        else {
            let quality_enabled = byte(context, SettingKey(0x769d + ordinal))? != 0;
            if quality_enabled && (context.read_u16(state(0x44))? as i16) > context.read_u16(state(0x4a))? as i16 { proposed }
            else {
                quality_filter = quality_enabled as u32;
                if byte(context, SettingKey(0x76a3 + ordinal))? == 0 { previous }
                else {
                    let key = match family { RateFamily::First => 0x76a9, RateFamily::Second => 0x76ac, RateFamily::Third => 0x76aa, RateFamily::Fourth => 0x76ad, RateFamily::Fifth => 0x76ae, RateFamily::Sixth => 0x76af };
                    let lower = (byte(context, SettingKey(key))? * 10) as u16 as i32;
                    let upper = if matches!(family, RateFamily::Third) { (byte(context, SettingKey(0x76ab))? * 10) as u16 as i32 } else { i16::MAX as i32 };
                    let distance = context.read_u16(state(0x28))? as i16 as i32;
                    if distance < lower || (matches!(family, RateFamily::Third) && distance > upper) { proposed }
                    else { distance_filter = 1; previous }
                }
            }
        }
    } else { proposed };
    if kind == 0 {
        context.write_u32(state(0x19e0), quality_filter)?;
        context.write_u32(state(0x19e4), distance_filter)?;
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundVote { pub accepted: u32, pub direction: Option<u32> }
macro_rules! zero_vote {
    ($name:ident) => {
        pub fn $name<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, _profile: u32) -> Result<BoundVote, MemoryError> {
            Ok(BoundVote { accepted: 0, direction: Some(0) })
        }
    };
}
zero_vote!(helper_f4a314);
zero_vote!(helper_f4a330);
zero_vote!(helper_f4a34c);
