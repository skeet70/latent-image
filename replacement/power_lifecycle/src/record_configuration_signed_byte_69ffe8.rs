//! The sign-extended configuration byte at 0x0069_ffe8.
use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_native_exports::stock_export;

/// `sxtb` of the configuration byte at `key`, the image-4 body at 0x0069_ffe8.
pub fn helper_69ffe8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<i32, MemoryError> {
    crate::record_configuration::helper_69f8fc(context, key).map(|value| i32::from(value as u8 as i8))
}

stock_export!(
    xm1_power_record_helper_69ffe8,
    helper_69ffe8_abi,
    (),
    (key: u32) -> i32,
    (key: u32),
    (key),
    crate::record_configuration_signed_byte_69ffe8::helper_69ffe8,
    |value| value
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn high_configuration_bytes_sign_extend() {
        let mut pointer = 0x1000u32.to_le_bytes();
        let mut bytes = [0x7f, 0x80, 0xfe, 0xff];
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(0x1000, 4), &mut bytes).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, ());
        assert_eq!(helper_69ffe8(&mut context, 0), Ok(0x7f));
        assert_eq!(helper_69ffe8(&mut context, 1), Ok(-0x80));
        assert_eq!(helper_69ffe8(&mut context, 2), Ok(-2));
        assert_eq!(helper_69ffe8(&mut context, 3), Ok(-1));
    }
}
