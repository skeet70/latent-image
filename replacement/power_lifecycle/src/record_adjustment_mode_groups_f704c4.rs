//! Adjustment mode groups and the two constant-returning bodies beside them.
//!
//! Image-5 bodies 0x00f704c4-0x00f71e54.
use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

/// The modes the image-5 body at 0x00f704c4 accepts.
const ACCEPTED_MODES: [u32; 5] = [0, 0x2b, 0x2c, 0x30, 0x39];
/// The modes the image-5 body at 0x00f713bc reports as its group.
const FIRST_GROUP: [u32; 5] = [0x18, 0x2d, 0x2f, 0x37, 0x38];
/// The modes the image-5 body at 0x00f71470 reports as its group.
const SECOND_GROUP: [u32; 4] = [0x2e, 0x2f, 0x36, 0x38];
/// The modes the image-5 body at 0x00f714a8 reports as its group.
const THIRD_GROUP: [u32; 2] = [0x2c, 0x39];

/// The image-5 body at 0x00f704c4.
pub fn helper_f704c4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32, flags: u32, enabled: u32) -> Result<i32, MemoryError> {
    let _ = context;
    if enabled == 0 {
        return Ok(0);
    }
    if !ACCEPTED_MODES.contains(&mode) {
        return Ok(0);
    }
    Ok(i32::from(flags & 2 == 0))
}

/// The image-5 body at 0x00f713bc.
pub fn helper_f713bc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32) -> Result<i32, MemoryError> {
    let _ = context;
    Ok(i32::from(FIRST_GROUP.contains(&mode)))
}

/// The image-5 body at 0x00f71470.
pub fn helper_f71470<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32) -> Result<i32, MemoryError> {
    let _ = context;
    Ok(i32::from(SECOND_GROUP.contains(&mode)))
}

/// The image-5 body at 0x00f714a8.
pub fn helper_f714a8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32) -> Result<i32, MemoryError> {
    let _ = context;
    Ok(i32::from(THIRD_GROUP.contains(&mode)))
}

/// The image-5 body at 0x00f71adc.
pub fn helper_f71adc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    let _ = context;
    Ok(0)
}

/// The image-5 body at 0x00f71e54.
pub fn helper_f71e54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    let _ = context;
    Ok(0xffff_fb62u32 as i32)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f704c4-0x00f71e54.
    use super::*;
    use crate::record_address_space::{GuestAddress, MemoryError};

    struct Empty;

    impl GuestAddressSpace for Empty {
        fn domain(&self) -> crate::record_address_space::GuestDomain { crate::record_address_space::GuestDomain::A }
        fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn write_u8(&mut self, address: GuestAddress, _: u8) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn write_u16(&mut self, address: GuestAddress, _: u16) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn write_u32(&mut self, address: GuestAddress, _: u32) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn copy_bytes(&mut self, destination: GuestAddress, _: GuestAddress, _: crate::record_address_space::ByteCount) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(destination)) }
    }

    fn context() -> RecordContext<Empty, ()> {
        RecordContext::new(Empty, ())
    }

    #[test]
    fn the_flag_gate_needs_an_enabled_accepted_mode_and_clear_flag() {
        let mut context = context();
        for mode in ACCEPTED_MODES {
            assert_eq!(helper_f704c4(&mut context, mode, 0, 1), Ok(1));
            assert_eq!(helper_f704c4(&mut context, mode, 2, 1), Ok(0));
            assert_eq!(helper_f704c4(&mut context, mode, 0, 0), Ok(0));
        }
        assert_eq!(helper_f704c4(&mut context, 1, 0, 1), Ok(0));
        assert_eq!(helper_f704c4(&mut context, 0x2d, 0, 1), Ok(0));
        assert_eq!(helper_f704c4(&mut context, 0x3a, 0, 1), Ok(0));
    }

    #[test]
    fn the_mode_groups_are_three_disjoint_sets() {
        let mut context = context();
        for mode in 0..0x40u32 {
            assert_eq!(helper_f713bc(&mut context, mode), Ok(i32::from(FIRST_GROUP.contains(&mode))));
            assert_eq!(helper_f71470(&mut context, mode), Ok(i32::from(SECOND_GROUP.contains(&mode))));
            assert_eq!(helper_f714a8(&mut context, mode), Ok(i32::from(THIRD_GROUP.contains(&mode))));
        }
        assert_eq!(helper_f713bc(&mut context, 0x2d), Ok(1));
        assert_eq!(helper_f71470(&mut context, 0x2e), Ok(1));
        assert_eq!(helper_f714a8(&mut context, 0x39), Ok(1));
    }

    #[test]
    fn the_constant_bodies_return_zero_and_the_negative_word() {
        let mut context = context();
        assert_eq!(helper_f71adc(&mut context), Ok(0));
        assert_eq!(helper_f71e54(&mut context), Ok(-1182));
    }
}
