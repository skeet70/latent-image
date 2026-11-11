use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{read_bank, resource_address, write_bank, ProfileField, ProfileHandle};

#[derive(Clone, Copy)]
struct ParameterHalfword(u16);
#[derive(Clone, Copy)]
struct ParameterWord(u32);

fn write_halfword<const INDEX: u32, M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: ProfileHandle, value: ParameterHalfword) -> Result<u32, MemoryError> {
    let Some(bank) = write_bank(context, handle, ProfileField(1))? else { return Ok(0); };
    let parameters = resource_address(bank).checked_add(0x90)?;
    context.write_u16(parameters.checked_add(INDEX * 2)?, value.0)?;
    Ok(parameters.get())
}

fn write_word<const INDEX: u32, M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: ProfileHandle, value: ParameterWord) -> Result<u32, MemoryError> {
    let Some(bank) = write_bank(context, handle, ProfileField(1))? else { return Ok(0); };
    let parameters = resource_address(bank).checked_add(0x90)?;
    context.write_u32(parameters.checked_add(INDEX * 4)?, value.0)?;
    Ok(parameters.get())
}

macro_rules! halfword_setter {
    ($name:ident, $index:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<u32, MemoryError> {
            write_halfword::<$index, M, E>(context, ProfileHandle(handle), ParameterHalfword(value as u16))
        }
    };
}

halfword_setter!(helper_750eb0, 0);
halfword_setter!(helper_750f48, 1);
halfword_setter!(helper_750fe0, 2);
halfword_setter!(helper_751078, 3);
halfword_setter!(helper_751110, 4);
halfword_setter!(helper_7511a8, 5);
halfword_setter!(helper_751240, 6);
halfword_setter!(helper_7512d8, 7);
halfword_setter!(helper_751370, 8);
halfword_setter!(helper_751408, 9);
halfword_setter!(helper_7514a0, 10);
halfword_setter!(helper_751538, 11);
halfword_setter!(helper_7515d0, 12);
halfword_setter!(helper_751668, 13);
halfword_setter!(helper_751700, 14);
halfword_setter!(helper_751798, 15);
halfword_setter!(helper_751830, 16);
halfword_setter!(helper_7518c8, 17);
halfword_setter!(helper_751960, 18);
halfword_setter!(helper_7519f8, 19);
halfword_setter!(helper_751a90, 20);
halfword_setter!(helper_751b28, 21);

macro_rules! word_setter {
    ($name:ident, $index:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<u32, MemoryError> {
            write_word::<$index, M, E>(context, ProfileHandle(handle), ParameterWord(value))
        }
    };
}

word_setter!(helper_751bc0, 11);
word_setter!(helper_751c58, 12);
word_setter!(helper_751cf0, 13);

pub fn helper_751d3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(2))?;
    let source = resource_address(bank).checked_add(0xc8)?;
    for group in 0..7 {
        let offset = group * 16;
        let first = context.read_u32(source.checked_add(offset)?)?;
        let second = context.read_u32(source.checked_add(offset + 4)?)?;
        let third = context.read_u32(source.checked_add(offset + 8)?)?;
        let fourth = context.read_u32(source.checked_add(offset + 12)?)?;
        context.write_u32(output.checked_add(offset)?, first)?;
        context.write_u32(output.checked_add(offset + 4)?, second)?;
        context.write_u32(output.checked_add(offset + 8)?, third)?;
        context.write_u32(output.checked_add(offset + 12)?, fourth)?;
    }
    let first = context.read_u32(source.checked_add(112)?)?;
    let second = context.read_u32(source.checked_add(116)?)?;
    let third = context.read_u32(source.checked_add(120)?)?;
    context.write_u32(output.checked_add(112)?, first)?;
    context.write_u32(output.checked_add(116)?, second)?;
    context.write_u32(output.checked_add(120)?, third)?;
    Ok(output)
}
