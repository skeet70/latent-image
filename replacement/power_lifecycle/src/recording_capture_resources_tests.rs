use crate::recording_capture_resources::*;

#[test]
fn every_capture_slot_is_available_once_before_exhaustion() {
    let count = CapturePoolCount::new(202).unwrap();
    let mut bitmap = [0x1234_5678; 10];
    initialize_bitmap(count, &mut bitmap);
    for expected in 0..202 { assert_eq!(acquire_bitmap(count, &mut bitmap), Some(CapturePoolIndex(expected))); }
    assert_eq!(acquire_bitmap(count, &mut bitmap), None);
    assert_eq!(acquire_bitmap(count, &mut bitmap), None);
}

#[test]
fn auxiliary_pool_has_only_one_slot_and_reinitialization_restores_it() {
    let count = CapturePoolCount::new(1).unwrap();
    let mut bitmap = [0; 10];
    initialize_bitmap(count, &mut bitmap);
    assert_eq!(acquire_bitmap(count, &mut bitmap), Some(CapturePoolIndex(0)));
    assert_eq!(acquire_bitmap(count, &mut bitmap), None);
    initialize_bitmap(count, &mut bitmap);
    assert_eq!(acquire_bitmap(count, &mut bitmap), Some(CapturePoolIndex(0)));
}

#[test]
fn an_empty_pool_never_allocates_and_capacity_is_bounded() {
    let count = CapturePoolCount::new(0).unwrap();
    let mut bitmap = [0; 10];
    initialize_bitmap(count, &mut bitmap);
    assert_eq!(acquire_bitmap(count, &mut bitmap), None);
    assert!(CapturePoolCount::new(320).is_some());
    assert!(CapturePoolCount::new(321).is_none());
}

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, GuestDomain, HostAddressSpace, MemoryRegion, RecordContext, RegionDescriptor, RegionDomain, RegionPermission, MemoryError};

fn region(base: u32, length: u32) -> RegionDescriptor {
    RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
}

#[test]
fn shared_resource_engine_allocates_and_queries_actual_backing() {
    let mut backing = [0xa5; 0x1ab0];
    let mut regions = [MemoryRegion::new(region(0x015b_5be8, 0x1ab0), &mut backing).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    helper_eb25d8(&mut context).unwrap();
    assert_eq!(context.read_u16(GuestAddress::new(0x015b_742a)), Ok(0xa5a5));
    let root = helper_eb26ac(&mut context, false).unwrap();
    assert_eq!(root, GuestAddress::new(0x015b_5be8));
    assert_eq!(helper_eb2838(&mut context, root), Ok(GuestAddress::new(0)));
    let primary = helper_eb276c(&mut context, root).unwrap();
    assert_eq!(primary, GuestAddress::new(0x015b_5c18));
    assert_eq!(helper_eb2838(&mut context, root), Ok(primary));
    assert_eq!(context.read_u32(primary), Ok(0));
    let limits = helper_eb288c(&mut context, root).unwrap();
    assert_eq!(helper_eb2958(&mut context, root), Ok(limits));
    assert_eq!(limits, GuestAddress::new(0x015b_5ca8));
    assert_eq!(context.read_u32(limits.checked_add(8).unwrap()), Ok(0));
    let auxiliary = helper_eb34bc(&mut context, root).unwrap();
    assert_eq!(auxiliary, GuestAddress::new(0x015b_73bc));
    assert_eq!(helper_eb35ec(&mut context, root), Ok(auxiliary));
}

#[test]
fn allocation_failure_retains_published_slot_and_completed_clear_bytes() {
    let mut header = [0xff; 0x30];
    let mut primary = [0xa5; 8];
    let mut pool = [0; 0x30];
    pool[..2].copy_from_slice(&202u16.to_le_bytes());
    let mut regions = [
        MemoryRegion::new(region(0x015b_5be8, 0x30), &mut header).unwrap(),
        MemoryRegion::new(region(0x015b_5c18, 8), &mut primary).unwrap(),
        MemoryRegion::new(region(0x015b_7458, 0x30), &mut pool).unwrap(),
    ];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    assert_eq!(helper_eb276c(&mut context, GuestAddress::new(0x015b_5be8)), Err(CaptureResourceError::Memory(MemoryError::Unmapped(GuestAddress::new(0x015b_5c20)))));
    assert_eq!(context.read_u16(GuestAddress::new(0x015b_5bea)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x015b_5c18)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x015b_5c1c)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x015b_7460)), Ok(1));
}
