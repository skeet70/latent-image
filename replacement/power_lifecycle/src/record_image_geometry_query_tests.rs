use super::*;
use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

struct Lock(i32);
impl SnapshotLock for Lock { fn acquire(&mut self) -> i32 { self.0 } fn release(&mut self) -> i32 { 0 } }
fn descriptor(base: u32, length: u32) -> RegionDescriptor {
    RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
}
#[test]
fn unavailable_snapshot_preserves_unwritten_result_fields() {
    let mut output = [0xa5; 0x430];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, output.len() as u32), &mut output).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, Lock(-50));
    assert_eq!(helper_10510e8(&mut context, 1, GuestAddress::new(0x2000), GuestAddress::new(0x1000), GuestAddress::new(0)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x1000)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x1004)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x1008)), Ok(0xa5a5a5a5));
    assert_eq!(context.read_u32(GuestAddress::new(0x100c)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x1010)), Ok(0xa5a5a5a5));
}
#[test]
fn rejected_kind_does_not_require_query_or_snapshot_memory() {
    let mut regions = [];
    let memory = HostAddressSpace::new(GuestDomain::B, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, Lock(0));
    assert_eq!(helper_10512cc(&mut context, 36, GuestAddress::new(0x2000), GuestAddress::new(0x1000), GuestAddress::new(0)), Ok(0x84));
}
#[test]
fn snapshot_copy_excludes_dimensions_and_clears_only_documented_extent() {
    let mut snapshot_bytes = [0; 48];
    for (index, value) in [100u32, 200, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10].into_iter().enumerate() {
        snapshot_bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut output = [0xa5; 0x424];
    let mut regions = [MemoryRegion::new(descriptor(0x4171b800, 48), &mut snapshot_bytes).unwrap(), MemoryRegion::new(descriptor(0x1000, 0x424), &mut output).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, Lock(0));
    assert_eq!(helper_10517a0(&mut context, GuestAddress::new(0x1000)), Ok(0));
    for (index, value) in [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10].into_iter().enumerate() {
        assert_eq!(context.read_u32(GuestAddress::new(0x1000 + index as u32 * 4)), Ok(value));
    }
    assert_eq!(context.read_u32(GuestAddress::new(0x1028)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x141c)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x1420)), Ok(0xa5a5a5a5));
}
