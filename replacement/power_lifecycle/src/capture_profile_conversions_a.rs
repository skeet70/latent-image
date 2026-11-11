use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::capture_profile_conversions_b::helper_73395c;
use crate::record_configuration::helper_69f8fc;
use crate::record_profile_aux_native::{helper_75df14, helper_75df34, helper_75df60, helper_75df80};

#[derive(Clone, Copy)]
#[repr(u32)]
enum SelectedChannel { First = 1, Second = 2, Third = 4, Fourth = 8 }

impl SelectedChannel {
    fn from_code(code: u32) -> Option<Self> {
        match code {
            0x41 => Some(Self::First), 0x42 => Some(Self::Second),
            0x43 => Some(Self::Third), 0x44 => Some(Self::Fourth), _ => None,
        }
    }
    fn index(self) -> u32 { (self as u32).trailing_zeros() }
}

macro_rules! pure_conversion {
    ($name:ident, ($($arg:ident),*), $body:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(
            _context: &mut RecordContext<M, E>, $($arg: u32),*
        ) -> Result<u32, MemoryError> { Ok($body) }
    };
}

pure_conversion!(helper_7371dc, (value), match value { 1..=6 => value, 7 => 6, _ => 0 });
pure_conversion!(helper_737258, (enabled, value), if enabled == 0 { 0 } else { value });
pure_conversion!(helper_73729c, (value), u32::from(value == 1));
pure_conversion!(helper_7372bc, (value), u32::from(value == 0));
pure_conversion!(helper_7375ec, (), 0);
pure_conversion!(helper_737604, (value), u32::from(value != 1));
pure_conversion!(helper_737620, (value), u32::from(value == 1));
pure_conversion!(helper_737640, (value), u32::from(value == 1));
pure_conversion!(helper_7377e8, (category, mode, enabled, selection, blocked),
    u32::from(selection == 1 && category != 0x2c && enabled != 0 && mode != 4 && blocked != 1));
pure_conversion!(helper_737830, (first, second), u32::from(first == 7 || second == 7));
pure_conversion!(helper_73788c, (selection, mode), u32::from(mode == 2 || matches!(selection, 5 | 7 | 10)));
pure_conversion!(helper_73790c, (enabled, code), if enabled != 1 { 0 } else {
    match code { 0xd0 | 0xd4 => 1, 0xd1 => 2, 0xd2 | 0xd3 | 0xd5..=0xd9 => 3, _ => 0 }
});
pure_conversion!(helper_7379a0, (_selection, category, mode), u32::from(category != 0x2c || mode == 1));
pure_conversion!(helper_737a90, (), 0);
pure_conversion!(helper_737cb4, (value), value);
pure_conversion!(helper_737cd0, (value), match value { 0x80 | 0xff => value, 0 => 0xa0, _ => value.wrapping_sub(1) });
pure_conversion!(helper_737df8, (category, mode, selection), match mode {
    3 | 7 => match selection { 0..=5 => selection + 3, 6 => 12, 7 => 14, 8 => 16, 9 => 32, _ => 1 },
    1 if !matches!(category, 15 | 29) => 0xffff, _ => 1,
});
pure_conversion!(helper_737ed8, (category, mode, selection),
    if !matches!(mode, 1 | 3 | 7) || (mode == 1 && matches!(category, 15 | 29)) { 0 } else {
        match selection { 0 => 3, 1..=9 => selection + 4, 10 => 15, 11 => 30, 12 => 60, 13 => 80, 14 => 160, _ => 0 }
    });
pure_conversion!(helper_738030, (value), if value <= 4 { 4 - value } else { 2 });
pure_conversion!(helper_738134, (value), u32::from(value == 1));
pure_conversion!(helper_738154, (value), match value { 0 => 1, 1 => 2, 3 | 4 => value, _ => 0 });
pure_conversion!(helper_738510, (value), u32::from(value == 1));
pure_conversion!(helper_738530, (), 0);
pure_conversion!(helper_738548, (), 0);
pure_conversion!(helper_738560, (), 0);
pure_conversion!(helper_738578, (value), u32::from(matches!(value, 5 | 6)));
pure_conversion!(helper_73859c, (value), match value { 1 => 2, 2 => 1, _ => 0 });
pure_conversion!(helper_7385c8, (value), u32::from(value == 2));
pure_conversion!(helper_7385e8, (value), u32::from(value == 1));
pure_conversion!(helper_738608, (value), match value { 0 | 1 => value, _ => loop { core::hint::spin_loop(); } });
pure_conversion!(helper_738638, (), 0);
pure_conversion!(helper_738650, (value), u32::from(value == 1));
pure_conversion!(helper_738670, (value), if (1..=6).contains(&value) { value } else { 0 });
pure_conversion!(helper_738ee4, (value), if (1..=11).contains(&value) { value } else { 0 });

pub fn helper_737278<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, first: u32, second: u32) -> Result<u32, MemoryError> {
    context.write_u32(output, first.wrapping_add(3))?;
    context.write_u32(output.checked_add(4)?, 3u32.wrapping_sub(second))?;
    Ok(output.get())
}

pub fn helper_7379d8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, mode: u32) -> Result<u32, MemoryError> {
    let rectangle = if mode == 1 { [160, 120, 240, 320] } else { [0; 4] };
    for (index, value) in rectangle.into_iter().enumerate() {
        context.write_u32(output.checked_add(index as u32 * 4)?, value)?;
    }
    Ok(output.get())
}

pub fn helper_737a4c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, mode: u32) -> Result<u32, MemoryError> {
    context.write_u32(output, if mode == 1 { 40 } else { 0 })?;
    context.write_u32(output.checked_add(4)?, if mode == 1 { 320 } else { 0 })?;
    Ok(output.get())
}

pub fn helper_737aa8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, mode: u32, code: u32, value: u32) -> Result<u32, MemoryError> {
    const LAST_CHANNEL: GuestAddress = GuestAddress::new(0x0091_34d8);
    context.write_u32(output, 0)?;
    context.write_u32(output.checked_add(4)?, 0)?;
    match mode {
        0 => {
            let previous = context.read_u32(LAST_CHANNEL)?;
            if matches!(previous, 1 | 2 | 4 | 8) { context.write_u32(output, previous)?; }
            context.write_u32(output.checked_add(8)?, 1)?;
            for index in 3..7 { context.write_u32(output.checked_add(index * 4)?, 0)?; }
        }
        1 => {
            if let Some(channel) = SelectedChannel::from_code(code) {
                context.write_u32(output, channel as u32)?;
                for index in 0..4 {
                    context.write_u32(output.checked_add((index + 3) * 4)?, if index == channel.index() { value } else { 0 })?;
                }
                context.write_u32(LAST_CHANNEL, channel as u32)?;
            }
            let selected = context.read_u32(output)?;
            context.write_u32(output.checked_add(4)?, selected)?;
            context.write_u32(output.checked_add(8)?, 1)?;
        }
        _ => {}
    }
    Ok(output.get())
}

pub fn helper_738090<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    if value <= 4 { helper_69f8fc(context, 0x9235 + value) } else { Ok(100) }
}
pub fn helper_7381ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    helper_738030(context, value)
}
pub fn helper_738394<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    let index = match value {
        0 => 0x22, 1 => 0x25, 2 => 0x28, 3 => 0x2b, 4 => 0x2c,
        6 => 0x2f, 7 => 0x30, 8 => 0x31, _ => 0x2e,
    };
    let numeric = helper_73395c(context, index)?;
    Ok(if value == 7 { 660 } else { numeric as i16 as i32 as u32 })
}
pub fn helper_73845c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, first: u32, second: u32) -> Result<u32, MemoryError> {
    context.write_u8(output, first.wrapping_add(3) as u8)?;
    context.write_u8(output.checked_add(1)?, 3u32.wrapping_sub(second) as u8)?;
    Ok(output.get())
}

fn desired_channels<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, snapshot: GuestAddress, parity: u32) -> Result<[u8; 4], MemoryError> {
    Ok([
        context.read_u8(snapshot.checked_add(0x3a4 + parity)?)?,
        context.read_u8(snapshot.checked_add(0x3a6 + parity)?)?,
        context.read_u8(snapshot.checked_add(0x3aa + parity)?)?,
        context.read_u8(snapshot.checked_add(0x3a8 + parity)?)?,
    ])
}
fn current_channels<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, scratch: GuestAddress) -> Result<[u8; 4], MemoryError> {
    Ok([
        context.read_u8(scratch)?, context.read_u8(scratch.checked_add(1)?)?,
        context.read_u8(scratch.checked_add(2)?)?, context.read_u8(scratch.checked_add(3)?)?,
    ])
}
fn store_channels<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, scratch: GuestAddress, channels: [u8; 4]) -> Result<(), MemoryError> {
    for (index, value) in channels.into_iter().enumerate() {
        context.write_u8(scratch.checked_add(index as u32)?, value)?;
    }
    Ok(())
}

pub fn helper_7386e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, snapshot: GuestAddress, scratch: GuestAddress) -> Result<u32, MemoryError> {
    helper_75df14(context, profile, scratch)?;
    let desired = desired_channels(context, snapshot, 0)?;
    if current_channels(context, scratch)? != desired {
        store_channels(context, scratch, desired)?;
        helper_75df34(context, profile, scratch)?;
    }
    let residue = helper_75df60(context, profile, scratch)?;
    let desired = desired_channels(context, snapshot, 1)?;
    if current_channels(context, scratch)? != desired {
        store_channels(context, scratch, desired)?;
        helper_75df80(context, profile, scratch)
    } else { Ok(residue) }
}
