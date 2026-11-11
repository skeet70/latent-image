//! Image-5 calibration pair export, bodies 0x00da_b8d8-0x00ee_62b0.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const PROFILE_SELECTOR: u32 = 0x0169_953c + 0x24;

/// The image-5 body at 0x00da_b8d8.
pub fn helper_dab8d8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _first: u32, _second: u32, _third: u32, out: GuestAddress) -> Result<(), MemoryError> {
    for index in 0..4u32 { context.write_u16(out.checked_add(index * 2)?, 0)?; }
    Ok(())
}

/// The image-5 body at 0x00ee_6278.
pub fn helper_ee6278(value: u32) -> u32 {
    match value { 1 => 1, 2 => 2, 3 => 3, _ => 0 }
}

/// The image-5 body at 0x00ee_62b0.
pub fn helper_ee62b0<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, _first: u32, profile: u32, out: GuestAddress,
) -> Result<(), MemoryError> {
    let _ = crate::record_hardware_deep_calibration::helper_ed7d84(context, profile)?;
    let selector = context.read_u32(GuestAddress::new(PROFILE_SELECTOR))?;
    let _ = helper_ee6278(selector);
    helper_dab8d8(context, 0, 0, 0, out)?;
    for index in 0..4u32 {
        let value = context.read_u16(out.checked_add(index * 2)?)? as i16 as i32 as u32;
        context.write_u32(out.checked_add(index * 4)?, value)?;
    }
    Ok(())
}

/// The image-5 body at 0x00ed_b0ac.
pub fn helper_edb0ac<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, first: u32, profile: u32, out: GuestAddress,
) -> Result<(), MemoryError> {
    helper_ee62b0(context, first, profile, out)
}

#[cfg(test)]
mod tests {
    //! Expected values are the image-5 stores at 0x00da_b8d8-0x00ee_62b0.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn the_selector_clamps_to_the_three_known_profiles() {
        assert_eq!(helper_ee6278(0), 0);
        assert_eq!(helper_ee6278(1), 1);
        assert_eq!(helper_ee6278(2), 2);
        assert_eq!(helper_ee6278(3), 3);
        assert_eq!(helper_ee6278(4), 0);
    }

    #[test]
    fn the_export_writes_four_zero_words_over_the_output() {
        for profile in [5u32, 40] {
            let mut selector = 2u32.to_le_bytes();
            let mut out = [0xffu8; 16];
            let mut profiles = [0u8; 0x240];
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(PROFILE_SELECTOR, 4), &mut selector).unwrap(),
                MemoryRegion::new(descriptor(0x0010_0000, 16), &mut out).unwrap(),
                MemoryRegion::new(descriptor(0x00ed_7e08, 0x240), &mut profiles).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let mut context = RecordContext::new(space, ());
            assert_eq!(helper_edb0ac(&mut context, 0, profile, GuestAddress::new(0x0010_0000)), Ok(()));
            assert_eq!(&out[..], &[0u8; 16]);
        }
    }
}
