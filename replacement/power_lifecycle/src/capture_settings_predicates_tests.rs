use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, GuestDomain, HostAddressSpace, MemoryError, MemoryRegion, RecordContext, RegionDescriptor, RegionDomain, RegionPermission};
use crate::capture_settings_predicates::{evaluate, SettingsDifference};
use crate::capture_settings_predicates_runtime::helper_dec93c;
use crate::record_profile_aux_native::{helper_735fc0, helper_75d668};

fn descriptor(base: u32, length: u32) -> RegionDescriptor {
    RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
}

#[test]
fn quad_update_returns_first_source_word_and_preserves_neighbors() {
    let mut bytes = [0x55; 0x200];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 0x200), &mut bytes).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    for (index, value) in [1, 2, 3, 4].into_iter().enumerate() {
        context.write_u32(GuestAddress::new(0x1100 + index as u32 * 4), value).unwrap();
    }
    assert_eq!(helper_735fc0(&mut context, GuestAddress::new(0x1000), GuestAddress::new(0x1100)), Ok(1));
    assert_eq!(helper_735fc0(&mut context, GuestAddress::new(0x1000), GuestAddress::new(0x1100)), Ok(0));
    assert_eq!(helper_75d668(&mut context, GuestAddress::new(0x1000)), Ok(0x55555555));
    assert_eq!(context.read_u32(GuestAddress::new(0x1018)), Ok(0x55555555));
    for (index, expected) in [1, 2, 3, 4].into_iter().enumerate() {
        assert_eq!(context.read_u32(GuestAddress::new(0x1008 + index as u32 * 4)), Ok(expected));
    }
}

#[test]
fn incomplete_quad_source_faults_before_register_group_is_stored() {
    let mut bytes = [0x55; 0x108];
    bytes[0x100..0x104].copy_from_slice(&1u32.to_le_bytes());
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 0x108), &mut bytes).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    assert_eq!(helper_735fc0(&mut context, GuestAddress::new(0x1000), GuestAddress::new(0x1100)), Err(MemoryError::Unmapped(GuestAddress::new(0x1108))));
    assert_eq!(context.read_u32(GuestAddress::new(0x1008)), Ok(0x55555555));
}

#[test]
fn grouped_setting_change_depends_on_membership_not_numeric_difference() {
    let mut before = [0; 0x3e0];
    let mut after = [0; 0x3e0];
    before[0x13c..0x140].copy_from_slice(&5u32.to_le_bytes());
    after[0x13c..0x140].copy_from_slice(&6u32.to_le_bytes());
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 0x3e0), &mut before).unwrap(), MemoryRegion::new(descriptor(0x2000, 0x3e0), &mut after).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    assert_eq!(evaluate(&mut context, SettingsDifference::PredicateDE4E34, GuestAddress::new(0x1000), GuestAddress::new(0x2000)), Ok(false));
    context.write_u32(GuestAddress::new(0x213c), 7).unwrap();
    assert_eq!(evaluate(&mut context, SettingsDifference::PredicateDE4E34, GuestAddress::new(0x1000), GuestAddress::new(0x2000)), Ok(true));
}

#[test]
fn query_returns_shared_auxiliary_state_and_preserves_completion_value() {
    let mut request = [0xff; 16];
    let mut event = [0xff; 2];
    let mut state = 3u16.to_le_bytes();
    let mut regions = [MemoryRegion::new(descriptor(0x01531f00, 16), &mut request).unwrap(), MemoryRegion::new(descriptor(0x015457f6, 2), &mut event).unwrap(), MemoryRegion::new(descriptor(0x015458b6, 2), &mut state).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    assert_eq!(helper_dec93c(&mut context), Ok(3));
    assert_eq!(context.read_u32(GuestAddress::new(0x01531f00)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x01531f04)), Ok(0));
    assert_eq!(context.read_u32(GuestAddress::new(0x01531f08)), Ok(3));
    assert_eq!(context.read_u32(GuestAddress::new(0x01531f0c)), Ok(u32::MAX));
    assert_eq!(context.read_u16(GuestAddress::new(0x015457f6)), Ok(0));
    assert_eq!(context.read_u16(GuestAddress::new(0x015458b6)), Ok(3));
}
