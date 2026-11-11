use crate::record_address_space::{GuestAddress as A, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc;

const RECORDING: u32 = 0x01526a78;
const FLAGS: u32 = 0x009168a0;
const MASK: u32 = 0x009168be;
fn current<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<A, MemoryError> {
    context.read_u32(A::new(RECORDING + 0x3540)).map(A::new)
}
pub(crate) fn expand_flags(flags: u32) -> u32 { (flags & 3) | ((flags & 4) << 2) }

pub(crate) fn update_work_flags<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, flags: u32, set: bool) -> Result<bool, MemoryError> {
    let before = context.read_u32(A::new(FLAGS))?;
    let value = context.read_u32(A::new(FLAGS))?;
    context.write_u32(A::new(FLAGS), if set { value | flags } else { value & !flags })?;
    let output = current(context)?;
    if output.get() != 0 {
        let value = context.read_u32(A::new(FLAGS))?;
        context.write_u32(output, value)?;
    }
    Ok(before != context.read_u32(A::new(FLAGS))?)
}
fn set_mask<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: u16) -> Result<(), MemoryError> {
    let before = context.read_u16(A::new(MASK))?;
    context.write_u16(A::new(MASK), before | mask)
}
fn apply_selection<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, settings: A) -> Result<(), MemoryError> {
    let _before = context.read_u16(A::new(MASK))?;
    let operation = context.read_u32(settings)?;
    let mask = match operation {
        36 => 4,
        35 | 12 => 1,
        18 => u16::from(context.read_u32(settings.checked_add(0x90)?)? != 1),
        34 | 45 | 63 => 0,
        0..=74 => match context.read_u32(settings.checked_add(4)?)? { 2 => 0, 3 => 4, _ => 1 },
        _ => 0,
    };
    if mask != 0 { set_mask(context, mask)?; }
    set_mask(context, 0x10)?;
    let _previous = context.read_u32(A::new(0x009167f0))?;
    context.write_u32(A::new(0x009167f0), 0)?;
    let _after = context.read_u16(A::new(MASK))?;
    Ok(())
}

pub fn update_work_settings<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, flags: u32, settings: A) -> Result<bool, MemoryError> {
    let lower = 1;
    let count = helper_69f8fc(context, 0x3e0f80)?.clamp(1, 100);
    let flags = expand_flags(flags);
    let output = current(context)?;
    let old_flags = context.read_u32(output)?;
    let old_mask = context.read_u16(output.checked_add(0x126)?)?;
    let old_selection = context.read_u32(output.checked_add(0x11c)?)?;
    if flags == 0 { update_work_flags(context, 0x13, false)?; }
    else {
        update_work_flags(context, flags, true)?;
        if flags & 0x11 != 0 {
            let value = u32::from(context.read_u8(A::new(0x0091682c))?);
            update_work_flags(context, 2, value < lower || value > count)?;
        }
        set_mask(context, 0x200)?;
        let update = if flags & 2 != 0 { true } else {
            let output = current(context)?;
            context.read_u8(output.checked_add(0x104)?)? == 0
        };
        if update {
            apply_selection(context, settings)?;
            let output = current(context)?;
            let mask = context.read_u16(A::new(MASK))?;
            context.write_u16(output.checked_add(0x126)?, mask)?;
            let output = current(context)?;
            let mask = context.read_u16(output.checked_add(0x126)?)?;
            context.write_u16(output.checked_add(0x126)?, mask | 2)?;
            let output = current(context)?;
            let selection_flags = context.read_u32(A::new(0x009167f0))?;
            let selection = if selection_flags & 0x20 != 0 { 4 } else if selection_flags & 8 != 0 { 2 }
                else if selection_flags & 0x10 != 0 { 3 } else { (selection_flags >> 2) & 1 };
            context.write_u32(output.checked_add(0x11c)?, selection)?;
        }
    }
    let output = current(context)?;
    let selection = context.read_u32(output.checked_add(0x11c)?)?;
    let flags = context.read_u32(output)?;
    let mask = context.read_u16(output.checked_add(0x126)?)?;
    Ok((flags, mask, selection) != (old_flags, old_mask, old_selection))
}
