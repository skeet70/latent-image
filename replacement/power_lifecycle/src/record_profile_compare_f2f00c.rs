//! Entries 0x00f2f00c, 0x00f2f440 and 0x00f2f480 of the image-5 cluster at 0x00f2f00c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_mapping::{helper_746864, helper_74714c};

const WORD_16BE000: u32 = 0x016b_e000;
const WORD_16BDFFC: u32 = 0x016b_dffc;

/// Returns one when the source and destination banks map the resolved index to different values.
pub fn helper_f2f00c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: u32, value: u32, destination: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    let resolved = helper_74714c(context, source, value, destination)?;
    context.write_u32(output, resolved)?;
    let mapped = helper_746864(context, value, source)?;
    let remapped = helper_746864(context, resolved, destination)?;
    Ok(u32::from(mapped != remapped))
}

pub fn helper_f2f440<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(WORD_16BE000), value)
}

pub fn helper_f2f480<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(WORD_16BDFFC), value)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_f2f00c, helper_f2f00c_abi, (), (source: u32, value: u32, destination: u32, output: u32) -> u32, (source: u32, value: u32, destination: u32, output: GuestAddress), (source, value, destination, GuestAddress::new(output)), crate::record_profile_compare_f2f00c::helper_f2f00c, |value| value);
    stock_export!(xm1_power_record_helper_f2f440, helper_f2f440_abi, (), (value: u32) -> (), (value: u32), (value), crate::record_profile_compare_f2f00c::helper_f2f440, |_| ());
    stock_export!(xm1_power_record_helper_f2f480, helper_f2f480_abi, (), (value: u32) -> (), (value: u32), (value), crate::record_profile_compare_f2f00c::helper_f2f480, |_| ());
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f2f00c, 0x00f2f440 and 0x00f2f480.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIGURATION: u32 = 0x0020_0000;
    const CONFIGURATION_LENGTH: usize = 0x3f_4640;
    const OUTPUT: u32 = 0x0010_0000;

    struct Memory {
        pointer: [u8; 4],
        configuration: std::vec::Vec<u8>,
        output: [u8; 8],
        words: [u8; 8],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION, CONFIGURATION_LENGTH as u32), &mut memory.configuration[..]).unwrap(),
            MemoryRegion::new(descriptor(OUTPUT, 8), &mut memory.output).unwrap(),
            MemoryRegion::new(descriptor(WORD_16BDFFC, 8), &mut memory.words).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn memory() -> Memory {
        Memory { pointer: CONFIGURATION.to_le_bytes(), configuration: std::vec![0; CONFIGURATION_LENGTH], output: [0; 8], words: [0; 8] }
    }

    fn put_half(memory: &mut Memory, key: u32, value: u16) { memory.configuration[key as usize..key as usize + 2].copy_from_slice(&value.to_le_bytes()); }

    #[test]
    fn the_resolved_index_is_stored_and_equal_mappings_report_zero() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_f2f00c(context, 0, 5, 1, GuestAddress::new(OUTPUT)), Ok(0));
        });
        assert_eq!(&memory.output[0..4], &0u32.to_le_bytes());
    }

    #[test]
    fn different_mappings_report_one() {
        let mut memory = memory();
        memory.configuration[0x3f4600] = 1;
        put_half(&mut memory, 2 * (0x1f1298 + 1), 7);
        memory.configuration[0x3f4600 + 31] = 2;
        put_half(&mut memory, 2 * (0x1f1298 + 2), 9);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_f2f00c(context, 0, 5, 1, GuestAddress::new(OUTPUT)), Ok(1));
        });
        assert_eq!(&memory.output[0..4], &0u32.to_le_bytes());
    }

    #[test]
    fn the_two_entries_store_their_own_word() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_f2f440(context, 0x1122_3344), Ok(()));
            assert_eq!(helper_f2f480(context, 0x5566_7788), Ok(()));
        });
        assert_eq!(&memory.words[0..4], &0x5566_7788u32.to_le_bytes());
        assert_eq!(&memory.words[4..8], &0x1122_3344u32.to_le_bytes());
    }
}
