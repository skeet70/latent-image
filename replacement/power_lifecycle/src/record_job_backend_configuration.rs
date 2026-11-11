//! X-M1 1.50 job configuration selectors at f5ecb0/f5ecc0/f6efcc/f6fa5c.
use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc;

pub fn helper_f6fa5c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, kind: u32, mode: u32, flags: u32) -> Result<u32, MemoryError> {
    if flags & 2 != 0 || kind == 1 { return Ok(0x64bc); }
    Ok(match mode { 1..=4 | 8 | 9 | 14 | 15 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42 => 0x64d4, 5..=7 | 24 | 25 | 34 | 37 | 40 => 0x64e0, 10 => 0x64c8, 11..=13 => 0x64ec, 16 | 33 => 0x64f8, 17 => 0x6504, _ => 0x64bc })
}

pub fn helper_f5ecb0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, mode: u32, flags: u32) -> Result<u32, MemoryError> {
    helper_f6fa5c(context, kind, mode, flags)
}

pub fn helper_f5ecc0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32, flags: u32, alternate: u32, timing: u32) -> Result<i32, MemoryError> {
    if alternate == 7 || alternate.wrapping_sub(4) < 2 { return helper_69f8fc(context, 0x6512).map(|value| value as i32); }
    if mode.wrapping_sub(11) < 3 {
        let value = helper_69f8fc(context, 0x6513)?;
        return Ok(if flags & 2 == 0 { value } else { helper_69f8fc(context, 0x6516)? } as i32);
    }
    helper_69f8fc(context, if timing == 0 { 0x6516 } else if timing == 2 { 0x6511 } else { 0x6515 }).map(|value| value as i32)
}

pub fn helper_f6efcc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, mode: u32, flags: u32, disabled: u32, alternate: u32, selector: u32) -> Result<u32, MemoryError> {
    if disabled != 0 || kind == 1 { return Ok(0); }
    if flags & 2 != 0 { return Ok(u32::from(helper_69f8fc(context, 0x6595)? & 0x20 != 0)); }
    let (key, mask) = match mode {
        1..=4 | 8 | 9 | 14 | 15 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42 => (0x6594, 1),
        5..=7 | 34 | 37 | 40 => (0x6594, 2), 10 => (0x6594, 4), 11 | 12 => (0x6594, 8),
        13 | 45..=47 | 54..=56 => return Ok(0), 18 => (0x6595, 1), 24 => (0x6595, 2), 25 => (0x6595, 4),
        43 | 44 | 57 => if alternate != 0 { (0x65a3, 1) } else { match selector { 2 => (0x65a3, 2), 3 => (0x65a3, 4), 0 => (0x65a3, 16), _ => return Ok(0) } },
        49 => (0x6595, 8), 50 => (0x6595, 16), 51 => (0x6595, 32), 52 => (0x6596, 1), _ => return Ok(1),
    };
    Ok(u32::from(helper_69f8fc(context, key)? & mask != 0))
}
