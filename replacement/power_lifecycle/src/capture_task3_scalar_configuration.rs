use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_72da80<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(0x9168d8), u32::from(value == 1)) }
pub fn helper_72dacc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, enabled: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(0x9168f4), u32::from(kind == 0x2c && enabled == 1)) }
pub fn helper_72db1c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(0x9168dc), value) }
pub fn helper_72db5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(0x9168e0), value) }
pub fn helper_72dbdc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(0x9168e8), value) }

pub fn helper_72ef48<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> { context.copy_bytes(GuestAddress::new(0x916880), source, crate::record_address_space::ByteCount(56)) }
pub fn helper_730ce4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(0x916904), value) }
pub fn helper_7310fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { context.write_u32(GuestAddress::new(0x916908), value) }
pub fn helper_73111c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, settings: GuestAddress) -> Result<(), MemoryError> {
    let value = context.read_u32(settings.checked_add(0x3d8)?)?;
    helper_730ce4(context, value)?;
    let value = context.read_u32(settings.checked_add(0x3e0)?)?;
    helper_7310fc(context, value)
}
pub fn helper_739114<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    const CATEGORY: [u32; 57] = [5, 0, 24, 7, 8, 0, 0, 48, 49, 11, 13, 12, 12, 51, 50, 14, 47, 16, 52, 53, 17, 54, 55, 18, 19, 56, 57, 20, 21, 22, 58, 23, 62, 64, 65, 66, 67, 68, 69, 70, 71, 72, 37, 33, 34, 35, 36, 73, 25, 26, 27, 28, 29, 0, 0, 0, 33];
    Ok(CATEGORY.get(value.wrapping_sub(1) as usize).copied().unwrap_or(0))
}
pub fn helper_7393a8<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok((value & 1) << 13 | (value & 2) << 9 | (value & 4) << 12) }
pub fn helper_df14b8<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, reverse: u32, kind: u32) -> Result<u32, MemoryError> {
    Ok(kind.wrapping_add(if reverse != 0 { 12 } else if kind <= 1 { 3 } else { 4 }))
}
pub fn helper_dcd8c8<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, kind: u32, flags: u32) -> Result<u32, MemoryError> {
    let bits = match kind {
        0 => 1, 5 => 0x800, 7 => 0x1800, 8 => 8, 12 => 4, 13 => 2, 16 => 0x10, 17 => 0x40, 19 => 0x20,
        0x30 => 0x808, 0x31 => 0x1808, 0x32 => 0x804, 0x33 => 0x802, 0x34 => 0x810, 0x35 => 0x1810, 0x36 => 0x840, 0x37 => 0x1840, 0x38 => 0x820, 0x39 => 0x1820,
        0x40 => 0x80, 0x41 => 0x880, 0x42 => 0x1880, 0x43 => 0x100, 0x44 => 0x900, 0x45 => 0x1900, 0x46 => 0x180, 0x47 => 0x980, 0x48 => 0x1980, 0x49 => 0x200,
        _ => 0,
    };
    Ok(bits | flags & 0x6400)
}
