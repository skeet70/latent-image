//! X-M1 parameter-field CPU preserves the installed signed-halfword and word widths.
use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{offset, read_bank, resource_address, read_header, HeaderField, ProfileHandle, ProfileField};
macro_rules! parameter_getter {
    ($name:ident, $field:expr) => { pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
        let bank = read_bank(context, ProfileHandle(handle), ProfileField(1))?;
        read_header(context, offset(resource_address(bank), 0x90), $field)
    } };
}
parameter_getter!(helper_750efc, HeaderField::SignedHalfword(0x2));
parameter_getter!(helper_750f94, HeaderField::SignedHalfword(0x4));
parameter_getter!(helper_75102c, HeaderField::SignedHalfword(0x6));
parameter_getter!(helper_7510c4, HeaderField::SignedHalfword(0x8));
parameter_getter!(helper_75115c, HeaderField::SignedHalfword(0xa));
parameter_getter!(helper_7511f4, HeaderField::SignedHalfword(0xc));
parameter_getter!(helper_75128c, HeaderField::SignedHalfword(0xe));
parameter_getter!(helper_751324, HeaderField::SignedHalfword(0x10));
parameter_getter!(helper_7513bc, HeaderField::SignedHalfword(0x12));
parameter_getter!(helper_751454, HeaderField::SignedHalfword(0x14));
parameter_getter!(helper_7514ec, HeaderField::SignedHalfword(0x16));
parameter_getter!(helper_751584, HeaderField::SignedHalfword(0x18));
parameter_getter!(helper_75161c, HeaderField::SignedHalfword(0x1a));
parameter_getter!(helper_7516b4, HeaderField::SignedHalfword(0x1c));
parameter_getter!(helper_75174c, HeaderField::SignedHalfword(0x1e));
parameter_getter!(helper_7517e4, HeaderField::SignedHalfword(0x20));
parameter_getter!(helper_75187c, HeaderField::SignedHalfword(0x22));
parameter_getter!(helper_751914, HeaderField::SignedHalfword(0x24));
parameter_getter!(helper_7519ac, HeaderField::SignedHalfword(0x26));
parameter_getter!(helper_751a44, HeaderField::SignedHalfword(0x28));
parameter_getter!(helper_751adc, HeaderField::SignedHalfword(0x2a));
parameter_getter!(helper_751b74, HeaderField::Word(0x2c));
parameter_getter!(helper_751c0c, HeaderField::Word(0x30));
parameter_getter!(helper_751ca4, HeaderField::Word(0x34));
