//! Raw profile-header CPU shares the canonical resource field and mask operations.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{copy_words, mark_mask_previous, offset, read_header, write_header, HeaderField};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct HeaderTriplet(pub [u16; 3]);
macro_rules! leaf_getter {
    ($name:ident, $field:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress) -> Result<u32, MemoryError> { read_header(context, resource, $field) }
    };
}
macro_rules! leaf_setter {
    ($name:ident, $field:expr, $low:expr, $high:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, value: u32) -> Result<u32, MemoryError> {
            write_header(context, resource, $field, value)?;
            mark_mask_previous(context, resource, $low, $high)
        }
    };
}
leaf_getter!(helper_75b054, HeaderField::Byte(0x30));
leaf_getter!(helper_75b088, HeaderField::Byte(0x31));
leaf_getter!(helper_75b0bc, HeaderField::Halfword(0x2e));
leaf_getter!(helper_75b0f0, HeaderField::Byte(0x32));
leaf_getter!(helper_75b124, HeaderField::Byte(0x33));
leaf_getter!(helper_75b158, HeaderField::Halfword(0x26));
leaf_getter!(helper_75b18c, HeaderField::SignedHalfword(0x18));
leaf_getter!(helper_75b1c0, HeaderField::Byte(0x5b));
leaf_getter!(helper_75b1f4, HeaderField::Byte(0x5c));
leaf_getter!(helper_75b228, HeaderField::Byte(0x5d));
leaf_getter!(helper_75b25c, HeaderField::Byte(0x34));
leaf_getter!(helper_75b2e8, HeaderField::Byte(0x35));
leaf_getter!(helper_75b31c, HeaderField::Byte(0x36));
leaf_getter!(helper_75b350, HeaderField::Byte(0x37));
leaf_getter!(helper_75b384, HeaderField::Byte(0x38));
leaf_getter!(helper_75b3b8, HeaderField::Byte(0x39));
leaf_getter!(helper_75b3ec, HeaderField::Byte(0x3a));
leaf_getter!(helper_75b420, HeaderField::Byte(0x3b));
leaf_getter!(helper_75b454, HeaderField::Byte(0x3c));
leaf_getter!(helper_75b488, HeaderField::Byte(0x3d));
leaf_getter!(helper_75b4f0, HeaderField::Byte(0x3f));
leaf_getter!(helper_75b4bc, HeaderField::Byte(0x3e));
leaf_getter!(helper_75b58c, HeaderField::Byte(0x40));
leaf_getter!(helper_75b5c0, HeaderField::Byte(0x41));
leaf_getter!(helper_75b5f4, HeaderField::Byte(0x42));
leaf_getter!(helper_75b524, HeaderField::SignedHalfword(0x1a));
leaf_getter!(helper_75b558, HeaderField::Byte(0x5e));
leaf_getter!(helper_75b628, HeaderField::Byte(0x43));
leaf_getter!(helper_75b65c, HeaderField::SignedHalfword(0x1c));
leaf_getter!(helper_75b690, HeaderField::Word(0x14));
leaf_getter!(helper_75b6c4, HeaderField::SignedHalfword(0x1e));
leaf_getter!(helper_75b6f8, HeaderField::Byte(0x44));
leaf_getter!(helper_75b72c, HeaderField::SignedHalfword(0x20));
leaf_getter!(helper_75b760, HeaderField::SignedHalfword(0x22));
leaf_getter!(helper_75b794, HeaderField::SignedHalfword(0x24));
leaf_getter!(helper_75b7c8, HeaderField::Byte(0x46));
leaf_getter!(helper_75b7fc, HeaderField::Byte(0x47));
leaf_getter!(helper_75b87c, HeaderField::Byte(0x5f));
leaf_getter!(helper_75b8b0, HeaderField::Byte(0x60));
leaf_getter!(helper_75b8e4, HeaderField::Byte(0x61));
leaf_getter!(helper_75b94c, HeaderField::Byte(0x48));
leaf_getter!(helper_75b918, HeaderField::Halfword(0x28));
leaf_getter!(helper_75b980, HeaderField::Byte(0x49));
leaf_getter!(helper_75b9b4, HeaderField::Byte(0x4a));
leaf_getter!(helper_75ba34, HeaderField::Halfword(0x2a));
leaf_getter!(helper_75ba68, HeaderField::Byte(0x45));
leaf_getter!(helper_75ba9c, HeaderField::Byte(0x4b));
leaf_getter!(helper_75bad0, HeaderField::Byte(0x4c));
leaf_getter!(helper_75bb04, HeaderField::Byte(0x4d));
leaf_getter!(helper_75bb38, HeaderField::Byte(0x4e));
leaf_getter!(helper_75bb6c, HeaderField::Byte(0x4f));
leaf_getter!(helper_75bba0, HeaderField::Byte(0x50));
leaf_getter!(helper_75bbd4, HeaderField::Byte(0x63));
leaf_getter!(helper_75bc08, HeaderField::Byte(0x62));
leaf_getter!(helper_75bc3c, HeaderField::Byte(0x6a));
leaf_getter!(helper_75bc70, HeaderField::Byte(0x65));
leaf_getter!(helper_75bca4, HeaderField::Byte(0x51));
leaf_getter!(helper_75bcd8, HeaderField::Byte(0x52));
leaf_getter!(helper_75bd0c, HeaderField::Byte(0x54));
leaf_getter!(helper_75bd40, HeaderField::Byte(0x53));
leaf_getter!(helper_75be44, HeaderField::Byte(0x6b));
leaf_getter!(helper_75bd74, HeaderField::Byte(0x55));
leaf_getter!(helper_75bda8, HeaderField::Byte(0x64));
leaf_getter!(helper_75bddc, HeaderField::Byte(0x66));
leaf_getter!(helper_75be10, HeaderField::Byte(0x67));
leaf_getter!(helper_75bed4, HeaderField::Byte(0x68));
leaf_getter!(helper_75bf08, HeaderField::Byte(0x69));
leaf_getter!(helper_75bf3c, HeaderField::Halfword(0x2c));
leaf_getter!(helper_75bf70, HeaderField::Byte(0x6c));
leaf_getter!(helper_75bfa4, HeaderField::Byte(0x6d));
leaf_setter!(helper_75b06c, HeaderField::Byte(0x30), 0x1, 0x0);
leaf_setter!(helper_75b0a0, HeaderField::Byte(0x31), 0x2, 0x0);
leaf_setter!(helper_75b0d4, HeaderField::Halfword(0x2e), 0x8000, 0x80000000);
leaf_setter!(helper_75b108, HeaderField::Byte(0x32), 0x4, 0x0);
leaf_setter!(helper_75b13c, HeaderField::Byte(0x33), 0x0, 0x8000);
leaf_setter!(helper_75b170, HeaderField::Halfword(0x26), 0x0, 0x10000);
leaf_setter!(helper_75b1a4, HeaderField::Halfword(0x18), 0x0, 0x20000);
leaf_setter!(helper_75b1d8, HeaderField::Byte(0x5b), 0x8, 0x0);
leaf_setter!(helper_75b20c, HeaderField::Byte(0x5c), 0x10, 0x0);
leaf_setter!(helper_75b240, HeaderField::Byte(0x5d), 0x20, 0x0);
leaf_setter!(helper_75b274, HeaderField::Byte(0x34), 0x40, 0x0);
leaf_setter!(helper_75b300, HeaderField::Byte(0x35), 0x0, 0x4000000);
leaf_setter!(helper_75b334, HeaderField::Byte(0x36), 0x100, 0x0);
leaf_setter!(helper_75b368, HeaderField::Byte(0x37), 0x200, 0x0);
leaf_setter!(helper_75b39c, HeaderField::Byte(0x38), 0x400, 0x0);
leaf_setter!(helper_75b3d0, HeaderField::Byte(0x39), 0x800, 0x0);
leaf_setter!(helper_75b404, HeaderField::Byte(0x3a), 0x0, 0x40000);
leaf_setter!(helper_75b438, HeaderField::Byte(0x3b), 0x0, 0x80000);
leaf_setter!(helper_75b46c, HeaderField::Byte(0x3c), 0x1000, 0x0);
leaf_setter!(helper_75b4a0, HeaderField::Byte(0x3d), 0x2000, 0x0);
leaf_setter!(helper_75b4d4, HeaderField::Byte(0x3e), 0x200, 0x80000000);
leaf_setter!(helper_75b508, HeaderField::Byte(0x3f), 0x4000, 0x0);
leaf_setter!(helper_75b5a4, HeaderField::Byte(0x40), 0x0, 0x200000);
leaf_setter!(helper_75b5d8, HeaderField::Byte(0x41), 0x0, 0x400000);
leaf_setter!(helper_75b60c, HeaderField::Byte(0x42), 0x40, 0x80000000);
leaf_setter!(helper_75b53c, HeaderField::Halfword(0x1a), 0x8000, 0x0);
leaf_setter!(helper_75b570, HeaderField::Byte(0x5e), 0x10000, 0x0);
leaf_setter!(helper_75b640, HeaderField::Byte(0x43), 0x20000, 0x0);
leaf_setter!(helper_75b674, HeaderField::Halfword(0x1c), 0x40000, 0x0);
leaf_setter!(helper_75b6a8, HeaderField::Word(0x14), 0x80000, 0x0);
leaf_setter!(helper_75b6dc, HeaderField::Halfword(0x1e), 0x100000, 0x0);
leaf_setter!(helper_75b710, HeaderField::Byte(0x44), 0x200000, 0x0);
leaf_setter!(helper_75b744, HeaderField::Halfword(0x20), 0x0, 0x800000);
leaf_setter!(helper_75b778, HeaderField::Halfword(0x22), 0x400000, 0x0);
leaf_setter!(helper_75b7ac, HeaderField::Halfword(0x24), 0x800000, 0x0);
leaf_setter!(helper_75b7e0, HeaderField::Byte(0x46), 0x1000000, 0x0);
leaf_setter!(helper_75b814, HeaderField::Byte(0x47), 0x2000000, 0x0);
leaf_setter!(helper_75b894, HeaderField::Byte(0x5f), 0x8000000, 0x0);
leaf_setter!(helper_75b8c8, HeaderField::Byte(0x60), 0x10000000, 0x0);
leaf_setter!(helper_75b8fc, HeaderField::Byte(0x61), 0x0, 0x400);
leaf_setter!(helper_75b930, HeaderField::Halfword(0x28), 0x0, 0x2000);
leaf_setter!(helper_75b964, HeaderField::Byte(0x48), 0x20000000, 0x0);
leaf_setter!(helper_75b998, HeaderField::Byte(0x49), 0x40000000, 0x0);
leaf_setter!(helper_75b9cc, HeaderField::Byte(0x4a), 0x80000000, 0x0);
leaf_setter!(helper_75ba4c, HeaderField::Halfword(0x2a), 0x0, 0x1000);
leaf_setter!(helper_75ba80, HeaderField::Byte(0x45), 0x0, 0x2);
leaf_setter!(helper_75bab4, HeaderField::Byte(0x4b), 0x0, 0x4);
leaf_setter!(helper_75bae8, HeaderField::Byte(0x4c), 0x0, 0x20);
leaf_setter!(helper_75bb1c, HeaderField::Byte(0x4d), 0x0, 0x40);
leaf_setter!(helper_75bb50, HeaderField::Byte(0x4e), 0x0, 0x8);
leaf_setter!(helper_75bb84, HeaderField::Byte(0x4f), 0x0, 0x10);
leaf_setter!(helper_75bbb8, HeaderField::Byte(0x50), 0x0, 0x800);
leaf_setter!(helper_75bbec, HeaderField::Byte(0x63), 0x0, 0x80);
leaf_setter!(helper_75bc20, HeaderField::Byte(0x62), 0x1, 0x80000000);
leaf_setter!(helper_75bc54, HeaderField::Byte(0x6a), 0x8, 0x80000000);
leaf_setter!(helper_75bc88, HeaderField::Byte(0x65), 0x0, 0x2000000);
leaf_setter!(helper_75bcbc, HeaderField::Byte(0x51), 0x0, 0x100);
leaf_setter!(helper_75bcf0, HeaderField::Byte(0x52), 0x0, 0x200);
leaf_setter!(helper_75bd24, HeaderField::Byte(0x54), 0x0, 0x100000);
leaf_setter!(helper_75bd58, HeaderField::Byte(0x53), 0x0, 0x1000000);
leaf_setter!(helper_75be5c, HeaderField::Byte(0x6b), 0x4, 0x80000000);
leaf_setter!(helper_75bd8c, HeaderField::Byte(0x55), 0x2, 0x80000000);
leaf_setter!(helper_75bdc0, HeaderField::Byte(0x64), 0x0, 0x4000);
leaf_setter!(helper_75bdf4, HeaderField::Byte(0x66), 0x0, 0x8000000);
leaf_setter!(helper_75be28, HeaderField::Byte(0x67), 0x0, 0x10000000);
leaf_setter!(helper_75beec, HeaderField::Byte(0x68), 0x0, 0x20000000);
leaf_setter!(helper_75bf20, HeaderField::Byte(0x69), 0x10, 0x80000000);
leaf_setter!(helper_75bf54, HeaderField::Halfword(0x2c), 0x20, 0x80000000);
leaf_setter!(helper_75bfbc, HeaderField::Byte(0x6d), 0x100, 0x80000000);
leaf_setter!(helper_75bf88, HeaderField::Byte(0x6c), 0x80, 0x80000000);

pub fn helper_75b290<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress) -> Result<u32, MemoryError> { context.read_u32(resource) }
pub fn helper_75b2c0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> { context.copy_bytes(resource, source, ByteCount(4))?; mark_mask_previous(context, resource, 0x80, 0) }
pub fn helper_75b830<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, resource: GuestAddress) -> Result<GuestAddress, MemoryError> { copy_words::<2>(context, destination, offset(resource, 4))?; Ok(destination) }
pub fn helper_75b854<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> { copy_words::<2>(context, offset(resource, 4), source)?; mark_mask_previous(context, resource, 0x4000000, 0) }
pub fn helper_75b9e8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, resource: GuestAddress) -> Result<GuestAddress, MemoryError> { context.copy_bytes(destination, offset(resource, 14), ByteCount(6))?; Ok(destination) }
pub fn helper_75ba08<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> { context.copy_bytes(offset(resource, 14), source, ByteCount(6))?; mark_mask_previous(context, resource, 0, 1) }
pub fn helper_75be78<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress) -> Result<u32, MemoryError> { context.read_u16(offset(resource, 12)).map(u32::from) }
pub fn helper_75bea8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, source: GuestAddress) -> Result<u32, MemoryError> { context.copy_bytes(offset(resource, 12), source, ByteCount(2))?; mark_mask_previous(context, resource, 0, 0x40000000) }
