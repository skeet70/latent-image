use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, GuestDomain, HostAddressSpace, MemoryRegion, RecordContext, RegionDescriptor, RegionDomain, RegionPermission};
use crate::record_numeric_grid::{self as grid, Centroid, ProfileRecord, displacement};

#[test]
fn displacement_uses_integer_magnitude_and_unknown_previous_position() {
    assert_eq!(displacement(Centroid { x: 0, y: 0 }, Centroid { x: 3, y: 4 }), 5);
    assert_eq!(displacement(Centroid { x: 0, y: 0 }, Centroid { x: -3, y: -4 }), 5);
    assert_eq!(displacement(Centroid { x: 9, y: 8 }, Centroid { x: 10, y: 9 }), 1);
    assert_eq!(displacement(Centroid { x: -1, y: 9 }, Centroid { x: 200, y: 300 }), 0);
    assert_eq!(displacement(Centroid { x: 20, y: 21 }, Centroid { x: 20, y: 21 }), 0);
}

#[test]
fn profile_halfword_update_preserves_other_fields() {
    let mut record = ProfileRecord { words: [0x12345678; 10] };
    record.set_half(4, 99);
    record.set_half(22, 30);
    assert_eq!(record.words[1], 0x12340063);
    assert_eq!(record.words[5], 0x001e5678);
    assert_eq!(record.words[0], 0x12345678);
    assert_eq!(record.half(6), 0x1234);
}

#[test]
fn speed_history_is_visible_in_the_callers_address_space_and_discards_oldest() {
    let mut storage = [0u8; 40];
    let speeds = GuestAddress::new(0x016d8ff4);
    let descriptor = RegionDescriptor::new(speeds, ByteCount(40), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    let mut regions = [MemoryRegion::new(descriptor, &mut storage).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    assert_eq!(grid::helper_fa816c(&mut context), Ok(speeds));
    grid::helper_fa8188(&mut context).unwrap();
    grid::helper_fa81a4(&mut context, 7).unwrap();
    grid::helper_fa81a4(&mut context, 0xffff).unwrap();
    assert_eq!(context.read_u16(speeds), Ok(7));
    assert_eq!(context.read_u16(speeds.checked_add(2).unwrap()), Ok(0xffff));
    for value in 1..=21 { grid::helper_fa81a4(&mut context, value).unwrap(); }
    assert_eq!(context.read_u16(speeds), Ok(21));
    assert_eq!(context.read_u16(speeds.checked_add(38).unwrap()), Ok(2));
    assert!(context.read_u16(speeds.checked_add(40).unwrap()).is_err());
}

#[test]
fn diagnostic_initialization_sets_the_actual_shared_buffer_regions() {
    let mut first = [0x5au8; 0xb598];
    let mut second = [0xa5u8; 0xb598];
    let first_descriptor = RegionDescriptor::new(GuestAddress::new(0x1fea2280), ByteCount(0xb598), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    let second_descriptor = RegionDescriptor::new(GuestAddress::new(0x5fead820), ByteCount(0xb598), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    let mut regions = [MemoryRegion::new(first_descriptor, &mut first).unwrap(), MemoryRegion::new(second_descriptor, &mut second).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    grid::helper_f5251c(&mut context).unwrap();
    for address in [0x1fea2280, 0x1fea4e98, 0x1fead298, 0x5fead820, 0x5feb0438, 0x5feb8838] {
        assert_eq!(context.read_u32(GuestAddress::new(address)), Ok(u32::MAX));
    }
    for address in [0x1fead408, 0x5feb89a8] { assert_eq!(context.read_u32(GuestAddress::new(address)), Ok(3)); }
    for address in [0x1fea4e80, 0x5feb0420, 0x1fead40c, 0x5feb89ac] { assert_eq!(context.read_u32(GuestAddress::new(address)), Ok(0)); }
}
