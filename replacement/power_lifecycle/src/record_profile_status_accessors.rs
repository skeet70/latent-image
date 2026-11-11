use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{can_write, ProfileField, ProfileHandle};

macro_rules! word_getters {
    ($($name:ident => $address:expr),* $(,)?) => { $(
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> {
            context.read_u32(GuestAddress::new($address))
        }
    )* };
}
macro_rules! signed_getters {
    ($($name:ident => $address:expr),* $(,)?) => { $(
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> {
            Ok(context.read_u16(GuestAddress::new($address))? as i16 as i32 as u32)
        }
    )* };
}
word_getters!(
    helper_757df0 => 0x0091_9768,
    helper_757e38 => 0x0091_976c,
    helper_757e80 => 0x0091_9770,
    helper_7580ec => 0x0091_9780,
    helper_758134 => 0x0091_9784,
    helper_75817c => 0x0091_9884,
);
signed_getters!(
    helper_757828 => 0x0091_92a8,
    helper_757c10 => 0x0091_975a,
    helper_757c60 => 0x0091_975c,
);
macro_rules! setters {
    ($($name:ident => ($field:expr, $address:expr)),* $(,)?) => { $(
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? {
                context.write_u32(GuestAddress::new($address), value)?;
            }
            Ok(())
        }
    )* };
}
setters!(
    helper_756b2c => (20, 0x0091_919c),
    helper_756dfc => (21, 0x0091_91c4),
    helper_75810c => (27, 0x0091_9780),
);

pub fn helper_756e90<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, _handle: u32) -> Result<GuestAddress, MemoryError> {
    let values = [context.read_u32(GuestAddress::new(0x0091_91e0))?, context.read_u32(GuestAddress::new(0x0091_91e4))?, context.read_u32(GuestAddress::new(0x0091_91e8))?, context.read_u32(GuestAddress::new(0x0091_91ec))?];
    for (index, value) in values.into_iter().enumerate() { context.write_u32(output.checked_add(index as u32 * 4)?, value)?; }
    Ok(output)
}

pub fn helper_756ebc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, record: GuestAddress) -> Result<u32, MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(23))? { return Ok(0); }
    let values = [context.read_u32(record)?, context.read_u32(record.checked_add(4)?)?, context.read_u32(record.checked_add(8)?)?, context.read_u32(record.checked_add(12)?)?];
    for (index, value) in values.into_iter().enumerate() { context.write_u32(GuestAddress::new(0x0091_91e0 + index as u32 * 4), value)?; }
    Ok(values[0])
}

pub fn helper_7581b4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, values: GuestAddress) -> Result<(), MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(27))? { return Ok(()); }
    let first = context.read_u16(values)?;
    let second = context.read_u16(values.checked_add(2)?)?;
    context.write_u16(GuestAddress::new(0x0091_9884), first)?;
    context.write_u16(GuestAddress::new(0x0091_9886), second)?;
    Ok(())
}
