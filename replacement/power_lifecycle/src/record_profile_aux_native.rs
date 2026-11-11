//! X-M1 1.50 auxiliary leaf effects, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

macro_rules! auxiliary_word {
    ($get:ident, $set:ident, $offset:literal) => {
        pub fn $get<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
            context.read_u32(record.checked_add($offset)?)
        }
        pub fn $set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, value: u32) -> Result<u32, MemoryError> {
            context.write_u32(record.checked_add($offset)?, value)?;
            Ok(record.get())
        }
    };
}
auxiliary_word!(helper_75d668, helper_75d680, 0x0);
auxiliary_word!(helper_75d698, helper_75d6b0, 0x4);
auxiliary_word!(helper_75d710, helper_75d728, 0x18);
auxiliary_word!(helper_75d740, helper_75d758, 0x1c);
auxiliary_word!(helper_75d770, helper_75d788, 0x20);
auxiliary_word!(helper_75d7a0, helper_75d7b8, 0x24);
auxiliary_word!(helper_75d860, helper_75d878, 0x40);
auxiliary_word!(helper_75d890, helper_75d8a8, 0x44);
auxiliary_word!(helper_75d8c0, helper_75d8d8, 0x48);
auxiliary_word!(helper_75d8f0, helper_75d908, 0x4c);
auxiliary_word!(helper_75d968, helper_75d980, 0x60);
auxiliary_word!(helper_75d998, helper_75d9b0, 0x64);

fn load_words<const N: usize>(memory: &mut impl GuestAddressSpace, source: GuestAddress) -> Result<[u32; N], MemoryError> {
    let mut words = [0; N];
    for (index, value) in words.iter_mut().enumerate() { *value = memory.read_u32(source.checked_add(index as u32 * 4)?)?; }
    Ok(words)
}
fn store_words<const N: usize>(memory: &mut impl GuestAddressSpace, destination: GuestAddress, words: [u32; N]) -> Result<(), MemoryError> {
    for (index, value) in words.into_iter().enumerate() { memory.write_u32(destination.checked_add(index as u32 * 4)?, value)?; }
    Ok(())
}
macro_rules! auxiliary_copy {
    ($get:ident, $set:ident, $offset:literal, $count:literal) => {
        pub fn $get<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, record: GuestAddress) -> Result<u32, MemoryError> {
            let words = load_words::<$count>(context, record.checked_add($offset)?)?;
            store_words(context, destination, words)?;
            Ok(destination.get())
        }
        pub fn $set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> {
            let words = load_words::<$count>(context, source)?;
            store_words(context, record.checked_add($offset)?, words)?;
            Ok(words[0])
        }
    };
}
auxiliary_copy!(helper_75d6c8, helper_75d6f0, 0x8, 4);
auxiliary_copy!(helper_75d7d0, helper_75d7f8, 0x28, 4);
auxiliary_copy!(helper_75d818, helper_75d840, 0x38, 2);
auxiliary_copy!(helper_75d920, helper_75d948, 0x50, 4);

fn update_quad<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, source: GuestAddress, offset: u32) -> Result<u32, MemoryError> {
    let destination = record.checked_add(offset)?;
    let previous = load_words::<4>(context, destination)?;
    for index in 0..16 {
        let value = context.read_u8(source.checked_add(index)?)?;
        let prior = previous[index as usize / 4].to_le_bytes()[index as usize % 4];
        if value != prior {
            let next = load_words::<4>(context, source)?;
            store_words(context, destination, next)?;
            return Ok(next[0]);
        }
    }
    Ok(0)
}
pub fn helper_735fc0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> { update_quad(context, record, source, 8) }
pub fn helper_7360a4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> { update_quad(context, record, source, 0x50) }
pub fn helper_7360f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> {
    let previous = helper_75d968(context, record)?;
    let next = context.read_u32(source)?;
    if previous == next { Ok(previous) } else { helper_75d980(context, record, next) }
}
pub fn helper_75d490<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, value: u32) -> Result<u32, MemoryError> {
    context.write_u32(record.checked_add(0x146c)?, value)?;
    Ok(record.get())
}
pub fn helper_75e6f0<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, command: u32) -> Result<u32, MemoryError> { Ok(command) }

fn mark_secondary(memory: &mut impl GuestAddressSpace, record: GuestAddress, mask: u32) -> Result<u32, MemoryError> {
    let low_address = record.checked_add(0x88)?;
    let high_address = record.checked_add(0x8c)?;
    let low = memory.read_u32(low_address)?;
    let high = memory.read_u32(high_address)?;
    memory.write_u32(low_address, low | mask)?;
    memory.write_u32(high_address, high)?;
    Ok(low)
}
macro_rules! header_byte {
    ($get:ident, $set:ident, $offset:literal, $mask:literal, $signed:literal) => {
        pub fn $get<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
            let value = context.read_u8(record.checked_add($offset)?)?;
            Ok(if $signed { value as i8 as i32 as u32 } else { value as u32 })
        }
        pub fn $set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, value: u32) -> Result<u32, MemoryError> {
            context.write_u8(record.checked_add($offset)?, value as u8)?;
            mark_secondary(context, record, $mask)
        }
    };
}
header_byte!(helper_75deac, helper_75dec4, 0x56, 0x400, false);
header_byte!(helper_75dee0, helper_75def8, 0x57, 0x800, false);
header_byte!(helper_75dfac, helper_75dfc4, 0x71, 0x4000, true);
header_byte!(helper_75dfe0, helper_75dff8, 0x70, 0x10000, false);
header_byte!(helper_75e014, helper_75e02c, 0x5a, 0x100000, false);
header_byte!(helper_75e048, helper_75e060, 0x6e, 0x20000, false);
header_byte!(helper_75e07c, helper_75e094, 0x58, 0x40000, false);
header_byte!(helper_75e0b0, helper_75e0c8, 0x59, 0x80000, false);
header_byte!(helper_75e0e4, helper_75e0fc, 0x6f, 0x200000, false);

macro_rules! header_pair {
    ($get:ident, $set:ident, $offset:literal, $mask:literal) => {
        pub fn $get<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, destination: GuestAddress) -> Result<u32, MemoryError> {
            context.copy_bytes(destination, record.checked_add($offset)?, ByteCount(4))?;
            Ok(destination.get())
        }
        pub fn $set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> {
            context.copy_bytes(record.checked_add($offset)?, source, ByteCount(4))?;
            mark_secondary(context, record, $mask)
        }
    };
}
header_pair!(helper_75df14, helper_75df34, 0x72, 0x1000);
header_pair!(helper_75df60, helper_75df80, 0x76, 0x2000);
