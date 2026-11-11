//! Image-4 helper.
//!
//! The other two functions of the cluster, 0x0075_a63c and 0x0075_a6cc, are not
//! written: both call 0x0075_aa3c, 0x0075_a9fc, 0x0075_e3a0, 0x0075_e3e0,
//! 0x0075_e698, 0x0075_e20c, 0x0075_aab4, 0x0075_a9e4 and 0x0075_e464, which
//! have no body in this tree.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const SHARED_BLOCK: u32 = 0x0091_904c;

const HALFWORD_OFFSET: u32 = 0x970;

pub fn helper_75a3f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    context.read_u16(GuestAddress::new(SHARED_BLOCK + HALFWORD_OFFSET)).map(|value| i32::from(value as i16))
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_75a3f0(_request: u32) -> i32 {
        crate::record_native_boundary::with_native_context::<(), _>(|context| match helper_75a3f0(context) {
            Ok(value) => value,
            Err(_) => crate::record_native_boundary::fault(),
        })
    }
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 body at 0x0075_a3f0.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn with_block<R>(block: &mut [u8; 0x1000], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(SHARED_BLOCK), ByteCount(0x1000), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, block).unwrap()];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    #[test]
    fn reads_the_signed_halfword_at_offset_970() {
        let mut block = [0u8; 0x1000];
        block[0x970..0x972].copy_from_slice(&0x8000u16.to_le_bytes());
        with_block(&mut block, |context| assert_eq!(helper_75a3f0(context), Ok(-32768)));
        block[0x970..0x972].copy_from_slice(&0xffffu16.to_le_bytes());
        with_block(&mut block, |context| assert_eq!(helper_75a3f0(context), Ok(-1)));
        block[0x970..0x972].copy_from_slice(&0x1234u16.to_le_bytes());
        with_block(&mut block, |context| assert_eq!(helper_75a3f0(context), Ok(0x1234)));
    }
}
