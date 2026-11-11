use super::*;
use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

#[test]
fn saved_request_retains_received_payload_after_next_receive() -> Result<(), MemoryError> {
    let mut bytes = [0u8; 492];
    let descriptor = RegionDescriptor::new(QUEUE_CONTEXT, ByteCount(492), RegionDomain::Shared, RegionPermission::ReadWrite)?;
    let mut regions = [MemoryRegion::new(descriptor, &mut bytes)?];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions)?;
    let mut context = RecordContext::new(memory, ());
    initialize_payload_addresses(&mut context)?;
    context.write_u16(CURRENT_REQUEST, 0x3f49)?;
    context.write_u32(CURRENT_REQUEST.checked_add(4)?, 176)?;
    context.write_u32(GuestAddress::new(0x016f_0ab4), 0x12345678)?;
    assert_eq!(helper_fe9540(&mut context)?, SAVED_REQUEST);
    context.write_u16(CURRENT_REQUEST, 0x3f58)?;
    context.write_u32(GuestAddress::new(0x016f_0ab4), 0x87654321)?;
    assert_eq!(context.read_u16(SAVED_REQUEST)?, 0x3f49);
    assert_eq!(context.read_u32(GuestAddress::new(0x016f_0ba0))?, 176);
    assert_eq!(context.read_u32(GuestAddress::new(0x016f_0ba4))?, 0x016f_0ab4);
    assert_eq!(context.read_u32(GuestAddress::new(0x016f_0ba8))?, 0x12345678);
    Ok(())
}

#[test]
fn context_snapshot_caps_copy_without_overwriting_following_byte() -> Result<(), MemoryError> {
    let mut source = [0x5au8; 492];
    let mut destination = [0xc3u8; 493];
    let source_descriptor = RegionDescriptor::new(QUEUE_CONTEXT, ByteCount(492), RegionDomain::Shared, RegionPermission::ReadOnly)?;
    let destination_descriptor = RegionDescriptor::new(GuestAddress::new(0x1000), ByteCount(493), RegionDomain::Shared, RegionPermission::ReadWrite)?;
    let mut regions = [MemoryRegion::new(source_descriptor, &mut source)?, MemoryRegion::new(destination_descriptor, &mut destination)?];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions)?;
    let mut context = RecordContext::new(memory, ());
    assert_eq!(helper_fe9474(&mut context, GuestAddress::new(0x1000), ByteCount(1000))?, ByteCount(492));
    assert_eq!(context.read_u8(GuestAddress::new(0x11eb))?, 0x5a);
    assert_eq!(context.read_u8(GuestAddress::new(0x11ec))?, 0xc3);
    Ok(())
}
