//! The descriptor triple at 0x016b_664c, image-5 body 0x00f2_4ba8.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_native_exports::stock_export;

pub const DESCRIPTOR: u32 = 0x016b_664c;

/// The image-5 body at 0x00f2_4ba8.
pub fn store_descriptor<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, first: u16, second: u16) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(DESCRIPTOR), value)?;
    context.write_u16(GuestAddress::new(DESCRIPTOR + 4), first)?;
    context.write_u16(GuestAddress::new(DESCRIPTOR + 6), second)
}

stock_export!(
    xm1_power_record_helper_f24ba8,
    helper_f24ba8_abi,
    (),
    (value: u32, first: u32, second: u32) -> (),
    (value: u32, first: u16, second: u16),
    (value, first as u16, second as u16),
    crate::record_descriptor_triplet_f24a94::store_descriptor,
    |_| ()
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn the_triple_stores_a_word_then_two_halfwords() {
        let mut block = [0u8; 8];
        let descriptor = RegionDescriptor::new(GuestAddress::new(DESCRIPTOR), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(store_descriptor(&mut context, 0x0a0b_0c0d, 0x1234, 0xabcd), Ok(()));
        assert_eq!(context.read_u32(GuestAddress::new(DESCRIPTOR)), Ok(0x0a0b_0c0d));
        assert_eq!(context.read_u16(GuestAddress::new(DESCRIPTOR + 4)), Ok(0x1234));
        assert_eq!(context.read_u16(GuestAddress::new(DESCRIPTOR + 6)), Ok(0xabcd));
        assert_eq!(store_descriptor(&mut context, 1, 0x2345, 0x0001), Ok(()));
        assert_eq!(context.read_u16(GuestAddress::new(DESCRIPTOR + 4)), Ok(0x2345));
        assert_eq!(context.read_u16(GuestAddress::new(DESCRIPTOR + 6)), Ok(0x0001));
    }
}
