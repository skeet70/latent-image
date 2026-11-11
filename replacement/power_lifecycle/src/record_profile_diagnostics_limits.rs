use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_configuration::{helper_69f8fc, helper_69fec8, helper_69fef0};

fn shift(value: u32, count: u32) -> u32 { value.checked_shl(count & 0xff).unwrap_or(0) }

pub fn helper_748018<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, mode: u32) -> Result<u32, MemoryError> {
    Ok(match mode { 1..=4 => 3, 6 | 11 | 12 => 2, 9 | 10 => 1, _ => 0 })
}

#[derive(Clone, Copy)]
enum LimitTable { Primary, Optional, Alternate, Packed, OptionalPacked }

fn read_limit<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, table: LimitTable, address: GuestAddress) -> Result<u32, MemoryError> {
    let key = context.read_u32(address)?;
    let value = match table {
        LimitTable::Packed | LimitTable::OptionalPacked => helper_69fef0(context, key)?,
        _ => {
            let value = helper_69fec8(context, key)?;
            let count = helper_69f8fc(context, 0x3e162f)?;
            shift(value, count)
        }
    };
    Ok(if value == 0 && matches!(table, LimitTable::Optional | LimitTable::Alternate | LimitTable::OptionalPacked) { 0xffff } else { value })
}

fn limit<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, table: LimitTable, side: u32, first: u32, mode: u32) -> Result<u32, MemoryError> {
    let base = GuestAddress::new(match table {
        LimitTable::Primary => 0x774d94, LimitTable::Optional => 0x774de4,
        LimitTable::Alternate => 0x818940, LimitTable::Packed => 0x818ab0,
        LimitTable::OptionalPacked => 0x81862c,
    });
    if matches!(table, LimitTable::Primary | LimitTable::Optional) {
        context.with_guest_frame(ByteCount(84), StackAlignment::Word, |context, frame| {
            context.copy_bytes(frame.base(), base, ByteCount(80))?;
            let class = helper_748018(context, mode)?;
            let index = side.wrapping_add(class * 2).wrapping_mul(2).wrapping_add(u32::from(first != 1));
            let address = frame.base().checked_add(index.wrapping_mul(4))?;
            read_limit(context, table, address)
        })
    } else {
        let class = helper_748018(context, mode)?;
        let index = side.wrapping_add(class * 2).wrapping_mul(2).wrapping_add(u32::from(first != 1));
        read_limit(context, table, base.checked_add(index.wrapping_mul(4))?)
    }
}

pub fn helper_748090<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, side: u32, first: u32, mode: u32) -> Result<u32, MemoryError> {
    limit(context, LimitTable::Primary, side, first, mode)
}
pub fn helper_748144<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, side: u32, first: u32, mode: u32) -> Result<u32, MemoryError> {
    limit(context, LimitTable::Optional, side, first, mode)
}
pub fn helper_748210<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, side: u32, first: u32, mode: u32) -> Result<u32, MemoryError> {
    limit(context, LimitTable::Alternate, side, first, mode)
}
pub fn helper_748104<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, side: u32, first: u32, mode: u32) -> Result<u32, MemoryError> {
    limit(context, LimitTable::Packed, side, first, mode)
}
pub fn helper_7481c4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, side: u32, first: u32, mode: u32) -> Result<u32, MemoryError> {
    limit(context, LimitTable::OptionalPacked, side, first, mode)
}
pub fn helper_747af8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let value = helper_69fec8(context, 0x3e15a6)?;
    Ok(shift(value, helper_69f8fc(context, 0x3e162f)?))
}
pub fn helper_747b24<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let value = helper_69fec8(context, 0x3e15a8)?;
    Ok(shift(value, helper_69f8fc(context, 0x3e162f)?))
}
