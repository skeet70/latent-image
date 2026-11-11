//! The adjustment profile table at 0x00f83484: one 64-byte row per profile.
//!
//! Image-5 bodies 0x00f77d90-0x00f78134.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

/// The table the image-5 bodies 0x00f77d90-0x00f78134 index.
pub const PROFILE_TABLE: u32 = 0x00f8_3484;
/// The row stride of that table.
pub const PROFILE_STRIDE: u32 = 64;

fn field(index: u32, offset: u32) -> GuestAddress {
    GuestAddress::new(PROFILE_TABLE.wrapping_add(index.wrapping_mul(PROFILE_STRIDE)).wrapping_add(offset))
}

/// The image-5 body at 0x00f77d90.
pub fn helper_f77d90<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    context.read_u32(field(index, 0x0c))
}

/// The image-5 body at 0x00f77e20.
pub fn helper_f77e20<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    context.read_u32(field(index, 0x34))
}

/// The image-5 body at 0x00f77e44.
pub fn helper_f77e44<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    context.read_u32(field(index, 0x20))
}

/// The image-5 body at 0x00f77e8c.
pub fn helper_f77e8c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    context.read_u32(field(index, 0x28))
}

/// The image-5 body at 0x00f77eb0.
pub fn helper_f77eb0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u8, MemoryError> {
    context.read_u8(field(index, 0x30))
}

/// The image-5 body at 0x00f77ed4.
pub fn helper_f77ed4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    context.read_u32(field(index, 0x24))
}

/// The image-5 body at 0x00f78134.
pub fn helper_f78134<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u16, MemoryError> {
    context.read_u16(field(index, 0x08))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f77d90-0x00f78134.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn with_table<R>(table: &mut [u8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(PROFILE_TABLE), ByteCount(table.len() as u32), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, table).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn each_accessor_reads_its_row_field() {
        let mut table = [0u8; 0x100];
        let row = PROFILE_STRIDE as usize * 2;
        table[row + 0x0c..row + 0x10].copy_from_slice(&0x1111_1111u32.to_le_bytes());
        table[row + 0x20..row + 0x24].copy_from_slice(&0x2222_2222u32.to_le_bytes());
        table[row + 0x24..row + 0x28].copy_from_slice(&0x3333_3333u32.to_le_bytes());
        table[row + 0x28..row + 0x2c].copy_from_slice(&0x4444_4444u32.to_le_bytes());
        table[row + 0x34..row + 0x38].copy_from_slice(&0x5555_5555u32.to_le_bytes());
        table[row + 0x30] = 0x66;
        table[row + 0x08..row + 0x0a].copy_from_slice(&0x7777u16.to_le_bytes());
        with_table(&mut table, |context| {
            assert_eq!(helper_f77d90(context, 2), Ok(0x1111_1111));
            assert_eq!(helper_f77e44(context, 2), Ok(0x2222_2222));
            assert_eq!(helper_f77ed4(context, 2), Ok(0x3333_3333));
            assert_eq!(helper_f77e8c(context, 2), Ok(0x4444_4444));
            assert_eq!(helper_f77e20(context, 2), Ok(0x5555_5555));
            assert_eq!(helper_f77eb0(context, 2), Ok(0x66));
            assert_eq!(helper_f78134(context, 2), Ok(0x7777));
            assert_eq!(helper_f77d90(context, 0), Ok(0));
        });
    }
}
