use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_numeric_grid::{Centroid, MotionChannel, MotionClass, ProfileHandle, ProfileRecord, SettingKey, displacement};
use crate::record_numeric_grid_frame::{self as frame, GridScratch};

pub const GRID: GuestAddress = GuestAddress::new(0x016d8780);
pub const SPEEDS: GuestAddress = GuestAddress::new(0x016d8ff4);
pub const CLASSES: GuestAddress = GuestAddress::new(0x016d8764);
pub const SNAPSHOT: GuestAddress = GuestAddress::new(0x5ffe6f80);

fn setting<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: SettingKey) -> Result<u32, MemoryError> {
    crate::record_configuration::helper_69f8fc(context, key.0)
}
fn at(base: GuestAddress, offset: u32) -> Result<GuestAddress, MemoryError> { base.checked_add(offset) }
fn half(c: &mut impl GuestAddressSpace, base: GuestAddress, offset: u32) -> Result<u16, MemoryError> { c.read_u16(at(base, offset)?) }
fn word(c: &mut impl GuestAddressSpace, base: GuestAddress, offset: u32) -> Result<u32, MemoryError> { c.read_u32(at(base, offset)?) }
fn put_half(c: &mut impl GuestAddressSpace, base: GuestAddress, offset: u32, value: u16) -> Result<(), MemoryError> { c.write_u16(at(base, offset)?, value) }
fn put_word(c: &mut impl GuestAddressSpace, base: GuestAddress, offset: u32, value: u32) -> Result<(), MemoryError> { c.write_u32(at(base, offset)?, value) }

pub fn reset_speeds(c: &mut impl GuestAddressSpace) -> Result<(), MemoryError> {
    for index in 0..20 { put_half(c, SPEEDS, index * 2, u16::MAX)?; }
    Ok(())
}
pub fn reset_classes(c: &mut impl GuestAddressSpace) -> Result<(), MemoryError> {
    for index in 0..20 { c.write_u8(at(CLASSES, index)?, 100)?; }
    Ok(())
}
pub fn push_speed(c: &mut impl GuestAddressSpace, value: u32) -> Result<(), MemoryError> {
    if value == 0xffff { return Ok(()); }
    let mut carried = c.read_u16(SPEEDS)?;
    c.write_u16(SPEEDS, value as u16)?;
    for index in 1..20 {
        if carried == u16::MAX { break; }
        let next = half(c, SPEEDS, index * 2)?;
        put_half(c, SPEEDS, index * 2, carried)?;
        carried = next;
    }
    Ok(())
}
pub fn push_class(c: &mut impl GuestAddressSpace, value: MotionClass) -> Result<(), MemoryError> {
    let mut carried = c.read_u8(CLASSES)?;
    c.write_u8(CLASSES, value as u8)?;
    for index in 1..20 {
        if carried == 100 { break; }
        let next = c.read_u8(at(CLASSES, index)?)?;
        c.write_u8(at(CLASSES, index)?, carried)?;
        carried = next;
    }
    Ok(())
}
pub fn filtered_speed<M: GuestAddressSpace, E>(c: &mut RecordContext<M, E>, channel: MotionChannel) -> Result<u16, MemoryError> {
    let count = setting(c, SettingKey(if channel.0 == 0 { 0x52ca } else { 0x52cb }))?.clamp(1, 20);
    let mut sum = 0u16;
    let mut valid = 0u32;
    for index in (0..count).rev() {
        let value = half(c, SPEEDS, index * 2)?;
        if value != u16::MAX { sum = sum.wrapping_add(value); valid += 1; }
    }
    Ok(if valid < count { 0 } else { (sum as i32 / valid as i32) as u16 })
}
pub fn filtered_class<M: GuestAddressSpace, E>(c: &mut RecordContext<M, E>, channel: MotionChannel) -> Result<MotionClass, MemoryError> {
    let count = setting(c, SettingKey(if channel.0 == 0 { 0x52b3 } else { 0x52b4 }))?.clamp(1, 20);
    if c.read_u8(at(CLASSES, count - 1)?)? == 100 { return Ok(MotionClass::Unknown); }
    let mut counts = [0u8; 4];
    for index in (0..count).rev() { counts[c.read_u8(at(CLASSES, index)?)? as usize] += 1; }
    let maximum = counts.iter().copied().max().unwrap_or(0);
    let priorities = [setting(c, SettingKey(0x52af))? as u8 as u32, setting(c, SettingKey(0x52b0))? as u8 as u32, setting(c, SettingKey(0x52b1))? as u8 as u32, setting(c, SettingKey(0x52b2))?];
    for class in priorities { if counts[class as usize] == maximum { return Ok(MotionClass::from_word(class)); } }
    Ok(MotionClass::Unknown)
}

pub fn grid<M: GuestAddressSpace, E>(c: &mut RecordContext<M, E>, profile: ProfileHandle, input: GuestAddress, alternate: bool, previous_x: GuestAddress, previous_y: GuestAddress, scratch: GridScratch) -> Result<(), MemoryError> {
    let dimensions = crate::record_profile_accessors::helper_74ec30(c, profile.0)?;
    let threshold = setting(c, SettingKey(if alternate { 0x528d } else { 0x528c }))? as i32;
    let required = setting(c, SettingKey(if alternate { 0x528f } else { 0x528e }))?;
    let duration = duration(c, profile, alternate, scratch)?;
    let rate = if duration == 0 { 0 } else { (1_000_000u32 / duration) as u16 };
    let previous = Centroid { x: c.read_u32(previous_x)? as i32, y: c.read_u32(previous_y)? as i32 };
    let first = GuestAddress::new(word(c, input, 0x18)?);
    let second = GuestAddress::new(word(c, input, 0x14)?);
    let mut sum_x = 0u32;
    let mut sum_y = 0u32;
    let mut count = 0u16;
    for index in 0..256u32 {
        let difference = half(c, first, index * 2)?.wrapping_sub(half(c, second, index * 2)?) as i16;
        let absolute = difference.wrapping_abs();
        put_half(c, GuestAddress::new(0x016d8508), index * 2, absolute as u16)?;
        if absolute as i32 >= threshold {
            sum_x = sum_x.wrapping_add(index & 15);
            sum_y = sum_y.wrapping_add((index >> 4) & 255);
            count += 1;
        }
    }
    let valid = count != 0 && count as u32 >= required;
    let current = if valid {
        let denominator = count as u32;
        let vertical_denominator = denominator.wrapping_mul(dimensions >> 16);
        Centroid {
            x: (sum_x.wrapping_mul(20).wrapping_add(denominator >> 1) / denominator) as i32,
            y: (sum_y.wrapping_mul(dimensions as u16 as u32).wrapping_mul(20).wrapping_add((vertical_denominator as i32 / 2) as u32) / vertical_denominator) as i32,
        }
    } else { Centroid { x: -1, y: -1 } };
    let distance = if valid { displacement(previous, current) } else { 0 };
    c.write_u32(previous_x, current.x as u32)?;
    c.write_u32(previous_y, current.y as u32)?;
    let speed = ((rate as u32).wrapping_mul(distance as u32).wrapping_add(15) as i32 / 30) as u16;
    let mut record = frame::profile(c, scratch)?;
    record.words[0] = valid as u32;
    record.set_half(4, speed);
    frame::publish(c, profile.0, scratch, &record)?;
    put_half(c, GRID, 0x800, count)?;
    put_half(c, GRID, 0x802, valid as u16)?;
    put_word(c, GRID, 0x804, current.x as u32)?;
    put_word(c, GRID, 0x808, current.y as u32)?;
    put_word(c, GRID, 0x80c, previous.x as u32)?;
    put_word(c, GRID, 0x810, previous.y as u32)?;
    put_half(c, GRID, 0x816, speed)?;
    put_half(c, GRID, 0x814, rate)
}

pub fn snapshot<M: GuestAddressSpace, E>(c: &mut RecordContext<M, E>, input: GuestAddress, channel: MotionChannel) -> Result<(), MemoryError> {
    crate::record_numeric_grid_diagnostics::copy_snapshot(c, channel)?;
    for index in 0..256 {
        let first = GuestAddress::new(word(c, input, 0x18)?);
        let value = half(c, first, index * 2)? as i16 as i32 as u32;
        put_word(c, SNAPSHOT, 4 + index * 4, value)?;
        let second = GuestAddress::new(word(c, input, 0x14)?);
        let value = half(c, second, index * 2)? as i16 as i32 as u32;
        put_word(c, SNAPSHOT, 0x404 + index * 4, value)?;
    }
    for (source, destination) in [(0x800,0x804),(0x802,0x808)] {
        let value = half(c, GRID, source)? as u32; put_word(c, SNAPSHOT, destination, value)?;
    }
    for (source, destination) in [(0x804,0x80c),(0x808,0x810),(0x80c,0x814),(0x810,0x818)] {
        let value = word(c, GRID, source)?; put_word(c, SNAPSHOT, destination, value)?;
    }
    for (source, destination) in [(0x814,0x81c),(0x816,0x820),(0x818,0x824)] {
        let value = half(c, GRID, source)? as u32; put_word(c, SNAPSHOT, destination, value)?;
    }
    for index in 0..20 { let value = c.read_u8(at(CLASSES, index)?)? as u32; put_word(c, SNAPSHOT, 0x828 + index * 4, value)?; }
    for (source, destination) in [(0x842,0x878),(0x844,0x87c)] {
        let value = half(c, GRID, source)? as u32; put_word(c, SNAPSHOT, destination, value)?;
    }
    for index in 0..20 { let value = half(c, SPEEDS, index * 2)? as u32; put_word(c, SNAPSHOT, 0x880 + index * 4, value)?; }
    let value = half(c, GRID, 0x86e)? as u32;
    put_word(c, SNAPSHOT, 0x8d0, value)
}
fn joint_override<M: GuestAddressSpace, E>(c: &mut RecordContext<M, E>, record: &ProfileRecord, channel: MotionChannel) -> Result<bool, MemoryError> {
    let count = half(c, GRID, 0x800)? as u32;
    let speed = half(c, GRID, 0x816)? as u32;
    let first = setting(c, SettingKey(if channel.0 == 0 { 0x5251 } else { 0x5253 }))?;
    let second = setting(c, SettingKey(if channel.0 == 0 { 0x5252 } else { 0x5254 }))?;
    if setting(c, SettingKey(0x52a2))? != 1 { return Ok(false); }
    let alternate = record.words[7] != 0;
    let lower = setting(c, SettingKey(if alternate { 0x52a9 } else { 0x1609f }))?;
    if record.words[6] < first.wrapping_mul(second).wrapping_mul(lower) { return Ok(false); }
    let upper = setting(c, SettingKey(if alternate { 0x52aa } else { 0x160a0 }))?;
    if record.words[6] >= first.wrapping_mul(second).wrapping_mul(upper) { return Ok(false); }
    if count < setting(c, SettingKey(if alternate { 0x160a5 } else { 0x160a1 }))? { return Ok(false); }
    if count >= setting(c, SettingKey(if alternate { 0x160a6 } else { 0x160a2 }))? { return Ok(false); }
    if speed < setting(c, SettingKey(if alternate { 0x52ad } else { 0x160a3 }))? { return Ok(false); }
    Ok(speed < setting(c, SettingKey(if alternate { 0x52ae } else { 0x160a4 }))?)
}

pub fn filter<M: GuestAddressSpace, E>(c: &mut RecordContext<M, E>, profile: ProfileHandle, channel: MotionChannel, scratch: GridScratch) -> Result<(), MemoryError> {
    crate::record_numeric_grid_diagnostics::publish_profile(c, profile, scratch)?;
    let mut record = frame::profile(c, scratch)?;
    let limit = setting(c, SettingKey(if channel.0 == 1 { 0x160a8 } else { 0x160a7 }))?;
    let primary = record.half(12) as u32 * 1000 + 1280;
    let mut speed = ((primary as i64 * 0x66666667i64) as u64 >> 42) as u32;
    let secondary = record.half(4) as u32 * 1000 + 160;
    let fallback_speed = (((secondary as i64 * 0x66666667i64) as u64 >> 39) as u32) & 0xffff;
    let jointly_enabled = (setting(c, SettingKey(0x5256))? & 2 != 0 && setting(c, SettingKey(0x5256))? & 1 != 0 && channel.0 == 0)
        || (setting(c, SettingKey(0x5257))? & 2 != 0 && setting(c, SettingKey(0x5257))? & 1 != 0 && channel.0 == 1);
    let class;
    if jointly_enabled {
        class = if record.words[2] == 0 && record.words[0] == 1 && joint_override(c, &record, channel)? { MotionClass::Moving }
        else {
            let index = record.words[0].wrapping_add(record.words[2].wrapping_mul(4));
            let key = word(c, GuestAddress::new(0x00faaf70), index.wrapping_mul(4))?;
            MotionClass::from_word(setting(c, SettingKey(key))?)
        };
        if speed < limit { speed = fallback_speed; }
    } else if (setting(c, SettingKey(0x5256))? & 2 != 0 && channel.0 == 0) || (setting(c, SettingKey(0x5257))? & 2 != 0 && channel.0 == 1) {
        class = match record.words[0] { 0 => MotionClass::Still, 1 => MotionClass::Moving, _ => MotionClass::Unknown };
        speed = fallback_speed;
    } else if (setting(c, SettingKey(0x5256))? & 1 != 0 && channel.0 == 0) || (setting(c, SettingKey(0x5257))? & 1 != 0 && channel.0 == 1) {
        class = match record.words[2] { 0 => MotionClass::Still, 1 => MotionClass::Moving, 2 => MotionClass::Alternate, _ => MotionClass::Unknown };
    } else { class = MotionClass::Unknown; speed = 0; }
    push_class(c, class)?;
    let filtered_class = filtered_class(c, channel)?;
    push_speed(c, speed)?;
    let filtered_speed = filtered_speed(c, channel)?;
    record.words[4] = filtered_class as u32;
    record.set_half(20, filtered_speed);
    record.set_half(22, 30);
    frame::publish(c, profile.0, scratch, &record)?;
    put_half(c, GRID, 0x818, class as u16)?;
    put_half(c, GRID, 0x842, filtered_class as u16)?;
    put_half(c, GRID, 0x844, speed as u16)?;
    put_half(c, GRID, 0x86e, filtered_speed)
}


fn duration<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileHandle, alternate: bool, scratch: GridScratch) -> Result<u32, MemoryError> {
    if alternate {
        let selector = crate::record_profile_shared_accessors::helper_757254(context, profile.0)?;
        return Ok(crate::record_task43_selection_clock::period(context, selector)?.unwrap_or(0));
    }
    let samples = scratch.samples()?;
    crate::record_profile_shared_accessors::helper_757ff0(context, profile.0, samples)?;
    let count = context.read_u16(samples.checked_add(40)?)?;
    let mut total = 0u32;
    for index in 0..u32::from(count) {
        total = total.wrapping_add(context.read_u32(samples.checked_add(index * 4)?)?);
    }
    Ok(total)
}
