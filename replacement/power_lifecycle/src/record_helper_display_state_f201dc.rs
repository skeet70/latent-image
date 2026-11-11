//! Image-5 helper.
//!
//! The cluster's other two functions, 0x00f2_01dc and 0x00f2_01f8, are not
//! written: 0x00f2_01dc calls 0x00f1_626c and 0x00f2_01f8 calls 0x0074_8928,
//! which have no body in this tree.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_image5_state::DISPLAY_STATE;
use crate::record_native_exports::stock_export;

pub fn helper_f2032c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.write_u32(GuestAddress::new(DISPLAY_STATE + 0x18), 0)?;
    Ok(1)
}

stock_export!(
    xm1_power_record_helper_f2032c,
    helper_f2032c_abi,
    (),
    () -> i32,
    (),
    (),
    crate::record_helper_display_state_f201dc::helper_f2032c,
    |value| value as i32
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f2_032c.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn clears_the_word_at_offset_18_and_reports_success() {
        let mut state = [0xa5u8; 0x20];
        let descriptor = RegionDescriptor::new(GuestAddress::new(DISPLAY_STATE), ByteCount(0x20), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut state).unwrap()];
        let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
        assert_eq!(helper_f2032c(&mut context), Ok(1));
        assert_eq!(&state[0x18..0x1c], &0u32.to_le_bytes());
        assert_eq!(state[0x17], 0xa5);
        assert_eq!(state[0x1c], 0xa5);
    }
}
