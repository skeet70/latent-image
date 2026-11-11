//! X-M1 profile layouts and CPU decisions confirmed against the pinned installed image-4 bytes.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ProfileHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ProfileField(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ProfileBank(pub u32);
#[repr(C)]
pub struct RegistrySlot { serial: u32, reserved: u32, bank: u32, permissions: [u8; 43], padding: u8 }
#[repr(C)]
pub struct ProfileRegistry { prefix: [u32; 2], selected_bank: u32, slots: [RegistrySlot; 16] }
#[repr(C)]
pub struct ProfileHeader { pub words00: [u32; 6], pub signed18: [i16; 7], pub halfwords26: [u16; 5], pub bytes30: [u8; 0x50], pub primary_mask: [u32; 2], pub secondary_mask: [u32; 2] }
#[repr(C)]
pub struct ProfileParameters { pub halfwords: [i16; 22], pub words: [u32; 3] }
#[repr(C)]
pub struct ProfileResource { pub header: ProfileHeader, pub parameters: ProfileParameters, pub remainder: [u8; 0x2158 - 0xc8] }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct ProfileWords<const N: usize>(pub [u32; N]);
const _: () = assert!(core::mem::size_of::<RegistrySlot>() == 0x38);
const _: () = assert!(core::mem::size_of::<ProfileHeader>() == 0x90);
const _: () = assert!(core::mem::size_of::<ProfileResource>() == 0x2158);

pub fn offset(address: GuestAddress, bytes: u32) -> GuestAddress { GuestAddress::new(address.get().wrapping_add(bytes)) }
pub fn resource_address(bank: ProfileBank) -> GuestAddress { GuestAddress::new(0x0096_ebd0u32.wrapping_add(bank.0.wrapping_mul(0x2158))) }
fn slot_address(handle: ProfileHandle) -> GuestAddress { GuestAddress::new(0x0091_5d44 + (handle.0 & 15) * 0x38) }
pub fn selected_bank(context: &mut impl GuestAddressSpace) -> Result<ProfileBank, MemoryError> { context.read_u32(GuestAddress::new(0x0091_5d40)).map(ProfileBank) }
pub fn valid(context: &mut impl GuestAddressSpace, handle: ProfileHandle) -> Result<bool, MemoryError> {
    if handle.0 == u32::MAX { return Ok(false); }
    Ok(((handle.0 >> 4) & 0xfff) == context.read_u32(slot_address(handle))?)
}
pub fn can_read(context: &mut impl GuestAddressSpace, handle: ProfileHandle, field: ProfileField) -> Result<bool, MemoryError> {
    if field.0 >= 43 || !valid(context, handle)? { return Ok(false); }
    let selection = (handle.0 >> 18) & 63;
    if selection != 63 && selection != field.0 { return Ok(false); }
    Ok(context.read_u8(offset(slot_address(handle), 12 + field.0))? & 1 != 0)
}
pub fn can_write(context: &mut impl GuestAddressSpace, handle: ProfileHandle, field: ProfileField) -> Result<bool, MemoryError> {
    if field.0 >= 43 || !valid(context, handle)? { return Ok(false); }
    let selection = (handle.0 >> 18) & 63;
    if (selection != 63 && selection != field.0) || handle.0 & (1 << 17) == 0 { return Ok(false); }
    Ok(context.read_u8(offset(slot_address(handle), 12 + field.0))? & 2 != 0)
}
pub fn handle_bank(context: &mut impl GuestAddressSpace, handle: ProfileHandle) -> Result<ProfileBank, MemoryError> {
    let bank = context.read_u32(offset(slot_address(handle), 8))?;
    if bank <= 1 { Ok(ProfileBank(bank)) } else { selected_bank(context) }
}
pub fn read_bank(context: &mut impl GuestAddressSpace, handle: ProfileHandle, field: ProfileField) -> Result<ProfileBank, MemoryError> {
    if can_read(context, handle, field)? { handle_bank(context, handle) } else { selected_bank(context) }
}
pub fn write_bank(context: &mut impl GuestAddressSpace, handle: ProfileHandle, field: ProfileField) -> Result<Option<ProfileBank>, MemoryError> {
    if can_write(context, handle, field)? { handle_bank(context, handle).map(Some) } else { Ok(None) }
}
pub fn mark_mask_previous(context: &mut impl GuestAddressSpace, resource: GuestAddress, low: u32, high: u32) -> Result<u32, MemoryError> {
    let address = offset(resource, if high & 0x8000_0000 != 0 { 0x88 } else { 0x80 });
    let previous_low = context.read_u32(address)?;
    let previous_high = context.read_u32(offset(address, 4))?;
    context.write_u32(address, previous_low | low)?;
    context.write_u32(offset(address, 4), previous_high | (high & 0x7fff_ffff))?;
    Ok(previous_low)
}
pub fn mark_mask(context: &mut impl GuestAddressSpace, resource: GuestAddress, low: u32, high: u32) -> Result<(), MemoryError> { mark_mask_previous(context, resource, low, high).map(|_| ()) }
pub fn header_contains_mask(context: &mut impl GuestAddressSpace, header: GuestAddress, low: u32, high: u32) -> Result<bool, MemoryError> {
    let address = offset(header, if high & 0x8000_0000 != 0 { 0x88 } else { 0x80 });
    let first = context.read_u32(address)?;
    let second = context.read_u32(offset(address, 4))?;
    Ok(first & low == low && second & (high & 0x7fff_ffff) == high & 0x7fff_ffff)
}
pub fn copy_words<const N: usize>(context: &mut impl GuestAddressSpace, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    let mut index = 0;
    while index < N {
        let count = (N - index).min(4);
        let words = match count {
            1 => crate::record_address_space::RegisterWordCount::One,
            2 => crate::record_address_space::RegisterWordCount::Two,
            3 => crate::record_address_space::RegisterWordCount::Three,
            _ => crate::record_address_space::RegisterWordCount::Four,
        };
        context.copy_register_words(offset(destination, index as u32 * 4), offset(source, index as u32 * 4), words)?;
        index += count;
    }
    Ok(())
}
#[derive(Clone, Copy)]
pub enum HeaderField { Byte(u32), Halfword(u32), SignedHalfword(u32), Word(u32) }
pub fn read_header(context: &mut impl GuestAddressSpace, resource: GuestAddress, field: HeaderField) -> Result<u32, MemoryError> {
    match field {
        HeaderField::Byte(at) => context.read_u8(offset(resource, at)).map(u32::from),
        HeaderField::Halfword(at) => context.read_u16(offset(resource, at)).map(u32::from),
        HeaderField::SignedHalfword(at) => context.read_u16(offset(resource, at)).map(|value| value as i16 as i32 as u32),
        HeaderField::Word(at) => context.read_u32(offset(resource, at)),
    }
}
pub fn write_header(context: &mut impl GuestAddressSpace, resource: GuestAddress, field: HeaderField, value: u32) -> Result<(), MemoryError> {
    match field {
        HeaderField::Byte(at) => context.write_u8(offset(resource, at), value as u8),
        HeaderField::Halfword(at) | HeaderField::SignedHalfword(at) => context.write_u16(offset(resource, at), value as u16),
        HeaderField::Word(at) => context.write_u32(offset(resource, at), value),
    }
}
macro_rules! header_getter {
    ($name:ident, $field:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
            let bank = read_bank(context, ProfileHandle(handle), ProfileField(0))?;
            read_header(context, resource_address(bank), $field)
        }
    };
}
macro_rules! header_setter {
    ($name:ident, $field:expr, $low:expr, $high:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(0))? {
                let resource = resource_address(bank);
                write_header(context, resource, $field, value)?;
                mark_mask(context, resource, $low, $high)?;
            }
            Ok(())
        }
    };
}
header_getter!(helper_74e5c0, HeaderField::Byte(0x30));
header_getter!(helper_74e648, HeaderField::Byte(0x31));
header_getter!(helper_74e718, HeaderField::Halfword(0x2e));
header_getter!(helper_74e7f0, HeaderField::Byte(0x32));
header_getter!(helper_74e878, HeaderField::Byte(0x33));
header_getter!(helper_74e900, HeaderField::Halfword(0x26));
header_getter!(helper_74e988, HeaderField::SignedHalfword(0x18));
header_getter!(helper_74ea10, HeaderField::Byte(0x5b));
header_getter!(helper_74ea98, HeaderField::Byte(0x5c));
header_getter!(helper_74eb20, HeaderField::Byte(0x5d));
header_getter!(helper_74eba8, HeaderField::Byte(0x34));
header_getter!(helper_74ece4, HeaderField::Byte(0x35));
header_getter!(helper_74ed6c, HeaderField::Byte(0x36));
header_getter!(helper_74edf4, HeaderField::Byte(0x37));
header_getter!(helper_74ee7c, HeaderField::Byte(0x38));
header_getter!(helper_74ef04, HeaderField::Byte(0x39));
header_getter!(helper_74ef8c, HeaderField::Byte(0x3a));
header_getter!(helper_74f014, HeaderField::Byte(0x3b));
header_getter!(helper_74f09c, HeaderField::Byte(0x3c));
header_getter!(helper_74f124, HeaderField::Byte(0x3d));
header_getter!(helper_74f1ac, HeaderField::Byte(0x3f));
header_getter!(helper_74f1f0, HeaderField::Byte(0x3e));
header_getter!(helper_74f2bc, HeaderField::Byte(0x40));
header_getter!(helper_74f344, HeaderField::Byte(0x41));
header_getter!(helper_74f3cc, HeaderField::Byte(0x42));
header_getter!(helper_74f454, HeaderField::SignedHalfword(0x1a));
header_getter!(helper_74f4dc, HeaderField::Byte(0x5e));
header_getter!(helper_74f564, HeaderField::Byte(0x43));
header_getter!(helper_74f5ec, HeaderField::SignedHalfword(0x1c));
header_getter!(helper_74f674, HeaderField::Word(0x14));
header_getter!(helper_74f6fc, HeaderField::SignedHalfword(0x1e));
header_getter!(helper_74f784, HeaderField::Byte(0x44));
header_getter!(helper_74f80c, HeaderField::SignedHalfword(0x20));
header_getter!(helper_74f894, HeaderField::SignedHalfword(0x22));
header_getter!(helper_74f91c, HeaderField::SignedHalfword(0x24));
header_getter!(helper_74f9a4, HeaderField::Byte(0x46));
header_getter!(helper_74fa2c, HeaderField::Byte(0x47));
header_getter!(helper_74fb54, HeaderField::Byte(0x5f));
header_getter!(helper_74fbdc, HeaderField::Byte(0x60));
header_getter!(helper_74fc64, HeaderField::Byte(0x61));
header_getter!(helper_74fcec, HeaderField::Byte(0x48));
header_getter!(helper_74fd74, HeaderField::Halfword(0x28));
header_getter!(helper_74fdfc, HeaderField::Byte(0x49));
header_getter!(helper_74fe84, HeaderField::Byte(0x4a));
header_getter!(helper_74ffac, HeaderField::Halfword(0x2a));
header_getter!(helper_750034, HeaderField::Byte(0x45));
header_getter!(helper_7500bc, HeaderField::Byte(0x4b));
header_getter!(helper_750144, HeaderField::Byte(0x4c));
header_getter!(helper_7501cc, HeaderField::Byte(0x4d));
header_getter!(helper_750254, HeaderField::Byte(0x4e));
header_getter!(helper_7502dc, HeaderField::Byte(0x4f));
header_getter!(helper_750364, HeaderField::Byte(0x50));
header_getter!(helper_7503ec, HeaderField::Byte(0x63));
header_getter!(helper_750474, HeaderField::Byte(0x62));
header_getter!(helper_7504fc, HeaderField::Byte(0x6a));
header_getter!(helper_750584, HeaderField::Byte(0x65));
header_getter!(helper_75060c, HeaderField::Byte(0x51));
header_getter!(helper_750694, HeaderField::Byte(0x52));
header_getter!(helper_75071c, HeaderField::Byte(0x54));
header_getter!(helper_7507a4, HeaderField::Byte(0x53));
header_getter!(helper_75082c, HeaderField::Byte(0x6b));
header_getter!(helper_7508b4, HeaderField::Byte(0x55));
header_getter!(helper_75093c, HeaderField::Byte(0x64));
header_getter!(helper_7509c4, HeaderField::Byte(0x66));
header_getter!(helper_750ac0, HeaderField::Byte(0x67));
header_getter!(helper_750c00, HeaderField::Byte(0x68));
header_getter!(helper_750c88, HeaderField::Byte(0x69));
header_getter!(helper_750d54, HeaderField::Byte(0x6c));
header_getter!(helper_750d98, HeaderField::Byte(0x6d));
header_setter!(helper_74e604, HeaderField::Byte(0x30), 0x1, 0x0);
header_setter!(helper_74e6d4, HeaderField::Byte(0x31), 0x2, 0x0);
header_setter!(helper_74e75c, HeaderField::Halfword(0x2e), 0x8000, 0x80000000);
header_setter!(helper_74e834, HeaderField::Byte(0x32), 0x4, 0x0);
header_setter!(helper_74e8bc, HeaderField::Byte(0x33), 0x0, 0x8000);
header_setter!(helper_74e944, HeaderField::Halfword(0x26), 0x0, 0x10000);
header_setter!(helper_74e9cc, HeaderField::Halfword(0x18), 0x0, 0x20000);
header_setter!(helper_74ea54, HeaderField::Byte(0x5b), 0x8, 0x0);
header_setter!(helper_74eadc, HeaderField::Byte(0x5c), 0x10, 0x0);
header_setter!(helper_74eb64, HeaderField::Byte(0x5d), 0x20, 0x0);
header_setter!(helper_74ebec, HeaderField::Byte(0x34), 0x40, 0x0);
header_setter!(helper_74ed28, HeaderField::Byte(0x35), 0x0, 0x4000000);
header_setter!(helper_74edb0, HeaderField::Byte(0x36), 0x100, 0x0);
header_setter!(helper_74ee38, HeaderField::Byte(0x37), 0x200, 0x0);
header_setter!(helper_74eec0, HeaderField::Byte(0x38), 0x400, 0x0);
header_setter!(helper_74ef48, HeaderField::Byte(0x39), 0x800, 0x0);
header_setter!(helper_74efd0, HeaderField::Byte(0x3a), 0x0, 0x40000);
header_setter!(helper_74f058, HeaderField::Byte(0x3b), 0x0, 0x80000);
header_setter!(helper_74f0e0, HeaderField::Byte(0x3c), 0x1000, 0x0);
header_setter!(helper_74f168, HeaderField::Byte(0x3d), 0x2000, 0x0);
header_setter!(helper_74f234, HeaderField::Byte(0x3e), 0x200, 0x80000000);
header_setter!(helper_74f278, HeaderField::Byte(0x3f), 0x4000, 0x0);
header_setter!(helper_74f300, HeaderField::Byte(0x40), 0x0, 0x200000);
header_setter!(helper_74f388, HeaderField::Byte(0x41), 0x0, 0x400000);
header_setter!(helper_74f410, HeaderField::Byte(0x42), 0x40, 0x80000000);
header_setter!(helper_74f498, HeaderField::Halfword(0x1a), 0x8000, 0x0);
header_setter!(helper_74f520, HeaderField::Byte(0x5e), 0x10000, 0x0);
header_setter!(helper_74f5a8, HeaderField::Byte(0x43), 0x20000, 0x0);
header_setter!(helper_74f630, HeaderField::Halfword(0x1c), 0x40000, 0x0);
header_setter!(helper_74f6b8, HeaderField::Word(0x14), 0x80000, 0x0);
header_setter!(helper_74f740, HeaderField::Halfword(0x1e), 0x100000, 0x0);
header_setter!(helper_74f7c8, HeaderField::Byte(0x44), 0x200000, 0x0);
header_setter!(helper_74f850, HeaderField::Halfword(0x20), 0x0, 0x800000);
header_setter!(helper_74f8d8, HeaderField::Halfword(0x22), 0x400000, 0x0);
header_setter!(helper_74f960, HeaderField::Halfword(0x24), 0x800000, 0x0);
header_setter!(helper_74f9e8, HeaderField::Byte(0x46), 0x1000000, 0x0);
header_setter!(helper_74fa70, HeaderField::Byte(0x47), 0x2000000, 0x0);
header_setter!(helper_74fb98, HeaderField::Byte(0x5f), 0x8000000, 0x0);
header_setter!(helper_74fc20, HeaderField::Byte(0x60), 0x10000000, 0x0);
header_setter!(helper_74fca8, HeaderField::Byte(0x61), 0x0, 0x400);
header_setter!(helper_74fd30, HeaderField::Halfword(0x28), 0x0, 0x2000);
header_setter!(helper_74fdb8, HeaderField::Byte(0x48), 0x20000000, 0x0);
header_setter!(helper_74fe40, HeaderField::Byte(0x49), 0x40000000, 0x0);
header_setter!(helper_74fec8, HeaderField::Byte(0x4a), 0x80000000, 0x0);
header_setter!(helper_74fff0, HeaderField::Halfword(0x2a), 0x0, 0x1000);
header_setter!(helper_750078, HeaderField::Byte(0x45), 0x0, 0x2);
header_setter!(helper_750100, HeaderField::Byte(0x4b), 0x0, 0x4);
header_setter!(helper_750188, HeaderField::Byte(0x4c), 0x0, 0x20);
header_setter!(helper_750210, HeaderField::Byte(0x4d), 0x0, 0x40);
header_setter!(helper_750298, HeaderField::Byte(0x4e), 0x0, 0x8);
header_setter!(helper_750320, HeaderField::Byte(0x4f), 0x0, 0x10);
header_setter!(helper_7503a8, HeaderField::Byte(0x50), 0x0, 0x800);
header_setter!(helper_750430, HeaderField::Byte(0x63), 0x0, 0x80);
header_setter!(helper_7504b8, HeaderField::Byte(0x62), 0x1, 0x80000000);
header_setter!(helper_750540, HeaderField::Byte(0x6a), 0x8, 0x80000000);
header_setter!(helper_7505c8, HeaderField::Byte(0x65), 0x0, 0x2000000);
header_setter!(helper_750650, HeaderField::Byte(0x51), 0x0, 0x100);
header_setter!(helper_7506d8, HeaderField::Byte(0x52), 0x0, 0x200);
header_setter!(helper_750760, HeaderField::Byte(0x54), 0x0, 0x100000);
header_setter!(helper_7507e8, HeaderField::Byte(0x53), 0x0, 0x1000000);
header_setter!(helper_750870, HeaderField::Byte(0x6b), 0x4, 0x80000000);
header_setter!(helper_7508f8, HeaderField::Byte(0x55), 0x2, 0x80000000);
header_setter!(helper_750980, HeaderField::Byte(0x64), 0x0, 0x4000);
header_setter!(helper_750a08, HeaderField::Byte(0x66), 0x0, 0x8000000);
header_setter!(helper_750b04, HeaderField::Byte(0x67), 0x0, 0x10000000);
header_setter!(helper_750c44, HeaderField::Byte(0x68), 0x0, 0x20000000);
header_setter!(helper_750ccc, HeaderField::Byte(0x69), 0x10, 0x80000000);
header_setter!(helper_750d10, HeaderField::Halfword(0x2c), 0x20, 0x80000000);
header_setter!(helper_750ddc, HeaderField::Byte(0x6d), 0x100, 0x80000000);
header_setter!(helper_750e20, HeaderField::Byte(0x6c), 0x80, 0x80000000);

pub fn helper_75a8c0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, field: u32) -> Result<u32, MemoryError> { can_write(context, ProfileHandle(handle), ProfileField(field)).map(u32::from) }
pub fn helper_75a910<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, field: u32) -> Result<u32, MemoryError> { can_read(context, ProfileHandle(handle), ProfileField(field)).map(u32::from) }
pub fn helper_75a960<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<u32, MemoryError> { Ok((second & 15) | ((first & 15) << 4)) }
pub fn helper_75a7c4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, handle: u32, field: u32, access: u32, tag: u32) -> Result<u32, MemoryError> {
    if handle == u32::MAX || (field != 63 && field > 42) || access & !3 != 0 { return Ok(u32::MAX); }
    let previous = (handle >> 18) & 63;
    let field = if field == 63 && previous != 63 { previous } else { field };
    if previous != 63 && access != previous { return Ok(u32::MAX); }
    if handle & (1 << 17) == 0 && access & 2 != 0 { return Ok(u32::MAX); }
    Ok((handle & 0xffff) | (((access >> 1) & 1) << 17) | ((field & 63) << 18) | ((tag & 255) << 24))
}
pub fn helper_74e020<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, _ignored: u32, low: u32, high: u32) -> Result<u32, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(0))?;
    header_contains_mask(context, resource_address(bank), low, high).map(u32::from)
}
pub fn helper_75ac74<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, _ignored: u32, low: u32, high: u32) -> Result<u32, MemoryError> { header_contains_mask(context, resource, low, high).map(u32::from) }
pub fn helper_75ac1c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, _ignored: u32, low: u32, high: u32) -> Result<u32, MemoryError> { mark_mask_previous(context, resource, low, high) }
pub fn helper_75ace4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: GuestAddress, _ignored: u32, low: u32, high: u32) -> Result<u32, MemoryError> {
    let (requested, other) = if high & 0x8000_0000 != 0 { (0x88, 0x80) } else { (0x80, 0x88) };
    let first = context.read_u32(offset(resource, requested))?;
    let second = context.read_u32(offset(resource, requested + 4))?;
    if [first, second] == [low, high & 0x7fff_ffff] { return Ok(1); }
    let first = context.read_u32(offset(resource, other))?;
    let second = context.read_u32(offset(resource, other + 4))?;
    Ok(u32::from(first | second == 0))
}
pub fn helper_74e68c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> { context.read_u32(GuestAddress::new(0x0091_9788)) }
pub fn helper_74e7a0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32) -> Result<u32, MemoryError> { context.read_u16(GuestAddress::new(0x0091_978c)).map(u32::from) }
header_getter!(helper_74ec30, HeaderField::Word(0));
header_getter!(helper_750b48, HeaderField::Halfword(12));
header_getter!(helper_74e340, HeaderField::Word(0x2118));
header_getter!(helper_74e3e0, HeaderField::Word(0x2120));
header_getter!(helper_74e480, HeaderField::Word(0x2124));
header_getter!(helper_74e520, HeaderField::Word(0x2128));
header_getter!(helper_75a178, HeaderField::Byte(0x6f));
pub fn helper_750a4c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(1))?;
    copy_words::<14>(context, destination, offset(resource_address(bank), 0x90))?;
    Ok(destination)
}
pub fn helper_74e1a4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(1))? { copy_words::<14>(context, offset(resource_address(bank), 0x90), source)?; }
    Ok(())
}
pub fn helper_74e284<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress, index: u32) -> Result<(), MemoryError> {
    for field in 5..=10 { if !can_write(context, ProfileHandle(handle), ProfileField(field))? { return Ok(()); } }
    let bank = handle_bank(context, ProfileHandle(handle))?;
    helper_75ae78(context, offset(resource_address(bank), 0x16cu32.wrapping_add(index.wrapping_mul(0xa8))), source)
}
pub fn helper_75ae3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> { copy_words::<14>(context, destination, source) }
pub fn helper_75ae78<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> { context.copy_bytes(destination, source, crate::record_address_space::ByteCount(168)) }
pub fn helper_75ae8c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> { context.copy_bytes(destination, source, crate::record_address_space::ByteCount(124)) }
pub fn helper_75af54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> { copy_words::<5>(context, destination, source) }
pub fn helper_74fab4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(0))?;
    crate::record_profile_header_fields::helper_75b830(context, destination, resource_address(bank))
}
pub fn helper_74ff0c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(0))?;
    crate::record_profile_header_fields::helper_75b9e8(context, destination, resource_address(bank))
}
macro_rules! special_setter {
    ($name:ident, $leaf:ident) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
            if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(0))? { crate::record_profile_header_fields::$leaf(context, resource_address(bank), source)?; }
            Ok(())
        }
    };
}
special_setter!(helper_74eca0, helper_75b2c0);
special_setter!(helper_74fb10, helper_75b854);
special_setter!(helper_74ff68, helper_75ba08);
special_setter!(helper_750bbc, helper_75bea8);

pub fn helper_74e6ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(27))? { context.write_u32(GuestAddress::new(0x0091_9788), value)?; }
    Ok(())
}
pub fn helper_74e7c4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(27))? { context.write_u16(GuestAddress::new(0x0091_978c), value as u16)?; }
    Ok(())
}
