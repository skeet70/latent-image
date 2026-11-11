//! Power mode selection and the guest string length: image-4 bodies
//! 0x006a_570c, 0x0072_1f30, 0x0074_001c, 0x0074_0d3c and 0x0074_0d50.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

/// The image-4 body at 0x006a_570c, whose argument is unread.
pub fn helper_6a570c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(0)
}

/// The image-4 body at 0x0072_1f30.
pub fn helper_721f30<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<u32, MemoryError> {
    let mut length = 0;
    let mut address = source;
    while context.read_u8(address)? != 0 {
        length += 1;
        address = address.checked_add(1)?;
    }
    Ok(length)
}

/// The image-4 body at 0x0074_001c: modes zero to six pass one bit each, which
/// the called body ignores.
pub fn helper_74001c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32, _flag: u32) -> Result<u32, MemoryError> {
    if mode > 6 { return Ok(0x84); }
    Ok(if helper_6a570c(context)? != 0 { 0x86 } else { 0 })
}

/// The image-4 body at 0x0074_0d3c.
pub fn helper_740d3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_74001c(context, 1, 1)
}

/// The image-4 body at 0x0074_0d50.
pub fn helper_740d50<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_74001c(context, 1, 0)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_740d3c, helper_740d3c_abi, (), () -> u32, (), (), crate::record_power_mode_6a570c::helper_740d3c, |value| value);
    stock_export!(xm1_power_record_helper_740d50, helper_740d50_abi, (), () -> u32, (), (), crate::record_power_mode_6a570c::helper_740d50, |value| value);
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x006a_570c-0x0074_0d50.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const TEXT: u32 = 0x0010_0000;

    fn with_text<R>(text: &mut [u8; 0x40], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(TEXT), ByteCount(0x40), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, text).unwrap()];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    #[test]
    fn the_mode_call_succeeds_without_a_memory_effect() {
        let mut text = [0u8; 0x40];
        with_text(&mut text, |context| {
            assert_eq!(helper_6a570c(context), Ok(0));
            assert_eq!(helper_74001c(context, 0, 0), Ok(0));
            assert_eq!(helper_74001c(context, 6, 1), Ok(0));
            assert_eq!(helper_74001c(context, 7, 0), Ok(0x84));
            assert_eq!(helper_740d3c(context), Ok(0));
            assert_eq!(helper_740d50(context), Ok(0));
        });
        assert_eq!(text, [0u8; 0x40]);
    }

    #[test]
    fn the_string_length_stops_at_the_first_zero() {
        let mut text = [0u8; 0x40];
        with_text(&mut text, |context| assert_eq!(helper_721f30(context, GuestAddress::new(TEXT)), Ok(0)));
        text[..3].copy_from_slice(b"abc");
        with_text(&mut text, |context| assert_eq!(helper_721f30(context, GuestAddress::new(TEXT)), Ok(3)));
        with_text(&mut text, |context| assert_eq!(helper_721f30(context, GuestAddress::new(TEXT + 1)), Ok(2)));
        with_text(&mut text, |context| assert_eq!(helper_721f30(context, GuestAddress::new(TEXT + 3)), Ok(0)));
        text = [b'x'; 0x40];
        with_text(&mut text, |context| assert_eq!(helper_721f30(context, GuestAddress::new(TEXT)), Err(MemoryError::Unmapped(GuestAddress::new(TEXT + 0x40)))));
    }
}
