use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

use crate::record_configuration::{self as configuration, ConfigurationKey};
#[derive(Clone, Copy)]
enum FieldWidth { Byte, UnsignedHalf, SignedByte, SignedHalf }

fn read<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: ConfigurationKey, width: FieldWidth) -> Result<u32, MemoryError> {
    match width {
        FieldWidth::Byte => configuration::helper_69f8fc(context, key.0),
        FieldWidth::UnsignedHalf => configuration::helper_69fec8(context, key.0),
        FieldWidth::SignedByte => configuration::helper_69ff68(context, key.0).map(|value| value as u32),
        FieldWidth::SignedHalf => configuration::helper_69ff7c(context, key.0).map(|value| value as u32),
    }
}

fn load<M: GuestAddressSpace, E>(memory: &mut RecordContext<M, E>, output: GuestAddress, index: u32, key: ConfigurationKey, width: FieldWidth) -> Result<(), MemoryError> {
    let value = read(memory, key, width)?;
    memory.write_u32(output.checked_add((index + 1) * 4)?, value)
}

fn byte_range<M: GuestAddressSpace, E>(memory: &mut RecordContext<M, E>, output: GuestAddress, index: u32, key: ConfigurationKey, count: u32) -> Result<(), MemoryError> {
    for offset in 0..count { load(memory, output, index + offset, ConfigurationKey(key.0 + offset), FieldWidth::Byte)?; }
    Ok(())
}

pub fn helper_fa89fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    use FieldWidth::{Byte, UnsignedHalf, SignedByte, SignedHalf};
    for (index, key, width) in [
        (0, 0x520e, Byte), (1, 0x520f, Byte), (2, 0x5210, UnsignedHalf), (3, 0x5212, UnsignedHalf),
        (4, 0x5214, Byte), (5, 0x5215, Byte), (6, 0x5216, UnsignedHalf), (7, 0x5218, UnsignedHalf),
        (8, 0x5201, Byte), (9, 0x5202, UnsignedHalf), (10, 0x5204, UnsignedHalf), (11, 0x5206, Byte),
        (12, 0x1609a, Byte), (13, 0x1609b, Byte), (14, 0x1609c, Byte), (15, 0x521a, UnsignedHalf),
        (16, 0x521c, Byte), (17, 0x521d, Byte), (18, 0x1609d, Byte), (19, 0x1609e, Byte),
    ] { load(context, output, index, ConfigurationKey(key), width)?; }
    byte_range(context, output, 20, ConfigurationKey(0x521e), 7)?;
    for (index, key, width) in [
        (27, 0x5225, UnsignedHalf), (28, 0x5227, Byte), (29, 0x5228, Byte), (30, 0x5229, Byte),
        (31, 0x522a, UnsignedHalf), (32, 0x522c, UnsignedHalf), (33, 0x522e, Byte), (34, 0x522f, UnsignedHalf),
    ] { load(context, output, index, ConfigurationKey(key), width)?; }
    byte_range(context, output, 35, ConfigurationKey(0x5231), 21)?;
    for (index, key) in [(56,0x5251),(58,0x5252),(57,0x5253),(59,0x5254),(60,0x5255),(61,0x5256),(62,0x5257),(63,0x525d)] {
        load(context, output, index, ConfigurationKey(key), Byte)?;
    }
    for (index, key) in [(64,0x525e),(65,0x5260),(66,0x5262),(67,0x5264),(70,0x5268)] { load(context,output,index,ConfigurationKey(key),UnsignedHalf)?; }
    for (index, key) in [(72,0x526a),(74,0x526b),(76,0x526c),(78,0x526d),(80,0x526e)] { load(context,output,index,ConfigurationKey(key),Byte)?; }
    byte_range(context, output, 82, ConfigurationKey(0x5284), 6)?;
    byte_range(context, output, 88, ConfigurationKey(0x528c), 4)?;
    byte_range(context, output, 92, ConfigurationKey(0x5296), 13)?;
    for offset in 0..6 { load(context, output, 105 + offset * 2, ConfigurationKey(0x1609f + offset), Byte)?; }
    byte_range(context, output, 117, ConfigurationKey(0x52af), 6)?;
    byte_range(context, output, 123, ConfigurationKey(0x160a7), 2)?;
    byte_range(context, output, 125, ConfigurationKey(0x52ca), 2)?;
    load(context, output, 127, ConfigurationKey(0x5258), Byte)?;
    load(context, output, 146, ConfigurationKey(0x52eb), Byte)?;
    load(context, output, 147, ConfigurationKey(0x52ec), SignedByte)?;
    load(context, output, 148, ConfigurationKey(0x52ee), SignedHalf)?;
    for offset in 0..5 {
        load(context, output, 149 + offset * 2, ConfigurationKey(0x52ef + offset), Byte)?;
        context.write_u32(output.checked_add((151 + offset * 2) * 4)?, 0)?;
    }
    load(context, output, 159, ConfigurationKey(0x5259), Byte)?;
    load(context, output, 160, ConfigurationKey(0x525a), Byte)?;
    byte_range(context, output, 68, ConfigurationKey(0x5266), 2)?;
    load(context, output, 71, ConfigurationKey(0x526f), UnsignedHalf)?;
    for offset in 0..5 { load(context, output, 73 + offset * 2, ConfigurationKey(0x5271 + offset), Byte)?; }
    for (index, key) in [(106,0x52a9),(108,0x52aa),(110,0x160a5),(112,0x160a6),(114,0x52ad),(116,0x52ae)] {
        load(context,output,index,ConfigurationKey(key),Byte)?;
    }
    byte_range(context, output, 128, ConfigurationKey(0x52d9), 18)
}
