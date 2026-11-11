use crate::record_address_space::{RecordContext, GuestAddressSpace, GuestAddress, MemoryError};

#[derive(Clone, Copy)]
struct Selection(u32);
#[derive(Clone, Copy)]
struct ConfigurationKey(u32);

fn configuration<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: ConfigurationKey) -> Result<u32, MemoryError> {
    crate::record_configuration::helper_69f8fc(context, key.0)
}
struct SelectionTable {
    base: GuestAddress,
    stride: u32,
    sentinel_index: u32,
}
const PRIMARY_TABLE: SelectionTable = SelectionTable { base: GuestAddress::new(0x817180), stride: 8, sentinel_index: 13 };
const ALTERNATE_TABLE: SelectionTable = SelectionTable { base: GuestAddress::new(0x817688), stride: 8, sentinel_index: 14 };
const MODE_TABLE: SelectionTable = SelectionTable { base: GuestAddress::new(0x8173e8), stride: 12, sentinel_index: 11 };

fn lookup<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selection: Selection, table: &SelectionTable, column: u32, missing: u32) -> Result<u32, MemoryError> {
    if selection.0 == 0 { return context.memory.read_u32(table.base.checked_add(column)?); }
    for index in 1..=table.sentinel_index {
        let entry = table.base.checked_add(index * table.stride)?;
        let key = context.memory.read_u32(entry)?;
        if (key as i32) < 0 { return Ok(missing); }
        if key == selection.0 { return context.memory.read_u32(entry.checked_add(column)?); }
    }
    Ok(missing)
}
fn one_or_two(selection: Selection) -> u32 {
    match selection.0 { 1 | 2 => selection.0, _ => 0 }
}
fn signed_thirds(selection: Selection) -> u32 {
    let value = match selection.0 as i32 {
        -6 => -200, -5 => -167, -4 => -133, -3 => -100, -2 => -67, -1 => -33,
        1 => 33, 2 => 67, 3 => 100, 4 => 133, 5 => 167, 6 => 200, _ => 0,
    };
    value as u32
}

pub fn helper_72d210<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, mode: u32, category: u32) -> Result<u32, MemoryError> {
    Ok(if category == 6 { 0 } else { mode })
}
pub fn helper_72dd38<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, mode: u32, enabled: u32) -> Result<u32, MemoryError> {
    Ok(((mode <= 0x3e && !(0x2c..0x2f).contains(&mode) || mode.wrapping_sub(0x40) <= 9) && enabled != 1) as u32)
}
pub fn helper_731b90<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, category: u32, mode: u32) -> Result<u32, MemoryError> {
    Ok((matches!(category, 0x23 | 0x24) || category == 0x2c && mode == 1 || matches!(category, 0x1d | 0x1f | 0x20) && matches!(mode, 1 | 3)) as u32)
}
pub fn helper_73395c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    match index {
        0 => Ok(0),
        u32::MAX => context.memory.read_u32(GuestAddress::new(0x9134d4)),
        13..=64 => context.memory.read_u32(GuestAddress::new(0x816bbc + index * 8)),
        _ => Ok(if (index as i32) <= 64 { (-491i32) as u32 } else { 1200 }),
    }
}
pub fn helper_735b78<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> {
    let (key, offset) = match selection {
        1 => (0x1902, 0), 2 => (0x1904, 0),
        3..=17 => (0x1906 + ((selection - 3) / 3) * 2, [0, 33, 67][((selection - 3) % 3) as usize]),
        18 => (0x1910, 0), 19 => (0x1912, 0), 20 => (0x1914, 0), _ => return Ok(0),
    };
    let low = configuration(context, ConfigurationKey(key))?;
    let high = configuration(context, ConfigurationKey(key + 1))?;
    Ok(((low | high << 8).wrapping_add(offset) as i16 as i32) as u32)
}
pub fn helper_736178<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, category: u32, mode: u32) -> Result<u32, MemoryError> {
    Ok((category == 0x2c || (matches!(category, 0..=0x25 | 0x2b | 0x2f..=0x3e | 0x40..=0x49) && mode == 1)) as u32)
}
pub fn helper_7362dc<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, category: u32, option: u32, mode: u32, extended: u32, variant: u32) -> Result<u32, MemoryError> {
    Ok(match category {
        2..=4 | 0x25 => 0x2c, 5 | 0x3c => 1, 6 => 2, 7 => 4, 8 => 5, 9 => 6, 10 => 7, 11 => 10,
        12 => if mode == 1 || option == 0 { 12 } else { 13 }, 13 => 11, 14 | 15 => 16,
        16 => 18, 17 => 21, 18 => 24, 19 => 25, 20 => 28, 21 => 29, 22 => 30, 23 => 32, 24 => 3,
        25 => 49, 26 => 50, 27 => 51, 28 => 52,
        29 => match variant { 1 => 54, 2 => 55, 3 => 56, _ => 53 },
        31 | 32 => match variant { 1 => 46, 2 => 45, 3 => 47, _ => 44 },
        33 => if extended == 1 { 57 } else { 44 }, 34 => 45, 35 => 46, 36 => 47,
        44 => if variant == 1 { 46 } else { 0 }, 47 | 61 => 17, 48 => 8, 49 => 9, 50 => 15, 51 => 14,
        52 => 19, 53 => 20, 54 => 22, 55 => 23, 56 => 26, 57 => 27, 58 => 31, 62 => 33,
        64..=72 => category - 30, 73 => 48, _ => 0,
    })
}
pub fn helper_736694<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32, category: u32, enabled: u32, override_value: u32) -> Result<u32, MemoryError> {
    if enabled == 0 || override_value == 1 || matches!(category, 1 | 44) { return Ok(0); }
    Ok(((selection >> 13) & 1) | (((selection >> 10) & 1) << 1) | (((selection >> 14) & 1) << 2))
}
pub fn helper_7366e8<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32, category: u32) -> Result<u32, MemoryError> {
    Ok(match selection { 1 if category == 15 => 5, 1 | 3 | 7 | 10 => 1, 4 => 2, 5 => 12, 6 => 13, 8 => 14, _ => 0 })
}
pub fn helper_736770<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok((selection == 11) as u32) }
pub fn helper_736790<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok((value == 1) as u32) }
pub fn helper_7367b0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, category: u32, value: u32, selection: u32, override_value: u32) -> Result<u32, MemoryError> {
    Ok((value == 1 && !((override_value == 1 || category == 44) && selection.wrapping_sub(7) <= 2)) as u32)
}
pub fn helper_7367ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, category: u32, selection: u32, enabled: u32, mode: u32, availability: u32) -> Result<u32, MemoryError> {
    if helper_72dd38(context, category, availability)? == 1 && enabled == 1 {
        return Ok(if category.wrapping_sub(3) < 2 { 2 } else { 1 });
    }
    Ok(match selection { 1 => 2, 2 => 3, 3 => 6, 4 => 4, 5 => 5, 6 => 7, 7 => 10, 8 => 9, 9 => 8, _ => (mode != 1) as u32 })
}
pub fn helper_7368c4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, selection: u32) -> Result<u32, MemoryError> {
    let pair = match selection { 0 => [3, 4], 1 => [2, 3], 2 => [9, 16], 3 => [1, 1], _ => return Ok(output.get()) };
    context.memory.write_u16(output, pair[0])?;
    context.memory.write_u16(output.checked_add(2)?, pair[1])?;
    Ok(output.get())
}
pub fn helper_73693c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok((selection.wrapping_sub(1) < 2) as u32) }
pub fn helper_736960<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(match selection { 1..=3 => selection, 4 => 5, 5 => 4, _ => 0 }) }
pub fn helper_7369c4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(match selection { 0 | 1 | 3 | 4 => selection, _ => 2 }) }
pub fn helper_736a1c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(one_or_two(Selection(selection))) }
pub fn helper_736a48<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(match selection { 2 => 1, 4 => 2, _ => 0 }) }
pub fn helper_736a74<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { lookup(context, Selection(selection), &PRIMARY_TABLE, 4, 0) }
pub fn helper_736adc<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32, category: u32, override_value: u32) -> Result<u32, MemoryError> {
    Ok(if override_value != 0 || category == 44 { 2 } else { match selection { 1..=5 => selection, _ => 0 } })
}
pub fn helper_736b5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { lookup(context, Selection(selection), &ALTERNATE_TABLE, 4, 0) }
pub fn helper_736bc4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selection: u32, alternate: u32, mode: u32) -> Result<u32, MemoryError> {
    match mode { 1 => Ok(7), 0 | 2 => lookup(context, Selection(selection), &MODE_TABLE, if mode == 0 && alternate == 1 { 8 } else { 4 }, 6), _ => Ok(6) }
}
pub fn helper_736d18<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok((selection.wrapping_sub(8) < 2) as u32) }
pub fn helper_736d3c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok((selection == 2) as u32) }
pub fn helper_736d5c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok((selection == 2) as u32) }
pub fn helper_736d7c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(signed_thirds(Selection(selection))) }
pub fn helper_736e3c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(1) }
pub fn helper_736e54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> {
    Ok(match configuration(context, ConfigurationKey(0x6000))? { 2 => 0, 3 => 1, 1 => 2, _ => one_or_two(Selection(selection)) })
}
pub fn helper_736eb0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(signed_thirds(Selection(selection))) }
pub fn helper_736f70<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok(value) }
pub fn helper_736f84<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok(helper_73395c(context, value)? as i16 as i32 as u32) }
pub fn helper_736f98<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok(if value == 0 { 0 } else { value.wrapping_sub(1) }) }
pub fn helper_736fac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, category: u32) -> Result<u32, MemoryError> {
    let index = match category { 0..=5 => 0, 6 | 7 => 1, 8 | 9 => 2, 10..=25 => category - 7, 27 | 28 => category - 8, _ => return Ok(0) };
    Ok(helper_735b78(context, index)? as i16 as i32 as u32)
}
pub fn helper_737154<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(match selection { 1 => 4, 2 => 3, 4 => 1, 5 => 0, _ => 2 }) }
pub fn helper_7371b0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(one_or_two(Selection(selection))) }
pub fn helper_7372d8<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(if selection.wrapping_sub(1) < 10 { selection } else { 0 }) }
pub fn helper_737378<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { let index = value.wrapping_add(9); Ok(if index <= 18 { index } else { 9 }) }
pub fn helper_73747c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, first: u32, second: u32, third: u32) -> Result<u32, MemoryError> {
    context.memory.write_u16(output, first as u16)?;
    context.memory.write_u16(output.checked_add(2)?, second as u16)?;
    context.memory.write_u16(output.checked_add(4)?, third as u16)?;
    Ok(output.get())
}
pub fn helper_7375c0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(one_or_two(Selection(selection))) }
pub fn helper_737660<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, category: u32, mode: u32) -> Result<u32, MemoryError> { Ok((category == 1 || mode == 1) as u32) }
pub fn helper_7377d0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, _value: u32, _second: u32, _third: u32) -> Result<u32, MemoryError> { Ok(0) }
pub fn helper_7380fc<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok(match selection { 1 => 33, 2 => 66, 3 => 100, _ => 0 }) }
pub fn helper_738204<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> {
    Ok(match selection {
        1 => 2550, 2 => 2650, 3 => 2700, 4 => 2800, 5 => 2850, 6 => 2950, 7 => 3000, 8 => 3100, 9 => 3200, 10 => 3300,
        11 => 3400, 12 => 3600, 13 => 3700, 14 => 3800, 15 => 4000, 16 => 4200, 17 => 4300, 18 => 4500, 19 => 4800, 20 => 5000,
        21 => 5300, 22 => 5600, 23 => 5900, 24 => 6300, 25 => 6700, 26 => 7100, 27 => 7700, 28 => 8300, 29 => 9100, 30 => 10000, _ => 2500,
    })
}
pub fn helper_73843c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, disabled: u32, value: u32) -> Result<u32, MemoryError> { Ok(if disabled != 0 { 0 } else { value }) }
pub fn helper_738480<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok((value != 0) as u32) }
pub fn helper_73849c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, _value: u32, _second: u32, _third: u32, _fourth: u32, _fifth: u32) -> Result<u32, MemoryError> { Ok(0) }
pub fn helper_7384b4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, category: u32) -> Result<u32, MemoryError> { Ok((category == 43) as u32) }
pub fn helper_7384d4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32, disabled: u32, category: u32, mode: u32) -> Result<u32, MemoryError> { Ok((disabled == 0 && (category == 12 && mode == 1 || selection == 1)) as u32) }
pub fn helper_737ff0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok((selection == 1) as u32) }
pub fn helper_738010<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, selection: u32) -> Result<u32, MemoryError> { Ok((selection == 1) as u32) }
