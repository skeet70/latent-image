//! Stock-shaped record helpers that return an image-5 constant.
//!
//! Each value is the literal pool word at the address in the helper's name plus 0x18.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

macro_rules! constant {
    ($name:ident, $value:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok($value) }
    };
}

constant!(helper_ef9bc0, 0x0169_d05c);
constant!(helper_ef9c00, 0x0169_d09c);
constant!(helper_ef9d18, 0x0169_d1ec);
constant!(helper_ef9dc0, 0x0169_d22c);
constant!(helper_ef9ddc, 0x0169_d254);
constant!(helper_ef9e14, 0x0169_d278);
constant!(helper_ef9e4c, 0x0169_d294);
constant!(helper_ef9e84, 0x0169_d2a8);
constant!(helper_ef9ea0, 0x0169_d2b0);
constant!(helper_ef9ebc, 0x0169_d2c4);
constant!(helper_ef9ed8, 0x0169_d2cc);
constant!(helper_ef9f10, 0x0169_d2dc);
constant!(helper_ef9f64, 0x0169_d308);

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::record_helper_native_memory::InstalledMemory;

    macro_rules! constant_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name() -> u32 {
                super::$helper(&mut RecordContext::new(InstalledMemory, ())).unwrap_or(0)
            }
        };
    }

    constant_export!(xm1_power_record_helper_ef9bc0, helper_ef9bc0);
    constant_export!(xm1_power_record_helper_ef9c00, helper_ef9c00);
    constant_export!(xm1_power_record_helper_ef9d18, helper_ef9d18);
    constant_export!(xm1_power_record_helper_ef9dc0, helper_ef9dc0);
    constant_export!(xm1_power_record_helper_ef9ddc, helper_ef9ddc);
    constant_export!(xm1_power_record_helper_ef9e14, helper_ef9e14);
    constant_export!(xm1_power_record_helper_ef9e4c, helper_ef9e4c);
    constant_export!(xm1_power_record_helper_ef9e84, helper_ef9e84);
    constant_export!(xm1_power_record_helper_ef9ea0, helper_ef9ea0);
    constant_export!(xm1_power_record_helper_ef9ebc, helper_ef9ebc);
    constant_export!(xm1_power_record_helper_ef9ed8, helper_ef9ed8);
    constant_export!(xm1_power_record_helper_ef9f10, helper_ef9f10);
    constant_export!(xm1_power_record_helper_ef9f64, helper_ef9f64);
}

#[cfg(test)]
mod tests {
    //! Expected values are the image-5 literal pool words at 0x00ef9bd8-0x00ef9f7c.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion};

    #[test]
    fn every_constant_is_its_literal_pool_word() {
        let mut regions: [MemoryRegion<'static>; 0] = [];
        let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
        assert_eq!(helper_ef9bc0(&mut context), Ok(0x0169_d05c));
        assert_eq!(helper_ef9c00(&mut context), Ok(0x0169_d09c));
        assert_eq!(helper_ef9d18(&mut context), Ok(0x0169_d1ec));
        assert_eq!(helper_ef9dc0(&mut context), Ok(0x0169_d22c));
        assert_eq!(helper_ef9ddc(&mut context), Ok(0x0169_d254));
        assert_eq!(helper_ef9e14(&mut context), Ok(0x0169_d278));
        assert_eq!(helper_ef9e4c(&mut context), Ok(0x0169_d294));
        assert_eq!(helper_ef9e84(&mut context), Ok(0x0169_d2a8));
        assert_eq!(helper_ef9ea0(&mut context), Ok(0x0169_d2b0));
        assert_eq!(helper_ef9ebc(&mut context), Ok(0x0169_d2c4));
        assert_eq!(helper_ef9ed8(&mut context), Ok(0x0169_d2cc));
        assert_eq!(helper_ef9f10(&mut context), Ok(0x0169_d2dc));
        assert_eq!(helper_ef9f64(&mut context), Ok(0x0169_d308));
    }
}
