use super::*;
use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

#[test]
fn request_publication_preserves_disposition_and_other_contexts() {
    let mut first = [0u8; 8];
    first[4..].copy_from_slice(&0x81234567u32.to_le_bytes());
    let mut second = [0u8; 8];
    second[4..].copy_from_slice(&7u32.to_le_bytes());
    let descriptor = RegionDescriptor::new(GuestAddress::new(0x016bfe48), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    let mut first_regions = [MemoryRegion::new(descriptor, &mut first).unwrap()];
    let mut second_regions = [MemoryRegion::new(descriptor, &mut second).unwrap()];
    let mut first_context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut first_regions).unwrap(), ());
    let mut second_context = RecordContext::new(HostAddressSpace::new(GuestDomain::B, &mut second_regions).unwrap(), ());
    helper_f363d8(&mut first_context, 0x3456789a).unwrap();
    assert_eq!(first_context.read_u32(GuestAddress::new(0x016bfe48)), Ok(0x3456789a));
    assert_eq!(helper_f363b8(&mut first_context), Ok(0x81234567u32 as i32));
    assert_eq!(second_context.read_u32(GuestAddress::new(0x016bfe48)), Ok(0));
    assert_eq!(helper_f363b8(&mut second_context), Ok(7));
    helper_f363d8(&mut second_context, 0xffffffff).unwrap();
    assert_eq!(first_context.read_u32(GuestAddress::new(0x016bfe48)), Ok(0x3456789a));
}

#[test]
fn request_publication_preserves_memory_refusals() {
    let mut bytes = [0u8; 8];
    let descriptor = RegionDescriptor::new(GuestAddress::new(0x016bfe48), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadOnly).unwrap();
    let mut regions = [MemoryRegion::new(descriptor, &mut bytes).unwrap()];
    let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
    assert_eq!(helper_f363d8(&mut context, 1), Err(MemoryError::ReadOnly(GuestAddress::new(0x016bfe48))));
    assert_eq!(context.read_u32(GuestAddress::new(0x016bfe48)), Ok(0));
}
