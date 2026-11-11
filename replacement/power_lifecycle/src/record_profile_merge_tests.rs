use super::*;
use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

#[test]
fn selected_fields_merge_with_signed_and_packed_values_and_write_permissions() -> Result<(), MemoryError> {
    let mut registry = [0u8; 0x38 * 16 + 12];
    let mut resource = [0u8; 0x2158];
    let mut source = [0u8; 0x90];
    let mut scratch = [0u8; 8];
    let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite);
    let mut regions = [
        MemoryRegion::new(descriptor(0x00915d38, registry.len() as u32)?, &mut registry)?,
        MemoryRegion::new(descriptor(0x0096ebd0, resource.len() as u32)?, &mut resource)?,
        MemoryRegion::new(descriptor(0x1000, 0x90)?, &mut source)?,
        MemoryRegion::new(descriptor(0x2000, 8)?, &mut scratch)?,
    ];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions)?;
    let mut context = RecordContext::with_stack(memory, (), GuestStack::new(GuestAddress::new(0x2000), ByteCount(8))?);
    context.write_u32(GuestAddress::new(0x00915d44), 1)?;
    context.write_u8(GuestAddress::new(0x00915d50), 3)?;
    context.write_u32(GuestAddress::new(0x1080), 0x04008001)?;
    context.write_u32(GuestAddress::new(0x1088), 0x10000)?;
    context.write_u8(GuestAddress::new(0x1030), 7)?;
    context.write_u16(GuestAddress::new(0x101a), 0xfffd)?;
    context.write_u32(GuestAddress::new(0x1004), 0x12345678)?;
    context.write_u32(GuestAddress::new(0x1008), 0xfedcba98)?;
    context.write_u8(GuestAddress::new(0x1070), 19)?;
    context.write_u32(GuestAddress::new(0x0096ec50), u32::MAX)?;
    helper_759250(&mut context, 0x00fe0010, GuestAddress::new(0x1000))?;
    assert_eq!(context.read_u8(GuestAddress::new(0x0096ec00))?, 7);
    assert_eq!(context.read_u16(GuestAddress::new(0x0096ebea))?, 0xfffd);
    assert_eq!(context.read_u32(GuestAddress::new(0x0096ebd4))?, 0x12345678);
    assert_eq!(context.read_u32(GuestAddress::new(0x0096ebd8))?, 0xfedcba98);
    assert_eq!(context.read_u8(GuestAddress::new(0x0096ec40))?, 19);
    assert_eq!(context.read_u32(GuestAddress::new(0x0096ec50))?, 0x04008001);
    assert_eq!(context.read_u32(GuestAddress::new(0x0096ec58))?, 0x10000);

    context.write_u8(GuestAddress::new(0x00915d50), 1)?;
    context.write_u8(GuestAddress::new(0x0096ec00), 11)?;
    helper_759250(&mut context, 0x00fe0010, GuestAddress::new(0x1000))?;
    assert_eq!(context.read_u8(GuestAddress::new(0x0096ec00))?, 11);
    assert_eq!(context.read_u32(GuestAddress::new(0x0096ec50))?, 0);
    assert_eq!(context.read_u32(GuestAddress::new(0x0096ec58))?, 0);
    Ok(())
}
