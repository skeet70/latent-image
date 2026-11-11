//! The bounded configuration lookups that fall back to a single bound byte.
//!
//! Image-4 bodies 0x00746cc8-0x00746ffc.
use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc;
use crate::record_configuration_search_746c60::{helper_746c60, helper_746cfc, helper_746da0, helper_746f54};

const UNBOUNDED: u32 = 0xffff;
const RANGE_LOWER_KEY: u32 = 0x3e22_c8;
const RANGE_UPPER_KEY: u32 = 0x3e23_2d;
const ALTERNATE_MODE_KEY: u32 = 0x3e47_60;
const ALTERNATE_LOWER_KEY: u32 = 0x3e47_c6;
const ALTERNATE_UPPER_KEY: u32 = 0x3e47_61;

/// The image-4 body at 0x00746cc8.
pub fn helper_746cc8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, limit: u32) -> Result<u32, MemoryError> {
    if limit == UNBOUNDED {
        helper_69f8fc(context, bank.wrapping_add(RANGE_LOWER_KEY))
    } else {
        helper_746c60(context, bank, limit)
    }
}

/// The image-4 body at 0x00746d6c.
pub fn helper_746d6c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, limit: u32) -> Result<u32, MemoryError> {
    if limit == UNBOUNDED {
        helper_69f8fc(context, bank.wrapping_add(RANGE_UPPER_KEY))
    } else {
        helper_746cfc(context, bank, limit)
    }
}

/// The image-4 body at 0x00746e48.
pub fn helper_746e48<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, limit: u32) -> Result<u32, MemoryError> {
    if limit == UNBOUNDED {
        let key = if helper_69f8fc(context, ALTERNATE_MODE_KEY)? == 1 { ALTERNATE_LOWER_KEY } else { RANGE_LOWER_KEY };
        helper_69f8fc(context, bank.wrapping_add(key))
    } else {
        helper_746da0(context, bank, limit)
    }
}

/// The image-4 body at 0x00746ffc.
pub fn helper_746ffc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, limit: u32) -> Result<u32, MemoryError> {
    if limit == UNBOUNDED {
        let key = if helper_69f8fc(context, ALTERNATE_MODE_KEY)? == 1 { ALTERNATE_UPPER_KEY } else { RANGE_UPPER_KEY };
        helper_69f8fc(context, bank.wrapping_add(key))
    } else {
        helper_746f54(context, bank, limit)
    }
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x00746cc8-0x00746ffc.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const MAPPED_ENTRY_KEY: usize = 0x3f46_00;

    fn with_config<R>(config: &mut [u8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn an_unbounded_lookup_returns_the_single_bound_byte() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[RANGE_LOWER_KEY as usize] = 2;
        config[RANGE_UPPER_KEY as usize] = 5;
        config[ALTERNATE_MODE_KEY as usize] = 0;
        config[ALTERNATE_LOWER_KEY as usize] = 3;
        config[ALTERNATE_UPPER_KEY as usize] = 6;
        with_config(&mut config, |context| {
            assert_eq!(helper_746cc8(context, 0, UNBOUNDED), Ok(2));
            assert_eq!(helper_746d6c(context, 0, UNBOUNDED), Ok(5));
            assert_eq!(helper_746e48(context, 0, UNBOUNDED), Ok(2));
            assert_eq!(helper_746ffc(context, 0, UNBOUNDED), Ok(5));
        });
        config[ALTERNATE_MODE_KEY as usize] = 1;
        with_config(&mut config, |context| {
            assert_eq!(helper_746e48(context, 0, UNBOUNDED), Ok(3));
            assert_eq!(helper_746ffc(context, 0, UNBOUNDED), Ok(6));
        });
    }

    #[test]
    fn a_bounded_lookup_searches_the_selected_range() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[RANGE_LOWER_KEY as usize] = 2;
        config[RANGE_UPPER_KEY as usize] = 5;
        config[ALTERNATE_MODE_KEY as usize] = 1;
        config[ALTERNATE_LOWER_KEY as usize] = 3;
        config[ALTERNATE_UPPER_KEY as usize] = 6;
        config[MAPPED_ENTRY_KEY] = 0xff;
        with_config(&mut config, |context| {
            assert_eq!(helper_746cc8(context, 0, 101), Ok(5));
            assert_eq!(helper_746d6c(context, 0, 99), Ok(2));
            assert_eq!(helper_746e48(context, 0, 101), Ok(6));
            assert_eq!(helper_746ffc(context, 0, 99), Ok(3));
        });
    }
}
