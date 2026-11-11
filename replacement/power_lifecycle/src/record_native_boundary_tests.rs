use crate::record_address_space::{
    ByteCount, CallbackRegistry, GuestAddress, GuestAddressSpace, GuestDomain, HostAddressSpace,
    MemoryRegion, RecordContext, RegionDescriptor, RegionDomain, RegionPermission,
};
use crate::record_native_boundary::AbiFault;

fn descriptor(base: u32, length: u32) -> RegionDescriptor {
    RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
}

fn context<'a>(regions: &'a mut [MemoryRegion<'a>]) -> RecordContext<HostAddressSpace<'a, 'a>, ()> {
    RecordContext::new(HostAddressSpace::new(GuestDomain::A, regions).unwrap(), ())
}

#[test]
fn zero_argument_adapter_returns_the_body_result() {
    let mut scratch = [0u8; 4];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 4), &mut scratch).unwrap()];
    let mut context = context(&mut regions);
    assert_eq!(crate::record_native_exports::record_image_init_state::helper_1050f8c_abi(&mut context), Ok(0));
}

#[test]
fn zero_argument_adapter_maps_a_body_error_to_the_fault() {
    let mut scratch = [0u8; 4];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 4), &mut scratch).unwrap()];
    let mut context = context(&mut regions);
    assert_eq!(crate::record_native_exports::record_profile_globals::helper_76d9c8_abi(&mut context), Err(AbiFault));
}

#[test]
fn one_argument_adapter_returns_the_body_result() {
    let mut scratch = [0u8; 4];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 4), &mut scratch).unwrap()];
    let mut context = context(&mut regions);
    assert_eq!(crate::record_native_exports::record_profile_aux_native::helper_75e6f0_abi(&mut context, 0x1234), Ok(0x1234));
    assert_eq!(crate::record_native_exports::record_scan_bounds_modes::helper_f4a0e4_abi(2), Ok(0x30));
}

#[test]
fn two_argument_adapter_returns_the_body_result() {
    assert_eq!(crate::record_native_exports::record_scan_bounds_modes::helper_f4a154_abi(1, 0x12), Ok(0x13));
}

#[test]
fn three_argument_adapter_returns_the_body_result() {
    let mut scratch = [0u8; 4];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 4), &mut scratch).unwrap()];
    let mut context = context(&mut regions);
    assert_eq!(crate::record_native_exports::record_scan_bounds_modes::helper_f4a848_abi(&mut context, 0, 0x22, 1), Ok(0x22));
}

#[test]
fn four_argument_adapter_writes_each_output() {
    let mut table = [0u8; 0x48];
    for (index, offset) in [0x34u32, 0x38, 0x3c, 0x40].into_iter().enumerate() {
        table[offset as usize..offset as usize + 4].copy_from_slice(&(0x100 + index as u32).to_le_bytes());
    }
    let mut outputs = [0u8; 16];
    let mut regions = [
        MemoryRegion::new(descriptor(0x914db8, 0x48), &mut table).unwrap(),
        MemoryRegion::new(descriptor(0x2000, 16), &mut outputs).unwrap(),
    ];
    let mut context = context(&mut regions);
    let result = crate::record_native_exports::record_profile_mapping::helper_740888_abi(
        &mut context,
        GuestAddress::new(0x2000),
        GuestAddress::new(0x2004),
        GuestAddress::new(0x2008),
        GuestAddress::new(0x200c),
    );
    assert_eq!(result, Ok(()));
    for (index, address) in [0x2000u32, 0x2004, 0x2008, 0x200c].into_iter().enumerate() {
        assert_eq!(context.read_u32(GuestAddress::new(address)), Ok(0x100 + index as u32));
    }
}

#[test]
fn five_argument_adapter_returns_the_body_result() {
    let mut scratch = [0u8; 4];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 4), &mut scratch).unwrap()];
    let mut context = context(&mut regions);
    assert_eq!(five::boundary_sample5_abi(&mut context, 1, 2, 3, 4, 5), Ok(15));
}

#[test]
fn seven_argument_adapter_maps_a_body_error_to_the_fault() {
    let mut scratch = [0u8; 4];
    let mut regions = [MemoryRegion::new(descriptor(0x1000, 4), &mut scratch).unwrap()];
    let mut context = context(&mut regions);
    assert_eq!(crate::record_native_exports::record_profile_mapping::helper_74947c_abi(&mut context, 0, 0, 0, 0, 0, 0, 0), Err(AbiFault));
}

#[test]
fn callback_word_zero_reaches_the_body_and_a_foreign_word_faults() {
    let mut result = [0u8; 4];
    let mut regions = [MemoryRegion::new(descriptor(0x0170_3090, 4), &mut result).unwrap()];
    let mut context = context(&mut regions);
    let registry = CallbackRegistry::<_, 1>::new();
    assert_eq!(crate::record_native_exports::record_image_init_state::helper_1039610_abi(&mut context, 0, &registry), Ok(0));
    assert_eq!(crate::record_native_exports::record_image_init_state::helper_1039610_abi(&mut context, 1, &registry), Err(AbiFault));
}

mod five {
    use super::*;

    fn five_body<M: crate::record_address_space::GuestAddressSpace, E>(
        _: &mut RecordContext<M, E>,
        a: u32,
        b: u32,
        c: u32,
        d: u32,
        e: u32,
    ) -> Result<u32, crate::record_address_space::MemoryError> {
        Ok(a.wrapping_add(b).wrapping_add(c).wrapping_add(d).wrapping_add(e))
    }

    crate::record_native_exports::stock_export!(
        xm1_power_record_helper_boundary_sample5,
        boundary_sample5_abi,
        (),
        (a: u32, b: u32, c: u32, d: u32, e: u32) -> u32,
        (a: u32, b: u32, c: u32, d: u32, e: u32),
        (a, b, c, d, e),
        five_body,
        |value| value
    );
}
