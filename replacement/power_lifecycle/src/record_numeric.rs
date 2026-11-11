use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors as profile;
use crate::record_profile_globals as globals;
use crate::record_profile_mapping as mapping;

fn state(memory: &mut impl GuestAddressSpace, offset: u32) -> Result<u32, MemoryError> { memory.read_u32(GuestAddress::new(0x016d8708).checked_add(offset)?) }
fn set_state(memory: &mut impl GuestAddressSpace, offset: u32, value: u32) -> Result<(), MemoryError> { memory.write_u32(GuestAddress::new(0x016d8708).checked_add(offset)?, value) }

pub fn helper_fa6904<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, active: u32, profile: u32) -> Result<u32, MemoryError> {
    if active != 0 { return Ok(4); }
    let category = context.read_u8(GuestAddress::new(0x00ef6df2).checked_add(profile.checked_mul(20).ok_or(MemoryError::AddressOverflow)?)?)?;
    Ok(if category == 0 { 2 } else { 3 })
}

pub fn helper_fa7634<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if profile::helper_7503ec(context, handle)? != 1 { profile::helper_74e648(context, handle) }
    else if profile::helper_74e5c0(context, handle)? == 1 { profile::helper_74e68c(context, handle) }
    else { Ok(0) }
}

pub fn helper_fa7954<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if profile::helper_7503ec(context, handle)? == 1 { profile::helper_74e68c(context, handle) } else { profile::helper_74e648(context, handle) }
}

pub fn helper_fa7984<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if profile::helper_7503ec(context, handle)? == 1 { profile::helper_74e7a0(context, handle) } else { profile::helper_74e718(context, handle) }
}

pub fn helper_fa7928<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if profile::helper_7503ec(context, handle)? == 1 { Ok(0) } else { profile::helper_74e648(context, handle) }
}

pub fn helper_fa6bb4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let initial = profile::helper_74f894(context, handle)?;
    let selected = if profile::helper_74e5c0(context, handle)? == 1 || initial == 0 { globals::helper_772f28(context, 3)? } else { initial };
    Ok(selected as i16 as i32 as u32)
}

pub fn helper_fa7edc<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(3) }
pub fn helper_fa8018<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { state(context, 0x48) }
pub fn helper_fa9588<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { state(context, 0x44) }
pub fn helper_faa790<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { state(context, 0x58) }
pub fn helper_faa770<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { set_state(context, 0x58, value) }
pub fn helper_fa7f40<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { state(context, 4) }
pub fn helper_fa7fd8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { state(context, 0x54) }
pub fn helper_fa7fb8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { set_state(context, 0x54, value) }
pub fn helper_fa7ff8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { set_state(context, 0x48, value) }
pub fn helper_fa8038<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { set_state(context, 0x4c, value) }
pub fn helper_fa8058<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { state(context, 0x4c) }
pub fn helper_fa7e8c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { set_state(context, 0x40, value) }
pub fn helper_fa7eac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { state(context, 0x40) }
pub fn helper_fa7ef4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(u32::from(state(context, 0)? == 1)) }

pub fn helper_fa7614<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _: u32, minimum: u32) -> Result<u32, MemoryError> {
    Ok(crate::record_configuration::helper_69f8fc(context, 0x8806)?.max(minimum))
}

pub fn helper_fab43c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: u32) -> Result<u32, MemoryError> {
    let value = mapping::helper_746864(context, channel, 0)?;
    let key = match value { 0..400 => 0x8ba1, 400..560 => 0x8ba2, 560..800 => 0x8ba3, _ => 0x8ba4 };
    Ok(crate::record_configuration::helper_69f8fc(context, key)? as i16 as i32 as u32)
}

pub fn helper_fab250<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32, category: u32, _: u32, enabled: u32) -> Result<u32, MemoryError> {
    if enabled != 1 || crate::record_configuration::helper_69f8fc(context, 0xa7e4)? & 1 == 0 { return Ok(0); }
    Ok(u32::from(mode == 1 || matches!(category, 0..=4 | 8..=9 | 14..=23 | 26..=27 | 30..=31 | 33 | 35..=36 | 38..=39 | 41..=50 | 57)))
}

pub fn helper_faa7b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, record: GuestAddress, position: GuestAddress, flags: GuestAddress) -> Result<(), MemoryError> {
    let value = profile::helper_74e648(context, handle)?;
    context.write_u32(position, value)?;
    let value = profile::helper_74e718(context, handle)? as u16;
    context.write_u16(flags, value)?;
    let value = if context.read_u32(record.checked_add(0x10)?)? == 1 { value | 1 } else { value & !1 };
    context.write_u16(flags, value)
}

pub fn helper_faaae8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if profile::helper_74e5c0(context, handle)? == 0 { Ok(1) } else { Ok(profile::helper_74f124(context, handle)?.max(1)) }
}

pub fn helper_fa9320<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, limit: u32, first: u32, baseline: u32, second: u32) -> Result<u32, MemoryError> {
    let offset = crate::record_configuration::helper_69f8fc(context, 0x8823)?;
    Ok(u32::from(baseline.wrapping_add(second.wrapping_mul(first) / 100).wrapping_add(offset.wrapping_mul(10)) < limit))
}

fn history_address(history: GuestAddress, index: u32) -> Result<GuestAddress, MemoryError> {
    history.checked_add(index.checked_mul(4).ok_or(MemoryError::AddressOverflow)?)
}

pub fn helper_fa936c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, history: GuestAddress, index: u32, cursor: GuestAddress, threshold: u32, phase: u32) -> Result<u32, MemoryError> {
    let required_phase = crate::record_configuration::helper_69f8fc(context, 0x8833)?;
    let required_other = crate::record_configuration::helper_69f8fc(context, 0x8831)?;
    let offset = crate::record_configuration::helper_69f8fc(context, 0x8823)?;
    if index == 0 { return Ok(0); }
    let current = context.read_u32(history_address(history, index)?)?;
    let mut position = index;
    let mut count = 0u8;
    while position != 0 {
        let previous = context.read_u32(history_address(history, position - 1)?)?;
        if context.read_u32(history_address(history, position)?)? >= previous { break; }
        context.write_u32(cursor, previous)?;
        position -= 1;
        count = count.wrapping_add(1);
        if u32::from(count) == index { break; }
    }
    if count == 0 { return Ok(0); }
    let required = if phase == 1 { required_phase } else { required_other };
    if required <= u32::from(count) { return Ok(1); }
    let scaled = (u64::from(context.read_u32(cursor)?) * u64::from(threshold) / 100) as u32;
    let difference = context.read_u32(history_address(history, position)?)?.wrapping_sub(current);
    Ok(u32::from(offset.wrapping_mul(10).wrapping_add(scaled) <= difference))
}

pub fn helper_fa9488<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, history: GuestAddress, index: u32) -> Result<u32, MemoryError> {
    let required = crate::record_configuration::helper_69f8fc(context, 0x8832)?;
    let threshold = crate::record_configuration::helper_69f8fc(context, 0x8830)?;
    if index == 0 { return Ok(0); }
    let current = context.read_u32(history_address(history, index)?)?;
    let mut position = index;
    let mut count = 0u8;
    while position != 0 {
        let current_position = context.read_u32(history_address(history, position)?)?;
        let previous = context.read_u32(history_address(history, position - 1)?)?;
        if current_position >= previous { break; }
        position -= 1;
        count = count.wrapping_add(1);
        if u32::from(count) == index { break; }
    }
    if count == 0 || u32::from(count) < required { return Ok(0); }
    Ok(u32::from(threshold.wrapping_mul(100) <= context.read_u32(history_address(history, position)?)?.wrapping_sub(current)))
}

fn rate_policy_index(first: u32, second: u32) -> u32 {
    if first == 1 { match second { 22 | 4 | 6 => 4, 23 | 15 => 5, 24 | 5 => 6, 25 | 16 => 7, _ => 2 } }
    else if matches!(second, 19 | 2 | 26 | 17 | 27) { 1 } else { 0 }
}

pub fn helper_fa7a7c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32, group: u32) -> Result<u32, MemoryError> {
    let index = rate_policy_index(first, second).checked_add(group.checked_mul(8).ok_or(MemoryError::AddressOverflow)?).ok_or(MemoryError::AddressOverflow)?;
    let key = context.read_u32(GuestAddress::new(0x00faaed8).checked_add(index.checked_mul(4).ok_or(MemoryError::AddressOverflow)?)?)?;
    crate::record_configuration::helper_69f8fc(context, key)
}

pub fn helper_fa7aa4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32, alternate: u32, category: u32, output: GuestAddress) -> Result<(), MemoryError> {
    let index = rate_policy_index(first, second);
    let table = match (alternate == 0, category == 2) { (true, true) => 0x00fab1f0, (true, false) => 0x00fab050, (false, true) => 0x00faafd0, (false, false) => 0x00fab0b0 };
    let address = GuestAddress::new(table).checked_add(index * 12)?;
    let first_key = context.read_u32(address)?;
    let second_key = context.read_u32(address.checked_add(4)?)?;
    let third_key = context.read_u32(address.checked_add(8)?)?;
    let value = crate::record_configuration::helper_69f8fc(context, first_key)?;
    context.write_u8(output.checked_add(0x40)?, value as u8)?;
    let value = crate::record_configuration::helper_69f8fc(context, second_key)?.wrapping_mul(10) as u16;
    context.write_u16(output.checked_add(0x3e)?, value)?;
    let value = crate::record_configuration::helper_69f8fc(context, third_key)?.wrapping_mul(10) as u16;
    context.write_u16(output.checked_add(0x3c)?, value)?;
    let value = crate::record_configuration::helper_69f8fc(context, 0x63b8)? as u16;
    context.write_u16(output.checked_add(0x3a)?, value)
}

pub fn helper_fa67a8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, query: GuestAddress) -> Result<u32, MemoryError> {
    if context.read_u32(query.checked_add(4)?)? == 1 { return context.read_u32(query.checked_add(24)?); }
    let kind = context.read_u16(query)? as i16 as i32 as u32;
    let kind = globals::helper_772ff8(context, kind)?;
    if kind <= 10 && (1u32 << kind) & 0x606 != 0 { return Ok(1); }
    if context.read_u32(query.checked_add(8)?)? == 57 { return Ok(2); }
    if context.read_u32(query.checked_add(20)?)?.wrapping_sub(2) <= 1 { return Ok(1); }
    // The X-M1 ef68a4 callee returns zero for every input.
    context.read_u32(query.checked_add(24)?)
}

pub fn helper_fa6590<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    let high = crate::record_configuration::helper_69f8fc(context, 0x6399)?;
    let middle = crate::record_configuration::helper_69f8fc(context, 0x6398)?;
    let low = crate::record_configuration::helper_69f8fc(context, 0x6397)?;
    Ok(value.wrapping_mul((high << 16) | (middle << 8) | low))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateWeights { pub first: u16, pub second: u16, pub third: u16 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateComponentSelection { All, First, Second, Third }
impl RateComponentSelection {
    pub const fn from_raw(value: u32) -> Self { match value { 1 => Self::First, 2 => Self::Second, 3 => Self::Third, _ => Self::All } }
}

pub fn weighted_rates(first: u32, second: u32, third: u32, selection: RateComponentSelection, weights: Option<RateWeights>) -> u32 {
    let Some(weights) = weights else { return 0; };
    let weighted = |value: u32, weight: u16| ((u64::from(value) * u64::from(weight)) >> 8) as u32;
    match selection {
        RateComponentSelection::Second => weighted(second, weights.second),
        RateComponentSelection::Third => weighted(third, weights.third),
        RateComponentSelection::First => weighted(first, weights.first),
        RateComponentSelection::All => weighted(second.wrapping_mul(3), weights.second).wrapping_add(weighted(third, weights.third)).wrapping_add(weighted(first.wrapping_mul(3), weights.first)),
    }
}

pub fn helper_fa6c04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32, third: u32, selection: u32, weights: GuestAddress) -> Result<u32, MemoryError> {
    if weights.get() == 0 { return Ok(0); }
    let selection = RateComponentSelection::from_raw(selection);
    let second_weight = if matches!(selection, RateComponentSelection::All | RateComponentSelection::Second) { context.read_u16(weights)? } else { 0 };
    let first_weight = if matches!(selection, RateComponentSelection::All | RateComponentSelection::First) { context.read_u16(weights.checked_add(2)?)? } else { 0 };
    let third_weight = if matches!(selection, RateComponentSelection::All | RateComponentSelection::Third) { context.read_u16(weights.checked_add(4)?)? } else { 0 };
    Ok(weighted_rates(first, second, third, selection, Some(RateWeights { first: first_weight, second: second_weight, third: third_weight })))
}

#[derive(Clone, Copy)]
enum DispatchMode { Zero, One }
#[derive(Clone, Copy)]
struct DispatchPolicy { active: u32, enabled: u32, first: u32, second: u32, third: u32, fourth: u32, state: u32, option: u8 }

fn dispatch_allowed(memory: &mut impl GuestAddressSpace, mode: DispatchMode, policy: DispatchPolicy) -> Result<u32, MemoryError> {
    if policy.active | policy.state == 0 || policy.enabled == 0 { return Ok(0); }
    let category = if policy.state != 0 { 4 } else if policy.option == 1 { 5 } else if policy.third == 0 { 2 } else if policy.third.wrapping_sub(4) <= 2 { 3 } else { 0 };
    let address = if policy.first != 0 { GuestAddress::new(0x00fa6a50).checked_add(category)? }
        else if policy.fourth != 0 { GuestAddress::new(0x00fa6a48).checked_add(category)? }
        else { GuestAddress::new(0x00fa6a58).checked_add(policy.second.checked_mul(6).ok_or(MemoryError::AddressOverflow)?)?.checked_add(category)? };
    let mask = match mode { DispatchMode::Zero => 2, DispatchMode::One => 1 };
    Ok(u32::from(u32::from(memory.read_u8(address)?) & mask == mask))
}

pub fn helper_fa66d4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, active: u32, enabled: u32, first: u32, second: u32, third: u32, fourth: u32, state: u32, option: u8) -> Result<u32, MemoryError> {
    dispatch_allowed(context, DispatchMode::One, DispatchPolicy { active, enabled, first, second, third, fourth, state, option })
}
pub fn helper_fa667c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, active: u32, enabled: u32, first: u32, second: u32, third: u32, fourth: u32, state: u32, option: u8) -> Result<u32, MemoryError> {
    dispatch_allowed(context, DispatchMode::Zero, DispatchPolicy { active, enabled, first, second, third, fourth, state, option })
}
pub fn helper_fa672c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, active: u32, enabled: u32, first: u32, second: u32, third: u32, fourth: u32, state: u32, option: u8) -> Result<u32, MemoryError> {
    let policy = DispatchPolicy { active, enabled, first, second, third, fourth, state, option };
    let one = dispatch_allowed(context, DispatchMode::One, policy)?;
    let zero = dispatch_allowed(context, DispatchMode::Zero, policy)?;
    Ok(u32::from(one | zero != 0))
}

pub fn round_tenths(value: i32) -> i32 {
    let quotient = value / 10;
    let remainder = value % 10;
    if remainder > 4 { quotient + 1 } else if remainder < -4 { quotient - 1 } else { quotient }
}
pub fn helper_741fa0<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, value: i32) -> Result<i32, MemoryError> { Ok(round_tenths(value)) }

pub fn helper_747d48<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: u32) -> Result<u32, MemoryError> {
    if (channel as i32) < 0 { return Ok(0xffff); }
    let count = crate::record_configuration::helper_69f8fc(context, 0x3e0f80)?.clamp(1, 100);
    if channel >= count { return Ok(0xffff); }
    let key = 0x3e2007 + channel * 3;
    Ok(crate::record_configuration::helper_69f8fc(context, key)? | (crate::record_configuration::helper_69f8fc(context, key + 1)? << 8) | (crate::record_configuration::helper_69f8fc(context, key + 2)? << 16))
}

pub fn helper_fab390<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, mode: u32, _: u32) -> Result<u32, MemoryError> {
    let enabled = profile::helper_74e5c0(context, handle)? == 1;
    let key = match mode { 0 => if enabled { 0x87e1 } else { 0x87df }, 1 => if enabled { 0x87ed } else { 0x87eb }, 5 => 0x8882, 6 => 0x8884, 7 => 0x8883, _ => if enabled { 0x87e7 } else { 0x87e5 } };
    Ok(crate::record_configuration::helper_69f8fc(context, key)?.wrapping_mul(10) & 0xfffe)
}

pub fn helper_fa8078<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32, profile: u32) -> Result<u32, MemoryError> {
    let category = context.read_u8(GuestAddress::new(0x00ef6df0).checked_add(profile.checked_mul(20).ok_or(MemoryError::AddressOverflow)?)?)?;
    let key = if first == 1 { match second { 6 => 0x82bf, 7 => 0x82ba, 5 => 0x82bd, _ => 0x82be } }
        else { context.read_u32(GuestAddress::new(0x00faaf18).checked_add(u32::from(category) * 4)?)? };
    if key == 0 { Ok(0) } else { crate::record_configuration::helper_69f8fc(context, key) }
}
