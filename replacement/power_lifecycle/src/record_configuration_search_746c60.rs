//! Index searches over a configuration range whose entries map to values.
//!
//! Image-4 bodies 0x00746c60-0x00746f54.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc;
use crate::record_profile_mapping::helper_746864;

const RANGE_LOWER_KEY: u32 = 0x3e22_c8;
const RANGE_UPPER_KEY: u32 = 0x3e23_2d;
const ALTERNATE_MODE_KEY: u32 = 0x3e47_60;
const ALTERNATE_LOWER_KEY: u32 = 0x3e47_c6;
const ALTERNATE_UPPER_KEY: u32 = 0x3e47_61;

/// The image-4 body at 0x00746c60.
pub fn helper_746c60<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, value: u32) -> Result<u32, MemoryError> {
    let (lower, upper) = bounds(context, bank)?;
    first_at_or_above(context, bank, lower, upper, value)
}

/// The image-4 body at 0x00746cfc.
pub fn helper_746cfc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, value: u32) -> Result<u32, MemoryError> {
    let (lower, upper) = bounds(context, bank)?;
    last_at_or_below(context, bank, lower, upper, value)
}

/// The image-4 body at 0x00746da0.
pub fn helper_746da0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, value: u32) -> Result<u32, MemoryError> {
    let (lower, upper) = selected_bounds(context, bank)?;
    first_at_or_above(context, bank, lower, upper, value)
}

/// The image-4 body at 0x00746f54.
pub fn helper_746f54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, value: u32) -> Result<u32, MemoryError> {
    let (lower, upper) = selected_bounds(context, bank)?;
    last_at_or_below(context, bank, lower, upper, value)
}

fn bounds<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<(u32, u32), MemoryError> {
    Ok((helper_69f8fc(context, bank.wrapping_add(RANGE_LOWER_KEY))?, helper_69f8fc(context, bank.wrapping_add(RANGE_UPPER_KEY))?))
}

fn selected_bounds<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<(u32, u32), MemoryError> {
    if helper_69f8fc(context, ALTERNATE_MODE_KEY)? == 1 {
        Ok((helper_69f8fc(context, bank.wrapping_add(ALTERNATE_LOWER_KEY))?, helper_69f8fc(context, bank.wrapping_add(ALTERNATE_UPPER_KEY))?))
    } else {
        bounds(context, bank)
    }
}

fn first_at_or_above<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, lower: u32, upper: u32, value: u32) -> Result<u32, MemoryError> {
    let mut index = lower;
    while index < upper && helper_746864(context, index, bank)? < value {
        index += 1;
    }
    Ok(index)
}

fn last_at_or_below<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32, lower: u32, upper: u32, value: u32) -> Result<u32, MemoryError> {
    let mut index = upper;
    while lower < index && value < helper_746864(context, index, bank)? {
        index -= 1;
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x00746c60-0x00746f54 and
    //! the mapping body at 0x00746864.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const KEY_ORIGIN: u32 = 0x3e00_00;
    const CONFIG_LENGTH: u32 = 0x15000;
    const MAPPED_ENTRY_KEY: u32 = 0x3f46_00;
    const SPLIT_COUNT_KEY: u32 = 0x3e0f_82;

    fn at(key: u32) -> usize {
        (key - KEY_ORIGIN) as usize
    }

    fn with_config<R>(config: &mut [u8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.wrapping_sub(KEY_ORIGIN).to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn ranged(config: &mut [u8], lower: u8, upper: u8) {
        config[at(RANGE_LOWER_KEY)] = lower;
        config[at(RANGE_UPPER_KEY)] = upper;
        config[at(MAPPED_ENTRY_KEY)] = 0xff;
    }

    #[test]
    fn an_unreached_limit_returns_the_upper_bound() {
        let mut config = std::vec![0u8; CONFIG_LENGTH as usize];
        ranged(&mut config, 2, 5);
        assert_eq!(config[at(SPLIT_COUNT_KEY)], 0);
        with_config(&mut config, |context| {
            assert_eq!(helper_746c60(context, 0, 100), Ok(2));
            assert_eq!(helper_746c60(context, 0, 101), Ok(5));
            assert_eq!(helper_746cfc(context, 0, 100), Ok(5));
            assert_eq!(helper_746cfc(context, 0, 99), Ok(2));
        });
    }

    #[test]
    fn the_alternate_mode_selects_the_alternate_range() {
        let mut config = std::vec![0u8; CONFIG_LENGTH as usize];
        ranged(&mut config, 2, 5);
        config[at(ALTERNATE_MODE_KEY)] = 1;
        config[at(ALTERNATE_LOWER_KEY)] = 3;
        config[at(ALTERNATE_UPPER_KEY)] = 6;
        with_config(&mut config, |context| {
            assert_eq!(helper_746da0(context, 0, 101), Ok(6));
            assert_eq!(helper_746f54(context, 0, 99), Ok(3));
            assert_eq!(helper_746c60(context, 0, 101), Ok(5));
        });
    }
}
