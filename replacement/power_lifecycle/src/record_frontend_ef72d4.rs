//! Entries 0x00ef72d4, 0x00ef73e4 and 0x00ef7820 of the image-5 cluster at 0x00ef72d4.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_globals::helper_772f28;

pub fn helper_ef72d4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<u32, MemoryError> {
    let first = helper_772f28(context, first)?;
    let second = helper_772f28(context, second)?;
    Ok((first.wrapping_sub(second) as i16 as i32) as u32)
}

pub fn helper_ef73e4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(match value { 2 => 1, 3 => 2, 4 => 3, 5 => 4, _ => 0 })
}

pub fn helper_ef7820<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(u32::from(value.wrapping_sub(3) <= 1))
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_ef72d4, helper_ef72d4_abi, (), (first: u32, second: u32) -> u32, (first: u32, second: u32), (first, second), crate::record_frontend_ef72d4::helper_ef72d4, |value| value);
    stock_export!(xm1_power_record_helper_ef73e4, helper_ef73e4_abi, (), (value: u32) -> u32, (value: u32), (value), crate::record_frontend_ef72d4::helper_ef73e4, |value| value);
    stock_export!(xm1_power_record_helper_ef7820, helper_ef7820_abi, (), (value: u32) -> u32, (value: u32), (value), crate::record_frontend_ef72d4::helper_ef7820, |value| value);
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ef72d4, 0x00ef73e4 and 0x00ef7820.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIGURATION: u32 = 0x0020_0000;

    struct Memory {
        pointer: [u8; 4],
        configuration: [u8; 0x2000],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION, 0x2000), &mut memory.configuration).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn memory() -> Memory { Memory { pointer: CONFIGURATION.to_le_bytes(), configuration: [0; 0x2000] } }

    fn put_half(memory: &mut Memory, at: usize, value: u16) { memory.configuration[at..at + 2].copy_from_slice(&value.to_le_bytes()); }

    #[test]
    fn the_difference_is_the_signed_halfword_of_two_mapped_values() {
        let mut memory = memory();
        put_half(&mut memory, 0x1904, 7);
        put_half(&mut memory, 0x1906, 5);
        put_half(&mut memory, 0x1908, 0x8001);
        put_half(&mut memory, 0x190a, 3);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ef72d4(context, 2, 2), Ok(0));
            assert_eq!(helper_ef72d4(context, 5, 3), Ok(0xffff_fffe));
            assert_eq!(helper_ef72d4(context, 3, 5), Ok(2));
            assert_eq!(helper_ef72d4(context, 4, 2), Ok(0x7ffa));
        });
    }

    #[test]
    fn the_table_maps_two_through_five_and_zero_every_other_value() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ef73e4(context, 0), Ok(0));
            assert_eq!(helper_ef73e4(context, 1), Ok(0));
            assert_eq!(helper_ef73e4(context, 2), Ok(1));
            assert_eq!(helper_ef73e4(context, 3), Ok(2));
            assert_eq!(helper_ef73e4(context, 4), Ok(3));
            assert_eq!(helper_ef73e4(context, 5), Ok(4));
            assert_eq!(helper_ef73e4(context, 6), Ok(0));
            assert_eq!(helper_ef73e4(context, u32::MAX), Ok(0));
        });
    }

    #[test]
    fn the_membership_test_covers_three_and_four() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ef7820(context, 0), Ok(0));
            assert_eq!(helper_ef7820(context, 2), Ok(0));
            assert_eq!(helper_ef7820(context, 3), Ok(1));
            assert_eq!(helper_ef7820(context, 4), Ok(1));
            assert_eq!(helper_ef7820(context, 5), Ok(0));
            assert_eq!(helper_ef7820(context, u32::MAX), Ok(0));
        });
    }
}
